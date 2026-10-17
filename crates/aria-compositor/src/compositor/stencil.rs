//! Masks: a stencil cuts a layer's content before its effect chain, and limits
//! where a canvas pass replaces the canvas.
//!
//! One mask draws its coverage and folds its op, opacity and inversion into
//! the remap the matte pass applies ([`CoverageMix::single`]). A stack of more
//! draws each mask's coverage into one slot and combines it with the stack
//! above it into one of two slots it alternates between, so a stack of any
//! length holds three slots.

use std::sync::LazyLock;

use aria_core::RgbaImage;

use crate::effect_render::{
    OffscreenPool, StackStep, blit_replace, blit_weighted_additive, run_mask_stack_pass,
    run_matte_pass,
};
use crate::error::CompositorError;
use crate::gpu::GpuContext;
use crate::layer::{
    CompositeLayer, CompositorConfig, CoverageMix, LayerEffects, LayerStencil, MatteRead,
    StencilCoverage, StencilMask,
};

use super::Compositor;

/// One opaque white texel. Stretched over a placed quad and cut by a shape in
/// the fx pipeline, it draws that shape's coverage for any layer content,
/// including content drawn by a pipeline with no fx variant.
static WHITE: LazyLock<RgbaImage> = LazyLock::new(|| RgbaImage::new(1, 1, vec![255; 4]));

/// The layer that draws `mask`'s coverage, before its inversion and mix.
fn coverage_layer<'a>(
    stencil: &LayerStencil<'a>,
    mask: &StencilMask,
) -> Result<CompositeLayer<'a>, CompositorError> {
    Ok(match mask.coverage {
        StencilCoverage::Image(index) => {
            let image = stencil
                .images
                .get(index)
                .ok_or(CompositorError::StencilImage {
                    index,
                    images: stencil.images.len(),
                })?;
            CompositeLayer::rgba(image, stencil.placement)
        }
        StencilCoverage::Shape(shape) => {
            CompositeLayer::rgba(&WHITE, stencil.placement).with_fx(LayerEffects {
                mask: Some(shape),
                chroma_key: None,
            })
        }
    })
}

/// Where a stencil's combined coverage ended up, and how the matte pass
/// reads it.
struct Coverage {
    slot: usize,
    mix: CoverageMix,
    softness: f32,
}

impl Compositor {
    /// Draw `stencil`'s combined coverage through pool slots `slots`, which
    /// must be distinct, and report the slot that holds it: `slots[1]` for a
    /// single mask, `slots[1]` or `slots[2]` for a stack. `None` for a stack
    /// with no masks, which covers everything.
    #[allow(clippy::too_many_arguments)]
    fn draw_stencil(
        &self,
        gpu: &GpuContext,
        encoder: &mut wgpu::CommandEncoder,
        config: &CompositorConfig,
        offscreen: &OffscreenPool,
        stencil: &LayerStencil<'_>,
        slots: [usize; 3],
    ) -> Result<Option<Coverage>, CompositorError> {
        let [mask_slot, first, second] = slots;
        let draw_mask = |encoder: &mut wgpu::CommandEncoder,
                         mask: &StencilMask,
                         into: usize,
                         scratch: usize|
         -> Result<(), CompositorError> {
            let coverage = coverage_layer(stencil, mask)?;
            self.draw_samples(
                gpu,
                encoder,
                config,
                offscreen,
                &coverage,
                stencil.blur_passes,
                into,
                scratch,
            )
        };
        match stencil.masks {
            [] => Ok(None),
            [mask] => {
                draw_mask(encoder, mask, first, second)?;
                Ok(Some(Coverage {
                    slot: first,
                    mix: CoverageMix::single(mask.op, mask.opacity, mask.invert),
                    softness: mask.softness,
                }))
            }
            [top, ..] => {
                let start = top.op.start();
                let mut held: Option<usize> = None;
                for mask in stencil.masks {
                    let out = if held == Some(first) { second } else { first };
                    draw_mask(encoder, mask, mask_slot, out)?;
                    run_mask_stack_pass(
                        &gpu.device,
                        encoder,
                        &self.pass_registry,
                        offscreen.passes(),
                        held.map(|slot| offscreen.view(slot)),
                        offscreen.view(mask_slot),
                        offscreen.view(out),
                        &StackStep {
                            op: mask.op,
                            opacity: mask.opacity,
                            softness: mask.softness,
                            invert: mask.invert,
                            start,
                        },
                        config.width,
                        config.height,
                    );
                    held = Some(out);
                }
                Ok(held.map(|slot| Coverage {
                    slot,
                    mix: CoverageMix::KEEP,
                    softness: 0.0,
                }))
            }
        }
    }

    /// Draw `layer`'s content, or the average of its motion-blur samples, cut
    /// by `stencil` into pool slot 0: the coverage into slot 1 or 2, the
    /// content into the other, and the cut into slot 0.
    pub(super) fn draw_masked_content(
        &self,
        gpu: &GpuContext,
        encoder: &mut wgpu::CommandEncoder,
        config: &CompositorConfig,
        offscreen: &OffscreenPool,
        layer: &CompositeLayer<'_>,
        stencil: &LayerStencil<'_>,
    ) -> Result<(), CompositorError> {
        let Some(coverage) =
            self.draw_stencil(gpu, encoder, config, offscreen, stencil, [0, 1, 2])?
        else {
            return self.draw_samples(
                gpu,
                encoder,
                config,
                offscreen,
                layer,
                layer.blur_passes,
                0,
                1,
            );
        };
        let content = if coverage.slot == 1 { 2 } else { 1 };
        self.draw_samples(
            gpu,
            encoder,
            config,
            offscreen,
            layer,
            layer.blur_passes,
            content,
            0,
        )?;
        run_matte_pass(
            &gpu.device,
            encoder,
            &self.pass_registry,
            offscreen.passes(),
            offscreen.view(content),
            offscreen.view(coverage.slot),
            offscreen.view(0),
            MatteRead::Alpha,
            coverage.mix,
            coverage.softness,
            config.width,
            config.height,
        );
        Ok(())
    }

    /// Replace `canvas` with the pool view `result` where `stencil` covers it
    /// and keep the canvas elsewhere.
    ///
    /// The result cut by the coverage and the canvas cut by its complement are
    /// summed, so a soft edge mixes the two rather than darkening.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn replace_masked(
        &self,
        gpu: &GpuContext,
        encoder: &mut wgpu::CommandEncoder,
        config: &CompositorConfig,
        offscreen: &OffscreenPool,
        result: &wgpu::TextureView,
        canvas: &wgpu::TextureView,
        stencil: &LayerStencil<'_>,
    ) -> Result<(), CompositorError> {
        let device = &gpu.device;
        let held = offscreen
            .index_of(result)
            .expect("a canvas pass result is an offscreen pool view");
        let mut free = (0..offscreen.slots()).filter(|slot| *slot != held);
        let slots = if stencil.masks.len() > 1 {
            let mut take = || free.next().expect("four pool slots for a mask stack");
            [take(), take(), take()]
        } else {
            let mut take = || free.next().expect("three pool slots");
            // A single mask leaves the mask slot unused, so it may repeat.
            let [first, second] = [take(), take()];
            [first, first, second]
        };
        let Some(coverage) = self.draw_stencil(gpu, encoder, config, offscreen, stencil, slots)?
        else {
            blit_replace(
                device,
                encoder,
                &self.pass_registry,
                offscreen.passes(),
                result,
                canvas,
            );
            return Ok(());
        };
        let cut = if coverage.slot == slots[1] {
            slots[2]
        } else {
            slots[1]
        };
        let matte = |encoder: &mut wgpu::CommandEncoder,
                     content: &wgpu::TextureView,
                     output: &wgpu::TextureView,
                     mix: CoverageMix| {
            run_matte_pass(
                device,
                encoder,
                &self.pass_registry,
                offscreen.passes(),
                content,
                offscreen.view(coverage.slot),
                output,
                MatteRead::Alpha,
                mix,
                coverage.softness,
                config.width,
                config.height,
            );
        };
        matte(encoder, result, offscreen.view(cut), coverage.mix);
        blit_replace(
            device,
            encoder,
            &self.pass_registry,
            offscreen.passes(),
            canvas,
            offscreen.view(held),
        );
        matte(
            encoder,
            offscreen.view(held),
            canvas,
            coverage.mix.complement(),
        );
        blit_weighted_additive(
            device,
            encoder,
            &self.pass_registry,
            offscreen.passes(),
            offscreen.view(cut),
            canvas,
            1.0,
        );
        Ok(())
    }
}
