//! CPU [`RgbaImage`] → GPUI [`RenderImage`] (BGRA, aspect-contained).

use std::sync::Arc;

use aria_core::RgbaImage;
use gpui::RenderImage;
use image::{Frame, ImageBuffer, Rgba};
use smallvec::SmallVec;

/// Convert a tightly packed straight-alpha RGBA8 buffer into a GPUI texture.
///
/// GPUI caches processed frames as BGRA. Channel order is swapped here so
/// the preview matches the compositor's CPU readback.
pub fn rgba_to_render(frame: &RgbaImage) -> Option<Arc<RenderImage>> {
    if frame.width == 0 || frame.height == 0 || !frame.is_well_formed() {
        return None;
    }
    let mut bgra = frame.pixels.clone();
    for px in bgra.chunks_exact_mut(4) {
        px.swap(0, 2);
    }
    let buffer = ImageBuffer::<Rgba<u8>, _>::from_raw(frame.width, frame.height, bgra)?;
    Some(Arc::new(RenderImage::new(SmallVec::from_const([
        Frame::new(buffer),
    ]))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_and_malformed() {
        assert!(rgba_to_render(&RgbaImage::transparent(0, 8)).is_none());
        assert!(rgba_to_render(&RgbaImage::new(2, 2, vec![1, 2, 3])).is_none());
    }

    #[test]
    fn converts_well_formed_rgba() {
        let mut pixels = vec![0u8; 2 * 2 * 4];
        pixels[0] = 255;
        pixels[3] = 255;
        let image = rgba_to_render(&RgbaImage::new(2, 2, pixels)).expect("render image");
        assert_eq!(image.frame_count(), 1);
        let size = image.size(0);
        assert_eq!(u32::from(size.width), 2);
        assert_eq!(u32::from(size.height), 2);
    }
}
