//! A layer drawn up to its blend: content or motion-blur average, mask,
//! effect chain, and LUT. The canvas path then applies styles and blends the
//! result; a reference draws it into the target an effect reads through a
//! layer parameter.

use crate::effect_render::{
    OffscreenPool, blit_premultiplied_to_canvas, effects_need_offscreen, run_effect_chain,
    run_lut_pass,
};
use crate::error::CompositorError;
use crate::gpu::GpuContext;
use crate::layer::{CompositeLayer, CompositorConfig, CompositorLayer};

use super::draw_layer::BlendDest;
use super::pairs::{MatteDraw, TransitionDraw};
use super::render::{draw_built_layer, pipeline_for};
use super::{Compositor, LayerGpu};

/// Whether `layer` draws its content straight onto a target: no mask, effect
/// chain that reads offscreen, LUT, or motion-blur samples. Styles and blend
/// mode are the caller's to check, since a reference target skips both.
pub(super) fn draws_content_direct(layer: &CompositeLayer<'_>) -> bool {
    layer.stencil.is_none()
        && !effects_need_offscreen(layer.effects)
        && layer.lut.is_none()
        && layer.blur_passes.is_empty()
}

/// Draw `built` into `view`, clearing it first when `clear` is `Some`.
pub(super) fn draw_layer_into(
    encoder: &mut wgpu::CommandEncoder,
    _label: &'static str,
    view: &wgpu::TextureView,
    clear: Option<wgpu::Color>,
    pipeline: &wgpu::RenderPipeline,
    built: &LayerGpu,
) {
    let load = match clear {
        Some(color) => wgpu::LoadOp::Clear(color),
        None => wgpu::LoadOp::Load,
    };
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: None,
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load,
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    draw_built_layer(&mut pass, pipeline, built);
}

impl Compositor {
    /// Draw `layer`'s content, or the average of its motion-blur samples, cut
    /// by its mask into pool slot 0, run its effect chain and LUT, and return
    /// the pool view holding the result.
    pub(super) fn layer_result<'p>(
        &self,
        gpu: &GpuContext,
        encoder: &mut wgpu::CommandEncoder,
        config: &CompositorConfig,
        offscreen: &'p OffscreenPool,
        layer: &CompositeLayer<'_>,
    ) -> Result<&'p wgpu::TextureView, CompositorError> {
        let device = &gpu.device;
        match &layer.stencil {
            Some(stencil) => {
                self.draw_masked_content(gpu, encoder, config, offscreen, layer, stencil)?;
            }
            None => {
                self.draw_samples(
                    gpu,
                    encoder,
                    config,
                    offscreen,
                    layer,
                    layer.blur_passes,
                    0,
                    1,
                )?;
            }
        }
        let mut result = run_effect_chain(
            device,
            encoder,
            &self.pass_registry,
            offscreen,
            &self.references,
            offscreen.view(0),
            layer.effects,
            config.width,
            config.height,
        )?;
        if let Some(lut) = &layer.lut {
            let uploaded = self.lut_cache.peek(lut.key).expect("LUT preloaded");
            let lut_output = if std::ptr::eq(result, offscreen.view(1)) {
                offscreen.view(2)
            } else {
                offscreen.view(1)
            };
            result = run_lut_pass(
                device,
                encoder,
                &self.pass_registry,
                offscreen.passes(),
                result,
                lut_output,
                &uploaded.view,
                uploaded.size,
                uploaded.domain_min,
                uploaded.domain_max,
                lut.intensity,
            );
        }
        Ok(result)
    }

    /// Draw `item` over the reference target of `clip`. A clip no effect of
    /// this frame reads has no target and draws nothing.
    pub(super) fn draw_reference(
        &self,
        gpu: &GpuContext,
        encoder: &mut wgpu::CommandEncoder,
        config: &CompositorConfig,
        offscreen: &OffscreenPool,
        clip: u64,
        item: &CompositorLayer<'_>,
    ) -> Result<(), CompositorError> {
        let Some(view) = self.references.view(clip) else {
            return Ok(());
        };
        let dest = BlendDest {
            view,
            texture: None,
        };
        match *item {
            CompositorLayer::Layer(layer) => {
                self.draw_layer_reference(gpu, encoder, config, offscreen, layer, view)
            }
            CompositorLayer::Matte {
                content,
                stencil,
                read,
                invert,
                softness,
            } => {
                let draw = MatteDraw {
                    content,
                    stencil,
                    read,
                    invert,
                    softness,
                };
                self.draw_matte(gpu, encoder, config, offscreen, &draw, &dest)
            }
            CompositorLayer::Transition {
                outgoing,
                incoming,
                transition_id,
                progress,
            } => {
                let draw = TransitionDraw {
                    outgoing,
                    incoming,
                    transition_id,
                    progress,
                    params: &[],
                };
                self.draw_transition(gpu, encoder, config, offscreen, &draw, &dest)
            }
            CompositorLayer::TransitionWithParams {
                outgoing,
                incoming,
                transition_id,
                progress,
                params,
            } => {
                let draw = TransitionDraw {
                    outgoing,
                    incoming,
                    transition_id,
                    progress,
                    params,
                };
                self.draw_transition(gpu, encoder, config, offscreen, &draw, &dest)
            }
            CompositorLayer::CanvasPass { .. }
            | CompositorLayer::Reference { .. }
            | CompositorLayer::Nest { .. } => Ok(()),
        }
    }

    /// Draw `layer` up to its blend over the reference target `view`.
    fn draw_layer_reference(
        &self,
        gpu: &GpuContext,
        encoder: &mut wgpu::CommandEncoder,
        config: &CompositorConfig,
        offscreen: &OffscreenPool,
        layer: &CompositeLayer<'_>,
        view: &wgpu::TextureView,
    ) -> Result<(), CompositorError> {
        if draws_content_direct(layer) {
            let built = self.build_layer(gpu, config, layer)?;
            let pipeline = pipeline_for(&built.pipeline, self);
            draw_layer_into(encoder, "aria.reference", view, None, pipeline, &built);
            return Ok(());
        }
        let result = self.layer_result(gpu, encoder, config, offscreen, layer)?;
        blit_premultiplied_to_canvas(
            &gpu.device,
            encoder,
            &self.pass_registry,
            offscreen.passes(),
            result,
            view,
        );
        Ok(())
    }
}
