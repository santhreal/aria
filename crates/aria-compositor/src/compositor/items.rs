//! One item of a frame drawn onto its destination: the canvas, or the target
//! of a nested sequence.
//!
//! A [`CompositorLayer::Nest`] draws its items onto a canvas-sized target of
//! its own, cleared to transparent at the start of the frame, the same way the
//! frame draws its items onto the canvas. A layer whose content is
//! [`LayerContent::Nest`](crate::LayerContent::Nest) then samples that target
//! through the RGBA pipelines, so the nested picture takes the layer's
//! placement, mask, effects, grade, LUT, styles, and blend mode.

use crate::effect_render::OffscreenPool;
use crate::error::CompositorError;
use crate::gpu::GpuContext;
use crate::grade::ColorGrade;
use crate::layer::{CompositeLayer, CompositorConfig, CompositorLayer};

use super::canvas_pass::CanvasPassDraw;
use super::draw_layer::{BlendDest, LayerTarget};
use super::pairs::{MatteDraw, TransitionDraw};
use super::scene::Scene;
use super::{Compositor, LayerGpu};
use crate::color_pass;

/// The texture an item draws onto and its view.
pub(super) type Dest<'a> = (&'a wgpu::Texture, &'a wgpu::TextureView);

/// Every item of `scene`, each nest followed by the items it draws.
pub(super) fn frame_items<'a>(scene: &Scene<'a>) -> impl Iterator<Item = CompositorLayer<'a>> {
    scene.iter().flat_map(|item| {
        let nested: &'a [CompositorLayer<'a>] = match item {
            CompositorLayer::Nest { items, .. } => items,
            _ => &[],
        };
        std::iter::once(item).chain(nested.iter().copied())
    })
}

impl Compositor {
    /// Draw `item` onto `dest`. A nest draws nothing here: the frame draws
    /// each nest at its top level through [`Self::draw_nest`]. Inside a nest
    /// (`in_nest`), a reference clears its target before it draws, so a
    /// sequence drawn twice in one frame reads its own frame each time.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn draw_item(
        &self,
        gpu: &GpuContext,
        encoder: &mut wgpu::CommandEncoder,
        config: &CompositorConfig,
        offscreen: &OffscreenPool,
        item: &CompositorLayer<'_>,
        (texture, view): Dest<'_>,
        in_nest: bool,
        pending_clear: &mut Option<wgpu::Color>,
    ) -> Result<(), CompositorError> {
        // A pair drawn onto the destination composites with its layers' blend
        // modes.
        let blend = BlendDest {
            view,
            texture: Some(texture),
        };
        match *item {
            CompositorLayer::Layer(layer) => {
                let onto = LayerTarget {
                    texture,
                    view,
                    clear: pending_clear.take(),
                };
                self.draw_layer_onto(gpu, encoder, config, offscreen, layer, &onto)
            }
            CompositorLayer::CanvasPass {
                effects,
                grade,
                lut,
                stencil,
            } => {
                if let Some(color) = pending_clear.take() {
                    color_pass::clear(encoder, view, color);
                }
                let pass = CanvasPassDraw {
                    effects,
                    grade,
                    lut,
                    stencil,
                };
                self.draw_canvas_pass(gpu, encoder, config, offscreen, view, &pass)
            }
            CompositorLayer::Reference { clip, item } => {
                if in_nest {
                    self.references.clear_one(encoder, clip);
                }
                self.draw_reference(gpu, encoder, config, offscreen, clip, item)
            }
            CompositorLayer::Transition {
                outgoing,
                incoming,
                transition_id,
                progress,
            } => {
                if let Some(color) = pending_clear.take() {
                    color_pass::clear(encoder, view, color);
                }
                let draw = TransitionDraw {
                    outgoing,
                    incoming,
                    transition_id,
                    progress,
                    params: &[],
                };
                self.draw_transition(gpu, encoder, config, offscreen, &draw, &blend)
            }
            CompositorLayer::TransitionWithParams {
                outgoing,
                incoming,
                transition_id,
                progress,
                params,
            } => {
                if let Some(color) = pending_clear.take() {
                    color_pass::clear(encoder, view, color);
                }
                let draw = TransitionDraw {
                    outgoing,
                    incoming,
                    transition_id,
                    progress,
                    params,
                };
                self.draw_transition(gpu, encoder, config, offscreen, &draw, &blend)
            }
            CompositorLayer::Matte {
                content,
                stencil,
                read,
                invert,
                softness,
            } => {
                if let Some(color) = pending_clear.take() {
                    color_pass::clear(encoder, view, color);
                }
                let draw = MatteDraw {
                    content,
                    stencil,
                    read,
                    invert,
                    softness,
                };
                self.draw_matte(gpu, encoder, config, offscreen, &draw, &blend)
            }
            CompositorLayer::Nest { .. } => Ok(()),
        }
    }

    /// Draw `items` onto the target of `nest`, which the frame cleared when
    /// it began.
    pub(super) fn draw_nest(
        &self,
        gpu: &GpuContext,
        encoder: &mut wgpu::CommandEncoder,
        config: &CompositorConfig,
        offscreen: &OffscreenPool,
        nest: u64,
        items: &[CompositorLayer<'_>],
    ) -> Result<(), CompositorError> {
        let Some(dest) = self.nests.target(nest) else {
            return Err(CompositorError::UndrawnNest(nest));
        };
        for item in items {
            self.draw_item(gpu, encoder, config, offscreen, item, dest, true, &mut None)?;
        }
        Ok(())
    }

    /// Bind the target of `nest` as the content of `layer`.
    pub(super) fn build_nest(
        &self,
        gpu: &GpuContext,
        config: &CompositorConfig,
        layer: &CompositeLayer<'_>,
        nest: u64,
        grade: Option<ColorGrade>,
    ) -> Result<LayerGpu, CompositorError> {
        let Some((texture, view)) = self.nests.target(nest) else {
            return Err(CompositorError::UndrawnNest(nest));
        };
        let target = (texture.clone(), view.clone());
        Ok(self.bind_rgba(gpu, config, layer, target, grade))
    }
}
