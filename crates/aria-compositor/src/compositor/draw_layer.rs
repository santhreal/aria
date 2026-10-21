//! One layer drawn onto a target: the canvas, or the side texture of a
//! transition or matte. A layer with no mask, effect chain, LUT, styles, motion
//! blur or blend mode draws straight onto the target. Any other layer is drawn
//! up to its blend through the pool, and its styles and blend composite the
//! result.

use crate::effect_render::{
    OffscreenPool, background_plate_layer, blit_premultiplied_to_canvas, composite_layer_outline,
    composite_layer_styles, run_blend_composite,
};
use crate::error::CompositorError;
use crate::gpu::GpuContext;
use crate::layer::{BlendMode, CompositeLayer, CompositorConfig};

use super::Compositor;
use super::reference::draws_content_direct;

/// A destination a pool slot composites over: the canvas, or the reference
/// target of a clip.
pub(super) struct BlendDest<'a> {
    pub(super) view: &'a wgpu::TextureView,
    /// The destination's texture, read under a blend mode other than Normal;
    /// `None` for a reference target, which composites as Normal.
    pub(super) texture: Option<&'a wgpu::Texture>,
}

/// Where [`Compositor::draw_layer_onto`] draws.
pub(super) struct LayerTarget<'a> {
    pub(super) texture: &'a wgpu::Texture,
    pub(super) view: &'a wgpu::TextureView,
    /// Clear color applied before drawing when `Some`, or `None` to load the
    /// existing target contents.
    pub(super) clear: Option<wgpu::Color>,
}

impl Compositor {
    /// Draw `layer` with its effect chain, LUT, motion blur, styles and blend
    /// onto `target`.
    pub(super) fn draw_layer_onto(
        &self,
        gpu: &GpuContext,
        encoder: &mut wgpu::CommandEncoder,
        config: &CompositorConfig,
        offscreen: &OffscreenPool,
        layer: &CompositeLayer<'_>,
        target: &LayerTarget<'_>,
    ) -> Result<(), CompositorError> {
        let mode = layer.blend_mode;
        if draws_content_direct(layer) && mode == BlendMode::Normal && layer.styles.is_empty() {
            let built = self.build_layer(gpu, encoder, config, layer)?;
            self.draw_layer_into(encoder, target.view, target.clear, &built);
            return Ok(());
        }
        if let Some(color) = target.clear {
            crate::color_pass::clear(encoder, target.view, color);
        }
        let device = &gpu.device;
        // The content cut by its mask, or the average of its motion-blur
        // samples, then its effect chain and LUT, end on one pool slot S.
        let result = self.layer_result(gpu, encoder, config, offscreen, layer)?;
        // Style order: background → shadow → glow → content → outline. Content
        // stays on S. Shadow/glow/outline silhouette+blur use A=(S+1)%3 and
        // B=(S+2)%3. A/B are free again before content (and after blend's
        // snapshot). Styles derive from the accumulated (motion-blurred)
        // result — acceptable vs true shutter integration.
        let src_slot = offscreen
            .index_of(result)
            .expect("effect/lut output is an offscreen pool view");
        if let Some(bg) = layer.styles.background.filter(|b| b.rgba[3] > 0) {
            let plate = background_plate_layer(&layer.placement, bg);
            let built = self.build_layer(gpu, encoder, config, &plate)?;
            self.draw_layer_into(encoder, target.view, None, &built);
        }
        if layer.styles.shadow.is_some() || layer.styles.glow.is_some() {
            composite_layer_styles(
                device,
                encoder,
                &self.pass_registry,
                offscreen,
                result,
                src_slot,
                target.view,
                layer.styles.shadow,
                layer.styles.glow,
                config.width,
                config.height,
            );
        }
        let onto = BlendDest {
            view: target.view,
            texture: Some(target.texture),
        };
        self.composite_slot(device, encoder, offscreen, src_slot, &onto, mode);
        if let Some(outline) = layer.styles.outline {
            composite_layer_outline(
                device,
                encoder,
                &self.pass_registry,
                offscreen,
                result,
                src_slot,
                target.view,
                outline,
                config.width,
                config.height,
            );
        }
        Ok(())
    }

    /// Composite pool slot `slot` over `dest` with `mode`.
    ///
    /// A mode other than Normal reads a snapshot of the destination copied
    /// into the next slot of the ring. A destination with no texture, a
    /// reference target, composites as Normal.
    pub(super) fn composite_slot(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        offscreen: &OffscreenPool,
        slot: usize,
        dest: &BlendDest<'_>,
        mode: BlendMode,
    ) {
        let source = offscreen.view(slot);
        let Some(texture) = dest.texture.filter(|_| mode != BlendMode::Normal) else {
            blit_premultiplied_to_canvas(
                device,
                encoder,
                &self.pass_registry,
                offscreen.passes(),
                source,
                dest.view,
            );
            return;
        };
        // See the OffscreenPool slot invariant: the source stays on its slot
        // and the destination snapshot goes into the next one.
        let snap_slot = (slot + 1) % offscreen.slots();
        copy_canvas(encoder, offscreen, texture, offscreen.texture(snap_slot));
        run_blend_composite(
            device,
            encoder,
            &self.pass_registry,
            offscreen.passes(),
            offscreen.view(snap_slot),
            source,
            dest.view,
            mode,
        );
    }
}

/// Copy the canvas-sized `from` into `to`.
pub(super) fn copy_canvas(
    encoder: &mut wgpu::CommandEncoder,
    offscreen: &OffscreenPool,
    from: &wgpu::Texture,
    to: &wgpu::Texture,
) {
    encoder.copy_texture_to_texture(
        wgpu::TexelCopyTextureInfo {
            texture: from,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyTextureInfo {
            texture: to,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::Extent3d {
            width: offscreen.width,
            height: offscreen.height,
            depth_or_array_layers: 1,
        },
    );
}
