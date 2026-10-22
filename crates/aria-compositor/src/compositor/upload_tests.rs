//! WHY: closes the stale-key class of the shared-bitmap key memo, where a
//! bitmap draws the texture of other pixels: a key served for an address that
//! another allocation took, a key served for pixels changed in place, and a
//! memo that grows without bound. It also states the cost contract: a shared
//! bitmap is hashed once per allocation and border, a borrowed one on every
//! draw.
//!
//! Does not catch a caller that passes a borrowed bitmap for pixels it keeps
//! across frames; the renderer's own tests cover its caches.

use std::sync::Arc;

use aria_core::RgbaImage;

use super::{MAX_SHARED_BITMAP_KEYS, RgbaTextureCache};
use crate::layer::Bitmap;

fn solid(rgba: [u8; 4]) -> RgbaImage {
    RgbaImage::new(2, 2, rgba.repeat(4))
}

fn key(cache: &mut RgbaTextureCache, bitmap: Bitmap<'_>, border: u32) -> [u8; 32] {
    let expected = bitmap.image().pixels.len();
    cache.key(bitmap, expected, border)
}

/// The key of `image` on a cache that has seen nothing.
fn fresh_key(image: &RgbaImage, border: u32) -> [u8; 32] {
    key(
        &mut RgbaTextureCache::default(),
        Bitmap::Borrowed(image),
        border,
    )
}

#[test]
fn a_shared_bitmap_is_hashed_once_per_border() {
    let mut cache = RgbaTextureCache::default();
    let still = Arc::new(solid([200, 10, 10, 255]));
    let plain = key(&mut cache, Bitmap::Shared(&still), 0);
    assert_eq!(plain, fresh_key(&still, 0));
    for frame in 0..3 {
        cache.begin_frame();
        assert_eq!(key(&mut cache, Bitmap::Shared(&still), 0), plain);
        assert_eq!(cache.bitmaps_hashed(), 0, "frame {frame} hashed again");
    }

    cache.begin_frame();
    let bordered = key(&mut cache, Bitmap::Shared(&still), 1);
    assert_eq!(bordered, fresh_key(&still, 1));
    assert_ne!(bordered, plain);
    assert_eq!(key(&mut cache, Bitmap::Shared(&still), 1), bordered);
    assert_eq!(cache.bitmaps_hashed(), 1);
}

#[test]
fn a_borrowed_bitmap_is_hashed_on_every_draw() {
    let mut cache = RgbaTextureCache::default();
    let mut raster = solid([200, 10, 10, 255]);
    let red = key(&mut cache, Bitmap::Borrowed(&raster), 0);
    raster.pixels.fill(30);
    let changed = key(&mut cache, Bitmap::Borrowed(&raster), 0);
    assert_eq!(changed, fresh_key(&raster, 0));
    assert_ne!(changed, red);
    assert_eq!(cache.bitmaps_hashed(), 2);
}

#[test]
fn equal_pixels_share_one_key() {
    let mut cache = RgbaTextureCache::default();
    let a = Arc::new(solid([1, 2, 3, 255]));
    let b = Arc::new(solid([1, 2, 3, 255]));
    let raster = solid([1, 2, 3, 255]);
    let key_a = key(&mut cache, Bitmap::Shared(&a), 0);
    assert_eq!(key(&mut cache, Bitmap::Shared(&b), 0), key_a);
    assert_eq!(key(&mut cache, Bitmap::Borrowed(&raster), 0), key_a);
}

#[test]
fn a_shared_bitmap_changed_through_its_arc_gets_the_key_of_its_new_pixels() {
    let mut cache = RgbaTextureCache::default();
    let blue = solid([10, 10, 200, 255]);

    let mut still = Arc::new(solid([200, 10, 10, 255]));
    key(&mut cache, Bitmap::Shared(&still), 0);
    assert!(
        Arc::get_mut(&mut still).is_none(),
        "a keyed bitmap can be changed in place under its kept key"
    );
    Arc::make_mut(&mut still)
        .pixels
        .copy_from_slice(&blue.pixels);
    assert_eq!(
        key(&mut cache, Bitmap::Shared(&still), 0),
        fresh_key(&blue, 0)
    );

    let mut taken = Arc::new(solid([200, 10, 10, 255]));
    key(&mut cache, Bitmap::Shared(&taken), 0);
    let mut image = Arc::into_inner(taken).expect("only the cache's weak reference remains");
    image.pixels.copy_from_slice(&blue.pixels);
    taken = Arc::new(image);
    assert_eq!(
        key(&mut cache, Bitmap::Shared(&taken), 0),
        fresh_key(&blue, 0)
    );
}

#[test]
fn a_freed_bitmap_address_never_serves_its_old_key() {
    let mut cache = RgbaTextureCache::default();
    for shade in 0..64u8 {
        let pixels = solid([shade, 0, 0, 255]);
        let want = fresh_key(&pixels, 0);
        // Freed on each turn, so the allocator offers its address to the next.
        let still = Arc::new(pixels);
        assert_eq!(
            key(&mut cache, Bitmap::Shared(&still), 0),
            want,
            "shade {shade} drew the key of an earlier bitmap"
        );
    }
}

#[test]
fn the_key_memo_stays_bounded_and_keeps_live_bitmaps() {
    let mut cache = RgbaTextureCache::default();
    let kept = Arc::new(solid([9, 9, 9, 255]));
    key(&mut cache, Bitmap::Shared(&kept), 0);
    for n in 0..3 * MAX_SHARED_BITMAP_KEYS as u32 {
        let gone = Arc::new(RgbaImage::new(1, 1, n.to_le_bytes().to_vec()));
        key(&mut cache, Bitmap::Shared(&gone), 0);
        assert!(cache.shared_keys.len() <= MAX_SHARED_BITMAP_KEYS);
    }
    cache.begin_frame();
    key(&mut cache, Bitmap::Shared(&kept), 0);
    assert_eq!(
        cache.bitmaps_hashed(),
        0,
        "a live bitmap lost its key while the memo dropped dead ones"
    );

    let live: Vec<_> = (0..2 * MAX_SHARED_BITMAP_KEYS as u32)
        .map(|n| Arc::new(RgbaImage::new(1, 1, n.to_le_bytes().to_vec())))
        .collect();
    for (n, still) in live.iter().enumerate() {
        assert_eq!(
            key(&mut cache, Bitmap::Shared(still), 0),
            fresh_key(still, 0),
            "live bitmap {n}"
        );
        assert!(cache.shared_keys.len() <= MAX_SHARED_BITMAP_KEYS);
    }
}
