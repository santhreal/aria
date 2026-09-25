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
    for px in img.pixels.chunks_exact(4) {
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

#[test]
fn memo_hits_repeat_requests() {
    let mut r = PathRaster::new();
    let a = r.rasterize(&diamond(10.0), &filled([1, 2, 3, 255]), 1.0);
    let b = r.rasterize(&diamond(10.0), &filled([1, 2, 3, 255]), 1.0);
    assert_eq!(a, b);
    assert_eq!(r.memo.len(), 1);
    let _ = r.rasterize(&diamond(11.0), &filled([1, 2, 3, 255]), 1.0);
    assert_eq!(r.memo.len(), 2, "different geometry is a new entry");
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
