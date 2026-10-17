//! Pixels read out of the frame on screen when it has no CPU copy.
//!
//! A Viewer frame presented as a GPU texture is never read back, so the Info
//! panel's colour under the pointer and the exposure readout copy the texels
//! they need out of that texture. A pointer move copies one texel on the
//! preview worker, so presentation keeps its zero-readback path and the window
//! thread never waits on the GPU queue.

use aria_compositor::wgpu;
use aria_core::RgbaImage;
use gpui::{GpuDevice, GpuTexture};

/// One pixel the Info panel asked for: the canvas pixel under the pointer, and
/// the texel of the frame on screen that covers it.
#[derive(Debug, Clone)]
pub struct ProbeAsk {
    /// The texture of the frame on screen.
    pub texture: GpuTexture,
    /// The texel of `texture` to read.
    pub texel: [u32; 2],
    /// The canvas pixel the texel covers.
    pub at: [u32; 2],
    /// The sequence tick the frame shows.
    pub tick: i64,
}

/// What a probe read: the canvas pixel, the frame it was read from, and the
/// straight-alpha colour there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Probed {
    pub at: [u32; 2],
    pub tick: i64,
    pub rgba: [u8; 4],
}

/// The `size` texels of `texture` at `origin`, straight alpha, tightly packed.
///
/// Blocks until the GPU queue has drained up to the copy. `None` when `device`
/// is not a wgpu device, `texture` is not an `Rgba8Unorm` frame target, the
/// region is empty or leaves the texture, or the copy fails.
pub fn read_texels(
    device: &GpuDevice,
    texture: &GpuTexture,
    origin: [u32; 2],
    size: [u32; 2],
) -> Option<RgbaImage> {
    let (_, _, device, queue) =
        device.downcast_ref::<(wgpu::Instance, wgpu::Adapter, wgpu::Device, wgpu::Queue)>()?;
    let (raw, _) = texture.downcast_ref::<(wgpu::Texture, wgpu::TextureView)>()?;
    let extent = raw.size();
    let ([x, y], [width, height]) = (origin, size);
    let inside = width > 0
        && height > 0
        && x.checked_add(width)
            .is_some_and(|right| right <= extent.width)
        && y.checked_add(height)
            .is_some_and(|bottom| bottom <= extent.height);
    if !inside || raw.format() != wgpu::TextureFormat::Rgba8Unorm {
        return None;
    }
    let row = width * 4;
    let padded =
        row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("aria.preview_probe"),
        size: u64::from(padded) * u64::from(height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("aria.preview_probe"),
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: raw,
            mip_level: 0,
            origin: wgpu::Origin3d { x, y, z: 0 },
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
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
    queue.submit(std::iter::once(encoder.finish()));
    let slice = buffer.slice(..);
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = sender.send(result);
    });
    device.poll(wgpu::PollType::wait_indefinitely()).ok()?;
    receiver.recv().ok()?.ok()?;
    let mut pixels = Vec::with_capacity(row as usize * height as usize);
    for line in slice.get_mapped_range().chunks_exact(padded as usize) {
        pixels.extend_from_slice(&line[..row as usize]);
    }
    buffer.unmap();
    // The compositor writes premultiplied texels; every CPU frame is straight.
    let mut image = RgbaImage::try_new(width, height, pixels).ok()?;
    image.unpremultiply();
    Some(image)
}

#[cfg(test)]
#[path = "preview_worker_probe_tests.rs"]
mod tests;
