use std::collections::HashMap;
use std::time::Instant;

#[cfg(test)]
mod conformance;
// `composite.rs` is the realized-layer → borrowed-`CompositeLayer` conversion,
// moved out of `layers.rs` to stay under the file ceiling.
mod composite;
mod layers;
mod pass_one;
mod text;

use composite::composite_from_realized;
use layers::{Realized, fixed_size};

use aria_compositor::{
    ColorGrade, CompositeLayer, CompositorConfig, CompositorLayer, FrameSink, ImageSink,
    LayerPlacement, MatteRead, PassInstance, SdfLayer, wgpu,
};
use aria_core::Cancel;
use aria_models::{MatteSource, MediaId, Project};
use aria_shapes::ShapeStyle;

use crate::error::RenderError;
use crate::motion_blur::{
    blur_passes_for_placement, layer_placements_from_bitmap_scaled_blur_passes,
    layer_placements_from_blur_passes,
};
use crate::scene::{LayerSource, Scene, SceneLayer, SceneLut, SizeSpec};

use super::effects::{blend_mode, layer_effects, layer_styles};
use super::media_cache::layer_lut;
use super::{FrameStats, PendingFrame, Renderer, SLOW_FRAME_LOG_MS, SeekPolicy};

/// Where a submitted frame's canvas goes.
#[derive(Clone, Copy)]
pub(in crate::render) enum Output<'t> {
    /// A readback buffer the CPU maps when the frame finishes.
    Readback,
    /// A caller texture, written on the GPU.
    Texture(&'t wgpu::Texture),
}

/// Which channel of the stencil the compositor reads. The compositor does not
/// depend on `aria-models`, so the model enum is mapped here at the boundary.
fn matte_read(source: MatteSource) -> MatteRead {
    match source {
        MatteSource::Alpha => MatteRead::Alpha,
        MatteSource::Luma => MatteRead::Luma,
    }
}

fn realized_scene_layer(layers: &[SceneLayer], scene_idx: usize) -> &SceneLayer {
    &layers[scene_idx]
}

fn transition_effects(layer: &SceneLayer) -> (&[PassInstance], &[PassInstance]) {
    let LayerSource::Transition {
        outgoing, incoming, ..
    } = &layer.source
    else {
        unreachable!("realized transition lost its source layer")
    };
    (&outgoing.effects, &incoming.effects)
}

fn matte_effects(layer: &SceneLayer) -> (&[PassInstance], &[PassInstance]) {
    let LayerSource::Matte { layers, .. } = &layer.source else {
        unreachable!("realized matte lost its source layer")
    };
    (&layers[0].effects, &layers[1].effects)
}

#[derive(Default)]
pub(super) struct RealizeScratch {
    realized: Vec<Realized>,
    scene_indices: Vec<usize>,
    blur_for_realized: Vec<Vec<LayerPlacement>>,
    occurrence: HashMap<MediaId, u32>,
}

impl RealizeScratch {
    fn begin_frame(&mut self) {
        self.realized.clear();
        self.scene_indices.clear();
        self.blur_for_realized.clear();
        self.occurrence.clear();
    }
}

impl Renderer {
    pub(super) fn submit_scene_once(
        &mut self,
        project: &Project,
        scene: &Scene,
        policy: SeekPolicy,
        output: Output<'_>,
    ) -> Result<PendingFrame, RenderError> {
        let mut scratch = std::mem::take(&mut self.realize_scratch);
        match self.submit_scene_once_with_scratch(project, scene, policy, output, &mut scratch) {
            Ok(mut pending) => {
                pending.scratch = Some(scratch);
                Ok(pending)
            }
            Err(error) => {
                self.realize_scratch = scratch;
                Err(error)
            }
        }
    }

    fn submit_scene_once_with_scratch(
        &mut self,
        project: &Project,
        scene: &Scene,
        policy: SeekPolicy,
        output: Output<'_>,
        scratch: &mut RealizeScratch,
    ) -> Result<PendingFrame, RenderError> {
        let realize_started = Instant::now();
        // The token governs this frame and no other: taking it means one left
        // set by a caller that never rendered cannot cancel a later frame.
        let cancel = std::mem::take(&mut self.cancel);
        if cancel.is_cancelled() {
            return Err(RenderError::Cancelled);
        }
        // New scene, new LRU stamp: frames touched below are eviction-exempt
        // until the next scene.
        self.lottie_stamp += 1;
        // First pass: decode/rasterize each layer into owned pixels and a final
        // placement. Held in `realized` so the borrowed `CompositeLayer`s built
        // below stay valid through the composite call.
        scratch.begin_frame();
        let decode_ms = self.realize_scene_layers(
            project,
            scene,
            policy,
            &cancel,
            &mut scratch.realized,
            &mut scratch.scene_indices,
            &mut scratch.blur_for_realized,
            &mut scratch.occurrence,
        )?;
        let realized = &scratch.realized;
        let blur_for_realized = &scratch.blur_for_realized;
        let mut layer_storage: Vec<CompositeLayer<'_>> = Vec::new();
        // Phase 1: build all composite layers (indices only for transitions).
        enum LayerJob<'a> {
            Plain {
                storage_idx: usize,
            },
            CanvasPass {
                effects: &'a [PassInstance],
                grade: Option<ColorGrade>,
                lut: &'a Option<SceneLut>,
            },
            Transition {
                out_idx: usize,
                in_idx: usize,
                transition_id: &'a str,
                progress: f32,
            },
            Matte {
                content_idx: usize,
                stencil_idx: usize,
                read: MatteSource,
                invert: bool,
                softness: f32,
            },
        }
        let mut jobs: Vec<LayerJob<'_>> = Vec::new();

        for (realized_idx, (r, &scene_idx)) in
            realized.iter().zip(&scratch.scene_indices).enumerate()
        {
            let scene_layer = realized_scene_layer(&scene.layers, scene_idx);
            match r {
                Realized::CanvasPass { grade, lut } => {
                    let effects = &scene_layer.effects;
                    jobs.push(LayerJob::CanvasPass {
                        effects,
                        grade: *grade,
                        lut,
                    });
                }
                Realized::Transition {
                    outgoing,
                    incoming,
                    transition_id,
                    progress,
                } => {
                    let (out_effects, in_effects) = transition_effects(scene_layer);
                    layer_storage.push(composite_from_realized(
                        outgoing.as_ref(),
                        &self.stills,
                        &self.stickers,
                        &self.lottie,
                        &self.luts,
                        out_effects,
                    ));
                    let out_idx = layer_storage.len() - 1;
                    layer_storage.push(composite_from_realized(
                        incoming.as_ref(),
                        &self.stills,
                        &self.stickers,
                        &self.lottie,
                        &self.luts,
                        in_effects,
                    ));
                    let in_idx = layer_storage.len() - 1;
                    jobs.push(LayerJob::Transition {
                        out_idx,
                        in_idx,
                        transition_id: transition_id.as_str(),
                        progress: *progress,
                    });
                }
                Realized::Matte {
                    content,
                    stencil,
                    read,
                    invert,
                    softness,
                } => {
                    let (content_effects, stencil_effects) = matte_effects(scene_layer);
                    layer_storage.push(composite_from_realized(
                        content.as_ref(),
                        &self.stills,
                        &self.stickers,
                        &self.lottie,
                        &self.luts,
                        content_effects,
                    ));
                    let content_idx = layer_storage.len() - 1;
                    layer_storage.push(composite_from_realized(
                        stencil.as_ref(),
                        &self.stills,
                        &self.stickers,
                        &self.lottie,
                        &self.luts,
                        stencil_effects,
                    ));
                    let stencil_idx = layer_storage.len() - 1;
                    jobs.push(LayerJob::Matte {
                        content_idx,
                        stencil_idx,
                        read: *read,
                        invert: *invert,
                        softness: *softness,
                    });
                }
                Realized::Glyphs {
                    background: Some((bg_image, bg_placement)),
                    fx,
                    color_grade,
                    lut,
                    blend_mode,
                    ..
                } => {
                    // Whole-run background card sits behind the glyphs.
                    let bg_effects = scene_layer.effects.as_slice();
                    layer_storage.push(
                        CompositeLayer::rgba(bg_image, *bg_placement)
                            .with_fx(*fx)
                            .with_effects(bg_effects)
                            .with_color_grade(*color_grade)
                            .with_lut(layer_lut(lut, &self.luts))
                            .with_blend_mode(*blend_mode),
                    );
                    jobs.push(LayerJob::Plain {
                        storage_idx: layer_storage.len() - 1,
                    });
                    // Glyphs share the same effect chain reference when present.
                    // Per-glyph instances own placement; transform motion blur
                    // is not applied to glyph runs (bitmap text still blurs).
                    let glyph_effects = bg_effects;
                    layer_storage.push(composite_from_realized(
                        r,
                        &self.stills,
                        &self.stickers,
                        &self.lottie,
                        &self.luts,
                        glyph_effects,
                    ));
                    jobs.push(LayerJob::Plain {
                        storage_idx: layer_storage.len() - 1,
                    });
                }
                other => {
                    let effects = scene_layer.effects.as_slice();
                    let mut cl = composite_from_realized(
                        other,
                        &self.stills,
                        &self.stickers,
                        &self.lottie,
                        &self.luts,
                        effects,
                    );
                    let mut blur = blur_for_realized[realized_idx].clone();
                    // BitmapScaled layers skip attach-time sampling; fill now.
                    if blur.is_empty()
                        && self.apply_motion_blur
                        && !matches!(other, Realized::Glyphs { .. })
                        && let Some(clip_id) = scene.layers[scene_idx].clip
                    {
                        blur = blur_passes_for_placement(
                            project,
                            scene,
                            clip_id,
                            cl.placement,
                            scene.layers[scene_idx].anchor_point,
                        );
                    }
                    if !blur.is_empty() {
                        cl = cl.with_blur_passes(blur);
                    }
                    layer_storage.push(cl);
                    jobs.push(LayerJob::Plain {
                        storage_idx: layer_storage.len() - 1,
                    });
                }
            }
        }

        // Phase 2: borrow storage immutably for compositor dispatch.
        let compositor_layers: Vec<CompositorLayer<'_>> = jobs
            .iter()
            .map(|job| match job {
                LayerJob::Plain { storage_idx } => {
                    CompositorLayer::layer(&layer_storage[*storage_idx])
                }
                LayerJob::CanvasPass {
                    effects,
                    grade,
                    lut,
                } => CompositorLayer::CanvasPass {
                    effects,
                    grade: *grade,
                    lut: layer_lut(lut, &self.luts),
                },
                LayerJob::Transition {
                    out_idx,
                    in_idx,
                    transition_id,
                    progress,
                } => CompositorLayer::Transition {
                    outgoing: &layer_storage[*out_idx],
                    incoming: &layer_storage[*in_idx],
                    transition_id,
                    progress: *progress,
                },
                LayerJob::Matte {
                    content_idx,
                    stencil_idx,
                    read,
                    invert,
                    softness,
                } => CompositorLayer::Matte {
                    content: &layer_storage[*content_idx],
                    stencil: &layer_storage[*stencil_idx],
                    read: matte_read(*read),
                    invert: *invert,
                    softness: *softness,
                },
            })
            .collect();

        // Submission is the last cancellation boundary. The pending copy owns
        // no scene borrows, so a later frame can realize while this one maps.
        if cancel.is_cancelled() {
            return Err(RenderError::Cancelled);
        }
        let config =
            CompositorConfig::new(scene.width, scene.height).with_background(scene.background);
        let realize_ms = realize_started.elapsed().as_secs_f64() * 1000.0;
        let submitted_at = Instant::now();
        let readback = match output {
            Output::Readback => Some(self.compositor.submit_compositor_layers(
                &self.gpu,
                &config,
                &compositor_layers,
            )?),
            Output::Texture(texture) => {
                self.compositor.submit_compositor_layers_to_texture(
                    &self.gpu,
                    &config,
                    &compositor_layers,
                    texture,
                )?;
                None
            }
        };
        let passes = self.compositor.pass_stats();
        Ok(PendingFrame {
            readback,
            scratch: None,
            decode_ms,
            raster_ms: (realize_ms - decode_ms).max(0.0),
            submitted_at,
            effect_passes: passes.passes,
            effect_pass_bind_groups: passes.created,
            layers: compositor_layers.len(),
            width: scene.width,
            height: scene.height,
        })
    }

    pub(super) fn finish_pending_into(
        &mut self,
        mut pending: PendingFrame,
        sink: &mut dyn FrameSink,
    ) -> Result<(), RenderError> {
        let result = pending
            .readback
            .take()
            .expect("a frame submitted for readback retains its readback")
            .read_into(sink);
        if let Some(scratch) = pending.scratch.take() {
            self.realize_scratch = scratch;
        }
        result?;
        let composite_ms = pending.submitted_at.elapsed().as_secs_f64() * 1000.0;
        self.record_pending_stats(&pending, composite_ms);
        Ok(())
    }

    /// Finish a frame written to a texture: nothing maps, so the scratch
    /// returns and the stats record the submission.
    pub(super) fn finish_pending_on_gpu(&mut self, mut pending: PendingFrame) {
        if let Some(scratch) = pending.scratch.take() {
            self.realize_scratch = scratch;
        }
        let composite_ms = pending.submitted_at.elapsed().as_secs_f64() * 1000.0;
        self.record_pending_stats(&pending, composite_ms);
    }

    pub(crate) fn finish_pending(
        &mut self,
        pending: PendingFrame,
    ) -> Result<aria_core::RgbaImage, RenderError> {
        let mut sink = ImageSink::default();
        self.finish_pending_into(pending, &mut sink)?;
        Ok(sink
            .into_image()
            .expect("a successful pending frame fills the image sink"))
    }

    fn record_pending_stats(&mut self, pending: &PendingFrame, composite_ms: f64) {
        self.last_stats = FrameStats {
            decode_ms: pending.decode_ms,
            raster_ms: pending.raster_ms,
            composite_ms,
            effect_passes: pending.effect_passes,
            effect_pass_bind_groups: pending.effect_pass_bind_groups,
        };
        let total_ms = pending.decode_ms + pending.raster_ms + composite_ms;
        if total_ms > SLOW_FRAME_LOG_MS {
            tracing::info!(
                decode_ms = %format_args!("{:.1}", pending.decode_ms),
                raster_ms = %format_args!("{:.1}", pending.raster_ms),
                composite_ms = %format_args!("{composite_ms:.1}"),
                layers = pending.layers,
                width = pending.width,
                height = pending.height,
                "slow frame render: {total_ms:.0} ms"
            );
        } else {
            tracing::trace!(
                decode_ms = %format_args!("{:.1}", pending.decode_ms),
                raster_ms = %format_args!("{:.1}", pending.raster_ms),
                composite_ms = %format_args!("{composite_ms:.1}"),
                layers = pending.layers,
                "frame render: {total_ms:.1} ms"
            );
        }
    }
}

#[cfg(test)]
#[path = "scratch_tests.rs"]
mod scratch_tests;
