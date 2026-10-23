//! One layer drawn onto a target: the canvas, a nested sequence target, a
//! reference target, or the side texture of a transition or matte.
//!
//! Consecutive draws onto the same target are queued in [`TargetPass`] and
//! encoded inside one [`wgpu::RenderPass`], switching pipelines and bind groups
//! in place. The batch flushes before any command reads the target texture or
//! overwrites an [`OffscreenPool`] slot bound by a queued draw.

use crate::color_pass::{self, QuadPipeline};
use crate::effect_render::{
    OffscreenPool, background_plate_layer, blit_premultiplied_to_target, composite_layer_outline,
    composite_layer_styles, run_blend_composite_to_target,
};
use crate::error::CompositorError;
use crate::gpu::GpuContext;
use crate::layer::{BlendMode, CompositeLayer, CompositorConfig};

use super::pipelines::SharedLayerPipelines;
use super::reference::draws_content_direct;
use super::render::draw_built_layer;
use super::{Compositor, LayerGpu};

const INLINE_TARGET_DRAWS: usize = 8;

/// One draw queued onto a [`TargetPass`].
#[allow(clippy::large_enum_variant)]
pub(crate) enum TargetDraw<'a> {
    Layer(LayerGpu),
    Quad {
        pipeline: QuadPipeline<'a>,
        bind_group: wgpu::BindGroup,
    },
}

impl TargetDraw<'_> {
    fn overwrites_target(&self) -> bool {
        match self {
            Self::Layer(built) => built.pipeline.overwrites_target(),
            Self::Quad { pipeline, .. } => pipeline.overwrites_target(),
        }
    }
}

/// Batched draws targeting one color attachment.
pub(crate) struct TargetPass<'a> {
    /// Target texture when destination snapshots are supported (`canvas`,
    /// `nest`, or transition/matte `side`); `None` for a reference target,
    /// which composites as `Normal`.
    pub(super) texture: Option<&'a wgpu::Texture>,
    pub(super) view: &'a wgpu::TextureView,
    clear: Option<wgpu::Color>,
    inline: [Option<TargetDraw<'a>>; INLINE_TARGET_DRAWS],
    len: usize,
    overflow: Vec<TargetDraw<'a>>,
    offscreen_mask: u8,
}

impl<'a> TargetPass<'a> {
    pub(super) fn new(
        texture: Option<&'a wgpu::Texture>,
        view: &'a wgpu::TextureView,
        clear: Option<wgpu::Color>,
    ) -> Self {
        Self {
            texture,
            view,
            clear,
            inline: std::array::from_fn(|_| None),
            len: 0,
            overflow: Vec::new(),
            offscreen_mask: 0,
        }
    }

    fn reset_for_overwrite(&mut self) {
        self.clear = None;
        let inline_count = self.len.min(INLINE_TARGET_DRAWS);
        for slot in &mut self.inline[..inline_count] {
            *slot = None;
        }
        self.overflow.clear();
        self.len = 0;
        self.offscreen_mask = 0;
    }

    fn push_draw(&mut self, draw: TargetDraw<'a>, offscreen_slots: u8) {
        if draw.overwrites_target() {
            self.reset_for_overwrite();
        }
        if self.len < INLINE_TARGET_DRAWS {
            self.inline[self.len] = Some(draw);
        } else {
            self.overflow.push(draw);
        }
        self.len += 1;
        self.offscreen_mask |= offscreen_slots;
    }

    pub(super) fn push_layer(&mut self, built: LayerGpu) {
        self.push_draw(TargetDraw::Layer(built), 0);
    }

    pub(crate) fn push_quad(
        &mut self,
        pipeline: QuadPipeline<'a>,
        bind_group: wgpu::BindGroup,
        offscreen_slots: u8,
    ) {
        self.push_draw(
            TargetDraw::Quad {
                pipeline,
                bind_group,
            },
            offscreen_slots,
        );
    }

    /// Flush queued draws if any of them samples an [`OffscreenPool`] slot in
    /// `slots_mask` that is about to be overwritten.
    pub(crate) fn flush_if_reading_slots(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        pipelines: &SharedLayerPipelines,
        slots_mask: u8,
    ) {
        if (self.offscreen_mask & slots_mask) != 0 {
            self.flush(encoder, pipelines);
        }
    }

    /// Flush queued draws if any of them samples an [`OffscreenPool`] slot.
    pub(super) fn flush_if_reading_offscreen(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        pipelines: &SharedLayerPipelines,
    ) {
        if self.offscreen_mask != 0 {
            self.flush(encoder, pipelines);
        }
    }

    /// Encode all queued draws (or any pending clear) into one render pass on
    /// `encoder`.
    pub(super) fn flush(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        pipelines: &SharedLayerPipelines,
    ) {
        if self.len == 0 {
            if let Some(color) = self.clear.take() {
                color_pass::clear(encoder, self.view, color);
            }
            return;
        }
        let clear = self.clear.take();
        let first_overwrites = self.inline[0]
            .as_ref()
            .is_some_and(TargetDraw::overwrites_target);
        let mut pass = if first_overwrites {
            // SAFETY: `push_draw` resets the batch whenever an overwriting draw
            // is pushed, so `inline[0]` is the first draw of `pass` and writes
            // every pixel of `self.view` with blending disabled.
            unsafe { color_pass::begin_overwrite(encoder, self.view) }
        } else {
            let load = match clear {
                Some(color) => wgpu::LoadOp::Clear(color),
                None => wgpu::LoadOp::Load,
            };
            color_pass::begin(encoder, self.view, load)
        };
        let inline_count = self.len.min(INLINE_TARGET_DRAWS);
        for draw in self.inline[..inline_count]
            .iter()
            .flatten()
            .chain(self.overflow.iter())
        {
            match draw {
                TargetDraw::Layer(built) => {
                    let pipeline = pipelines.layer_pipeline(&built.pipeline);
                    draw_built_layer(&mut pass, pipeline, built);
                }
                TargetDraw::Quad {
                    pipeline,
                    bind_group,
                } => {
                    color_pass::draw_quad_in(&mut pass, pipeline.raw(), bind_group);
                }
            }
        }
        drop(pass);
        for slot in &mut self.inline[..inline_count] {
            *slot = None;
        }
        self.overflow.clear();
        self.len = 0;
        self.offscreen_mask = 0;
    }
}

impl Compositor {
    /// Draw `layer` with its effect chain, LUT, motion blur, styles and blend
    /// onto `target`.
    pub(super) fn draw_layer_onto<'a>(
        &'a self,
        gpu: &GpuContext,
        encoder: &mut wgpu::CommandEncoder,
        config: &CompositorConfig,
        offscreen: &OffscreenPool,
        layer: &CompositeLayer<'_>,
        target: &mut TargetPass<'a>,
    ) -> Result<(), CompositorError> {
        let mode = layer.blend_mode;
        if draws_content_direct(layer) && mode == BlendMode::Normal && layer.styles.is_empty() {
            let built = self.build_layer(gpu, encoder, config, layer)?;
            target.push_layer(built);
            return Ok(());
        }
        target.flush_if_reading_offscreen(encoder, &self.pipelines);
        let device = &gpu.device;
        // The content cut by its mask, or the average of its motion-blur
        // samples, then its effect chain and LUT, end on one pool slot S.
        let result = self.layer_result(gpu, encoder, config, offscreen, layer)?;
        // Style order: background → shadow → glow → content → outline. Content
        // stays on S. Shadow/glow/outline silhouette+blur use A=(S+1)%3 and
        // B=(S+2)%3. A/B are free again before content (and after blend's
        // snapshot). Styles derive from the accumulated (motion-blurred)
        // result.
        let src_slot = offscreen
            .index_of(result)
            .expect("effect/lut output is an offscreen pool view");
        if let Some(bg) = layer.styles.background.filter(|b| b.rgba[3] > 0) {
            let plate = background_plate_layer(&layer.placement, bg);
            let built = self.build_layer(gpu, encoder, config, &plate)?;
            target.push_layer(built);
        }
        if layer.styles.shadow.is_some() || layer.styles.glow.is_some() {
            composite_layer_styles(
                device,
                encoder,
                &self.pipelines,
                &self.pass_registry,
                offscreen,
                result,
                src_slot,
                target,
                layer.styles.shadow,
                layer.styles.glow,
                config.width,
                config.height,
            );
        }
        self.composite_slot(device, encoder, offscreen, src_slot, target, mode);
        if let Some(outline) = layer.styles.outline {
            composite_layer_outline(
                device,
                encoder,
                &self.pipelines,
                &self.pass_registry,
                offscreen,
                result,
                src_slot,
                target,
                outline,
                config.width,
                config.height,
            );
        }
        Ok(())
    }

    /// Composite pool slot `slot` over `target` with `mode`.
    ///
    /// A mode other than Normal reads a snapshot of the destination copied
    /// into the next slot of the ring. A destination with no texture, a
    /// reference target, composites as Normal.
    pub(super) fn composite_slot<'a>(
        &'a self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        offscreen: &OffscreenPool,
        slot: usize,
        target: &mut TargetPass<'a>,
        mode: BlendMode,
    ) {
        let source = offscreen.view(slot);
        let slot_mask = 1u8 << slot;
        let Some(texture) = target.texture.filter(|_| mode != BlendMode::Normal) else {
            blit_premultiplied_to_target(
                device,
                &self.pass_registry,
                offscreen.passes(),
                source,
                slot_mask,
                target,
            );
            return;
        };
        // See the OffscreenPool slot invariant: the source stays on its slot
        // and the destination snapshot goes into the next one.
        target.flush(encoder, &self.pipelines);
        let snap_slot = (slot + 1) % offscreen.slots();
        copy_canvas(encoder, offscreen, texture, offscreen.texture(snap_slot));
        run_blend_composite_to_target(
            device,
            &self.pass_registry,
            offscreen.passes(),
            offscreen.view(snap_slot),
            source,
            slot_mask | (1u8 << snap_slot),
            target,
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
