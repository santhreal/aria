use crate::effect_render::{OffscreenPool, draw_layer_to_offscreen};
use crate::error::CompositorError;
use crate::gpu::GpuContext;
use crate::layer::{BlendMode, CompositeLayer, CompositorConfig, CompositorLayer, LayerLut};
use crate::lut::CubeLut;
use crate::passes::PassInstance;

use super::items::frame_items;
use super::scene::Scene;
use super::{
    CANVAS_FORMAT, Compositor, FrameSink, LUT_CACHE_MAX, LayerGpu, LayerPipeline, LutTexture,
    PendingReadback, ReadbackPool, TargetCache,
};
use aria_core::RgbaImage;

fn prepare_lut_upload<'a>(scratch: &'a mut Vec<u8>, lut: &CubeLut) -> &'a [u8] {
    scratch.resize(lut.rgba8_texel_len(), 0);
    lut.rgba8_texels_into(scratch);
    scratch
}

impl Compositor {
    /// Make `self.target` hold a canvas + readback buffer for `width`×`height`,
    /// reusing the cached pair when the size already matches.
    fn ensure_target(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        if self
            .target
            .as_ref()
            .is_some_and(|t| t.width == width && t.height == height)
        {
            return;
        }

        let unpadded = width * 4;
        let padded = unpadded.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("aria.canvas"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: CANVAS_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let readbacks = ReadbackPool::new(device, u64::from(padded) * u64::from(height));

        self.target = Some(TargetCache {
            width,
            height,
            texture,
            view,
            readbacks: std::cell::RefCell::new(readbacks),
            padded_bytes_per_row: padded,
        });
    }

    /// Upload `lut` as a 3D texture under `key`, reusing the cached copy when
    /// it's already there. Called for every LUT in the scene before the pass
    /// encodes, so lookups inside the render loop are immutable.
    fn ensure_lut(&mut self, gpu: &GpuContext, key: &str, lut: &CubeLut) {
        if self.lut_cache.get(key).is_some() {
            return;
        }
        let n = lut.size();
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("aria.lut3d"),
            size: wgpu::Extent3d {
                width: n,
                height: n,
                depth_or_array_layers: n,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D3,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let texels = prepare_lut_upload(&mut self.lut_upload_scratch, lut);
        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            texels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(n * 4),
                rows_per_image: Some(n),
            },
            wgpu::Extent3d {
                width: n,
                height: n,
                depth_or_array_layers: n,
            },
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.lut_cache.insert(
            key.to_owned(),
            LutTexture {
                _texture: texture,
                view,
                size: n,
                domain_min: lut.domain_min(),
                domain_max: lut.domain_max(),
            },
            LUT_CACHE_MAX,
        );
    }

    fn ensure_offscreen(
        &mut self,
        gpu: &GpuContext,
        width: u32,
        height: u32,
        (required_slots, sides): (usize, bool),
    ) {
        let needed = required_slots.max(OffscreenPool::DEFAULT_SLOTS);
        if !self
            .offscreen
            .as_ref()
            .is_some_and(|pool| pool.width == width && pool.height == height)
        {
            self.offscreen = Some(OffscreenPool::ensure(
                &gpu.device,
                gpu.queue.clone(),
                width,
                height,
            ));
        }
        let pool = self.offscreen.as_mut().expect("offscreen pool set above");
        pool.ensure_slots(&gpu.device, needed);
        if sides {
            pool.ensure_sides(&gpu.device);
        }
    }

    /// Composite `layers` and return tightly packed RGBA pixels.
    pub fn render(
        &mut self,
        gpu: &GpuContext,
        config: &CompositorConfig,
        layers: &[CompositeLayer<'_>],
    ) -> Result<RgbaImage, CompositorError> {
        self.submit(gpu, config, layers)?.into_image()
    }

    /// [`render`](Self::render) writing rows directly into caller storage.
    pub fn render_into(
        &mut self,
        gpu: &GpuContext,
        config: &CompositorConfig,
        layers: &[CompositeLayer<'_>],
        sink: &mut dyn FrameSink,
    ) -> Result<(), CompositorError> {
        self.submit(gpu, config, layers)?.read_into(sink)
    }

    /// Submit plain layers without waiting for GPU readback.
    pub fn submit(
        &mut self,
        gpu: &GpuContext,
        config: &CompositorConfig,
        layers: &[CompositeLayer<'_>],
    ) -> Result<PendingReadback, CompositorError> {
        self.submit_scene(gpu, config, Scene::Plain(layers))
    }

    /// Composite effect chains, canvas passes, transitions, and mattes.
    pub fn render_compositor_layers(
        &mut self,
        gpu: &GpuContext,
        config: &CompositorConfig,
        layers: &[CompositorLayer<'_>],
    ) -> Result<RgbaImage, CompositorError> {
        self.submit_compositor_layers(gpu, config, layers)?
            .into_image()
    }

    /// [`render_compositor_layers`](Self::render_compositor_layers) into caller storage.
    pub fn render_compositor_layers_into(
        &mut self,
        gpu: &GpuContext,
        config: &CompositorConfig,
        layers: &[CompositorLayer<'_>],
        sink: &mut dyn FrameSink,
    ) -> Result<(), CompositorError> {
        self.submit_compositor_layers(gpu, config, layers)?
            .read_into(sink)
    }

    /// Submit full compositor layers without waiting for GPU readback.
    pub fn submit_compositor_layers(
        &mut self,
        gpu: &GpuContext,
        config: &CompositorConfig,
        layers: &[CompositorLayer<'_>],
    ) -> Result<PendingReadback, CompositorError> {
        self.submit_scene(gpu, config, Scene::Full(layers))
    }

    /// The one submission body both layer-list shapes share: the canvas copied
    /// into a readback buffer.
    fn submit_scene(
        &mut self,
        gpu: &GpuContext,
        config: &CompositorConfig,
        scene: Scene<'_>,
    ) -> Result<PendingReadback, CompositorError> {
        let mut encoder = self.encode_scene(gpu, config, scene)?;
        let target = self
            .target
            .as_ref()
            .expect("encode_scene populates the target");
        let lease = target.readbacks.borrow_mut().acquire()?;
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &target.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &lease.buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(target.padded_bytes_per_row),
                    rows_per_image: Some(config.height),
                },
            },
            wgpu::Extent3d {
                width: config.width,
                height: config.height,
                depth_or_array_layers: 1,
            },
        );
        self.submit_frame(gpu, encoder);
        self.readback_count += 1;
        Ok(PendingReadback::begin(
            &gpu.device,
            lease,
            target.padded_bytes_per_row,
            config.width,
            config.height,
        ))
    }

    /// Submit a frame's commands, then recall the plane staging they copy from,
    /// so the upload pool reuses it once the GPU has read it.
    pub(super) fn submit_frame(&self, gpu: &GpuContext, encoder: wgpu::CommandEncoder) {
        gpu.queue.submit(std::iter::once(encoder.finish()));
        self.plane_textures.borrow_mut().recall();
    }

    /// Draw `scene` onto the cached canvas and return the unsubmitted encoder,
    /// so the caller appends the copy out of the canvas.
    pub(super) fn encode_scene(
        &mut self,
        gpu: &GpuContext,
        config: &CompositorConfig,
        scene: Scene<'_>,
    ) -> Result<wgpu::CommandEncoder, CompositorError> {
        self.prepare_scene(gpu, config, &scene)?;
        self.ensure_target(&gpu.device, config.width, config.height);
        let target = self
            .target
            .as_ref()
            .expect("ensure_target populates the cache");
        self.encode_prepared_scene(gpu, config, scene, (&target.texture, &target.view))
    }

    /// Ensure offscreen pools, reference/nest targets, LUT uploads, and
    /// per-frame cache epochs are ready to encode `scene`.
    pub(super) fn prepare_scene(
        &mut self,
        gpu: &GpuContext,
        config: &CompositorConfig,
        scene: &Scene<'_>,
    ) -> Result<(), CompositorError> {
        if config.width == 0 || config.height == 0 {
            return Err(CompositorError::InvalidDimensions {
                width: config.width,
                height: config.height,
            });
        }

        let device = &gpu.device;
        self.ensure_offscreen(gpu, config.width, config.height, scene_demand(scene));
        self.references.ensure(
            device,
            config.width,
            config.height,
            frame_items(scene).filter_map(|item| match item {
                CompositorLayer::Reference { clip, .. } => Some(clip),
                _ => None,
            }),
        );
        self.nests.ensure(
            device,
            config.width,
            config.height,
            scene.iter().filter_map(|item| match item {
                CompositorLayer::Nest { nest, .. } => Some(nest),
                _ => None,
            }),
        );
        self.lut_cache.begin_frame();
        self.glyph_atlases.borrow_mut().begin_frame();
        // Upload every LUT the scene references before target/offscreen are
        // borrowed; the loop below only reads the cache.
        for item in frame_items(scene) {
            for LayerLut { key, lut, .. } in item_luts(&item).into_iter().flatten() {
                self.ensure_lut(gpu, key, lut);
            }
        }
        self.layer_passes.begin_frame();
        self.plane_textures.borrow_mut().begin_frame();
        self.rgba_textures.borrow_mut().begin_frame();
        let offscreen = self.offscreen.as_ref().expect("offscreen pool");
        offscreen.passes().begin_frame();
        offscreen.passes().set_frame_budget(config.frame_budget);
        Ok(())
    }

    /// Encode `scene` onto `canvas` after [`Self::prepare_scene`] has run.
    pub(super) fn encode_prepared_scene(
        &self,
        gpu: &GpuContext,
        config: &CompositorConfig,
        scene: Scene<'_>,
        canvas: (&wgpu::Texture, &wgpu::TextureView),
    ) -> Result<wgpu::CommandEncoder, CompositorError> {
        let offscreen = self.offscreen.as_ref().expect("offscreen pool");
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });

        let bg = config.background;
        let mut pending_clear = Some(wgpu::Color {
            r: f64::from(bg[0]) / 255.0,
            g: f64::from(bg[1]) / 255.0,
            b: f64::from(bg[2]) / 255.0,
            a: f64::from(bg[3]) / 255.0,
        });
        self.references.clear(&mut encoder);
        self.nests.clear(&mut encoder);

        // Every nest draws before the items that show it; the frame draws the
        // rest of its items onto the canvas, fusing the background clear into
        // the first direct layer pass when possible.
        for item in scene.iter() {
            match item {
                CompositorLayer::Nest { nest, items } => {
                    self.draw_nest(gpu, &mut encoder, config, offscreen, nest, items)?;
                }
                item => {
                    self.draw_item(
                        gpu,
                        &mut encoder,
                        config,
                        offscreen,
                        &item,
                        canvas,
                        false,
                        &mut pending_clear,
                    )?;
                }
            }
        }
        if let Some(color) = pending_clear.take() {
            crate::color_pass::clear(&mut encoder, canvas.1, color);
        }

        Ok(encoder)
    }
}

pub(super) fn draw_built_layer(
    pass: &mut wgpu::RenderPass<'_>,
    pipeline: &wgpu::RenderPipeline,
    built: &LayerGpu,
) {
    let instances = built.instances.as_ref().map(|i| (&i.buffer, i.count));
    draw_layer_to_offscreen(pass, pipeline, &built.bind_group, instances);
}

/// The LUTs `item` grades with: its own, or one per side of a transition or
/// matte. A reference grades what it draws.
fn item_luts<'a>(item: &CompositorLayer<'a>) -> [Option<LayerLut<'a>>; 2] {
    match item {
        CompositorLayer::Layer(layer) => [layer.lut, None],
        CompositorLayer::CanvasPass { lut, .. } => [*lut, None],
        // A nest's items upload their own LUTs.
        CompositorLayer::Nest { .. }
        | CompositorLayer::Reference {
            item:
                CompositorLayer::CanvasPass { .. }
                | CompositorLayer::Reference { .. }
                | CompositorLayer::Nest { .. },
            ..
        } => [None, None],
        CompositorLayer::Reference { item, .. } => item_luts(item),
        CompositorLayer::Transition {
            outgoing: a,
            incoming: b,
            ..
        }
        | CompositorLayer::TransitionWithParams {
            outgoing: a,
            incoming: b,
            ..
        }
        | CompositorLayer::Matte {
            content: a,
            stencil: b,
            ..
        } => [a.lut, b.lut],
    }
}

/// Offscreen pool demand of `scene`: the slots its most demanding item draws
/// through, and whether any item draws a pair into the side textures.
fn scene_demand(scene: &Scene<'_>) -> (usize, bool) {
    frame_items(scene).fold(
        (OffscreenPool::DEFAULT_SLOTS, false),
        |(slots, sides), item| (slots.max(item_slots(&item)), sides || draws_pair(&item)),
    )
}

/// Whether drawing `item` draws a transition or matte.
fn draws_pair(item: &CompositorLayer<'_>) -> bool {
    match item {
        CompositorLayer::Transition { .. }
        | CompositorLayer::TransitionWithParams { .. }
        | CompositorLayer::Matte { .. } => true,
        CompositorLayer::Reference { item, .. } => draws_pair(item),
        // A nest's items are visited on their own.
        CompositorLayer::Layer(_)
        | CompositorLayer::CanvasPass { .. }
        | CompositorLayer::Nest { .. } => false,
    }
}

/// Offscreen pool slots drawing `item` uses. A transition or matte draws each
/// side through the slots, then its own pass into slot 0.
fn item_slots(item: &CompositorLayer<'_>) -> usize {
    match item {
        CompositorLayer::Layer(layer) => layer_slots(layer),
        CompositorLayer::Reference { item, .. } => item_slots(item),
        // A nest's items are visited on their own.
        CompositorLayer::Nest { .. } => 0,
        CompositorLayer::Transition {
            outgoing: a,
            incoming: b,
            ..
        }
        | CompositorLayer::TransitionWithParams {
            outgoing: a,
            incoming: b,
            ..
        }
        | CompositorLayer::Matte {
            content: a,
            stencil: b,
            ..
        } => layer_slots(a).max(layer_slots(b)),
        CompositorLayer::CanvasPass {
            effects,
            lut,
            stencil,
            ..
        } => {
            let chain = effects.iter().map(PassInstance::slot_demand).max();
            let mut pass_slots = 2usize.max(chain.unwrap_or(0));
            // A LUT writes past its input; a mask holds the result, the
            // coverage, and the cut result at once, and a stack of masks one
            // more for the mask it combines.
            if lut.is_some() || stencil.is_some() {
                pass_slots = pass_slots.max(3);
            }
            if stencil.as_ref().is_some_and(|s| s.masks.len() > 1) {
                pass_slots = pass_slots.max(4);
            }
            pass_slots
        }
    }
}

/// Offscreen pool slots drawing `layer` uses.
fn layer_slots(layer: &CompositeLayer<'_>) -> usize {
    let mut layer_slots = 2usize;
    if !layer.blur_passes.is_empty() {
        layer_slots = layer_slots.max(3);
    }
    let chain = layer.effects.iter().map(PassInstance::slot_demand).max();
    layer_slots = layer_slots.max(chain.unwrap_or(0));
    if layer.styles.shadow.is_some()
        || layer.styles.glow.is_some()
        || layer.styles.outline.is_some()
    {
        layer_slots = layer_slots.max(3);
        if layer.blend_mode != BlendMode::Normal {
            layer_slots = layer_slots.max(4);
        }
    }
    // The content, the coverage, and the cut content are held at once; a
    // stack of masks draws each mask into the slot the cut lands in.
    if layer.lut.is_some() || layer.stencil.is_some() {
        layer_slots = layer_slots.max(3);
    }
    layer_slots
}

pub(super) fn pipeline_for<'a>(
    kind: &LayerPipeline,
    comp: &'a Compositor,
) -> &'a wgpu::RenderPipeline {
    comp.pipelines.layer_pipeline(kind)
}

#[cfg(test)]
mod tests {
    use super::prepare_lut_upload;
    use crate::lut::CubeLut;

    #[test]
    fn lut_upload_scratch_survives_changed_tables() {
        let first = CubeLut::from_fn(3, |rgb| rgb);
        let second = CubeLut::from_fn(3, |rgb| [1.0 - rgb[0], rgb[1], rgb[2]]);
        let mut scratch = Vec::new();

        let first_bytes = prepare_lut_upload(&mut scratch, &first);
        let storage = first_bytes.as_ptr();
        let first_red = first_bytes[0];
        let second_bytes = prepare_lut_upload(&mut scratch, &second);

        assert_eq!(second_bytes.as_ptr(), storage);
        assert_ne!(second_bytes[0], first_red);
    }
}
