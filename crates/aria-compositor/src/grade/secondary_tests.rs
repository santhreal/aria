//! What each later grade stage does to a pixel, on the CPU mirror.
//!
//! The GPU side is held to these by `render_grade`'s parity test; these pin the
//! stages' own meaning: where a curve knot lands, where a hue knot reads, what
//! a key reaches, and that a stage at neutral leaves the pixel bit for bit.
//! Not covered here: how a stage reads on screen, which needs a frame.

use super::{ColorGrade, curve, hue_chroma, hue_knots};

/// Colours across the hue circle, the greys, and both ends.
const SWATCHES: &[[f32; 3]] = &[
    [0.0, 0.0, 0.0],
    [1.0, 1.0, 1.0],
    [0.5, 0.5, 0.5],
    [0.47, 0.235, 0.235],
    [0.745, 0.59, 0.35],
    [0.35, 0.59, 0.31],
    [0.27, 0.63, 0.63],
    [0.27, 0.35, 0.47],
    [0.67, 0.31, 0.63],
];

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

#[test]
fn every_later_stage_at_neutral_returns_the_pixel_bit_for_bit() {
    for rgb in SWATCHES {
        let out = ColorGrade::IDENTITY.apply_secondary(*rgb);
        assert_eq!(
            out.map(f32::to_bits),
            rgb.map(f32::to_bits),
            "{rgb:?} moved with every later stage at neutral"
        );
    }
}

#[test]
fn a_curve_knot_moves_its_point_a_quarter_and_leaves_the_ends() {
    for k in [-1.0, -0.4, 0.5, 1.0] {
        assert!(close(curve(0.25, [k, 0.0, 0.0]), 0.25 + 0.25 * k));
        assert!(close(curve(0.5, [0.0, k, 0.0]), 0.5 + 0.25 * k));
        assert!(close(curve(0.75, [0.0, 0.0, k]), 0.75 + 0.25 * k));
        for x in [0.0, 1.0, -0.2, 1.3] {
            assert_eq!(curve(x, [k, k, k]), x, "the curve moved its end at {x}");
        }
    }
    assert_eq!(curve(0.3, [0.0; 3]), 0.3, "a flat curve moved the value");
}

#[test]
fn a_hue_knot_reads_its_own_value_at_its_hue() {
    let knots = [0.9, -0.3, 0.6, -0.8, 0.2, 0.5];
    for (index, value) in knots.iter().enumerate() {
        let at = hue_knots(index as f32 / 6.0, knots);
        assert!(close(at, *value), "knot {index} read {at}, not {value}");
    }
    // The loop closes: a hue just under one turn reads next to red.
    assert!((hue_knots(0.999, knots) - knots[0]).abs() < 0.02);
}

#[test]
fn hue_is_measured_in_turns_from_red() {
    let cases = [
        ([1.0, 0.0, 0.0], 0.0),
        ([1.0, 1.0, 0.0], 1.0 / 6.0),
        ([0.0, 1.0, 0.0], 2.0 / 6.0),
        ([0.0, 1.0, 1.0], 3.0 / 6.0),
        ([0.0, 0.0, 1.0], 4.0 / 6.0),
        ([1.0, 0.0, 1.0], 5.0 / 6.0),
    ];
    for (rgb, turns) in cases {
        let (hue, chroma) = hue_chroma(rgb);
        assert!(close(hue, turns), "{rgb:?} read hue {hue}");
        assert!(close(chroma, 1.0));
    }
    assert_eq!(hue_chroma([0.4, 0.4, 0.4]), (0.0, 0.0));
}

#[test]
fn whites_and_blacks_move_the_white_and_black_points() {
    let blacks = ColorGrade {
        blacks: 1.0,
        ..ColorGrade::IDENTITY
    };
    assert!(close(blacks.apply_secondary([0.0; 3])[0], 0.2));
    assert!(close(blacks.apply_secondary([1.0; 3])[0], 1.0));
    let crushed = ColorGrade {
        blacks: -1.0,
        ..ColorGrade::IDENTITY
    };
    assert!(close(crushed.apply_secondary([0.25; 3])[0], 0.0));
    let whites = ColorGrade {
        whites: 1.0,
        ..ColorGrade::IDENTITY
    };
    assert!(close(whites.apply_secondary([0.75; 3])[0], 1.0));
    assert!(close(whites.apply_secondary([0.0; 3])[0], 0.0));
}

#[test]
fn faded_film_lifts_black_and_leaves_white() {
    let faded = ColorGrade {
        faded_film: 1.0,
        ..ColorGrade::IDENTITY
    };
    assert!(close(faded.apply_secondary([0.0; 3])[0], 0.3));
    assert_eq!(faded.apply_secondary([1.0; 3]), [1.0; 3]);
}

#[test]
fn vibrance_reaches_muted_colours_further_than_saturated_ones() {
    let vibrant = ColorGrade {
        vibrance: 1.0,
        ..ColorGrade::IDENTITY
    };
    let spread = |rgb: [f32; 3]| rgb[0].max(rgb[1]).max(rgb[2]) - rgb[0].min(rgb[1]).min(rgb[2]);
    let muted = [0.5, 0.45, 0.45];
    let vivid = [0.9, 0.1, 0.1];
    let muted_gain = spread(vibrant.apply_secondary(muted)) / spread(muted);
    let vivid_gain = spread(vibrant.apply_secondary(vivid)) / spread(vivid);
    assert!(
        muted_gain > vivid_gain,
        "muted grew {muted_gain}, vivid grew {vivid_gain}"
    );
    let grey = vibrant.apply_secondary([0.4; 3]);
    assert!(
        grey.iter().all(|ch| close(*ch, 0.4)),
        "a grey took colour: {grey:?}"
    );
}

#[test]
fn the_secondary_reaches_only_the_keyed_hue() {
    let red = [0.8, 0.2, 0.2];
    let green = [0.2, 0.8, 0.2];
    let grey_out = ColorGrade {
        hsl_saturation: -1.0,
        ..ColorGrade::IDENTITY
    };
    let greyed = grey_out.apply_secondary(red);
    assert!(
        close(greyed[0], greyed[1]) && close(greyed[1], greyed[2]),
        "the keyed red kept colour: {greyed:?}"
    );
    assert_eq!(
        grey_out.apply_secondary(green),
        green,
        "a green outside the key moved"
    );

    // Turned to green, the key reaches green and leaves red alone.
    let on_green = ColorGrade {
        hsl_key_hue: 2.0 / 3.0,
        ..grey_out
    };
    assert_eq!(on_green.apply_secondary(red), red);
    let greyed = on_green.apply_secondary(green);
    assert!(close(greyed[0], greyed[2]) && close(greyed[0], greyed[1]));
}

#[test]
fn a_wider_key_reaches_a_neighbouring_hue() {
    // This orange sits 45 degrees from red: outside the default window of 30
    // degrees and its soft edge, inside the widest window of 60.
    let orange = [0.9, 0.75, 0.3];
    let grey_out = ColorGrade {
        hsl_saturation: -1.0,
        ..ColorGrade::IDENTITY
    };
    let narrow = grey_out.apply_secondary(orange);
    let wide = ColorGrade {
        hsl_key_width: 1.0,
        ..grey_out
    }
    .apply_secondary(orange);
    let spread = |rgb: [f32; 3]| rgb[0].max(rgb[1]).max(rgb[2]) - rgb[0].min(rgb[1]).min(rgb[2]);
    assert!(
        spread(wide) < 0.01 && spread(narrow) > 0.1,
        "narrow {narrow:?}, wide {wide:?}"
    );
}
