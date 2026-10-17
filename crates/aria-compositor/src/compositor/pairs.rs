//! Items drawn from two layers: a transition blends its sides by progress, and
//! a matte cuts its content by its stencil. Both draw each side into a side
//! texture of the pool as the layer draws alone on a transparent canvas, with
//! its mask, effect chain, LUT, motion blur and styles, then run their pass
//! into pool slot 0 and composite slot 0 over the destination: the canvas, or
//! the reference target of a clip.
//!
//! On the canvas a pair composites with its layers' blend modes. A matte
//! composites with its content's mode. A transition whose sides share a mode
//! composites the blended sides with that mode. Sides with different modes
//! each draw over a copy of the canvas with their own mode, and the blend of
//! the two copies replaces the canvas. A reference target draws a pair as
//! Normal, as it draws a layer.

use crate::effect_render::{OffscreenPool, blit_replace, run_matte_pass, run_transition_pass};
use crate::error::CompositorError;
use crate::gpu::GpuContext;
use crate::layer::{CompositeLayer, CompositorConfig, CoverageMix, MatteRead};

use super::Compositor;
use super::draw_layer::{BlendDest, LayerTarget, copy_canvas};

/// A transition as [`Compositor::draw_transition`] draws it.
pub(super) struct TransitionDraw<'a> {
    pub(super) outgoing: &'a CompositeLayer<'a>,
    pub(super) incoming: &'a CompositeLayer<'a>,
    pub(super) transition_id: &'a str,
    pub(super) progress: f32,
    pub(super) params: &'a [f32],
}

/// A matte as [`Compositor::draw_matte`] draws it.
pub(super) struct MatteDraw<'a> {
    pub(super) content: &'a CompositeLayer<'a>,
    pub(super) stencil: &'a CompositeLayer<'a>,
    pub(super) read: MatteRead,
    pub(super) invert: bool,
    pub(super) softness: f32,
}

impl Compositor {
    /// Draw `sides[i]` into side texture `i` of `offscreen` with its blend
    /// mode. With no `under`, each side texture is cleared first, so the mode
    /// composites the side over its own styles only, as it draws alone on a
    /// transparent canvas; with one, each starts as a copy of `under` and the
    /// side composites over it.
    #[allow(clippy::too_many_arguments)]
    fn draw_sides(
        &self,
        gpu: &GpuContext,
        encoder: &mut wgpu::CommandEncoder,
        config: &CompositorConfig,
        offscreen: &OffscreenPool,
        labels: [&'static str; 2],
        sides: [&CompositeLayer<'_>; 2],
        under: Option<&wgpu::Texture>,
    ) -> Result<(), CompositorError> {
        for (index, (label, side)) in labels.into_iter().zip(sides).enumerate() {
            let (texture, view) = offscreen.side(index);
            if let Some(under) = under {
                copy_canvas(encoder, offscreen, under, texture);
            }
            let target = LayerTarget {
                texture,
                view,
                clear: under.is_none(),
                label,
            };
            self.draw_layer_onto(gpu, encoder, config, offscreen, side, &target)?;
        }
        Ok(())
    }

    /// Blend a transition's sides and composite the result over `dest` with
    /// the sides' blend modes.
    pub(super) fn draw_transition(
        &self,
        gpu: &GpuContext,
        encoder: &mut wgpu::CommandEncoder,
        config: &CompositorConfig,
        offscreen: &OffscreenPool,
        draw: &TransitionDraw<'_>,
        dest: &BlendDest<'_>,
    ) -> Result<(), CompositorError> {
        let device = &gpu.device;
        let modes = [draw.outgoing.blend_mode, draw.incoming.blend_mode];
        // Sides with different modes cannot share one composite: each draws
        // over the destination instead.
        let under = dest.texture.filter(|_| modes[0] != modes[1]);
        self.draw_sides(
            gpu,
            encoder,
            config,
            offscreen,
            ["aria.transition.out", "aria.transition.in"],
            [draw.outgoing, draw.incoming],
            under,
        )?;
        run_transition_pass(
            device,
            encoder,
            &self.pass_registry,
            offscreen.passes(),
            offscreen.side(0).1,
            offscreen.side(1).1,
            offscreen.view(0),
            draw.transition_id,
            draw.progress,
            draw.params,
            config.width,
            config.height,
        );
        if under.is_some() {
            blit_replace(
                device,
                encoder,
                &self.pass_registry,
                offscreen.passes(),
                offscreen.view(0),
                dest.view,
            );
        } else {
            self.composite_slot(device, encoder, offscreen, 0, dest, modes[0]);
        }
        Ok(())
    }

    /// Cut a matte's content by its stencil and composite the result over
    /// `dest` with the content's blend mode.
    pub(super) fn draw_matte(
        &self,
        gpu: &GpuContext,
        encoder: &mut wgpu::CommandEncoder,
        config: &CompositorConfig,
        offscreen: &OffscreenPool,
        draw: &MatteDraw<'_>,
        dest: &BlendDest<'_>,
    ) -> Result<(), CompositorError> {
        let device = &gpu.device;
        self.draw_sides(
            gpu,
            encoder,
            config,
            offscreen,
            ["aria.matte.content", "aria.matte.stencil"],
            [draw.content, draw.stencil],
            None,
        )?;
        run_matte_pass(
            device,
            encoder,
            &self.pass_registry,
            offscreen.passes(),
            offscreen.side(0).1,
            offscreen.side(1).1,
            offscreen.view(0),
            draw.read,
            if draw.invert {
                CoverageMix::INVERT
            } else {
                CoverageMix::KEEP
            },
            draw.softness,
            config.width,
            config.height,
        );
        self.composite_slot(device, encoder, offscreen, 0, dest, draw.content.blend_mode);
        Ok(())
    }
}
