#[cfg(any(target_vendor = "apple", target_os = "windows", target_os = "linux"))]
use aria_core::PixelFormat;
use aria_core::{FrameData, RgbaImage, VideoFrame};

use crate::error::CompositorError;
use crate::gpu::GpuContext;
use crate::grade::ColorGrade;
use crate::layer::{CompositeLayer, CompositorConfig, LayerContent, LayerEffects, LayerPlacement};
use crate::planes::PlaneSet;

use super::upload::{
    luma_coeffs, pack_grade, placement_affine, placement_affine_rot, placement_rotation,
    rgba_image_key, upload_cpu_planes, visible_uv_rect,
};
use super::{
    Bound, Compositor, FxEffectsUniform, LayerGpu, LayerPipeline, RgbaUniforms, SdfUniforms,
    SolidUniforms, YuvUniforms,
};

impl Compositor {
    pub(super) fn build_layer(
        &self,
        gpu: &GpuContext,
        config: &CompositorConfig,
        layer: &CompositeLayer<'_>,
    ) -> Result<LayerGpu, CompositorError> {
        let grade = layer.color_grade;
        match &layer.content {
            LayerContent::Solid(rgba) => self.build_solid(gpu, config, layer, *rgba, grade),
            LayerContent::Frame(frame) => self.build_frame(gpu, config, layer, frame, grade),
            LayerContent::Rgba(image) => self.build_rgba(gpu, config, layer, image, grade),
            LayerContent::Sdf(shape) => self.build_sdf(gpu, config, layer, shape, grade),
            LayerContent::Glyphs(glyphs) => self.build_glyphs(gpu, config, layer, glyphs, grade),
        }
    }

    fn build_sdf(
        &self,
        gpu: &GpuContext,
        config: &CompositorConfig,
        layer: &CompositeLayer<'_>,
        shape: &crate::layer::SdfLayer,
        color_grade: Option<ColorGrade>,
    ) -> Result<LayerGpu, CompositorError> {
        use aria_shapes::SdfParams;

        let placement = &layer.placement;
        let (linear, mut trans) = placement_affine(config, &layer.placement, 0.0);
        let stroke = shape.stroke.filter(|s| s.width > 0.0);
        trans[3] = stroke.map_or(0.0, |s| s.width);

        let h = shape.shape.half;
        // Shape kind ids and parameter packing per shape.wgsl's `Sdf` block.
        let (kind, radius, star) = match shape.shape.params {
            SdfParams::RoundedRect { radius } => (0.0, radius, [0.0, 0.0]),
            SdfParams::Ellipse => (1.0, 0.0, [0.0, 0.0]),
            SdfParams::Star {
                points,
                inner,
                round,
            } => (2.0, round, [points as f32, inner]),
            SdfParams::Line => (3.0, 0.0, [0.0, 0.0]),
            SdfParams::Arrow => (4.0, 0.0, [0.0, 0.0]),
            SdfParams::Heart => (5.0, 0.0, [0.0, 0.0]),
        };

        let color = |rgba: [u8; 4]| {
            [
                f32::from(rgba[0]) / 255.0,
                f32::from(rgba[1]) / 255.0,
                f32::from(rgba[2]) / 255.0,
                f32::from(rgba[3]) / 255.0,
            ]
        };
        let grade = pack_grade(color_grade);
        let uniforms = SdfUniforms {
            fill: color(shape.fill),
            stroke_color: stroke.map_or([0.0; 4], |s| color(s.rgba)),
            linear,
            trans_opacity: trans,
            geo: [h[0], h[1], kind, radius],
            star: [
                star[0],
                star[1],
                placement.size[0] * 0.5,
                placement.size[1] * 0.5,
            ],
            grade,
        };
        let bind_group = self.layer_bind_group(
            gpu,
            &self.sdf_layout,
            "aria.sdf.bg",
            &[Bound::Uniform(bytemuck::bytes_of(&uniforms))],
        );
        Ok(LayerGpu {
            pipeline: LayerPipeline::Sdf,
            bind_group,
            _planes: None,
            _keep_alive: None,
            instances: None,
        })
    }

    fn build_solid(
        &self,
        gpu: &GpuContext,
        config: &CompositorConfig,
        layer: &CompositeLayer<'_>,
        rgba: [u8; 4],
        color_grade: Option<ColorGrade>,
    ) -> Result<LayerGpu, CompositorError> {
        let (linear, trans) = placement_affine(config, &layer.placement, 0.0);
        let grade = pack_grade(color_grade);
        let uniforms = SolidUniforms {
            color: [
                f32::from(rgba[0]) / 255.0,
                f32::from(rgba[1]) / 255.0,
                f32::from(rgba[2]) / 255.0,
                f32::from(rgba[3]) / 255.0,
            ],
            linear,
            trans_opacity: trans,
            grade,
        };
        let bind_group = self.layer_bind_group(
            gpu,
            &self.solid_layout,
            "aria.solid.bg",
            &[Bound::Uniform(bytemuck::bytes_of(&uniforms))],
        );
        Ok(LayerGpu {
            pipeline: LayerPipeline::Solid,
            bind_group,
            _planes: None,
            _keep_alive: None,
            instances: None,
        })
    }

    fn build_frame(
        &self,
        gpu: &GpuContext,
        config: &CompositorConfig,
        layer: &CompositeLayer<'_>,
        frame: &VideoFrame,
        color_grade: Option<ColorGrade>,
    ) -> Result<LayerGpu, CompositorError> {
        let (cw, ch) = frame.coded_size;
        if cw == 0 || ch == 0 {
            return Err(CompositorError::MalformedFrame(format!(
                "coded size is {cw}x{ch}"
            )));
        }

        // Acquire the plane textures: CPU frames upload their planes; GPU frames
        // map their native surface straight into textures with no pixel copy.
        // `nv12` selects the shader's chroma path; `keep_alive` parks any native
        // wrappers (e.g. CVMetalTexture) that must outlive the GPU read.
        let (planes, keep_alive) = match &frame.data {
            FrameData::Cpu(image) => {
                let planes = upload_cpu_planes(
                    gpu,
                    &mut self.plane_textures.borrow_mut(),
                    frame,
                    image,
                    cw,
                    ch,
                )?;
                (planes, None)
            }
            FrameData::Gpu(surface) => self.import_gpu_planes(gpu, frame, surface, cw, ch)?,
        };
        let nv12 = planes.is_biplanar();

        let resolved = frame.color.resolved();
        let (kr, kb) = luma_coeffs(resolved.matrix);
        let full = if resolved.range.is_full() { 1.0 } else { 0.0 };
        let plane_mode = if nv12 { 1.0 } else { 0.0 };

        // Container rotation rides on top of the placement rotation.
        let rotation = placement_rotation(layer, frame);
        let (linear, trans) = placement_affine_rot(config, &layer.placement, rotation);
        let uv_rect = visible_uv_rect(frame, layer.uv);

        let grade = pack_grade(color_grade);
        let uniforms = YuvUniforms {
            linear,
            trans_opacity: trans,
            uv_rect,
            coeffs: [kr, kb, full, plane_mode],
            grade,
        };

        // For NV12 the V slot is bound to the same CbCr texture (unsampled).
        let (y_view, u_view, v_view) = planes.yuv_views().ok_or_else(|| {
            CompositorError::MalformedFrame("video layer has no YUV planes".into())
        })?;

        if layer.fx.is_identity() {
            let bind_group = self.layer_bind_group(
                gpu,
                &self.yuv_layout,
                "aria.yuv.bg",
                &[
                    Bound::Texture(y_view),
                    Bound::Texture(u_view),
                    Bound::Texture(v_view),
                    Bound::Sampler(&self.sampler),
                    Bound::Uniform(bytemuck::bytes_of(&uniforms)),
                ],
            );
            return Ok(LayerGpu {
                pipeline: LayerPipeline::Yuv,
                bind_group,
                _planes: Some(planes),
                _keep_alive: keep_alive,
                instances: None,
            });
        }

        let fx_uniforms = pack_fx_uniforms(&layer.fx, &layer.placement);
        let bind_group = self.layer_bind_group(
            gpu,
            &self.yuv_fx_layout,
            "aria.yuv_fx.bg",
            &[
                Bound::Texture(y_view),
                Bound::Texture(u_view),
                Bound::Texture(v_view),
                Bound::Sampler(&self.sampler),
                Bound::Uniform(bytemuck::bytes_of(&uniforms)),
                Bound::Uniform(bytemuck::bytes_of(&fx_uniforms)),
            ],
        );

        Ok(LayerGpu {
            pipeline: LayerPipeline::YuvFx,
            bind_group,
            _planes: Some(planes),
            _keep_alive: keep_alive,
            instances: None,
        })
    }

    /// Map a zero-copy [`FrameData::Gpu`] surface into plane textures the YUV
    /// pipeline can sample. Returns the planes and a native keep-alive bundle.
    /// On platforms/formats without an import path this is a
    /// [`CompositorError::UnsupportedFormat`] so the caller can fall back to a
    /// CPU-mode decode.
    fn import_gpu_planes(
        &self,
        gpu: &GpuContext,
        frame: &VideoFrame,
        surface: &aria_core::GpuSurface,
        cw: u32,
        ch: u32,
    ) -> Result<(PlaneSet, Option<Box<dyn core::any::Any + Send>>), CompositorError> {
        #[cfg(target_vendor = "apple")]
        {
            use aria_core::GpuSurfaceKind;
            match surface.kind() {
                GpuSurfaceKind::CoreVideoPixelBuffer => {
                    if frame.format != PixelFormat::Nv12 {
                        return Err(CompositorError::UnsupportedFormat(format!(
                            "zero-copy import supports NV12 CoreVideo surfaces; got {:?}",
                            frame.format
                        )));
                    }
                    let importer = self.metal_import.as_ref().ok_or_else(|| {
                        CompositorError::UnsupportedFormat(
                            "Metal surface import is unavailable on this device".into(),
                        )
                    })?;
                    let (planes, keep) = importer.import_nv12(gpu, surface, (cw, ch))?;
                    Ok((planes, Some(Box::new(keep))))
                }
                other => Err(CompositorError::UnsupportedFormat(format!(
                    "GPU surface kind {other:?} import is not implemented yet"
                ))),
            }
        }
        #[cfg(target_os = "windows")]
        {
            use aria_core::GpuSurfaceKind;
            match surface.kind() {
                GpuSurfaceKind::D3D11Texture2D => {
                    if frame.format != PixelFormat::Nv12 {
                        return Err(CompositorError::UnsupportedFormat(format!(
                            "zero-copy import supports NV12 D3D11 surfaces; got {:?}",
                            frame.format
                        )));
                    }
                    let importer = self.dx12_import.as_ref().ok_or_else(|| {
                        CompositorError::UnsupportedFormat(
                            "D3D12 surface import is unavailable on this device".into(),
                        )
                    })?;
                    let planes = importer.import_nv12(gpu, surface, (cw, ch))?;
                    // No compositor-side keep-alive needed: the wgpu texture
                    // holds the opened ID3D12Resource, and the decoder-side
                    // pool slot rides in the *frame's* keep-alive, which the
                    // renderer holds until the pass completes.
                    Ok((planes, None))
                }
                other => Err(CompositorError::UnsupportedFormat(format!(
                    "GPU surface kind {other:?} import is not implemented yet"
                ))),
            }
        }
        // Android (`AHardwareBuffer`) import is not wired yet; the decoder
        // delivers CPU planes there, so this branch is currently unreachable
        // in practice. See the historical note in git history for the AHB
        // YCbCr-sampler blocker (wgpu 29 cannot express immutable YCbCr
        // samplers or `VK_FORMAT_UNDEFINED` external formats).
        #[cfg(target_os = "linux")]
        {
            use aria_core::GpuSurfaceKind;
            match surface.kind() {
                GpuSurfaceKind::DmaBuf => {
                    if frame.format != PixelFormat::Nv12 {
                        return Err(CompositorError::UnsupportedFormat(format!(
                            "zero-copy import supports NV12 DMABuf surfaces; got {:?}",
                            frame.format
                        )));
                    }
                    let importer = self.vulkan_import.as_ref().ok_or_else(|| {
                        CompositorError::UnsupportedFormat(
                            "Vulkan DMABuf import is unavailable on this device".into(),
                        )
                    })?;
                    let planes = importer.import_nv12(gpu, surface, (cw, ch))?;
                    // wgpu holds the imported VkImage; the decoder keep-alive
                    // on the frame holds the DMA-BUF fds until the pass ends.
                    Ok((planes, None))
                }
                other => Err(CompositorError::UnsupportedFormat(format!(
                    "GPU surface kind {other:?} import is not implemented yet"
                ))),
            }
        }
        #[cfg(not(any(target_vendor = "apple", target_os = "windows", target_os = "linux")))]
        {
            let _ = (gpu, frame, surface, cw, ch);
            Err(CompositorError::UnsupportedFormat(
                "zero-copy GPU surface import is not implemented on this platform yet".into(),
            ))
        }
    }

    fn build_rgba(
        &self,
        gpu: &GpuContext,
        config: &CompositorConfig,
        layer: &CompositeLayer<'_>,
        image: &RgbaImage,
        color_grade: Option<ColorGrade>,
    ) -> Result<LayerGpu, CompositorError> {
        if image.width == 0 || image.height == 0 {
            return Err(CompositorError::MalformedFrame(format!(
                "rgba bitmap is {}x{}",
                image.width, image.height
            )));
        }
        let expected = (image.width as usize)
            .checked_mul(image.height as usize)
            .and_then(|n| n.checked_mul(4))
            .ok_or_else(|| CompositorError::MalformedFrame("rgba bitmap size overflow".into()))?;
        if image.pixels.len() < expected {
            return Err(CompositorError::MalformedFrame(format!(
                "rgba bitmap: {} bytes < {}×{}×4",
                image.pixels.len(),
                image.width,
                image.height
            )));
        }

        let image_key = rgba_image_key(image, expected);
        let cached = self.rgba_textures.borrow_mut().get(&image_key);
        let (texture, view) = if let Some(cached) = cached {
            cached
        } else {
            // Premultiply straight-alpha input so bilinear sampling and src-over
            // blending do not leak transparent-texel color into glyph edges.
            // Oversized unwrapped text is fitted only for upload; the authored
            // on-canvas placement below remains unchanged.
            let max_dimension = gpu.device.limits().max_texture_dimension_2d;
            let mut premul = self.premul_scratch.borrow_mut();
            let (upload_width, upload_height) =
                premultiply_for_upload(image, expected, max_dimension, &mut premul)?;
            let upload_len = upload_width as usize * upload_height as usize * 4;
            let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("aria.rgba.tex"),
                size: wgpu::Extent3d {
                    width: upload_width,
                    height: upload_height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            gpu.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &premul[..upload_len],
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(upload_width * 4),
                    rows_per_image: Some(upload_height),
                },
                wgpu::Extent3d {
                    width: upload_width,
                    height: upload_height,
                    depth_or_array_layers: 1,
                },
            );
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            self.rgba_textures
                .borrow_mut()
                .insert(image_key, (texture.clone(), view.clone()));
            (texture, view)
        };

        let (linear, trans) = placement_affine(config, &layer.placement, 0.0);
        let grade = pack_grade(color_grade);
        let placement_uniforms = RgbaUniforms {
            linear,
            trans_opacity: trans,
            uv_rect: layer.uv,
            grade,
        };

        if layer.fx.is_identity() {
            let bind_group = self.layer_bind_group(
                gpu,
                &self.rgba_layout,
                "aria.rgba.bg",
                &[
                    Bound::Texture(&view),
                    Bound::Sampler(&self.sampler),
                    Bound::Uniform(bytemuck::bytes_of(&placement_uniforms)),
                ],
            );

            return Ok(LayerGpu {
                pipeline: LayerPipeline::Rgba,
                bind_group,
                _planes: Some(PlaneSet::Single((texture, view))),
                _keep_alive: None,
                instances: None,
            });
        }

        let fx_uniforms = pack_fx_uniforms(&layer.fx, &layer.placement);
        let bind_group = self.layer_bind_group(
            gpu,
            &self.rgba_fx_layout,
            "aria.rgba_fx.bg",
            &[
                Bound::Texture(&view),
                Bound::Sampler(&self.sampler),
                Bound::Uniform(bytemuck::bytes_of(&placement_uniforms)),
                Bound::Uniform(bytemuck::bytes_of(&fx_uniforms)),
            ],
        );

        Ok(LayerGpu {
            pipeline: LayerPipeline::RgbaFx,
            bind_group,
            _planes: Some(PlaneSet::Single((texture, view))),
            _keep_alive: None,
            instances: None,
        })
    }
}

/// Premultiply straight-alpha RGBA into `scratch`, reducing oversized bitmaps
/// to the device's 2D texture limit, and return the upload size in pixels.
///
/// On return the first `width * height * 4` bytes of `scratch` are the upload.
/// The buffer is the caller's to keep: this runs for every bitmap layer of
/// every frame, and the conversion is the only reason a copy exists at all.
fn premultiply_for_upload(
    image: &RgbaImage,
    expected: usize,
    max_dimension: u32,
    scratch: &mut Vec<u8>,
) -> Result<(u32, u32), CompositorError> {
    if scratch.len() < expected {
        scratch.resize(expected, 0);
    }
    for (dst, src) in scratch[..expected]
        .chunks_exact_mut(4)
        .zip(image.pixels.chunks_exact(4))
    {
        let a = u16::from(src[3]);
        dst[0] = ((u16::from(src[0]) * a + 127) / 255) as u8;
        dst[1] = ((u16::from(src[1]) * a + 127) / 255) as u8;
        dst[2] = ((u16::from(src[2]) * a + 127) / 255) as u8;
        dst[3] = src[3];
    }

    if image.width <= max_dimension && image.height <= max_dimension {
        return Ok((image.width, image.height));
    }
    if max_dimension == 0 {
        return Err(CompositorError::MalformedFrame(
            "GPU reports a zero 2D texture limit".into(),
        ));
    }

    let scale = (max_dimension as f64 / f64::from(image.width))
        .min(max_dimension as f64 / f64::from(image.height));
    let width = ((f64::from(image.width) * scale).round() as u32).clamp(1, max_dimension);
    let height = ((f64::from(image.height) * scale).round() as u32).clamp(1, max_dimension);
    // Rare path (text wider than the GPU's texture limit): the `image` crate
    // resampler wants an owned buffer, so hand it the scratch and take the
    // resized one back.
    scratch.truncate(expected);
    let source = image::RgbaImage::from_raw(image.width, image.height, std::mem::take(scratch))
        .ok_or_else(|| CompositorError::MalformedFrame("invalid RGBA upload buffer".into()))?;
    let resized = image::imageops::resize(
        &source,
        width,
        height,
        image::imageops::FilterType::Lanczos3,
    );
    *scratch = resized.into_raw();
    Ok((width, height))
}

/// Floor for mask size axes so the shader can divide without a zero check.
const MASK_SIZE_EPS: f32 = 1e-3;

fn pack_fx_uniforms(effects: &LayerEffects, placement: &LayerPlacement) -> FxEffectsUniform {
    let (mask, half_zw, mask_geo) = match effects.mask {
        Some(m) => (
            [m.kind as f32, m.feather, m.invert as f32, 1.0],
            [m.rotation_rad, m.roundness],
            [
                m.center[0],
                m.center[1],
                m.size[0].max(MASK_SIZE_EPS),
                m.size[1].max(MASK_SIZE_EPS),
            ],
        ),
        None => ([0.0, 0.0, 0.0, 0.0], [0.0, 0.0], [0.0, 0.0, 1.0, 1.0]),
    };
    let chroma = effects
        .chroma_key
        .map(|c| [c.rgb[0], c.rgb[1], c.rgb[2], 1.0]);
    let chroma_params = effects.chroma_key.map(|c| [c.strength, c.shadow, 0.0, 0.0]);
    FxEffectsUniform {
        mask,
        chroma: chroma.unwrap_or([0.0, 0.0, 0.0, 0.0]),
        chroma_params: chroma_params.unwrap_or([0.0, 0.0, 0.0, 0.0]),
        half: [
            placement.size[0] * 0.5,
            placement.size[1] * 0.5,
            half_zw[0],
            half_zw[1],
        ],
        mask_geo,
    }
}

#[cfg(test)]
mod tests {
    use super::premultiply_for_upload;
    use crate::{CompositeLayer, Compositor, CompositorConfig, GpuContext, LayerPlacement};
    use aria_core::RgbaImage;

    #[test]
    fn rgba_upload_premultiplies_without_resizing_small_images() {
        let image = RgbaImage::new(1, 1, vec![200, 100, 50, 128]);
        let mut scratch = Vec::new();
        let size = premultiply_for_upload(&image, 4, 16_384, &mut scratch).unwrap();
        assert_eq!(size, (1, 1));
        assert_eq!(scratch[..4], [100, 50, 25, 128]);
    }

    #[test]
    fn rgba_upload_overwrites_every_byte_of_a_reused_scratch() {
        let image = RgbaImage::new(1, 1, vec![200, 100, 50, 128]);
        let mut scratch = vec![0xAB; 64];
        premultiply_for_upload(&image, 4, 16_384, &mut scratch).unwrap();
        assert_eq!(scratch[..4], [100, 50, 25, 128]);
    }

    #[test]
    fn rgba_upload_fits_unwrapped_text_to_gpu_texture_limit() {
        let image = RgbaImage::new(19_285, 4, vec![255; 19_285 * 4 * 4]);
        let mut scratch = Vec::new();
        let (width, height) =
            premultiply_for_upload(&image, image.pixels.len(), 16_384, &mut scratch).unwrap();
        assert_eq!(width, 16_384);
        assert!(height <= 16_384);
        assert_eq!(scratch.len(), width as usize * height as usize * 4);
    }

    #[test]
    fn compositor_accepts_bitmap_wider_than_device_texture_limit() {
        let Ok(gpu) = GpuContext::new_headless_blocking() else {
            return;
        };
        let width = gpu.device.limits().max_texture_dimension_2d + 1;
        let bitmap = RgbaImage::new(width, 2, vec![255; width as usize * 2 * 4]);
        let config = CompositorConfig::new(8, 8);
        let layer = CompositeLayer::rgba(&bitmap, LayerPlacement::full_canvas(&config));
        let mut compositor = Compositor::new(&gpu);
        let rendered = compositor
            .render(&gpu, &config, &[layer])
            .expect("oversized RGBA layer should be fitted before upload");
        assert_eq!(rendered.pixel(4, 4), [255, 255, 255, 255]);
    }
}
