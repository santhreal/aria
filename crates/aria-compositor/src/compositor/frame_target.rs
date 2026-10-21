//! A composited frame written into a caller texture, with no copy to the CPU.

use crate::error::CompositorError;
use crate::gpu::GpuContext;
use crate::layer::{CompositorConfig, CompositorLayer};

use super::scene::Scene;
use super::{CANVAS_FORMAT, Compositor};

/// Allocate a `width`×`height` `Rgba8Unorm` texture on `device` with
/// `RENDER_ATTACHMENT | COPY_DST | COPY_SRC | TEXTURE_BINDING` usage, ready for
/// [`Compositor::submit_compositor_layers_to_texture`] and window sampling.
pub fn create_frame_target(device: &wgpu::Device, width: u32, height: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("aria.preview_frame"),
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
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    })
}

/// Fails unless `texture` can receive a `width`×`height` frame: the canvas
/// format (`Rgba8Unorm`), `COPY_DST` usage, and that size.
pub fn check_frame_target(
    texture: &wgpu::Texture,
    width: u32,
    height: u32,
) -> Result<(), CompositorError> {
    let size = texture.size();
    if texture.format() == CANVAS_FORMAT
        && texture.usage().contains(wgpu::TextureUsages::COPY_DST)
        && (size.width, size.height) == (width, height)
    {
        return Ok(());
    }
    Err(CompositorError::FrameTarget {
        width,
        height,
        format: texture.format(),
        usage: texture.usage(),
        target_width: size.width,
        target_height: size.height,
    })
}

/// Maximum number of caller `FrameTarget` texture views retained across frames
/// (covers single targets and 3-slot ring sinks without per-frame view creation).
const FRAME_TARGET_VIEW_CACHE_CAP: usize = 4;

/// Whether the compositor can render a scene straight into `texture` without
/// staging through [`Compositor::target`] and copying.
fn can_render_directly_to(texture: &wgpu::Texture) -> bool {
    texture.sample_count() == 1
        && texture.mip_level_count() == 1
        && texture.dimension() == wgpu::TextureDimension::D2
        && texture.usage().contains(
            wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::TEXTURE_BINDING,
        )
}

impl Compositor {
    fn ensure_frame_target_view_index(&mut self, texture: &wgpu::Texture) -> usize {
        if let Some(idx) = self
            .frame_target_views
            .iter()
            .position(|(cached, _)| cached == texture)
        {
            return idx;
        }
        if self.frame_target_views.len() >= FRAME_TARGET_VIEW_CACHE_CAP {
            self.frame_target_views.remove(0);
        }
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.frame_target_views.push((texture.clone(), view));
        self.frame_target_views.len() - 1
    }

    /// Composite `layers` into `texture`, premultiplied.
    /// `texture` lives on `gpu` and passes [`check_frame_target`] for the
    /// config's size; any other texture is refused before anything is drawn.
    pub fn submit_compositor_layers_to_texture(
        &mut self,
        gpu: &GpuContext,
        config: &CompositorConfig,
        layers: &[CompositorLayer<'_>],
        texture: &wgpu::Texture,
    ) -> Result<(), CompositorError> {
        check_frame_target(texture, config.width, config.height)?;
        let scene = Scene::Full(layers);
        if can_render_directly_to(texture) {
            self.prepare_scene(gpu, config, &scene)?;
            let view_idx = self.ensure_frame_target_view_index(texture);
            let view = &self.frame_target_views[view_idx].1;
            let encoder = self.encode_prepared_scene(gpu, config, scene, (texture, view))?;
            self.submit_frame(gpu, encoder);
            return Ok(());
        }
        let mut encoder = self.encode_scene(gpu, config, scene)?;
        let canvas = &self
            .target
            .as_ref()
            .expect("encode_scene populates the target")
            .texture;
        encoder.copy_texture_to_texture(
            canvas.as_image_copy(),
            texture.as_image_copy(),
            wgpu::Extent3d {
                width: config.width,
                height: config.height,
                depth_or_array_layers: 1,
            },
        );
        self.submit_frame(gpu, encoder);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer::{BlendMode, CompositeLayer, LayerPlacement};

    fn read_texture_rgba(
        gpu: &GpuContext,
        texture: &wgpu::Texture,
        width: u32,
        height: u32,
    ) -> Vec<u8> {
        let padded = (width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("test.readback"),
            size: u64::from(padded) * u64::from(height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        enc.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        gpu.queue.submit(std::iter::once(enc.finish()));
        let slice = buf.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
        let mapped = slice.get_mapped_range();
        let mut out = Vec::with_capacity((width * height * 4) as usize);
        for row in 0..height as usize {
            let start = row * padded as usize;
            out.extend_from_slice(&mapped[start..start + (width as usize * 4)]);
        }
        out
    }

    /// WHY: `submit_compositor_layers_to_texture` previously rendered every
    /// preview frame into `self.target` (`aria.canvas` + `ReadbackPool`) and
    /// copied the full canvas into `texture` with `copy_texture_to_texture`,
    /// and `encode_scene` opened a standalone clear render pass before the
    /// first direct layer pass. Closes the class of redundant canvas staging
    /// allocations and texture-to-texture copies when the caller provides a
    /// renderable `FrameTarget`, while preserving the `self.target` copy
    /// fallback for `COPY_DST`-only caller textures and exact pixel parity
    /// across empty scenes, single direct layers, and multi-layer blends.
    #[test]
    fn a_renderable_caller_texture_is_drawn_directly_and_matches_the_copy_fallback() {
        let gpu = GpuContext::new_headless_blocking().expect("headless GPU context");
        let mut comp = Compositor::new(&gpu);
        let mut config = CompositorConfig::new(8, 6);
        config.background = [12, 34, 56, 255];

        let direct_target = create_frame_target(&gpu.device, 8, 6);
        assert!(
            direct_target
                .usage()
                .contains(wgpu::TextureUsages::RENDER_ATTACHMENT),
            "create_frame_target must include RENDER_ATTACHMENT so the compositor renders straight into it"
        );

        // 1. Empty scene (background clear only) + direct layer + blended overlay.
        comp.submit_compositor_layers_to_texture(&gpu, &config, &[], &direct_target)
            .expect("empty scene renders directly");
        assert!(
            comp.target.is_none(),
            "direct FrameTarget rendering must not allocate internal TargetCache + ReadbackPool"
        );
        let empty_direct = read_texture_rgba(&gpu, &direct_target, 8, 6);
        assert_eq!(&empty_direct[..4], &[12, 34, 56, 255]);

        let base = CompositeLayer::solid([200, 40, 60, 255], LayerPlacement::full_canvas(&config));
        let mut overlay = CompositeLayer::solid(
            [20, 180, 90, 128],
            LayerPlacement {
                center: [4.0, 3.0],
                size: [4.0, 4.0],
                ..LayerPlacement::full_canvas(&config)
            },
        );
        overlay.blend_mode = BlendMode::Screen;
        let layers = [
            CompositorLayer::layer(&base),
            CompositorLayer::layer(&overlay),
        ];

        let ring = [
            create_frame_target(&gpu.device, 8, 6),
            create_frame_target(&gpu.device, 8, 6),
            create_frame_target(&gpu.device, 8, 6),
        ];
        for cycle in 0..2 {
            for tex in &ring {
                comp.submit_compositor_layers_to_texture(&gpu, &config, &layers, tex)
                    .expect("ring target renders directly");
            }
            assert!(
                comp.target.is_none(),
                "cycle {cycle}: direct rendering across a 3-texture ring must not allocate TargetCache"
            );
            assert_eq!(
                comp.frame_target_views.len(),
                4,
                "cycle {cycle}: direct_target plus 3-texture ring must reuse cached TextureViews without growing past 4"
            );
        }
        let direct_pixels = read_texture_rgba(&gpu, &ring[0], 8, 6);

        // 2. Fallback path: a COPY_DST | TEXTURE_BINDING texture without RENDER_ATTACHMENT
        // (such as a plugin host texture) stages through `self.target` and copies.
        let copy_only_target = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("test.copy_only"),
            size: wgpu::Extent3d {
                width: 8,
                height: 6,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: CANVAS_FORMAT,
            usage: wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        comp.submit_compositor_layers_to_texture(&gpu, &config, &layers, &copy_only_target)
            .expect("copy-only target renders via staging canvas");
        assert!(
            comp.target.is_some(),
            "COPY_DST-only target must allocate staging TargetCache and copy"
        );
        let fallback_pixels = read_texture_rgba(&gpu, &copy_only_target, 8, 6);
        assert_eq!(
            direct_pixels, fallback_pixels,
            "direct FrameTarget render must match staging-canvas copy byte for byte"
        );
    }
}
