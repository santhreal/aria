// grade.wgsl — per-layer color grade in the fragment shader (no extra pass).
//
// The lanes come from `ColorGrade::lanes`, and every slider is read back through
// an accessor generated ahead of this file: `grade_brightness(a)`,
// `grade_gain_master(a)`, `grade_enabled(a)`, one per row of the slider list. No
// slot is named here, so a slider added to that list arrives with a lane and an
// accessor of its own and only its math is left to write.
//
// Params are signed strengths in roughly [-1, 1] (sharpness/vignette in [0, 1]);
// `grade_enabled` at 0 is the identity fast path (one branch, no math).
//
// Sharpness (4-tap laplacian) and vignette (radial darken) need UVs / texels and
// are applied by the textured callers via `apply_grade_sharpness` /
// `apply_grade_vignette` after this per-pixel grade.

fn apply_color_grade(rgb: vec3<f32>, a: array<vec4<f32>, GRADE_LANES>) -> vec3<f32> {
    if (grade_enabled(a) < 0.5) {
        return rgb;
    }
    var c = rgb;

    // Lift, Gamma, Gain (standard 3-way order).
    let lift = vec3(grade_lift_r(a), grade_lift_g(a), grade_lift_b(a))
        + vec3(grade_lift_master(a));
    let gamma = vec3(grade_gamma_r(a), grade_gamma_g(a), grade_gamma_b(a))
        * vec3(grade_gamma_master(a));
    let gain = vec3(grade_gain_r(a), grade_gain_g(a), grade_gain_b(a))
        * vec3(grade_gain_master(a));

    if (any(lift != vec3(0.0)) || any(gain != vec3(1.0))) {
        c = (c + lift * (vec3(1.0) - c)) * gain;
    }
    if (any(gamma != vec3(1.0))) {
        c = pow(max(c, vec3(0.0)), vec3(1.0) / max(gamma, vec3(1e-4)));
    } else {
        c = max(c, vec3(0.0));
    }
    let exposure = grade_exposure(a);
    if (exposure != 0.0) {
        c *= exp2(2.0 * exposure);
    }
    let temperature = grade_temperature(a);
    c.r += temperature * 0.25;
    c.b -= temperature * 0.25;
    c.g += grade_tint(a) * 0.25;
    c += grade_brightness(a) * 0.25;
    c = (c - vec3(0.5)) * (1.0 + grade_contrast(a)) + vec3(0.5);
    let sat = grade_saturation(a);
    if (sat != 0.0) {
        let luma0 = dot(c, vec3(0.2126, 0.7152, 0.0722));
        c = mix(vec3(luma0), c, 1.0 + sat);
    }

    // YIQ hue rotation: 30° at ±1.
    let hue = grade_hue(a);
    if (hue != 0.0) {
        let y = dot(c, vec3(0.299, 0.587, 0.114));
        let i = dot(c, vec3(0.596, -0.274, -0.322));
        let q = dot(c, vec3(0.211, -0.523, 0.312));
        // Negated so +hue rotates red toward yellow (standard editor feel).
        let angle = -hue * 0.5235987755982988; // −π/6 per unit
        let cos_a = cos(angle);
        let sin_a = sin(angle);
        let i2 = i * cos_a - q * sin_a;
        let q2 = i * sin_a + q * cos_a;
        c = vec3(
            y + 0.956 * i2 + 0.621 * q2,
            y - 0.272 * i2 - 0.647 * q2,
            y - 1.106 * i2 + 1.703 * q2,
        );
    }

    // Highlights and shadows adjustments.
    let hl = grade_highlights(a);
    let sh = grade_shadows(a);
    if (hl != 0.0 || sh != 0.0) {
        let luma = dot(c, vec3(0.2126, 0.7152, 0.0722));
        if (hl != 0.0) {
            let w_hl = smoothstep(0.5, 1.0, luma);
            let lift_hl = select(c, vec3(1.0) - c, hl >= 0.0);
            c += vec3(hl * w_hl * 0.35) * lift_hl;
        }
        if (sh != 0.0) {
            let w_sh = 1.0 - smoothstep(0.0, 0.5, luma);
            let lift_sh = select(c, vec3(1.0) - c, sh >= 0.0);
            c += vec3(sh * w_sh * 0.35) * lift_sh;
        }
    }
    if (grade_enabled(a) > 1.5) {
        c = apply_grade_secondary(c, a);
    }
    return clamp(c, vec3(0.0), vec3(1.0));
}

/// The stages after highlights and shadows, unclamped: whites and blacks,
/// faded film, vibrance, the RGB curves, the hue curves and the HSL
/// secondary. `ColorGrade::apply_secondary` is the CPU mirror. A stage whose
/// sliders sit at neutral is skipped, so a grade that leaves them alone reads
/// as it did before they existed.
fn apply_grade_secondary(rgb: vec3<f32>, a: array<vec4<f32>, GRADE_LANES>) -> vec3<f32> {
    var c = rgb;

    // Whites and blacks: a levels remap of the black and white points.
    let whites = grade_whites(a);
    let blacks = grade_blacks(a);
    if (whites != 0.0 || blacks != 0.0) {
        let black_point = -0.25 * blacks;
        let white_point = 1.0 - 0.25 * whites;
        c = (c - vec3(black_point)) / max(white_point - black_point, 1e-3);
    }

    // Faded film: lifts the floor and leaves white where it was.
    let faded = grade_faded_film(a);
    if (faded != 0.0) {
        let lifted = max(vec3(1.0) - c, vec3(0.0));
        c += faded * 0.3 * lifted * lifted;
    }

    // Vibrance: saturation weighted toward the colours that have little.
    let vibrance = grade_vibrance(a);
    if (vibrance != 0.0) {
        let luma = dot(c, vec3(0.2126, 0.7152, 0.0722));
        let hi = max(max(c.r, c.g), c.b);
        let held = clamp((hi - min(min(c.r, c.g), c.b)) / max(hi, 1e-4), 0.0, 1.0);
        c = vec3(luma) + (c - vec3(luma)) * (1.0 + vibrance * (1.0 - held));
    }

    // RGB curves: the master curve, then each channel's own.
    let master = vec3(
        grade_curve_master_shadows(a),
        grade_curve_master_midtones(a),
        grade_curve_master_highlights(a),
    );
    if (any(master != vec3(0.0))) {
        c = vec3(
            grade_curve(c.r, master),
            grade_curve(c.g, master),
            grade_curve(c.b, master),
        );
    }
    let red = vec3(
        grade_curve_red_shadows(a),
        grade_curve_red_midtones(a),
        grade_curve_red_highlights(a),
    );
    if (any(red != vec3(0.0))) {
        c.r = grade_curve(c.r, red);
    }
    let green = vec3(
        grade_curve_green_shadows(a),
        grade_curve_green_midtones(a),
        grade_curve_green_highlights(a),
    );
    if (any(green != vec3(0.0))) {
        c.g = grade_curve(c.g, green);
    }
    let blue = vec3(
        grade_curve_blue_shadows(a),
        grade_curve_blue_midtones(a),
        grade_curve_blue_highlights(a),
    );
    if (any(blue != vec3(0.0))) {
        c.b = grade_curve(c.b, blue);
    }

    // The hue curves and the secondary read the hue once, and only when one
    // of them is off neutral.
    let sat_active = grade_hue_sat_red(a) != 0.0
        || grade_hue_sat_yellow(a) != 0.0
        || grade_hue_sat_green(a) != 0.0
        || grade_hue_sat_cyan(a) != 0.0
        || grade_hue_sat_blue(a) != 0.0
        || grade_hue_sat_magenta(a) != 0.0;
    let turn_active = grade_hue_hue_red(a) != 0.0
        || grade_hue_hue_yellow(a) != 0.0
        || grade_hue_hue_green(a) != 0.0
        || grade_hue_hue_cyan(a) != 0.0
        || grade_hue_hue_blue(a) != 0.0
        || grade_hue_hue_magenta(a) != 0.0;
    let luma_active = grade_hue_luma_red(a) != 0.0
        || grade_hue_luma_yellow(a) != 0.0
        || grade_hue_luma_green(a) != 0.0
        || grade_hue_luma_cyan(a) != 0.0
        || grade_hue_luma_blue(a) != 0.0
        || grade_hue_luma_magenta(a) != 0.0;
    let secondary = vec3(grade_hsl_hue(a), grade_hsl_saturation(a), grade_hsl_luma(a));
    if (!sat_active && !turn_active && !luma_active && all(secondary == vec3(0.0))) {
        return c;
    }
    let sat_knots = array<f32, 6>(
        grade_hue_sat_red(a),
        grade_hue_sat_yellow(a),
        grade_hue_sat_green(a),
        grade_hue_sat_cyan(a),
        grade_hue_sat_blue(a),
        grade_hue_sat_magenta(a),
    );
    let turn_knots = array<f32, 6>(
        grade_hue_hue_red(a),
        grade_hue_hue_yellow(a),
        grade_hue_hue_green(a),
        grade_hue_hue_cyan(a),
        grade_hue_hue_blue(a),
        grade_hue_hue_magenta(a),
    );
    let luma_knots = array<f32, 6>(
        grade_hue_luma_red(a),
        grade_hue_luma_yellow(a),
        grade_hue_luma_green(a),
        grade_hue_luma_cyan(a),
        grade_hue_luma_blue(a),
        grade_hue_luma_magenta(a),
    );
    let hue_chroma = grade_hue_chroma(c);
    let hue = hue_chroma.x;
    let colour = smoothstep(0.0, 0.1, hue_chroma.y);
    let sat = grade_hue_knots(hue, sat_knots);
    let turn = grade_hue_knots(hue, turn_knots);
    let luma = grade_hue_knots(hue, luma_knots);

    // The secondary's key: a hue window around the key centre, soft at its
    // edge, and nothing without colour.
    let centre = 0.5 * grade_hsl_key_hue(a);
    let reach = (1.0 + grade_hsl_key_width(a)) * (30.0 / 360.0);
    let around = abs(fract(hue - centre + 0.5) - 0.5);
    let keyed = (1.0 - smoothstep(reach, reach + 0.04, around)) * colour;

    return grade_hue_correction(
        c,
        sat * colour + secondary.y * keyed,
        turn * colour + secondary.x * keyed,
        luma * colour + secondary.z * keyed,
    );
}

/// One RGB curve: knots at 0, 1/4, 1/2, 3/4 and 1, the inner three moved by
/// up to a quarter each, through a Catmull-Rom spline. The ends stay put, so
/// a value outside `[0, 1]` passes through.
fn grade_curve(x: f32, knots: vec3<f32>) -> f32 {
    if (all(knots == vec3(0.0)) || x <= 0.0 || x >= 1.0) {
        return x;
    }
    // Phantom knots past each end mirror the first inner knot through the end
    // it sits beside, so the spline leaves 0 and 1 along the curve's slope.
    let y0 = -0.25 - 0.25 * knots.x;
    let y1 = 0.0;
    let y2 = 0.25 + 0.25 * knots.x;
    let y3 = 0.5 + 0.25 * knots.y;
    let y4 = 0.75 + 0.25 * knots.z;
    let y5 = 1.0;
    let y6 = 1.25 - 0.25 * knots.z;
    let scaled = x * 4.0;
    let seg = min(i32(floor(scaled)), 3);
    let t = scaled - f32(seg);
    let p0 = select(select(select(y0, y1, seg == 1), y2, seg == 2), y3, seg == 3);
    let p1 = select(select(select(y1, y2, seg == 1), y3, seg == 2), y4, seg == 3);
    let p2 = select(select(select(y2, y3, seg == 1), y4, seg == 2), y5, seg == 3);
    let p3 = select(select(select(y3, y4, seg == 1), y5, seg == 2), y6, seg == 3);
    return grade_catmull(p0, p1, p2, p3, t);
}

/// Six hue knots (red, yellow, green, cyan, blue, magenta) on a closed
/// Catmull-Rom loop, read at hue `h` in turns.
fn grade_hue_knots(h: f32, knots: array<f32, 6>) -> f32 {
    if (grade_flat(knots)) {
        return 0.0;
    }
    let k0 = knots[0];
    let k1 = knots[1];
    let k2 = knots[2];
    let k3 = knots[3];
    let k4 = knots[4];
    let k5 = knots[5];
    let scaled = fract(h) * 6.0;
    let seg = min(i32(floor(scaled)), 5);
    let t = scaled - f32(seg);
    let p0 = select(select(select(select(select(k5, k0, seg == 1), k1, seg == 2), k2, seg == 3), k3, seg == 4), k4, seg == 5);
    let p1 = select(select(select(select(select(k0, k1, seg == 1), k2, seg == 2), k3, seg == 3), k4, seg == 4), k5, seg == 5);
    let p2 = select(select(select(select(select(k1, k2, seg == 1), k3, seg == 2), k4, seg == 3), k5, seg == 4), k0, seg == 5);
    let p3 = select(select(select(select(select(k2, k3, seg == 1), k4, seg == 2), k5, seg == 3), k0, seg == 4), k1, seg == 5);
    return grade_catmull(p0, p1, p2, p3, t);
}

/// True when all six knots sit at zero.
fn grade_flat(k: array<f32, 6>) -> bool {
    return k[0] == 0.0 && k[1] == 0.0 && k[2] == 0.0 && k[3] == 0.0 && k[4] == 0.0
        && k[5] == 0.0;
}

/// The Catmull-Rom spline through `p1` and `p2`, at `t` between them.
fn grade_catmull(p0: f32, p1: f32, p2: f32, p3: f32, t: f32) -> f32 {
    let t2 = t * t;
    let t3 = t2 * t;
    return 0.5 * (2.0 * p1
        + (p2 - p0) * t
        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
        + (3.0 * p1 - p0 - 3.0 * p2 + p3) * t3);
}

/// Hue in turns and chroma (max minus min) of `c`.
fn grade_hue_chroma(c: vec3<f32>) -> vec2<f32> {
    let hi = max(max(c.r, c.g), c.b);
    let lo = min(min(c.r, c.g), c.b);
    let d = hi - lo;
    if (d <= 1e-6) {
        return vec2(0.0, 0.0);
    }
    var h: f32;
    if (hi == c.r) {
        h = (c.g - c.b) / d;
    } else if (hi == c.g) {
        h = (c.b - c.r) / d + 2.0;
    } else {
        h = (c.r - c.g) / d + 4.0;
    }
    return vec2(fract(h / 6.0), d);
}

/// Saturation scale, hue rotation (one unit is 60 degrees) and luma offset,
/// each already weighted by the pixel's hue.
fn grade_hue_correction(rgb: vec3<f32>, sat: f32, hue: f32, luma: f32) -> vec3<f32> {
    if (sat == 0.0 && hue == 0.0 && luma == 0.0) {
        return rgb;
    }
    let l = dot(rgb, vec3(0.2126, 0.7152, 0.0722));
    let c = vec3(l) + (rgb - vec3(l)) * max(1.0 + sat, 0.0);
    let y = dot(c, vec3(0.299, 0.587, 0.114));
    let i = dot(c, vec3(0.596, -0.274, -0.322));
    let q = dot(c, vec3(0.211, -0.523, 0.312));
    let angle = -hue * 1.0471975511965976; // -pi/3 per unit
    let cos_a = cos(angle);
    let sin_a = sin(angle);
    let i2 = i * cos_a - q * sin_a;
    let q2 = i * sin_a + q * cos_a;
    return vec3(
        y + 0.956 * i2 + 0.621 * q2,
        y - 0.272 * i2 - 0.647 * q2,
        y - 1.106 * i2 + 1.703 * q2,
    ) + vec3(0.25 * luma);
}

/// 4-tap unsharp mask on a straight-alpha RGBA texture. `amount` is
/// `grade_sharpness(a)`.
fn apply_grade_sharpness(
    rgb: vec3<f32>,
    uv: vec2<f32>,
    tex: texture_2d<f32>,
    samp: sampler,
    amount: f32,
) -> vec3<f32> {
    if (amount <= 0.0) {
        return rgb;
    }
    let dims = vec2<f32>(textureDimensions(tex));
    let texel = vec2(1.0) / dims;
    let s0 = textureSample(tex, samp, uv + vec2(texel.x, 0.0));
    let s1 = textureSample(tex, samp, uv - vec2(texel.x, 0.0));
    let s2 = textureSample(tex, samp, uv + vec2(0.0, texel.y));
    let s3 = textureSample(tex, samp, uv - vec2(0.0, texel.y));
    var n0 = s0.rgb;
    if (s0.a > 1e-4) { n0 = s0.rgb / s0.a; }
    var n1 = s1.rgb;
    if (s1.a > 1e-4) { n1 = s1.rgb / s1.a; }
    var n2 = s2.rgb;
    if (s2.a > 1e-4) { n2 = s2.rgb / s2.a; }
    var n3 = s3.rgb;
    if (s3.a > 1e-4) { n3 = s3.rgb / s3.a; }
    let avg = (n0 + n1 + n2 + n3) * 0.25;
    return clamp(rgb + amount * 1.5 * (rgb - avg), vec3(0.0), vec3(1.0));
}

/// Radial vignette from layer-UV center. `amount` is `grade_vignette(a)`.
/// `uv` is in 0..1 across the layer quad (or equivalent normalized coords).
fn apply_grade_vignette(rgb: vec3<f32>, uv: vec2<f32>, amount: f32) -> vec3<f32> {
    if (amount <= 0.0) {
        return rgb;
    }
    let dist_norm = length(uv - vec2(0.5)) / (0.5 * sqrt(2.0));
    return rgb * (1.0 - amount * smoothstep(0.4, 0.9, dist_norm));
}
