use crate::effect_render::{
    OffscreenPool, background_plate_layer, blit_premultiplied_to_canvas, blit_replace,
    blit_weighted_additive, composite_layer_outline, composite_layer_styles,
    draw_layer_to_offscreen, effects_need_offscreen, run_blend_composite, run_effect_chain,
    run_grade_pass, run_lut_pass, run_matte_pass, run_transition_pass,
};
use crate::error::CompositorError;
use crate::gpu::GpuContext;
use crate::layer::{
    BlendMode, CompositeLayer, CompositorConfig, CompositorLayer, LayerContent, LayerLut,
    LayerPlacement,
};
use crate::lut::CubeLut;
use crate::passes::PassInstance;

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
        required_slots: usize,
    ) {
        let needed = required_slots.max(OffscreenPool::DEFAULT_SLOTS);
        if let Some(pool) = &mut self.offscreen {
            if pool.width == width && pool.height == height {
                pool.ensure_slots(&gpu.device, needed);
                return;
            }
        }
        let mut pool = OffscreenPool::ensure(&gpu.device, gpu.queue.clone(), width, height);
        pool.ensure_slots(&gpu.device, needed);
        self.offscreen = Some(pool);
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
        gpu.queue.submit(std::iter::once(encoder.finish()));
        Ok(PendingReadback::begin(
            &gpu.device,
            lease,
            target.padded_bytes_per_row,
            config.width,
            config.height,
        ))
    }

    /// Draw `scene` onto the cached canvas and return the unsubmitted encoder,
    /// so the caller appends the copy out of the canvas.
    pub(super) fn encode_scene(
        &mut self,
        gpu: &GpuContext,
        config: &CompositorConfig,
        scene: Scene<'_>,
    ) -> Result<wgpu::CommandEncoder, CompositorError> {
        if config.width == 0 || config.height == 0 {
            return Err(CompositorError::InvalidDimensions {
                width: config.width,
                height: config.height,
            });
        }

        let device = &gpu.device;
        self.ensure_target(device, config.width, config.height);
        let required_slots = required_slots_for_scene(&scene);
        self.ensure_offscreen(gpu, config.width, config.height, required_slots);
        self.lut_cache.begin_frame();
        self.glyph_atlases.borrow_mut().begin_frame();
        // Upload every LUT the scene references before target/offscreen are
        // borrowed; the loop below only reads the cache.
        for item in scene.iter() {
            let lut = match &item {
                CompositorLayer::Layer(layer) => layer.lut,
                CompositorLayer::CanvasPass { lut, .. } => *lut,
                // Transition and matte sides skip LUTs (like effect chains).
                CompositorLayer::Transition { .. } | CompositorLayer::Matte { .. } => None,
            };
            if let Some(LayerLut { key, lut, .. }) = lut {
                self.ensure_lut(gpu, key, lut);
            }
        }
        self.layer_passes.begin_frame();
        self.plane_textures.borrow_mut().begin_frame();
        self.rgba_textures.borrow_mut().begin_frame();
        let target = self
            .target
            .as_ref()
            .expect("ensure_target populates the cache");
        let offscreen = self.offscreen.as_ref().expect("offscreen pool");
        offscreen.passes().begin_frame();

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("aria.encoder"),
        });

        // Clear canvas.
        {
            let bg = config.background;
            let _clear = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("aria.clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: f64::from(bg[0]) / 255.0,
                            g: f64::from(bg[1]) / 255.0,
                            b: f64::from(bg[2]) / 255.0,
                            a: f64::from(bg[3]) / 255.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }

        for item in scene.iter() {
            match &item {
                CompositorLayer::Layer(layer)
                    if !effects_need_offscreen(layer.effects)
                        && layer.lut.is_none()
                        && layer.blend_mode == BlendMode::Normal
                        && layer.styles.is_empty()
                        && layer.blur_passes.is_empty() =>
                {
                    let built = self.build_layer(gpu, config, layer)?;
                    let pipeline = pipeline_for(&built.pipeline, self);
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("aria.pass"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &target.view,
                            depth_slice: None,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Load,
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: None,
                        timestamp_writes: None,
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });
                    draw_built_layer(&mut pass, pipeline, &built);
                }
                CompositorLayer::Layer(layer) => {
                    // Motion blur: draw content at each sub-placement into a
                    // cleared accum target at weight 1/N (additive), then run
                    // effects/styles/blend on that average. Texture stays at T.
                    let base_view = if layer.blur_passes.is_empty() {
                        let built = self.build_layer(gpu, config, layer)?;
                        let pipeline = pipeline_for(&built.pipeline, self);
                        {
                            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                                label: Some("aria.offscreen.base"),
                                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                    view: offscreen.view(0),
                                    depth_slice: None,
                                    resolve_target: None,
                                    ops: wgpu::Operations {
                                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                                        store: wgpu::StoreOp::Store,
                                    },
                                })],
                                depth_stencil_attachment: None,
                                timestamp_writes: None,
                                occlusion_query_set: None,
                                multiview_mask: None,
                            });
                            draw_built_layer(&mut pass, pipeline, &built);
                        }
                        offscreen.view(0)
                    } else {
                        accumulate_motion_blur_samples(
                            self,
                            gpu,
                            device,
                            &mut encoder,
                            config,
                            offscreen,
                            layer,
                        )?;
                        offscreen.view(0)
                    };
                    let mut result = run_effect_chain(
                        device,
                        &mut encoder,
                        &self.pass_registry,
                        offscreen,
                        base_view,
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
                            &mut encoder,
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
                    // Style order: background → shadow → glow → content →
                    // outline. Content stays on S. Shadow/glow/outline
                    // silhouette+blur use A=(S+1)%3 and B=(S+2)%3. A/B are
                    // free again before content (and after blend's snapshot).
                    // Styles derive from the accumulated (motion-blurred)
                    // result — acceptable vs true shutter integration.
                    let src_slot = offscreen
                        .index_of(result)
                        .expect("effect/lut output is an offscreen pool view");
                    if let Some(bg) = layer.styles.background.filter(|b| b.rgba[3] > 0) {
                        let plate = background_plate_layer(&layer.placement, bg);
                        let built = self.build_layer(gpu, config, &plate)?;
                        let pipeline = pipeline_for(&built.pipeline, self);
                        {
                            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                                label: Some("aria.style.background"),
                                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                    view: &target.view,
                                    depth_slice: None,
                                    resolve_target: None,
                                    ops: wgpu::Operations {
                                        load: wgpu::LoadOp::Load,
                                        store: wgpu::StoreOp::Store,
                                    },
                                })],
                                depth_stencil_attachment: None,
                                timestamp_writes: None,
                                occlusion_query_set: None,
                                multiview_mask: None,
                            });
                            draw_built_layer(&mut pass, pipeline, &built);
                        }
                    }
                    if layer.styles.shadow.is_some() || layer.styles.glow.is_some() {
                        composite_layer_styles(
                            device,
                            &mut encoder,
                            &self.pass_registry,
                            offscreen,
                            result,
                            src_slot,
                            &target.view,
                            layer.styles.shadow,
                            layer.styles.glow,
                            config.width,
                            config.height,
                        );
                    }
                    if layer.blend_mode == BlendMode::Normal {
                        blit_premultiplied_to_canvas(
                            device,
                            &mut encoder,
                            &self.pass_registry,
                            offscreen.passes(),
                            result,
                            &target.view,
                        );
                    } else {
                        // See OffscreenPool slot invariant: result on S,
                        // canvas snapshot into (S+1)%slots, blend into canvas.
                        let snap_slot = (src_slot + 1) % offscreen.slots();
                        encoder.copy_texture_to_texture(
                            wgpu::TexelCopyTextureInfo {
                                texture: &target.texture,
                                mip_level: 0,
                                origin: wgpu::Origin3d::ZERO,
                                aspect: wgpu::TextureAspect::All,
                            },
                            wgpu::TexelCopyTextureInfo {
                                texture: offscreen.texture(snap_slot),
                                mip_level: 0,
                                origin: wgpu::Origin3d::ZERO,
                                aspect: wgpu::TextureAspect::All,
                            },
                            wgpu::Extent3d {
                                width: config.width,
                                height: config.height,
                                depth_or_array_layers: 1,
                            },
                        );
                        run_blend_composite(
                            device,
                            &mut encoder,
                            &self.pass_registry,
                            offscreen.passes(),
                            offscreen.view(snap_slot),
                            result,
                            &target.view,
                            layer.blend_mode,
                        );
                    }
                    if let Some(outline) = layer.styles.outline {
                        composite_layer_outline(
                            device,
                            &mut encoder,
                            &self.pass_registry,
                            offscreen,
                            result,
                            src_slot,
                            &target.view,
                            outline,
                            config.width,
                            config.height,
                        );
                    }
                }
                CompositorLayer::CanvasPass {
                    effects,
                    grade,
                    lut,
                } => {
                    if !effects_need_offscreen(effects) && grade.is_none() && lut.is_none() {
                        continue;
                    }

                    // Snapshot the current canvas, process it as a texture,
                    // then replace the canvas. Alpha-over blits would double
                    // the already-drawn stack.
                    blit_replace(
                        device,
                        &mut encoder,
                        &self.pass_registry,
                        offscreen.passes(),
                        &target.view,
                        offscreen.view(0),
                    );
                    let mut result = if effects_need_offscreen(effects) {
                        run_effect_chain(
                            device,
                            &mut encoder,
                            &self.pass_registry,
                            offscreen,
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
                            &mut encoder,
                            &self.pass_registry,
                            offscreen.passes(),
                            result,
                            grade_output,
                            *grade,
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
                            &mut encoder,
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
                    blit_replace(
                        device,
                        &mut encoder,
                        &self.pass_registry,
                        offscreen.passes(),
                        result,
                        &target.view,
                    );
                }
                CompositorLayer::Transition {
                    outgoing,
                    incoming,
                    transition_id,
                    progress,
                } => {
                    let out_built = self.build_layer(gpu, config, outgoing)?;
                    let in_built = self.build_layer(gpu, config, incoming)?;
                    let out_pipe = pipeline_for(&out_built.pipeline, self);
                    let in_pipe = pipeline_for(&in_built.pipeline, self);
                    {
                        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("aria.transition.out"),
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view: offscreen.view(0),
                                depth_slice: None,
                                resolve_target: None,
                                ops: wgpu::Operations {
                                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                                    store: wgpu::StoreOp::Store,
                                },
                            })],
                            depth_stencil_attachment: None,
                            timestamp_writes: None,
                            occlusion_query_set: None,
                            multiview_mask: None,
                        });
                        draw_built_layer(&mut pass, out_pipe, &out_built);
                    }
                    {
                        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("aria.transition.in"),
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view: offscreen.view(1),
                                depth_slice: None,
                                resolve_target: None,
                                ops: wgpu::Operations {
                                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                                    store: wgpu::StoreOp::Store,
                                },
                            })],
                            depth_stencil_attachment: None,
                            timestamp_writes: None,
                            occlusion_query_set: None,
                            multiview_mask: None,
                        });
                        draw_built_layer(&mut pass, in_pipe, &in_built);
                    }
                    run_transition_pass(
                        device,
                        &mut encoder,
                        &self.pass_registry,
                        offscreen.passes(),
                        offscreen.view(0),
                        offscreen.view(1),
                        offscreen.view(2),
                        transition_id,
                        *progress,
                        config.width,
                        config.height,
                    );
                    blit_premultiplied_to_canvas(
                        device,
                        &mut encoder,
                        &self.pass_registry,
                        offscreen.passes(),
                        offscreen.view(2),
                        &target.view,
                    );
                }
                CompositorLayer::Matte {
                    content,
                    stencil,
                    read,
                    invert,
                    softness,
                } => {
                    let content_built = self.build_layer(gpu, config, content)?;
                    let stencil_built = self.build_layer(gpu, config, stencil)?;
                    let content_pipe = pipeline_for(&content_built.pipeline, self);
                    let stencil_pipe = pipeline_for(&stencil_built.pipeline, self);
                    {
                        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("aria.matte.content"),
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view: offscreen.view(0),
                                depth_slice: None,
                                resolve_target: None,
                                ops: wgpu::Operations {
                                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                                    store: wgpu::StoreOp::Store,
                                },
                            })],
                            depth_stencil_attachment: None,
                            timestamp_writes: None,
                            occlusion_query_set: None,
                            multiview_mask: None,
                        });
                        draw_built_layer(&mut pass, content_pipe, &content_built);
                    }
                    {
                        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("aria.matte.stencil"),
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view: offscreen.view(1),
                                depth_slice: None,
                                resolve_target: None,
                                ops: wgpu::Operations {
                                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                                    store: wgpu::StoreOp::Store,
                                },
                            })],
                            depth_stencil_attachment: None,
                            timestamp_writes: None,
                            occlusion_query_set: None,
                            multiview_mask: None,
                        });
                        draw_built_layer(&mut pass, stencil_pipe, &stencil_built);
                    }
                    run_matte_pass(
                        device,
                        &mut encoder,
                        &self.pass_registry,
                        offscreen.passes(),
                        offscreen.view(0),
                        offscreen.view(1),
                        offscreen.view(2),
                        *read,
                        *invert,
                        *softness,
                        config.width,
                        config.height,
                    );
                    blit_premultiplied_to_canvas(
                        device,
                        &mut encoder,
                        &self.pass_registry,
                        offscreen.passes(),
                        offscreen.view(2),
                        &target.view,
                    );
                }
            }
        }

        Ok(encoder)
    }
}

fn draw_built_layer(
    pass: &mut wgpu::RenderPass<'_>,
    pipeline: &wgpu::RenderPipeline,
    built: &LayerGpu,
) {
    let instances = built.instances.as_ref().map(|i| (&i.buffer, i.count));
    draw_layer_to_offscreen(pass, pipeline, &built.bind_group, instances);
}

/// Draw each blur sample into slot 1, then weighted-additive into cleared slot 0.
fn required_slots_for_scene(scene: &Scene<'_>) -> usize {
    let mut max_slots = OffscreenPool::DEFAULT_SLOTS;
    for item in scene.iter() {
        match item {
            CompositorLayer::Layer(layer) => {
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
                if layer.lut.is_some() {
                    layer_slots = layer_slots.max(3);
                }
                max_slots = max_slots.max(layer_slots);
            }
            CompositorLayer::Transition { .. } => {
                max_slots = max_slots.max(4);
            }
            CompositorLayer::Matte { .. } => {
                max_slots = max_slots.max(3);
            }
            CompositorLayer::CanvasPass { effects, lut, .. } => {
                let mut pass_slots = 2usize;
                let chain = effects.iter().map(PassInstance::slot_demand).max();
                pass_slots = pass_slots.max(chain.unwrap_or(0));
                if lut.is_some() {
                    pass_slots = pass_slots.max(3);
                }
                max_slots = max_slots.max(pass_slots);
            }
        }
    }
    max_slots
}

fn accumulate_motion_blur_samples(
    comp: &Compositor,
    gpu: &GpuContext,
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    config: &CompositorConfig,
    offscreen: &OffscreenPool,
    layer: &CompositeLayer<'_>,
) -> Result<(), CompositorError> {
    let n = layer.blur_passes.len().max(1) as f32;
    let weight = 1.0 / n;
    // Clear accum (slot 0).
    {
        let _clear = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("aria.motion_blur.clear"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: offscreen.view(0),
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
    }
    for placement in &layer.blur_passes {
        let sample = layer_at_placement(layer, *placement);
        let built = comp.build_layer(gpu, config, &sample)?;
        let pipeline = pipeline_for(&built.pipeline, comp);
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("aria.motion_blur.sample"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: offscreen.view(1),
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            draw_built_layer(&mut pass, pipeline, &built);
        }
        blit_weighted_additive(
            device,
            encoder,
            &comp.pass_registry,
            offscreen.passes(),
            offscreen.view(1),
            offscreen.view(0),
            weight,
        );
    }
    Ok(())
}

/// Borrow `layer`'s content/fx at a different placement (no styles/blur nested).
fn layer_at_placement<'a>(
    layer: &CompositeLayer<'a>,
    placement: LayerPlacement,
) -> CompositeLayer<'a> {
    CompositeLayer {
        content: match &layer.content {
            LayerContent::Frame(f) => LayerContent::Frame(f),
            LayerContent::Rgba(i) => LayerContent::Rgba(i),
            LayerContent::Solid(c) => LayerContent::Solid(*c),
            LayerContent::Sdf(s) => LayerContent::Sdf(*s),
            LayerContent::Glyphs(g) => LayerContent::Glyphs(crate::layer::GlyphsLayer {
                atlas_key: g.atlas_key,
                glyphs: g.glyphs,
                instances: g.instances,
            }),
        },
        placement,
        uv: layer.uv,
        effects: &[], // effects run once on the accumulated average
        fx: layer.fx,
        color_grade: layer.color_grade,
        lut: None, // LUT runs once after accum
        blend_mode: BlendMode::Normal,
        styles: crate::layer::LayerStyles::default(),
        blur_passes: Vec::new(),
    }
}

fn pipeline_for<'a>(kind: &LayerPipeline, comp: &'a Compositor) -> &'a wgpu::RenderPipeline {
    match kind {
        LayerPipeline::Yuv => &comp.yuv_pipeline,
        LayerPipeline::YuvFx => &comp.yuv_fx_pipeline,
        LayerPipeline::Rgba => &comp.rgba_pipeline,
        LayerPipeline::RgbaFx => &comp.rgba_fx_pipeline,
        LayerPipeline::Solid => &comp.solid_pipeline,
        LayerPipeline::Sdf => &comp.sdf_pipeline,
        LayerPipeline::Glyphs => &comp.glyphs_pipeline,
    }
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
