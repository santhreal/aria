//! Contracts for reading texels out of a GPU-only Viewer frame.
//!
//! The class closed here: a pixel read from the frame on screen that reports
//! the wrong texel (row padding left in, origin ignored), reports premultiplied
//! colour as straight, or reads a region the texture does not cover instead of
//! refusing it. Not covered: which frame the Info panel asks about, which the
//! Info tests pin.

use aria_compositor::{create_frame_target, wgpu};
use gpui::{DevicePixels, GpuDevice, GpuTexture, size};

use super::read_texels;

const WIDTH: u32 = 5;
const HEIGHT: u32 = 3;

/// Premultiplied texel `(x, y)` of the test frame: every texel differs, and
/// the right column is half transparent.
fn texel(x: u32, y: u32) -> [u8; 4] {
    let alpha = if x == WIDTH - 1 { 128 } else { 255 };
    let channel = |base: u32| ((base * u32::from(alpha)) / 255) as u8;
    [
        channel(40 * x + 10),
        channel(60 * y + 20),
        channel(200),
        alpha,
    ]
}

/// The straight colour `texel(x, y)` stands for, as [`aria_core::RgbaImage`]
/// rounds it.
fn straight(x: u32, y: u32) -> [u8; 4] {
    let mut image = aria_core::RgbaImage::new(1, 1, texel(x, y).to_vec());
    image.unpremultiply();
    image.pixel(0, 0)
}

/// A frame target holding [`texel`], and the device that owns it: the one the
/// render plugin brings up. `None` on a host with no GPU.
fn frame() -> Option<(GpuDevice, GpuTexture)> {
    let gpu = crate::test_support::mounted_gpu(&aria_plugin::PluginContext::new())?;
    let raw = create_frame_target(&gpu.device, WIDTH, HEIGHT);
    let bytes: Vec<u8> = (0..HEIGHT)
        .flat_map(|y| (0..WIDTH).flat_map(move |x| texel(x, y)))
        .collect();
    gpu.queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &raw,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(WIDTH * 4),
            rows_per_image: Some(HEIGHT),
        },
        wgpu::Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
    );
    let view = raw.create_view(&wgpu::TextureViewDescriptor::default());
    let texture = GpuTexture::new(
        (raw, view),
        size(DevicePixels(WIDTH as i32), DevicePixels(HEIGHT as i32)),
    );
    let device = GpuDevice::new((gpu.instance, gpu.adapter, gpu.device, gpu.queue));
    Some((device, texture))
}

#[test]
fn every_region_reads_the_texels_it_covers_with_straight_alpha() {
    let Some((device, texture)) = frame() else {
        return;
    };
    // Every origin and every extent that fits: a single texel, a row narrower
    // than the copy alignment, the whole frame.
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            for height in 1..=HEIGHT - y {
                for width in 1..=WIDTH - x {
                    let read = read_texels(&device, &texture, [x, y], [width, height])
                        .unwrap_or_else(|| panic!("{width}x{height} at ({x}, {y}) was refused"));
                    assert_eq!((read.width, read.height), (width, height));
                    for row in 0..height {
                        for column in 0..width {
                            assert_eq!(
                                read.pixel(column, row),
                                straight(x + column, y + row),
                                "texel ({column}, {row}) of {width}x{height} at ({x}, {y})"
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn a_region_the_frame_does_not_cover_is_refused() {
    let Some((device, texture)) = frame() else {
        return;
    };
    for (origin, extent) in [
        ([0, 0], [0, 1]),
        ([0, 0], [1, 0]),
        ([WIDTH, 0], [1, 1]),
        ([0, HEIGHT], [1, 1]),
        ([WIDTH - 1, 0], [2, 1]),
        ([0, HEIGHT - 1], [1, 2]),
        ([u32::MAX, 0], [2, 1]),
        ([0, u32::MAX], [1, 2]),
    ] {
        assert!(
            read_texels(&device, &texture, origin, extent).is_none(),
            "{extent:?} at {origin:?} was read"
        );
    }
    // A handle that is not a wgpu device reads nothing.
    let foreign = GpuDevice::new(());
    assert!(read_texels(&foreign, &texture, [0, 0], [1, 1]).is_none());
}
