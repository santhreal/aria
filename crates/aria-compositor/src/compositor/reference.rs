//! A layer drawn up to its blend: content or motion-blur average, mask,
//! effect chain, and LUT. The canvas path then applies styles and blends the
//! result; a reference draws it into the target an effect reads through a
//! layer parameter.

use crate::color_pass;
use crate::effect_render::{
    OffscreenPool, blit_premultiplied_to_target, effects_need_offscreen, run_effect_chain,
    run_lut_pass,
};
use crate::error::CompositorError;
use crate::gpu::GpuContext;
use crate::layer::{CompositeLayer, CompositorConfig, CompositorLayer};

use super::draw_layer::TargetPass;
use super::pairs::{MatteDraw, TransitionDraw};
use super::render::draw_built_layer;
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

impl Compositor {
    /// Draw `built` into `view` through its pipeline, clearing `view` first
    /// when `clear` is `Some`. A layer that overwrites its target draws onto
    /// it unloaded instead.
    pub(super) fn draw_layer_into(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        clear: Option<wgpu::Color>,
        built: &LayerGpu,
    ) {
        let pipeline = self.pipelines.layer_pipeline(&built.pipeline);
        let mut pass = if built.pipeline.overwrites_target() {
            // SAFETY: the pass's only draw is `built`, whose pipeline writes
            // every pixel of the target with blending disabled.
            #[allow(unsafe_code)]
            unsafe {
                color_pass::begin_overwrite(encoder, view)
            }
        } else {
            let load = match clear {
                Some(color) => wgpu::LoadOp::Clear(color),
                None => wgpu::LoadOp::Load,
            };
            color_pass::begin(encoder, view, load)
        };
        draw_built_layer(&mut pass, pipeline, built);
    }

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
    #[allow(clippy::too_many_arguments)]
    pub(super) fn draw_reference(
        &self,
        gpu: &GpuContext,
        encoder: &mut wgpu::CommandEncoder,
        config: &CompositorConfig,
        offscreen: &OffscreenPool,
        clip: u64,
        item: &CompositorLayer<'_>,
        caller_target: &mut TargetPass<'_>,
    ) -> Result<(), CompositorError> {
        let Some(view) = self.references.view(clip) else {
            return Ok(());
        };
        let clear = self.references.take_clear(clip);
        let mut target = TargetPass::new(None, view, clear);
        match *item {
            CompositorLayer::Layer(layer) => {
                self.draw_layer_reference(
                    gpu,
                    encoder,
                    config,
                    offscreen,
                    layer,
                    &mut target,
                    caller_target,
                )?;
            }
            CompositorLayer::Matte {
                content,
                stencil,
                read,
                invert,
                softness,
            } => {
                caller_target.flush_if_reading_offscreen(encoder, &self.pipelines);
                let draw = MatteDraw {
                    content,
                    stencil,
                    read,
                    invert,
                    softness,
                };
                self.draw_matte(gpu, encoder, config, offscreen, &draw, &mut target)?;
            }
            CompositorLayer::Transition {
                outgoing,
                incoming,
                transition_id,
                progress,
            } => {
                caller_target.flush_if_reading_offscreen(encoder, &self.pipelines);
                let draw = TransitionDraw {
                    outgoing,
                    incoming,
                    transition_id,
                    progress,
                    params: &[],
                };
                self.draw_transition(gpu, encoder, config, offscreen, &draw, &mut target)?;
            }
            CompositorLayer::TransitionWithParams {
                outgoing,
                incoming,
                transition_id,
                progress,
                params,
            } => {
                caller_target.flush_if_reading_offscreen(encoder, &self.pipelines);
                let draw = TransitionDraw {
                    outgoing,
                    incoming,
                    transition_id,
                    progress,
                    params,
                };
                self.draw_transition(gpu, encoder, config, offscreen, &draw, &mut target)?;
            }
            CompositorLayer::CanvasPass { .. }
            | CompositorLayer::Reference { .. }
            | CompositorLayer::Nest { .. } => {}
        }
        target.flush(encoder, &self.pipelines);
        Ok(())
    }

    /// Draw `layer` up to its blend over the reference target `target`.
    #[allow(clippy::too_many_arguments)]
    fn draw_layer_reference<'a>(
        &'a self,
        gpu: &GpuContext,
        encoder: &mut wgpu::CommandEncoder,
        config: &CompositorConfig,
        offscreen: &OffscreenPool,
        layer: &CompositeLayer<'_>,
        target: &mut TargetPass<'a>,
        caller_target: &mut TargetPass<'_>,
    ) -> Result<(), CompositorError> {
        if draws_content_direct(layer) {
            let built = self.build_layer(gpu, encoder, config, layer)?;
            target.push_layer(built);
            return Ok(());
        }
        caller_target.flush_if_reading_offscreen(encoder, &self.pipelines);
        let result = self.layer_result(gpu, encoder, config, offscreen, layer)?;
        let slot_mask = offscreen.index_of(result).map_or(0, |s| 1u8 << s);
        blit_premultiplied_to_target(
            &gpu.device,
            &self.pass_registry,
            offscreen.passes(),
            result,
            slot_mask,
            target,
        );
        Ok(())
    }
}
