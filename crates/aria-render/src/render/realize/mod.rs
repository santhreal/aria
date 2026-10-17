use std::ops::Range;
use std::time::Instant;

#[cfg(test)]
mod conformance;
// `composite.rs` is the realized-layer → borrowed-`CompositeLayer` conversion.
mod composite;
mod layers;
mod pass_one;
mod text;

use composite::{Caches, composite_card, composite_leaf};
use layers::{Nests, Realized};

use aria_compositor::{
    ColorGrade, CompositeLayer, CompositorConfig, CompositorLayer, FrameSink, ImageSink,
    LayerPlacement, LayerStencil, MatteRead, PassInstance, wgpu,
};
use aria_core::Cancel;
use aria_models::{MatteSource, Project};

use crate::error::RenderError;
use crate::scene::{LayerSource, Scene, SceneLayer, SceneLut};

use super::effects::layer_effects;
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
    /// The range of `realized` each level holds: the frame's scene, then the
    /// scene of each nest.
    levels: Vec<Range<usize>>,
    /// The range of a frame's compositor items each level holds, in the same
    /// order.
    item_levels: Vec<Range<usize>>,
}

impl RealizeScratch {
    fn begin_frame(&mut self) {
        self.realized.clear();
        self.scene_indices.clear();
        self.levels.clear();
        self.item_levels.clear();
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
        // New scene, new stamp: Lottie frames and decoders touched below are
        // eviction-exempt until the next scene.
        self.scene_stamp += 1;
        // First pass: decode/rasterize each layer into owned pixels and a final
        // placement. Held in `realized` so the borrowed `CompositeLayer`s built
        // below stay valid through the composite call.
        scratch.begin_frame();
        let mut nests = Nests::new(scene);
        let decode_ms =
            self.realize_scene_layers(project, scene, policy, &cancel, &mut nests, scratch)?;
        self.retire_decoders();
        let realized = &scratch.realized;
        let caches = Caches {
            stills: &self.stills,
            stickers: &self.stickers,
            lottie: &self.lottie,
            luts: &self.luts,
        };
        let mut layer_storage: Vec<CompositeLayer<'_>> = Vec::new();
        // The masks of canvas passes, which a pass holds by reference.
        let mut stencils: Vec<LayerStencil<'_>> = Vec::new();
        // Phase 1: build all composite layers (indices only for transitions).
        enum LayerJob<'a> {
            Plain {
                storage_idx: usize,
            },
            CanvasPass {
                effects: &'a [PassInstance],
                grade: Option<ColorGrade>,
                lut: &'a Option<SceneLut>,
                stencil: Option<usize>,
            },
            Transition {
                out_idx: usize,
                in_idx: usize,
                transition_id: &'a str,
                progress: f32,
                params: &'a [f32],
            },
            Matte {
                content_idx: usize,
                stencil_idx: usize,
                read: MatteSource,
                invert: bool,
                softness: f32,
            },
        }
        // Each job with the clip whose reference target it draws into, `None`
        // for a job drawn onto the canvas.
        let mut jobs: Vec<(Option<u64>, LayerJob<'_>)> = Vec::new();

        let level_scenes = std::iter::once(scene).chain(nests.scenes.iter().copied());
        for (level_scene, level) in level_scenes.zip(&scratch.levels) {
            let start = jobs.len();
            for (r, &scene_idx) in realized[level.clone()]
                .iter()
                .zip(&scratch.scene_indices[level.clone()])
            {
                let scene_layer = level_scene.drawn(scene_idx);
                let reference = match scene_layer.clip {
                    _ if scene_idx >= level_scene.references.len() => None,
                    Some(clip) => Some(clip.raw()),
                    // The resolver places a reference only for a clip.
                    None => continue,
                };
                match r {
                    Realized::CanvasPass {
                        grade,
                        lut,
                        stencil,
                    } => {
                        let stencil = stencil.as_ref().map(|stencil| {
                            stencils.push(stencil.borrow(&[]));
                            stencils.len() - 1
                        });
                        jobs.push((
                            reference,
                            LayerJob::CanvasPass {
                                effects: &scene_layer.effects,
                                grade: *grade,
                                lut,
                                stencil,
                            },
                        ));
                    }
                    Realized::Transition {
                        outgoing,
                        incoming,
                        transition_id,
                        progress,
                    } => {
                        let (out_effects, in_effects) = transition_effects(scene_layer);
                        layer_storage.push(composite_leaf(outgoing, &caches, out_effects));
                        let out_idx = layer_storage.len() - 1;
                        layer_storage.push(composite_leaf(incoming, &caches, in_effects));
                        let in_idx = layer_storage.len() - 1;
                        let params = scene_layer
                            .effects
                            .first()
                            .map_or(&[][..], |pass| pass.params.as_slice());
                        jobs.push((
                            reference,
                            LayerJob::Transition {
                                out_idx,
                                in_idx,
                                transition_id: transition_id.as_str(),
                                progress: *progress,
                                params,
                            },
                        ));
                    }
                    Realized::Matte {
                        content,
                        stencil,
                        read,
                        invert,
                        softness,
                    } => {
                        let (content_effects, stencil_effects) = matte_effects(scene_layer);
                        layer_storage.push(composite_leaf(content, &caches, content_effects));
                        let content_idx = layer_storage.len() - 1;
                        layer_storage.push(composite_leaf(stencil, &caches, stencil_effects));
                        let stencil_idx = layer_storage.len() - 1;
                        jobs.push((
                            reference,
                            LayerJob::Matte {
                                content_idx,
                                stencil_idx,
                                read: *read,
                                invert: *invert,
                                softness: *softness,
                            },
                        ));
                    }
                    Realized::Layer(leaf) => {
                        let effects = scene_layer.effects.as_slice();
                        // A text run's background card draws behind its glyphs.
                        let card = composite_card(leaf, &caches, effects);
                        for layer in card
                            .into_iter()
                            .chain([composite_leaf(leaf, &caches, effects)])
                        {
                            layer_storage.push(layer);
                            jobs.push((
                                reference,
                                LayerJob::Plain {
                                    storage_idx: layer_storage.len() - 1,
                                },
                            ));
                        }
                    }
                }
            }
            scratch.item_levels.push(start..jobs.len());
        }

        // Phase 2: borrow storage immutably for compositor dispatch.
        let items: Vec<CompositorLayer<'_>> = jobs
            .iter()
            .map(|(_, job)| match job {
                LayerJob::Plain { storage_idx } => {
                    CompositorLayer::layer(&layer_storage[*storage_idx])
                }
                LayerJob::CanvasPass {
                    effects,
                    grade,
                    lut,
                    stencil,
                } => CompositorLayer::CanvasPass {
                    effects,
                    grade: *grade,
                    lut: layer_lut(lut, caches.luts),
                    stencil: stencil.map(|index| &stencils[index]),
                },
                LayerJob::Transition {
                    out_idx,
                    in_idx,
                    transition_id,
                    progress,
                    params,
                } => CompositorLayer::TransitionWithParams {
                    outgoing: &layer_storage[*out_idx],
                    incoming: &layer_storage[*in_idx],
                    transition_id,
                    progress: *progress,
                    params,
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
        // A reference wraps the item it draws into a clip's target, and every
        // reference of a level is submitted before the items whose effects
        // read it. A level keeps the range its jobs hold.
        let mut ordered: Vec<CompositorLayer<'_>> = Vec::with_capacity(items.len());
        for level in &scratch.item_levels {
            let level = || jobs[level.clone()].iter().zip(&items[level.clone()]);
            ordered.extend(level().filter_map(|((reference, _), item)| {
                reference.map(|clip| CompositorLayer::Reference { clip, item })
            }));
            ordered.extend(
                level()
                    .filter(|((reference, _), _)| reference.is_none())
                    .map(|(_, item)| *item),
            );
        }
        // Each nest draws before the level that shows it. A nest is realized
        // after every nest before it, so the nests a nest shows come later in
        // the list and are submitted first.
        let nested: Vec<CompositorLayer<'_>>;
        let compositor_layers: &[CompositorLayer<'_>] = match scratch.item_levels.split_first() {
            Some((top, inner)) if !inner.is_empty() => {
                nested = inner
                    .iter()
                    .enumerate()
                    .rev()
                    .map(|(nest, level)| CompositorLayer::Nest {
                        nest: nest as u64,
                        items: &ordered[level.clone()],
                    })
                    .chain(ordered[top.clone()].iter().copied())
                    .collect();
                &nested
            }
            _ => &ordered,
        };

        // Submission is the last cancellation boundary. The pending copy owns
        // no scene borrows, so a later frame can realize while this one maps.
        if cancel.is_cancelled() {
            return Err(RenderError::Cancelled);
        }
        let mut config =
            CompositorConfig::new(scene.width, scene.height).with_background(scene.background);
        if scene.tick.rate.is_valid() {
            config = config.with_frame_budget(std::time::Duration::from_secs_f64(
                scene.tick.rate.seconds_per_unit(),
            ));
        }
        let realize_ms = realize_started.elapsed().as_secs_f64() * 1000.0;
        let submitted_at = Instant::now();
        let readback = match output {
            Output::Readback => Some(self.compositor.submit_compositor_layers(
                &self.gpu,
                &config,
                compositor_layers,
            )?),
            Output::Texture(texture) => {
                self.compositor.submit_compositor_layers_to_texture(
                    &self.gpu,
                    &config,
                    compositor_layers,
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
