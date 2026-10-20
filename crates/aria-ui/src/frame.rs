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
    if !frame.is_well_formed() {
        return None;
    }
    rgba_bytes_to_render(frame.width, frame.height, &frame.pixels)
}

/// [`rgba_to_render`] over borrowed pixels: `width * height * 4` straight-alpha
/// RGBA8 bytes, copied once into the BGRA texture.
pub fn rgba_bytes_to_render(width: u32, height: u32, rgba: &[u8]) -> Option<Arc<RenderImage>> {
    let len = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))?;
    if width == 0 || height == 0 || rgba.len() != len {
        return None;
    }
    let mut bgra = rgba.to_vec();
    for px in bgra.as_chunks_mut::<4>().0 {
        px.swap(0, 2);
    }
    bgra_to_render(width, height, bgra)
}

/// Edge of one square of the transparency grid, in frame pixels.
pub const GRID_CELL: usize = 8;

/// The transparency grid's two greys as RGB: the square at the frame's
/// top-left corner, then its neighbours.
pub const GRID_GREYS: [[u8; 3]; 2] = [[102, 102, 102], [153, 153, 153]];

/// [`rgba_bytes_to_render`] for a frame rendered over a transparent canvas.
///
/// `rgba` is premultiplied, as the compositor reads it back. Each pixel is
/// laid over the transparency grid, so the image is opaque and shows the
/// grid in proportion to how transparent the pixel is.
pub fn rgba_over_grid_to_render(width: u32, height: u32, rgba: &[u8]) -> Option<Arc<RenderImage>> {
    let len = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))?;
    if width == 0 || height == 0 || rgba.len() != len {
        return None;
    }
    let row = width as usize;
    let mut bgra = Vec::with_capacity(len);
    for (index, px) in rgba.as_chunks::<4>().0.iter().enumerate() {
        let [r, g, b, alpha] = *px;
        if alpha == u8::MAX {
            bgra.extend_from_slice(&[b, g, r, u8::MAX]);
            continue;
        }
        let (x, y) = (index % row, index / row);
        let grey = GRID_GREYS[(x / GRID_CELL + y / GRID_CELL) % 2];
        let uncovered = u16::from(u8::MAX - alpha);
        let over = |colour: u8, under: u8| {
            let shown = u16::from(colour) + (u16::from(under) * uncovered + 127) / 255;
            shown.min(u16::from(u8::MAX)) as u8
        };
        bgra.extend_from_slice(&[
            over(b, grey[2]),
            over(g, grey[1]),
            over(r, grey[0]),
            u8::MAX,
        ]);
    }
    bgra_to_render(width, height, bgra)
}

fn bgra_to_render(width: u32, height: u32, bgra: Vec<u8>) -> Option<Arc<RenderImage>> {
    let buffer = ImageBuffer::<Rgba<u8>, _>::from_raw(width, height, bgra)?;
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

    /// A pixel shows the grid in proportion to how transparent it is: none
    /// through an opaque pixel, all of it through a transparent one, and a
    /// premultiplied half-covered pixel adds about half the grey to its
    /// colour. The squares alternate along the row.
    #[test]
    fn a_transparent_frame_shows_the_grid_in_proportion_to_its_alpha() {
        let (w, h) = (2 * GRID_CELL as u32, 1);
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        rgba[4..8].copy_from_slice(&[200, 10, 20, 255]);
        let half = (GRID_CELL + 1) * 4;
        rgba[half..half + 4].copy_from_slice(&[100, 0, 0, 128]);
        let image = rgba_over_grid_to_render(w, h, &rgba).expect("image");
        let bgra = image.as_bytes(0).expect("one frame");
        let at = |x: usize| {
            [
                bgra[x * 4],
                bgra[x * 4 + 1],
                bgra[x * 4 + 2],
                bgra[x * 4 + 3],
            ]
        };
        let [first, second] = GRID_GREYS;
        assert_eq!(at(0), [first[2], first[1], first[0], 255]);
        assert_eq!(at(GRID_CELL), [second[2], second[1], second[0], 255]);
        assert_eq!(at(1), [20, 10, 200, 255], "an opaque pixel showed the grid");
        let under = |grey: u8| ((u16::from(grey) * 127 + 127) / 255) as u8;
        assert_eq!(
            at(GRID_CELL + 1),
            [
                under(second[2]),
                under(second[1]),
                100 + under(second[0]),
                255
            ]
        );
        assert!(rgba_over_grid_to_render(w, h, &rgba[4..]).is_none());
    }
}
