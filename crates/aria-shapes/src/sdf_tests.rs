//! Signed-distance field evaluation tests.

use super::*;
use crate::{SdfParams, Stroke};

fn fill(rgba: [u8; 4]) -> ShapeStyle {
    ShapeStyle {
        fill: Some(rgba),
        stroke: None,
    }
}

#[test]
fn rounded_rect_signs_and_corner() {
    let s = SdfParams::RoundedRect { radius: 10.0 }.with_half([50.0, 30.0]);
    assert!(eval(&s, [0.0, 0.0]) < 0.0, "center is inside");
    assert!(eval(&s, [60.0, 0.0]) > 0.0, "beyond +x is outside");
    // Sharp-corner point is shaved off by the rounding.
    assert!(eval(&s, [49.5, 29.5]) > 0.0);
    // Edge midpoints sit on the boundary.
    assert!(eval(&s, [50.0, 0.0]).abs() < 1e-3);
    assert!(eval(&s, [0.0, 30.0]).abs() < 1e-3);
}

#[test]
fn ellipse_axis_points_are_exact() {
    let s = SdfParams::Ellipse.with_half([40.0, 20.0]);
    assert!(eval(&s, [40.0, 0.0]).abs() < 1e-3);
    assert!(eval(&s, [0.0, 20.0]).abs() < 1e-3);
    assert!(eval(&s, [0.0, 0.0]) <= -19.0, "center is deep inside");
    assert!(eval(&s, [40.0, 20.0]) > 0.0, "box corner is outside");
}

#[test]
fn polygon_is_star_with_midpoint_inners() {
    // A triangle: apex up, base down, inscribed in the circle.
    let tri = SdfParams::polygon(3, 0.0).with_half([50.0, 50.0]);
    assert!(eval(&tri, [0.0, -49.0]) < 0.0, "apex region inside");
    assert!(eval(&tri, [0.0, 49.0]) > 0.0, "below base is outside");
    assert!(eval(&tri, [0.0, 0.0]) < 0.0, "centroid inside");
    // The base edge of an inscribed up-triangle sits at y = +h/2 * cos60·2…
    // just check left/right symmetry instead of chasing constants.
    let l = eval(&tri, [-20.0, 10.0]);
    let r = eval(&tri, [20.0, 10.0]);
    assert!((l - r).abs() < 1e-4, "triangle SDF must be x-symmetric");
}

#[test]
fn star_spikes_alternate_inside_outside() {
    let s = SdfParams::Star {
        points: 5,
        inner: 0.5,
        round: 0.0,
    }
    .with_half([50.0, 50.0]);
    // Top spike tip is on the boundary; between spikes (rotated by one
    // half-step at the same radius) is far outside.
    assert!(eval(&s, [0.0, -50.0]).abs() < 0.5);
    let between = std::f32::consts::PI / 5.0;
    let p = [
        (-std::f32::consts::FRAC_PI_2 + between).cos() * 50.0,
        (-std::f32::consts::FRAC_PI_2 + between).sin() * 50.0,
    ];
    assert!(eval(&s, p) > 5.0);
    assert!(eval(&s, [0.0, 0.0]) < 0.0);
}

#[test]
fn star_negative_or_unclamped_params_remain_safe_and_do_not_invert() {
    for (inner, round) in [
        (-1.0, 0.0),
        (0.0, -10.0),
        (-5.0, -5.0),
        (f32::NAN, 0.0),
        (0.5, f32::NAN),
        (10.0, 100.0),
    ] {
        let s = SdfParams::Star {
            points: 5,
            inner,
            round,
        }
        .with_half([50.0, 50.0]);
        let d_center = eval(&s, [0.0, 0.0]);
        let d_far = eval(&s, [100.0, 100.0]);
        assert!(d_center.is_finite());
        assert!(d_far.is_finite());
        assert!(
            d_center < d_far,
            "geometry was inverted for inner={inner}, round={round}"
        );
    }
}

#[test]
fn capsule_ends_are_round() {
    let s = SdfParams::Line.with_half([60.0, 6.0]);
    assert!(eval(&s, [0.0, 0.0]) < 0.0);
    assert!(eval(&s, [60.0, 0.0]).abs() < 1e-3, "tip on boundary");
    // The box corner is outside (round cap, not square): exactly
    // r·(√2−1) ≈ 2.49px beyond a radius-6 cap.
    assert!(eval(&s, [60.0, 6.0]) > 2.0);
}

#[test]
fn arrow_tip_shaft_and_notch() {
    let s = SdfParams::Arrow.with_half([60.0, 25.0]);
    assert!(eval(&s, [59.0, 0.0]) < 0.0, "just inside the tip");
    assert!(eval(&s, [-59.0, 0.0]) < 0.0, "inside the tail");
    // Above the shaft, behind the head: outside (the notch).
    assert!(eval(&s, [0.0, -20.0]) > 0.0);
    assert!(eval(&s, [0.0, -5.0]) < 0.0, "inside the shaft");
}

#[test]
fn heart_lobes_and_tip() {
    let s = SdfParams::Heart.with_half([50.0, 50.0]);
    assert!(eval(&s, [0.0, 49.5]).abs() < 1.0, "tip at box bottom");
    assert!(eval(&s, [-25.0, -25.0]) < 0.0, "left lobe inside");
    assert!(eval(&s, [25.0, -25.0]) < 0.0, "right lobe inside");
    assert!(eval(&s, [0.0, -49.0]) > 0.0, "cleft between lobes");
    let l = eval(&s, [-30.0, 10.0]);
    let r = eval(&s, [30.0, 10.0]);
    assert!((l - r).abs() < 1e-4, "heart SDF must be x-symmetric");
}

#[test]
fn raster_fill_center_and_transparent_corner() {
    let img = raster(
        &SdfParams::Ellipse.with_half([20.0, 12.0]),
        &fill([255, 0, 0, 255]),
    );
    assert!(img.is_well_formed());
    let c = img.pixel(img.width / 2, img.height / 2);
    assert_eq!(c, [255, 0, 0, 255]);
    assert_eq!(img.pixel(0, 0)[3], 0, "padded corner is transparent");
}

#[test]
fn raster_stroke_rings_the_fill() {
    let style = ShapeStyle {
        fill: Some([0, 0, 255, 255]),
        stroke: Some(Stroke {
            rgba: [255, 255, 0, 255],
            width: 6.0,
        }),
    };
    let shape = SdfParams::RoundedRect { radius: 0.0 }.with_half([30.0, 20.0]);
    let img = raster(&shape, &style);
    let cx = img.width / 2;
    let cy = img.height / 2;
    // Center: pure fill.
    assert_eq!(img.pixel(cx, cy), [0, 0, 255, 255]);
    // On the right edge (x = +30 from center): pure stroke color.
    let edge = img.pixel(cx + 30, cy);
    assert_eq!(
        [edge[0], edge[1], edge[2]],
        [255, 255, 0],
        "edge pixel should be stroke-colored, got {edge:?}"
    );
    // Well outside the stroke: transparent.
    assert_eq!(img.pixel(cx + 40, cy)[3], 0);
}

#[test]
fn stroke_alpha_folds_into_coverage() {
    let translucent = ShapeStyle {
        fill: None,
        stroke: Some(Stroke {
            rgba: [255, 255, 255, 128],
            width: 4.0,
        }),
    };
    let img = raster(&SdfParams::Ellipse.with_half([20.0, 20.0]), &translucent);
    let cx = img.width / 2;
    let cy = img.height / 2;
    let on_edge = img.pixel(cx + 20, cy);
    assert!(
        (on_edge[3] as i32 - 128).abs() <= 4,
        "stroke alpha should cap coverage, got {}",
        on_edge[3]
    );
    assert_eq!(
        img.pixel(cx, cy)[3],
        0,
        "no fill inside a stroke-only shape"
    );
}

#[test]
fn hit_test_includes_stroke_overhang() {
    let s = SdfParams::Ellipse.with_half([20.0, 20.0]);
    assert!(s.hit_test([0.0, 0.0], 0.0));
    assert!(!s.hit_test([23.0, 0.0], 0.0));
    // A 10px stroke extends 5px beyond the edge.
    assert!(s.hit_test([23.0, 0.0], 10.0));
}

#[test]
fn coverage_ramp_endpoints() {
    assert_eq!(coverage(-SDF_AA), 1.0);
    assert_eq!(coverage(SDF_AA), 0.0);
    assert!((coverage(0.0) - 0.5).abs() < 1e-6);
}

/// Every parametric shape rasters at one fixed size.
///
/// The per-shape tests above check the distance field; these check that the
/// field reaches pixels — a shape whose evaluator is right and whose raster is
/// empty still draws nothing. Coverage is compared against the shape's own box,
/// so each entry states how much of its box it is supposed to fill.
///
/// Fail-by-default: [`variant_name`] matches every [`SdfParams`] variant, so a
/// new shape stops this file compiling until it is listed here.
#[cfg(test)]
mod raster_coverage_tests {
    use super::*;
    use crate::{SdfParams, Stroke};

    /// One box for every shape, so coverage numbers are comparable.
    const HALF: [f32; 2] = [32.0, 24.0];
    const OPAQUE: [u8; 4] = [10, 200, 90, 255];

    fn variant_name(params: SdfParams) -> &'static str {
        match params {
            SdfParams::RoundedRect { .. } => "rounded_rect",
            SdfParams::Ellipse => "ellipse",
            SdfParams::Star { .. } => "star",
            SdfParams::Line => "line",
            SdfParams::Arrow => "arrow",
            SdfParams::Heart => "heart",
        }
    }

    /// `(params, lowest, highest)` — the fraction of its own box the shape's
    /// ink is allowed to cover. A convex shape fills most of its box; a star or
    /// an arrow leaves a lot of it empty.
    fn shapes() -> Vec<(SdfParams, f32, f32)> {
        vec![
            // A rectangle fills its whole box, plus the anti-aliased fringe
            // the raster pads for, so it reads slightly over 1.
            (SdfParams::RoundedRect { radius: 8.0 }, 0.9, 1.12),
            (SdfParams::Ellipse, 0.6, 0.85),
            (SdfParams::polygon(6, 0.0), 0.55, 0.9),
            (
                SdfParams::Star {
                    points: 5,
                    inner: 0.4,
                    round: 0.0,
                },
                0.2,
                0.6,
            ),
            (SdfParams::Line, 0.7, 1.0),
            (SdfParams::Arrow, 0.3, 0.8),
            (SdfParams::Heart, 0.4, 0.9),
        ]
    }

    fn inked(img: &RgbaImage) -> usize {
        img.pixels
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[3] != 0)
            .count()
    }

    #[test]
    fn every_shape_rasters_ink_inside_its_box() {
        let box_area = HALF[0] * 2.0 * HALF[1] * 2.0;
        for (params, low, high) in shapes() {
            let name = variant_name(params);
            let img = raster(
                &params.with_half(HALF),
                &ShapeStyle {
                    fill: Some(OPAQUE),
                    stroke: None,
                },
            );
            assert!(img.is_well_formed(), "{name} raster is malformed");
            // The raster pads for anti-aliasing, so it is bigger than the box
            // and its outermost corner is always empty.
            assert!(img.width > HALF[0] as u32 * 2, "{name} is too narrow");
            assert!(img.height > HALF[1] as u32 * 2, "{name} is too short");
            assert_eq!(img.pixel(0, 0)[3], 0, "{name} inked its padded corner");

            let fraction = inked(&img) as f32 / box_area;
            assert!(
                fraction >= low && fraction <= high,
                "{name} covered {fraction:.2} of its box, wanted {low}…{high}"
            );
        }
    }

    #[test]
    fn every_shape_fills_its_centre_with_the_fill_colour() {
        for (params, _, _) in shapes() {
            let name = variant_name(params);
            let img = raster(
                &params.with_half(HALF),
                &ShapeStyle {
                    fill: Some(OPAQUE),
                    stroke: None,
                },
            );
            assert_eq!(
                img.pixel(img.width / 2, img.height / 2),
                OPAQUE,
                "{name} left its centre unfilled"
            );
        }
    }

    #[test]
    fn no_two_shapes_raster_the_same_pixels() {
        let mut seen: Vec<(&'static str, Vec<u8>)> = Vec::new();
        for (params, _, _) in shapes() {
            let name = variant_name(params);
            let img = raster(
                &params.with_half(HALF),
                &ShapeStyle {
                    fill: Some(OPAQUE),
                    stroke: None,
                },
            );
            for (other, pixels) in &seen {
                assert_ne!(
                    pixels, &img.pixels,
                    "{name} and {other} rastered identically"
                );
            }
            seen.push((name, img.pixels));
        }
        assert_eq!(seen.len(), 7, "one entry per listed shape");
    }

    #[test]
    fn a_stroke_only_shape_leaves_its_middle_empty() {
        for (params, _, _) in shapes() {
            let name = variant_name(params);
            let img = raster(
                &params.with_half(HALF),
                &ShapeStyle {
                    fill: None,
                    stroke: Some(Stroke {
                        rgba: OPAQUE,
                        width: 4.0,
                    }),
                },
            );
            assert_eq!(
                img.pixel(img.width / 2, img.height / 2)[3],
                0,
                "{name} filled its centre with no fill colour set"
            );
            assert!(inked(&img) > 0, "{name} drew no stroke");
        }
    }
}
