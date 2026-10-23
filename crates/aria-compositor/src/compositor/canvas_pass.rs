//! A canvas pass: an effect chain, grade and LUT run over everything drawn so
//! far. The result replaces the canvas, or replaces it inside a mask.

use crate::effect_render::{
    OffscreenPool, blit_replace, blit_replace_to_target, effects_need_offscreen, run_effect_chain,
    run_grade_pass, run_lut_pass,
};
use crate::error::CompositorError;
use crate::gpu::GpuContext;
use crate::grade::ColorGrade;
use crate::layer::{CompositorConfig, LayerLut, LayerStencil};
use crate::passes::PassInstance;

use super::Compositor;
use super::draw_layer::TargetPass;

/// A canvas pass as [`Compositor::draw_canvas_pass`] draws it.
#[derive(Clone, Copy)]
pub(super) struct CanvasPassDraw<'a> {
    pub(super) effects: &'a [PassInstance],
    pub(super) grade: Option<ColorGrade>,
    pub(super) lut: Option<LayerLut<'a>>,
    pub(super) stencil: Option<&'a LayerStencil<'a>>,
}

impl Compositor {
    /// Run `pass` over `target` and replace `target` with the result.
    pub(super) fn draw_canvas_pass<'a>(
        &'a self,
        gpu: &GpuContext,
        encoder: &mut wgpu::CommandEncoder,
        config: &CompositorConfig,
        offscreen: &OffscreenPool,
        target: &mut TargetPass<'a>,
        pass: &CanvasPassDraw<'_>,
    ) -> Result<(), CompositorError> {
        let CanvasPassDraw {
            effects,
            grade,
            lut,
            stencil,
        } = *pass;
        if !effects_need_offscreen(effects) && grade.is_none() && lut.is_none() {
            return Ok(());
        }
        let device = &gpu.device;
        target.flush(encoder, &self.pipelines);

        // Snapshot the current canvas, process it as a texture,
        // then replace the canvas. Alpha-over blits would double
        // the already-drawn stack.
        blit_replace(
            device,
            encoder,
            &self.pass_registry,
            offscreen.passes(),
            target.view,
            offscreen.view(0),
        );
        let mut result = if effects_need_offscreen(effects) {
            run_effect_chain(
                device,
                encoder,
                &self.pass_registry,
                offscreen,
                &self.references,
                offscreen.view(0),
                effects,
                config.width,
                config.height,
            )?
        } else {
            offscreen.view(0)
        };
        if let Some(grade) = grade {
            let grade_output = if std::ptr::eq(result, offscreen.view(0)) {
                offscreen.view(1)
            } else {
                offscreen.view(0)
            };
            result = run_grade_pass(
                device,
                encoder,
                &self.pass_registry,
                offscreen.passes(),
                result,
                grade_output,
                grade,
            );
        }
        if let Some(lut) = lut {
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
        let slot_mask = offscreen.index_of(result).map_or(0, |s| 1u8 << s);
        match stencil {
            Some(stencil) => {
                self.replace_masked(gpu, encoder, config, offscreen, result, target, stencil)?;
            }
            None => blit_replace_to_target(
                device,
                &self.pass_registry,
                offscreen.passes(),
                result,
                slot_mask,
                target,
            ),
        }
        Ok(())
    }
}
