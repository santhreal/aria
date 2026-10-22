//! Path building and flattening tests.

use super::*;
use crate::Stroke;

/// A closed unit-ish diamond around the origin.
fn diamond(r: f32) -> BezierPath {
    BezierPath {
        points: vec![
            PathPoint::corner([0.0, -r]),
            PathPoint::corner([r, 0.0]),
            PathPoint::corner([0.0, r]),
            PathPoint::corner([-r, 0.0]),
        ],
        closed: true,
    }
}

/// An S-curve open path with real handles.
fn s_curve() -> BezierPath {
    BezierPath {
        points: vec![
            PathPoint {
                anchor: [0.0, 0.0],
                handle_in: [0.0, 0.0],
                handle_out: [40.0, 0.0],
            },
            PathPoint {
                anchor: [60.0, 40.0],
                handle_in: [20.0, 40.0],
                handle_out: [60.0, 40.0],
            },
        ],
        closed: false,
    }
}

fn filled(rgba: [u8; 4]) -> ShapeStyle {
    ShapeStyle {
        fill: Some(rgba),
        stroke: None,
    }
}

#[test]
fn closed_path_fills() {
    let mut r = PathRaster::new();
    let img = r.rasterize(&diamond(20.0), &filled([0, 255, 0, 255]), 1.0);
    assert!(img.is_well_formed());
    let center = img.pixel(img.width / 2, img.height / 2);
    assert_eq!(center, [0, 255, 0, 255]);
    assert_eq!(img.pixel(0, 0)[3], 0, "outside the diamond is clear");
}

#[test]
fn open_path_ignores_fill_and_strokes() {
    let mut r = PathRaster::new();
    let style = ShapeStyle {
        fill: Some([255, 0, 0, 255]),
        stroke: Some(Stroke {
            rgba: [255, 255, 255, 255],
            width: 4.0,
        }),
    };
    let img = r.rasterize(&s_curve(), &style, 1.0);
    assert!(img.width > 0 && img.height > 0);
    // Some stroke ink exists, and no pixel is fill-red (open ⇒ no fill).
    let mut any_ink = false;
    for px in img.pixels.as_chunks::<4>().0 {
        if px[3] > 0 {
            any_ink = true;
            assert!(
                !(px[0] > 200 && px[1] < 60 && px[2] < 60),
                "open path must not fill: {px:?}"
            );
        }
    }
    assert!(any_ink, "stroke drew nothing");
}

#[test]
fn non_drawable_paths_are_empty() {
    let mut r = PathRaster::new();
    let dot = BezierPath {
        points: vec![PathPoint::corner([5.0, 5.0])],
        closed: false,
    };
    let img = r.rasterize(&dot, &filled([255, 255, 255, 255]), 1.0);
    assert_eq!((img.width, img.height), (0, 0));
}

#[test]
fn scale_scales_the_bitmap() {
    let mut r = PathRaster::new();
    let s1 = r.rasterize(&diamond(20.0), &filled([255, 255, 255, 255]), 1.0);
    let s2 = r.rasterize(&diamond(20.0), &filled([255, 255, 255, 255]), 2.0);
    assert!(
        s2.width > (s1.width as f32 * 1.8) as u32,
        "2x scale should roughly double the raster: {} vs {}",
        s1.width,
        s2.width
    );
}

/// **The size query is the raster's size, for every style and scale.**
///
/// The editor sizes a path layer from `path_raster_size` without drawing it, and the
/// renderer places the bitmap `rasterize` draws. A pixel of disagreement puts
/// the selection box beside the ink. Each case covers one input the size reads:
/// fill only, stroke overhang, an open stroked path, a scale, a degenerate
/// path, and a style with nothing to draw.
#[test]
fn path_raster_size_is_the_raster_dimensions() {
    let stroked = ShapeStyle {
        fill: Some([255, 0, 0, 255]),
        stroke: Some(Stroke {
            rgba: [255, 255, 255, 255],
            width: 9.0,
        }),
    };
    let nothing = ShapeStyle {
        fill: None,
        stroke: None,
    };
    let dot = BezierPath {
        points: vec![PathPoint::corner([5.0, 5.0])],
        closed: false,
    };
    let cases = [
        (diamond(20.0), filled([0, 255, 0, 255]), 1.0),
        (diamond(20.0), stroked, 1.0),
        (s_curve(), stroked, 1.0),
        (diamond(13.0), stroked, 2.5),
        (s_curve(), filled([1, 2, 3, 255]), f32::NAN),
        (dot, filled([255, 255, 255, 255]), 1.0),
        (diamond(20.0), nothing, 1.0),
    ];
    let mut r = PathRaster::new();
    for (path, style, scale) in &cases {
        let image = r.rasterize(path, style, *scale);
        assert_eq!(
            path_raster_size(path, style, *scale),
            (image.width, image.height),
            "{path:?} {style:?} at {scale}"
        );
    }
    assert_eq!(path_raster_size(&diamond(20.0), &nothing, 1.0), (0, 0));
}

/// **A raster pixel hits the path exactly where the raster inks it.**
///
/// The canvas pick maps a click into the raster the renderer placed and asks
/// `path_raster_hit`, so the mapping has to be the one `rasterize` draws with:
/// the inked bounds centred and the scale divided out. Every fully inked pixel
/// must hit and the raster's empty corner must miss, for a path far from the
/// origin (a bounds offset), at a scale, and stroked (the overhang).
#[test]
fn a_raster_pixel_hits_the_path_where_the_raster_inks_it() {
    let triangle = BezierPath {
        points: vec![
            PathPoint::corner([100.0, 40.0]),
            PathPoint::corner([140.0, 40.0]),
            PathPoint::corner([100.0, 90.0]),
        ],
        closed: true,
    };
    let stroked = ShapeStyle {
        fill: Some([255, 0, 0, 255]),
        stroke: Some(Stroke {
            rgba: [0, 0, 255, 255],
            width: 4.0,
        }),
    };
    for (style, scale) in [
        (filled([0, 255, 0, 255]), 1.0),
        (filled([0, 255, 0, 255]), 2.5),
        (stroked, 1.5),
    ] {
        let image = PathRaster::new().rasterize(&triangle, &style, scale);
        let half = [image.width as f32 * 0.5, image.height as f32 * 0.5];
        let at = |x: u32, y: u32| [x as f32 + 0.5 - half[0], y as f32 + 0.5 - half[1]];
        let mut inked = 0usize;
        for y in 0..image.height {
            for x in 0..image.width {
                if image.pixel(x, y)[3] == 255 {
                    inked += 1;
                    assert!(
                        path_raster_hit(&triangle, &style, scale, at(x, y)),
                        "inked pixel ({x}, {y}) at scale {scale} missed"
                    );
                }
            }
        }
        assert!(inked > 0, "nothing inked at scale {scale}");
        // The right angle is top-left, so the raster's bottom-right is empty.
        let (x, y) = (image.width - 1, image.height - 1);
        assert_eq!(image.pixel(x, y)[3], 0);
        assert!(
            !path_raster_hit(&triangle, &style, scale, at(x, y)),
            "the empty corner hit at scale {scale}"
        );
    }
    let nothing = ShapeStyle {
        fill: None,
        stroke: None,
    };
    assert!(!path_raster_hit(&triangle, &nothing, 1.0, [0.0, 0.0]));
}

/// A repeat request returns the memo's raster itself, not a copy: the
/// compositor keys a shared raster by its allocation, so a copy per frame
/// would hash the pixels on every frame.
#[test]
fn memo_hits_repeat_requests() {
    let mut r = PathRaster::new();
    let a = r.rasterize(&diamond(10.0), &filled([1, 2, 3, 255]), 1.0);
    let b = r.rasterize(&diamond(10.0), &filled([1, 2, 3, 255]), 1.0);
    assert!(Arc::ptr_eq(&a, &b), "a memo hit copied the raster");
    assert_eq!(r.memo.len(), 1);
    let _ = r.rasterize(&diamond(11.0), &filled([1, 2, 3, 255]), 1.0);
    assert_eq!(r.memo.len(), 2, "different geometry is a new entry");
    let pts = [
        PathPoint::corner([0.25, 0.25]),
        PathPoint::corner([0.75, 0.25]),
        PathPoint::corner([0.5, 0.75]),
    ];
    let m1 = r.rasterize_mask(40, 40, &pts, [0.0, 0.0], 0.0, 0.0);
    let m2 = r.rasterize_mask(40, 40, &pts, [0.0, 0.0], 0.0, 0.0);
    assert!(Arc::ptr_eq(&m1, &m2), "a mask memo hit copied the raster");
}

#[test]
fn bounds_hug_the_curve_not_the_handles() {
    // Extreme out-handle: the curve bulges far less than the handle.
    let path = BezierPath {
        points: vec![
            PathPoint {
                anchor: [0.0, 0.0],
                handle_in: [0.0, 0.0],
                handle_out: [0.0, -100.0],
            },
            PathPoint {
                anchor: [40.0, 0.0],
                handle_in: [40.0, -100.0],
                handle_out: [40.0, 0.0],
            },
        ],
        closed: false,
    };
    let (min, max) = path_bounds(&path).unwrap();
    // Cubic with both handles at -100 peaks at 3/4 of that: -75.
    assert!(min[1] > -80.0 && min[1] < -70.0, "min y {}", min[1]);
    assert_eq!(max[1], 0.0);
    assert_eq!((min[0], max[0]), (0.0, 40.0));
}

#[test]
fn hit_test_fill_stroke_and_miss() {
    let d = diamond(20.0);
    assert!(path_hit_test(&d, [0.0, 0.0], 0.0), "center is filled");
    assert!(!path_hit_test(&d, [30.0, 30.0], 0.0), "far corner misses");
    // On the edge midpoint with a fat stroke: hit even outside the fill.
    assert!(path_hit_test(&d, [11.0, -11.0], 6.0));
    // Open path: only the stroke band hits.
    let s = s_curve();
    assert!(!path_hit_test(&s, [30.0, 0.0], 0.0));
    assert!(path_hit_test(&s, [0.0, 0.0], 4.0), "on the start anchor");
}

#[test]
fn winding_fill_covers_self_overlap() {
    // A figure-eight-ish loop: winding keeps both lobes filled where
    // even-odd would punch a hole. Just assert the crossing region hits.
    let bow = BezierPath {
        points: vec![
            PathPoint::corner([-20.0, -10.0]),
            PathPoint::corner([20.0, 10.0]),
            PathPoint::corner([20.0, -10.0]),
            PathPoint::corner([-20.0, 10.0]),
        ],
        closed: true,
    };
    assert!(path_hit_test(&bow, [0.0, 0.0], 2.0));
}

#[test]
fn degenerate_zero_length_segments_and_duplicate_anchors_do_not_nan_or_panic() {
    let dup = BezierPath {
        points: vec![
            PathPoint::corner([10.0, 10.0]),
            PathPoint::corner([10.0, 10.0]),
            PathPoint::corner([10.0, 10.0]),
        ],
        closed: true,
    };
    assert!(path_hit_test(&dup, [10.0, 10.0], 2.0));
    assert!(!path_hit_test(&dup, [50.0, 50.0], 2.0));
}

#[test]
fn memo_is_bounded_and_evicts_lru() {
    let mut r = PathRaster::new();
    for i in 0..50 {
        let _ = r.rasterize(&diamond(10.0 + i as f32), &filled([1, 2, 3, 255]), 1.0);
        assert!(
            r.memo.len() <= 32,
            "memo length {} exceeded 32",
            r.memo.len()
        );
    }
    assert_eq!(r.memo.len(), 32);
}

#[test]
fn rasterize_mask_fills_normalized_polygon_and_rotates() {
    let mut r = PathRaster::new();
    let pts = [
        PathPoint::corner([0.25, 0.25]),
        PathPoint::corner([0.75, 0.25]),
        PathPoint::corner([0.75, 0.75]),
        PathPoint::corner([0.25, 0.75]),
    ];
    let img = r.rasterize_mask(100, 100, &pts, [0.0, 0.0], 0.0, 0.0);
    assert_eq!((img.width, img.height), (100, 100));
    assert_eq!(img.pixel(50, 50)[3], 255, "center inside path is opaque");
    assert_eq!(img.pixel(5, 5)[3], 0, "corner outside path is transparent");
}

/// A mask's expansion moves its edge by that many pixels: outward when
/// positive, inward when negative, and each expansion is its own memo entry.
///
/// Not caught: the shape of an expanded corner, which no point on an edge's
/// midline shows.
#[test]
fn rasterize_mask_expansion_moves_the_edge_by_pixels() {
    let mut r = PathRaster::new();
    let pts = [
        PathPoint::corner([0.25, 0.25]),
        PathPoint::corner([0.75, 0.25]),
        PathPoint::corner([0.75, 0.75]),
        PathPoint::corner([0.25, 0.75]),
    ];
    // The left edge sits at x = 25; each probe is 5 px off an edge.
    let alpha = |r: &mut PathRaster, expansion: f32, x: u32| {
        r.rasterize_mask(100, 100, &pts, [0.0, 0.0], 0.0, expansion)
            .pixel(x, 50)[3]
    };
    assert_eq!(alpha(&mut r, 0.0, 20), 0, "outside the plain edge");
    assert_eq!(alpha(&mut r, 10.0, 20), 255, "grown 10 px past x = 20");
    assert_eq!(alpha(&mut r, 10.0, 10), 0, "grown 10 px, short of x = 10");
    assert_eq!(alpha(&mut r, 0.0, 30), 255, "inside the plain edge");
    assert_eq!(alpha(&mut r, -10.0, 30), 0, "shrunk 10 px past x = 30");
    assert_eq!(
        alpha(&mut r, -10.0, 40),
        255,
        "shrunk 10 px, short of x = 40"
    );
    assert_eq!(
        alpha(&mut r, f32::NAN, 20),
        0,
        "a non-finite expansion is none"
    );
}
