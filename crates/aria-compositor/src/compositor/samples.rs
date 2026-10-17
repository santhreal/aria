//! A layer drawn into one pool slot, once or as the average of its
//! motion-blur samples.

use crate::effect_render::{OffscreenPool, blit_weighted_additive};
use crate::error::CompositorError;
use crate::gpu::GpuContext;
use crate::layer::{BlendMode, CompositeLayer, CompositorConfig, LayerPlacement, LayerStyles};

use super::Compositor;
use super::draw_layer::clear_view;
use super::reference::draw_layer_into;
use super::render::pipeline_for;

impl Compositor {
    /// Clear pool slot `into` and draw `layer` into it at its placement, or,
    /// when `passes` is non-empty, at each placement in `passes` at weight
    /// `1/N`, drawing each sample into slot `scratch` first.
    ///
    /// Only the content is sampled: the effect chain, LUT, styles and blend
    /// mode run once on the average.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn draw_samples(
        &self,
        gpu: &GpuContext,
        encoder: &mut wgpu::CommandEncoder,
        config: &CompositorConfig,
        offscreen: &OffscreenPool,
        layer: &CompositeLayer<'_>,
        passes: &[LayerPlacement],
        into: usize,
        scratch: usize,
    ) -> Result<(), CompositorError> {
        if passes.is_empty() {
            let built = self.build_layer(gpu, config, layer)?;
            let pipeline = pipeline_for(&built.pipeline, self);
            draw_layer_into(
                encoder,
                "aria.offscreen.base",
                offscreen.view(into),
                true,
                pipeline,
                &built,
            );
            return Ok(());
        }
        clear_view(encoder, "aria.motion_blur.clear", offscreen.view(into));
        let weight = 1.0 / passes.len() as f32;
        for placement in passes {
            let sample = content_at(layer, *placement);
            let built = self.build_layer(gpu, config, &sample)?;
            let pipeline = pipeline_for(&built.pipeline, self);
            draw_layer_into(
                encoder,
                "aria.motion_blur.sample",
                offscreen.view(scratch),
                true,
                pipeline,
                &built,
            );
            blit_weighted_additive(
                &gpu.device,
                encoder,
                &self.pass_registry,
                offscreen.passes(),
                offscreen.view(scratch),
                offscreen.view(into),
                weight,
            );
        }
        Ok(())
    }
}

/// `layer`'s content, grade and fx at `placement`, with nothing that runs once
/// on the average of its samples.
fn content_at<'a>(layer: &CompositeLayer<'a>, placement: LayerPlacement) -> CompositeLayer<'a> {
    CompositeLayer {
        content: layer.content,
        placement,
        uv: layer.uv,
        effects: &[],
        fx: layer.fx,
        color_grade: layer.color_grade,
        lut: None,
        blend_mode: BlendMode::Normal,
        styles: LayerStyles::default(),
        blur_passes: &[],
        stencil: None,
    }
}
