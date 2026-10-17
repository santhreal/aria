//! The grade's later stages, after highlights and shadows: whites and blacks,
//! faded film, vibrance, the RGB curves, the hue curves and the HSL secondary.
//!
//! `apply_grade_secondary` in `grade.wgsl` is the same math stage for stage,
//! and `render_grade::every_slider_matches_its_cpu_mirror` holds the two to the
//! same pixels. A stage whose sliders sit at neutral is skipped on both sides,
//! so a grade that leaves them alone composites as it did before they existed.

use super::{ColorGrade, smoothstep};

impl ColorGrade {
    /// The stages after highlights and shadows, unclamped.
    pub(super) fn apply_secondary(&self, rgb: [f32; 3]) -> [f32; 3] {
        let mut c = rgb;

        // Whites and blacks: a levels remap of the black and white points.
        if self.whites != 0.0 || self.blacks != 0.0 {
            let black_point = -0.25 * self.blacks;
            let white_point = 1.0 - 0.25 * self.whites;
            let span = (white_point - black_point).max(1e-3);
            c = c.map(|ch| (ch - black_point) / span);
        }

        // Faded film: lifts the floor and leaves white where it was.
        if self.faded_film != 0.0 {
            c = c.map(|ch| {
                let lifted = (1.0 - ch).max(0.0);
                ch + self.faded_film * 0.3 * lifted * lifted
            });
        }

        // Vibrance: saturation weighted toward the colours that have little.
        if self.vibrance != 0.0 {
            let luma = luma709(c);
            let hi = c[0].max(c[1]).max(c[2]);
            let lo = c[0].min(c[1]).min(c[2]);
            let held = ((hi - lo) / hi.max(1e-4)).clamp(0.0, 1.0);
            let scale = 1.0 + self.vibrance * (1.0 - held);
            c = c.map(|ch| luma + (ch - luma) * scale);
        }

        // RGB curves: the master curve, then each channel's own.
        let master = [
            self.curve_master_shadows,
            self.curve_master_midtones,
            self.curve_master_highlights,
        ];
        let channels = [
            [
                self.curve_red_shadows,
                self.curve_red_midtones,
                self.curve_red_highlights,
            ],
            [
                self.curve_green_shadows,
                self.curve_green_midtones,
                self.curve_green_highlights,
            ],
            [
                self.curve_blue_shadows,
                self.curve_blue_midtones,
                self.curve_blue_highlights,
            ],
        ];
        for (ch, knots) in c.iter_mut().zip(channels) {
            *ch = curve(curve(*ch, master), knots);
        }

        // The hue curves and the secondary read the hue once, and only when
        // one of them is off neutral.
        let sat = [
            self.hue_sat_red,
            self.hue_sat_yellow,
            self.hue_sat_green,
            self.hue_sat_cyan,
            self.hue_sat_blue,
            self.hue_sat_magenta,
        ];
        let turn = [
            self.hue_hue_red,
            self.hue_hue_yellow,
            self.hue_hue_green,
            self.hue_hue_cyan,
            self.hue_hue_blue,
            self.hue_hue_magenta,
        ];
        let luma = [
            self.hue_luma_red,
            self.hue_luma_yellow,
            self.hue_luma_green,
            self.hue_luma_cyan,
            self.hue_luma_blue,
            self.hue_luma_magenta,
        ];
        let secondary = [self.hsl_hue, self.hsl_saturation, self.hsl_luma];
        if [sat, turn, luma] == [[0.0; 6]; 3] && secondary == [0.0; 3] {
            return c;
        }
        let (hue, chroma) = hue_chroma(c);
        let colour = smoothstep(0.0, 0.1, chroma);

        // The secondary's key: a hue window around the key centre, soft at its
        // edge, and nothing without colour.
        let centre = 0.5 * self.hsl_key_hue;
        let reach = (1.0 + self.hsl_key_width) * (30.0 / 360.0);
        let around = (fract(hue - centre + 0.5) - 0.5).abs();
        let keyed = (1.0 - smoothstep(reach, reach + 0.04, around)) * colour;

        hue_correction(
            c,
            hue_knots(hue, sat) * colour + secondary[1] * keyed,
            hue_knots(hue, turn) * colour + secondary[0] * keyed,
            hue_knots(hue, luma) * colour + secondary[2] * keyed,
        )
    }
}

/// BT.709 luma.
fn luma709(c: [f32; 3]) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

/// WGSL's `fract`: `x - floor(x)`, so a negative input wraps into `[0, 1)`.
fn fract(x: f32) -> f32 {
    x - x.floor()
}

/// One RGB curve: knots at 0, 1/4, 1/2, 3/4 and 1, the inner three moved by up
/// to a quarter each, through a Catmull-Rom spline. The ends stay put, so a
/// value outside `[0, 1]` passes through.
fn curve(x: f32, knots: [f32; 3]) -> f32 {
    if knots == [0.0; 3] || x <= 0.0 || x >= 1.0 {
        return x;
    }
    // Phantom knots past each end mirror the first inner knot through the end
    // it sits beside, so the spline leaves 0 and 1 along the curve's slope.
    let y = [
        -0.25 - 0.25 * knots[0],
        0.0,
        0.25 + 0.25 * knots[0],
        0.5 + 0.25 * knots[1],
        0.75 + 0.25 * knots[2],
        1.0,
        1.25 - 0.25 * knots[2],
    ];
    let scaled = x * 4.0;
    let seg = (scaled.floor() as usize).min(3);
    let t = scaled - seg as f32;
    catmull([y[seg], y[seg + 1], y[seg + 2], y[seg + 3]], t)
}

/// Six hue knots (red, yellow, green, cyan, blue, magenta) on a closed
/// Catmull-Rom loop, read at hue `h` in turns.
fn hue_knots(h: f32, k: [f32; 6]) -> f32 {
    if k == [0.0; 6] {
        return 0.0;
    }
    let scaled = fract(h) * 6.0;
    let seg = (scaled.floor() as usize).min(5);
    let t = scaled - seg as f32;
    catmull(
        [k[(seg + 5) % 6], k[seg], k[(seg + 1) % 6], k[(seg + 2) % 6]],
        t,
    )
}

/// The Catmull-Rom spline through `p[1]` and `p[2]`, at `t` between them.
fn catmull(p: [f32; 4], t: f32) -> f32 {
    let t2 = t * t;
    let t3 = t2 * t;
    0.5 * (2.0 * p[1]
        + (p[2] - p[0]) * t
        + (2.0 * p[0] - 5.0 * p[1] + 4.0 * p[2] - p[3]) * t2
        + (3.0 * p[1] - p[0] - 3.0 * p[2] + p[3]) * t3)
}

/// Hue in turns and chroma (max minus min) of `c`.
fn hue_chroma(c: [f32; 3]) -> (f32, f32) {
    let hi = c[0].max(c[1]).max(c[2]);
    let lo = c[0].min(c[1]).min(c[2]);
    let d = hi - lo;
    if d <= 1e-6 {
        return (0.0, 0.0);
    }
    let h = if hi == c[0] {
        (c[1] - c[2]) / d
    } else if hi == c[1] {
        (c[2] - c[0]) / d + 2.0
    } else {
        (c[0] - c[1]) / d + 4.0
    };
    (fract(h / 6.0), d)
}

/// Saturation scale, hue rotation (one unit is 60 degrees) and luma offset,
/// each already weighted by the pixel's hue.
fn hue_correction(rgb: [f32; 3], sat: f32, hue: f32, luma: f32) -> [f32; 3] {
    if sat == 0.0 && hue == 0.0 && luma == 0.0 {
        return rgb;
    }
    let l = luma709(rgb);
    let scale = (1.0 + sat).max(0.0);
    let c = rgb.map(|ch| l + (ch - l) * scale);
    let y = 0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2];
    let i = 0.596 * c[0] - 0.274 * c[1] - 0.322 * c[2];
    let q = 0.211 * c[0] - 0.523 * c[1] + 0.312 * c[2];
    let (sin_a, cos_a) = (-hue * std::f32::consts::FRAC_PI_3).sin_cos();
    let i2 = i * cos_a - q * sin_a;
    let q2 = i * sin_a + q * cos_a;
    [
        y + 0.956 * i2 + 0.621 * q2 + 0.25 * luma,
        y - 0.272 * i2 - 0.647 * q2 + 0.25 * luma,
        y - 1.106 * i2 + 1.703 * q2 + 0.25 * luma,
    ]
}

#[cfg(test)]
#[path = "secondary_tests.rs"]
mod tests;
