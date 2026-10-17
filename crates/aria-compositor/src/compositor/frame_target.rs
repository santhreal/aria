//! A composited frame written into a caller texture, with no copy to the CPU.

use crate::error::CompositorError;
use crate::gpu::GpuContext;
use crate::layer::{CompositorConfig, CompositorLayer};

use super::scene::Scene;
use super::{CANVAS_FORMAT, Compositor};

/// Allocate a `width`×`height` `Rgba8Unorm` texture on `device` with
/// `COPY_DST | COPY_SRC | TEXTURE_BINDING` usage, ready for
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
        usage: wgpu::TextureUsages::COPY_DST
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

impl Compositor {
    /// Composite `layers` and copy the canvas into `texture`, premultiplied.
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
        let mut encoder = self.encode_scene(gpu, config, Scene::Full(layers))?;
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
        gpu.queue.submit(std::iter::once(encoder.finish()));
        Ok(())
    }
}
