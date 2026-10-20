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
use layers::{Leaf, Nests, Pixels, Realized};

use aria_compositor::{
    ColorGrade, CompositeLayer, CompositorConfig, CompositorLayer, FrameSink, LayerPlacement,
    LayerStencil, MatteRead, PassInstance, wgpu,
};
use aria_core::Cancel;
use aria_models::{MatteSource, Project};

use crate::error::RenderError;
use crate::scene::{LayerSource, Scene, SceneLayer, SceneLut, SizeSpec};

use super::effects::layer_effects;
use super::look_ahead::Shown;
use super::media_cache::layer_lut;
use super::{FrameStats, PendingFrame, Renderer, SLOW_FRAME_LOG_MS, SeekPolicy};

/// Where a submitted frame's canvas goes.
#[derive(Clone, Copy)]
pub(in crate::render) enum Output<'t> {
    /// A readback buffer the CPU maps when the frame finishes.
    Readback,
    /// A readback buffer unless the realized scene is a single unmodified
    /// full-canvas CPU NV12 media frame matching the canvas size.
    ExportReadback { allow_nv12_passthrough: bool },
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

    /// Move every decoded frame out of `realized`, for its decoder to keep.
    fn take_shown(&mut self, mut keep: impl FnMut(Shown)) {
        let mut leaf = |leaf: Leaf| {
            if let Pixels::Frame(shown) = leaf.pixels {
                keep(shown);
            }
        };
        for realized in self.realized.drain(..) {
            match realized {
                Realized::Layer(layer) => leaf(layer),
                Realized::Transition {
                    outgoing, incoming, ..
                } => {
                    leaf(*outgoing);
                    leaf(*incoming);
                }
                Realized::Matte {
                    content, stencil, ..
                } => {
                    leaf(*content);
                    leaf(*stencil);
                }
                Realized::CanvasPass { .. } => {}
            }
        }
    }
}

/// Fixed-capacity stack buffer with transparent `Vec` spill for per-frame
/// realization tables, avoiding heap allocation on typical timelines while
/// supporting arbitrary layer counts.
struct InlineVec<T: Copy, const N: usize> {
    inline: [T; N],
    len: usize,
    heap: Vec<T>,
}

impl<T: Copy, const N: usize> InlineVec<T, N> {
    fn new(empty: T) -> Self {
        Self {
            inline: [empty; N],
            len: 0,
            heap: Vec::new(),
        }
    }

    fn len(&self) -> usize {
        if self.heap.is_empty() {
            self.len
        } else {
            self.heap.len()
        }
    }

    fn push(&mut self, item: T) {
        if !self.heap.is_empty() {
            self.heap.push(item);
        } else if self.len < N {
            self.inline[self.len] = item;
            self.len += 1;
        } else {
            self.heap.reserve(N * 2);
            self.heap.extend_from_slice(&self.inline[..self.len]);
            self.heap.push(item);
        }
    }

    fn as_slice(&self) -> &[T] {
        if self.heap.is_empty() {
            &self.inline[..self.len]
        } else {
            &self.heap
        }
    }
}

impl<T: Copy, const N: usize> std::ops::Index<usize> for InlineVec<T, N> {
    type Output = T;

    fn index(&self, index: usize) -> &Self::Output {
        &self.as_slice()[index]
    }
}

impl<T: Copy, const N: usize> std::ops::Index<Range<usize>> for InlineVec<T, N> {
    type Output = [T];

    fn index(&self, index: Range<usize>) -> &Self::Output {
        &self.as_slice()[index]
    }
}

impl<T: Copy, const N: usize> Extend<T> for InlineVec<T, N> {
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        for item in iter {
            self.push(item);
        }
    }
}

/// Whether `scene` is one decoded CPU NV12 BT.709 media frame drawn unchanged
/// over the whole canvas, so the export encoder takes the decoded planes and
/// no GPU composite, readback, or RGBA to NV12 conversion runs.
///
/// Every [`Scene`] and [`SceneLayer`] field is named, so a field added later
/// fails to compile here until its passthrough value is stated.
fn scene_is_nv12_passthrough(scene: &Scene, nests: &Nests<'_>, scratch: &RealizeScratch) -> bool {
    let Scene {
        width,
        height,
        background: _,
        tick: _,
        layers,
        references,
    } = scene;
    if !references.is_empty() || !nests.scenes.is_empty() {
        return false;
    }
    let [layer] = layers.as_slice() else {
        return false;
    };
    let SceneLayer {
        clip: _,
        source,
        center,
        anchor_point,
        size,
        rotation,
        opacity,
        uv,
        effects,
        masks,
        chroma_key,
        color_grade,
        lut,
        blend_mode,
        styles,
        blur_passes,
    } = layer;
    let (w, h) = (*width as f32, *height as f32);
    let unchanged = matches!(source, LayerSource::Media { .. })
        && *center == [w * 0.5, h * 0.5]
        && *anchor_point == [0.5, 0.5]
        && *size == SizeSpec::Fixed([w, h])
        && *rotation == 0.0
        && *opacity == 1.0
        && *uv == [0.0, 0.0, 1.0, 1.0]
        && effects.is_empty()
        && masks.is_empty()
        && chroma_key.is_none()
        && color_grade.is_none()
        && lut.is_none()
        && *blend_mode == aria_models::BlendMode::Normal
        && styles.is_none()
        && blur_passes.is_empty();
    if !unchanged {
        return false;
    }
    let [
        Realized::Layer(Leaf {
            pixels: Pixels::Frame(shown),
            ..
        }),
    ] = scratch.realized.as_slice()
    else {
        return false;
    };
    let frame = &shown.frame;
    frame.format == aria_core::PixelFormat::Nv12
        && frame.visible == aria_core::Rect::from_size(*width, *height)
        && frame.rotation == aria_core::Rotation::None
        && frame.color == aria_core::ColorSpace::BT709
        && matches!(frame.data, aria_core::FrameData::Cpu(_))
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
        if self.text_memo.len() > 32 {
            self.text_memo.clear();
        }
        let mut nests = Nests::new(scene);
        let decode_ms =
            self.realize_scene_layers(project, scene, policy, &cancel, &mut nests, scratch)?;
        self.retire_decoders();
        if matches!(
            output,
            Output::ExportReadback {
                allow_nv12_passthrough: true
            }
        ) && scene_is_nv12_passthrough(scene, &nests, scratch)
        {
            if cancel.is_cancelled() {
                return Err(RenderError::Cancelled);
            }
            let realize_ms = realize_started.elapsed().as_secs_f64() * 1000.0;
            return Ok(PendingFrame {
                readback: None,
                scratch: None,
                decode_ms,
                raster_ms: (realize_ms - decode_ms).max(0.0),
                submitted_at: Instant::now(),
                effect_passes: 0,
                effect_pass_bind_groups: 0,
                layers: 1,
                width: scene.width,
                height: scene.height,
            });
        }
        let realized = &scratch.realized;
        let caches = Caches {
            stills: &self.stills,
            stickers: &self.stickers,
            lottie: &self.lottie,
            luts: &self.luts,
            text_memo: &self.text_memo,
        };
        let mut layer_storage: InlineVec<CompositeLayer<'_>, 16> =
            InlineVec::new(CompositeLayer::EMPTY);
        // The masks of canvas passes, which a pass holds by reference.
        let mut stencils: InlineVec<LayerStencil<'_>, 8> = InlineVec::new(LayerStencil::EMPTY);
        // Phase 1: build all composite layers (indices only for transitions).
        #[derive(Clone, Copy)]
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
        let mut jobs: InlineVec<(Option<u64>, LayerJob<'_>), 16> =
            InlineVec::new((None, LayerJob::Plain { storage_idx: 0 }));
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
        let mut items: InlineVec<CompositorLayer<'_>, 16> =
            InlineVec::new(CompositorLayer::Layer(&CompositeLayer::EMPTY));
        for (_, job) in jobs.as_slice() {
            let item = match job {
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
            };
            items.push(item);
        }
        // A reference wraps the item it draws into a clip's target, and every
        // reference of a level is submitted before the items whose effects
        // read it. A level keeps the range its jobs hold.
        let mut ordered: InlineVec<CompositorLayer<'_>, 16> =
            InlineVec::new(CompositorLayer::Layer(&CompositeLayer::EMPTY));
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
        let mut nested: InlineVec<CompositorLayer<'_>, 16> =
            InlineVec::new(CompositorLayer::Layer(&CompositeLayer::EMPTY));
        let compositor_layers: &[CompositorLayer<'_>] = match scratch.item_levels.split_first() {
            Some((top, inner)) if !inner.is_empty() => {
                nested.extend(
                    inner
                        .iter()
                        .enumerate()
                        .rev()
                        .map(|(nest, level)| CompositorLayer::Nest {
                            nest: nest as u64,
                            items: &ordered[level.clone()],
                        })
                        .chain(ordered[top.clone()].iter().copied()),
                );
                nested.as_slice()
            }
            _ => ordered.as_slice(),
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
            Output::Readback | Output::ExportReadback { .. } => Some(
                self.compositor
                    .submit_compositor_layers(&self.gpu, &config, compositor_layers)?,
            ),
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

    pub(crate) fn finish_pending_into(
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
            self.return_scratch(scratch);
        }
        result?;
        let composite_ms = pending.submitted_at.elapsed().as_secs_f64() * 1000.0;
        self.record_pending_stats(&pending, composite_ms);
        Ok(())
    }

    pub(crate) fn finish_pending_passthrough(
        &mut self,
        mut pending: PendingFrame,
        pts: aria_core::RationalTime,
        push: impl FnOnce(&aria_core::VideoFrame) -> Result<(), aria_core::EncodeError>,
    ) -> Result<(), RenderError> {
        let mut scratch = pending
            .scratch
            .take()
            .expect("a passthrough pending frame retains its scratch");
        let result = match scratch.realized.first_mut() {
            Some(Realized::Layer(Leaf {
                pixels: Pixels::Frame(shown),
                ..
            })) => {
                let prev_pts = shown.frame.pts;
                let prev_dur = shown.frame.duration;
                shown.frame.pts = pts;
                shown.frame.duration = None;
                let res = push(&shown.frame);
                shown.frame.pts = prev_pts;
                shown.frame.duration = prev_dur;
                res
            }
            _ => unreachable!("passthrough pending frame holds one realized media frame"),
        };
        self.return_scratch(scratch);
        result?;
        let composite_ms = pending.submitted_at.elapsed().as_secs_f64() * 1000.0;
        self.record_pending_stats(&pending, composite_ms);
        Ok(())
    }

    /// Finish a frame written to a texture: nothing maps, so the scratch
    /// returns and the stats record the submission.
    pub(super) fn finish_pending_on_gpu(&mut self, mut pending: PendingFrame) {
        if let Some(scratch) = pending.scratch.take() {
            self.return_scratch(scratch);
        }
        let composite_ms = pending.submitted_at.elapsed().as_secs_f64() * 1000.0;
        self.record_pending_stats(&pending, composite_ms);
    }

    /// Take back the scratch of a finished frame, returning the frames it drew
    /// to their decoders (see [`Renderer::keep_shown`]).
    fn return_scratch(&mut self, mut scratch: RealizeScratch) {
        scratch.take_shown(|shown| self.keep_shown(shown));
        self.realize_scratch = scratch;
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
