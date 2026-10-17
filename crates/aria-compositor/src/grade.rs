//! Resolved per-layer color grade parameters consumed by the GPU shaders.
//!
//! The render bridge maps persisted [`aria_models::ColorAdjustments`] and
//! filter presets into this compact form. Per-clip effects, masks, and canvas
//! passes use adjacent compositor plumbing, but share this grade representation.
//!
//! # Spatial controls
//!
//! `sharpness` and `vignette` need neighboring texels / layer UVs. They are
//! packed into the grade uniform block and applied in the textured layer
//! shaders (`rgba` / `rgba_fx` / `yuv` / `yuv_fx`) and the canvas grade pass —
//! not in solid/shape/glyph paths (no useful neighbors). The CPU mirror
//! [`ColorGrade::apply`] covers the per-pixel ops; [`ColorGrade::apply_image`]
//! adds the same 4-tap sharpness and radial vignette for full-frame parity.

use std::sync::LazyLock;

mod secondary;

/// The value a slider of each shape sits at when it changes nothing.
///
/// The shape column of the row list is the only place a neutral is stated, so
/// the struct, its `Default` and its identity test cannot disagree.
macro_rules! grade_neutral {
    (signed) => {
        0.0
    };
    (positive) => {
        0.0
    };
    (multiplicative) => {
        1.0
    };
}

/// Expand the grade slider rows into the value the shaders are fed.
macro_rules! define_grade {
    ($($field:ident => $variant:ident, $shape:ident, $id:literal, $label:literal,
       $doc:literal,)*) => {
        /// Manual color grade + filter-preset strengths, ready for the WGSL
        /// uniform block.
        ///
        /// One field per row of
        /// [`adjust_slider_rows!`](aria_models::adjust_slider_rows), in row
        /// order. That order is also the order the private `lanes` helper packs the
        /// uniform in and the order the generated WGSL accessors read it back,
        /// so a new slider is a row there and a lane on both sides of the
        /// boundary.
        #[derive(Debug, Clone, Copy, PartialEq)]
        pub struct ColorGrade {
            $(
                #[doc = $doc]
                pub $field: f32,
            )*
        }

        impl ColorGrade {
            /// Identity grade: every control is neutral and the shader returns
            /// the pixel it was given.
            pub const IDENTITY: Self = Self {
                $($field: grade_neutral!($shape),)*
            };

            /// Field names in row order, for the generated WGSL accessors.
            pub(crate) const FIELDS: &'static [&'static str] = &[$(stringify!($field),)*];

            /// True when every slider sits at neutral, so grade work can be skipped.
            pub const fn is_identity(&self) -> bool {
                true $(&& self.$field == grade_neutral!($shape))*
            }

            /// Pack the sliders into the uniform's `vec4` lanes.
            ///
            /// `enabled` is `1.0` for a grade the shader should run and `0.0`
            /// for the identity fast path. It owns [`Self::ENABLED_SLOT`], the
            /// first lane's last component, because it is the one value the
            /// shader reads before anything else.
            pub(crate) fn lanes(&self, enabled: f32) -> [[f32; 4]; Self::LANES] {
                let values = [$(self.$field,)*];
                let mut lanes = [[0.0f32; 4]; Self::LANES];
                for (row, value) in values.iter().enumerate() {
                    let slot = Self::slot_of(row);
                    lanes[slot / 4][slot % 4] = *value;
                }
                lanes[Self::ENABLED_SLOT / 4][Self::ENABLED_SLOT % 4] = enabled;
                lanes
            }
        }
    };
}

aria_models::adjust_slider_rows!(define_grade);

impl Default for ColorGrade {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl ColorGrade {
    /// The slot carrying the enabled flag, and the only slot no row may take.
    pub(crate) const ENABLED_SLOT: usize = 3;

    /// One slot per slider, plus the enabled flag.
    pub(crate) const SLOTS: usize = Self::FIELDS.len() + 1;

    /// `vec4` lanes needed to carry every slot. A row that crosses a multiple
    /// of four grows the uniform, and it grows on the shader side in the same
    /// build because the WGSL lane count is generated from this number.
    pub(crate) const LANES: usize = (Self::SLOTS + 3) / 4;

    /// The slot a row packs into: rows fill slots in order, stepping over the
    /// enabled flag.
    pub(crate) const fn slot_of(row: usize) -> usize {
        if row >= Self::ENABLED_SLOT {
            row + 1
        } else {
            row
        }
    }
}
impl ColorGrade {
    /// Hold a positive strength to the compositor's unit interval.
    pub fn clamp_unit(value: f32) -> f32 {
        if value.is_finite() {
            value.clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    /// Hold a signed adjustment to the compositor's accepted interval.
    pub fn clamp_adjust(value: f32) -> f32 {
        if value.is_finite() {
            value.clamp(-1.0, 1.0)
        } else {
            0.0
        }
    }

    /// Hold a multiplicative adjustment to the compositor's accepted interval.
    pub fn clamp_positive(value: f32) -> f32 {
        if value.is_finite() {
            value.clamp(0.0, 4.0)
        } else {
            1.0
        }
    }
}

impl ColorGrade {
    /// CPU mirror of `grade.wgsl`'s `apply_color_grade` (per-pixel ops only —
    /// sharpness / vignette need [`Self::apply_image`]).
    pub fn apply(&self, rgb: [f32; 3]) -> [f32; 3] {
        // A neutral grade returns the pixel it was given, bit for bit. Every
        // stage below is algebraically identity at rest, but not numerically:
        // the luma split alone rewrites 0.123 as 0.122999996, so a clip with no
        // grade would come back subtly different from one that was never
        // graded. It is also the cheapest path — the whole chain is skipped.
        if self.is_identity() {
            return rgb;
        }
        let mut c = rgb;

        // Lift, Gain, Gamma (standard 3-way color grading order)
        let lift = [
            self.lift_r + self.lift_master,
            self.lift_g + self.lift_master,
            self.lift_b + self.lift_master,
        ];
        let gamma = [
            self.gamma_r * self.gamma_master,
            self.gamma_g * self.gamma_master,
            self.gamma_b * self.gamma_master,
        ];
        let gain = [
            self.gain_r * self.gain_master,
            self.gain_g * self.gain_master,
            self.gain_b * self.gain_master,
        ];

        for i in 0..3 {
            c[i] = (c[i] + lift[i] * (1.0 - c[i])) * gain[i];
            c[i] = c[i].max(0.0).powf(1.0 / gamma[i].max(1e-4));
        }

        let exposure = 2f32.powf(2.0 * self.exposure);
        for ch in &mut c {
            *ch *= exposure;
        }
        c[0] += 0.25 * self.temperature;
        c[2] -= 0.25 * self.temperature;
        c[1] += 0.25 * self.tint;
        for ch in &mut c {
            *ch += 0.25 * self.brightness;
            *ch = (*ch - 0.5) * (1.0 + self.contrast) + 0.5;
        }
        let luma = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
        let sat = 1.0 + self.saturation;
        c = c.map(|ch| luma + (ch - luma) * sat);

        // YIQ hue rotation: ±1 → ±30°.
        if self.hue != 0.0 {
            let y = 0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2];
            let i = 0.596 * c[0] - 0.274 * c[1] - 0.322 * c[2];
            let q = 0.211 * c[0] - 0.523 * c[1] + 0.312 * c[2];
            // Negated so +hue rotates red toward yellow (matches the shader).
            let angle = -self.hue * std::f32::consts::PI / 6.0;
            let (sin_a, cos_a) = angle.sin_cos();
            let i2 = i * cos_a - q * sin_a;
            let q2 = i * sin_a + q * cos_a;
            c = [
                y + 0.956 * i2 + 0.621 * q2,
                y - 0.272 * i2 - 0.647 * q2,
                y - 1.106 * i2 + 1.703 * q2,
            ];
        }

        let luma = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
        if self.highlights != 0.0 {
            let w = smoothstep(0.5, 1.0, luma);
            let hl = self.highlights;
            for ch in &mut c {
                let lift = if hl >= 0.0 { 1.0 - *ch } else { *ch };
                *ch += hl * w * 0.35 * lift;
            }
        }
        if self.shadows != 0.0 {
            let w = 1.0 - smoothstep(0.0, 0.5, luma);
            let sh = self.shadows;
            for ch in &mut c {
                let lift = if sh >= 0.0 { 1.0 - *ch } else { *ch };
                *ch += sh * w * 0.35 * lift;
            }
        }

        self.apply_secondary(c).map(|ch| ch.clamp(0.0, 1.0))
    }

    /// Full-image CPU mirror of the textured grade path: per-pixel [`Self::apply`],
    /// then 4-tap unsharp mask and radial vignette (same formulas as the shaders).
    pub fn apply_image(&self, width: u32, height: u32, rgba: &mut [u8]) {
        assert_eq!(rgba.len(), (width * height * 4) as usize);
        if self.is_identity() {
            return;
        }

        // Grade every pixel first (sharpness neighbors must see graded colors).
        let mut graded = vec![[0f32; 3]; (width * height) as usize];
        for y in 0..height {
            for x in 0..width {
                let i = ((y * width + x) * 4) as usize;
                let rgb = [
                    f32::from(rgba[i]) / 255.0,
                    f32::from(rgba[i + 1]) / 255.0,
                    f32::from(rgba[i + 2]) / 255.0,
                ];
                graded[(y * width + x) as usize] = self.apply(rgb);
            }
        }

        let sharp = self.sharpness.max(0.0);
        let vig = self.vignette.max(0.0);
        let half_diag = 0.5 * (2f32).sqrt();

        for y in 0..height {
            for x in 0..width {
                let idx = (y * width + x) as usize;
                let mut c = graded[idx];

                if sharp > 0.0 && width > 1 && height > 1 {
                    let sample = |sx: i32, sy: i32| {
                        let sx = sx.clamp(0, width as i32 - 1) as u32;
                        let sy = sy.clamp(0, height as i32 - 1) as u32;
                        graded[(sy * width + sx) as usize]
                    };
                    let xi = x as i32;
                    let yi = y as i32;
                    let avg = [
                        (sample(xi + 1, yi)[0]
                            + sample(xi - 1, yi)[0]
                            + sample(xi, yi + 1)[0]
                            + sample(xi, yi - 1)[0])
                            * 0.25,
                        (sample(xi + 1, yi)[1]
                            + sample(xi - 1, yi)[1]
                            + sample(xi, yi + 1)[1]
                            + sample(xi, yi - 1)[1])
                            * 0.25,
                        (sample(xi + 1, yi)[2]
                            + sample(xi - 1, yi)[2]
                            + sample(xi, yi + 1)[2]
                            + sample(xi, yi - 1)[2])
                            * 0.25,
                    ];
                    c = [
                        c[0] + sharp * 1.5 * (c[0] - avg[0]),
                        c[1] + sharp * 1.5 * (c[1] - avg[1]),
                        c[2] + sharp * 1.5 * (c[2] - avg[2]),
                    ];
                }

                if vig > 0.0 {
                    let uv = [
                        (x as f32 + 0.5) / width as f32,
                        (y as f32 + 0.5) / height as f32,
                    ];
                    let dist_norm = ((uv[0] - 0.5).hypot(uv[1] - 0.5)) / half_diag;
                    let mul = 1.0 - vig * smoothstep(0.4, 0.9, dist_norm);
                    c = [c[0] * mul, c[1] * mul, c[2] * mul];
                }

                let i = idx * 4;
                rgba[i] = quant(c[0]);
                rgba[i + 1] = quant(c[1]);
                rgba[i + 2] = quant(c[2]);
            }
        }
    }
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    if edge0 == edge1 {
        return if x < edge0 { 0.0 } else { 1.0 };
    }
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn quant(x: f32) -> u8 {
    (x.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// `grade.wgsl`, prefixed by the lane layout [`ColorGrade::lanes`] packs.
///
/// The preamble declares the lane count the uniform is sized by and one
/// accessor per slider, so the shader math names a slider instead of counting
/// components. A row added to the list arrives on the shader side in the same
/// build: the uniform grows, the accessor exists, and a shader that never
/// reads it composites the pixel unchanged, which is what
/// `render_grade::every_slider_moves_the_frame_it_grades` refuses.
///
/// Every pipeline that grades prefixes this, so it is built once.
pub(crate) static GRADE_WGSL: LazyLock<String> = LazyLock::new(|| {
    let mut wgsl = String::from(
        "// Generated from aria_models::adjust_slider_rows! — see aria_compositor::grade.\n",
    );
    wgsl.push_str(&format!(
        "const GRADE_LANES: u32 = {}u;\n",
        ColorGrade::LANES
    ));
    for (row, field) in ColorGrade::FIELDS.iter().enumerate() {
        wgsl.push_str(&lane_accessor(field, ColorGrade::slot_of(row)));
    }
    wgsl.push_str(&lane_accessor("enabled", ColorGrade::ENABLED_SLOT));
    wgsl.push_str(include_str!("../shaders/grade.wgsl"));
    wgsl
});

/// One WGSL function reading one slot out of the packed lanes.
fn lane_accessor(name: &str, slot: usize) -> String {
    let component = ["x", "y", "z", "w"][slot % 4];
    format!(
        "fn grade_{name}(a: array<vec4<f32>, GRADE_LANES>) -> f32 {{ return a[{}].{component}; }}\n",
        slot / 4,
    )
}

/// The lane layout is a contract with the shaders, so it is named after what
/// it guards: `cargo test -p aria-compositor --lib -- grade_lane_census`.
#[cfg(test)]
mod grade_lane_census {
    use super::{ColorGrade, GRADE_WGSL};

    /// One writer per row, so a test can set a slider it does not name.
    ///
    /// The row list expands in item position, so the table is a const rather
    /// than an expression.
    macro_rules! grade_setters {
        ($($field:ident => $variant:ident, $shape:ident, $id:literal, $label:literal,
           $doc:literal,)*) => {
            const SETTERS: &[fn(&mut ColorGrade, f32)] =
                &[$(|grade: &mut ColorGrade, value: f32| grade.$field = value,)*];
        };
    }

    aria_models::adjust_slider_rows!(grade_setters);

    #[test]
    fn every_row_packs_into_its_own_slot() {
        assert_eq!(SETTERS.len(), ColorGrade::FIELDS.len());
        // Each slider carries a value no other slider has, so two rows sharing
        // a slot read back as a lost value rather than as a plausible one.
        let mut grade = ColorGrade::IDENTITY;
        for (row, set) in SETTERS.iter().enumerate() {
            set(&mut grade, row as f32 + 1.0);
        }
        let lanes = grade.lanes(1.0);
        for row in 0..SETTERS.len() {
            let slot = ColorGrade::slot_of(row);
            assert_ne!(
                slot,
                ColorGrade::ENABLED_SLOT,
                "row {row} took the enabled flag's slot"
            );
            assert_eq!(
                lanes[slot / 4][slot % 4],
                row as f32 + 1.0,
                "slot {slot} does not hold row {row} ({})",
                ColorGrade::FIELDS[row]
            );
        }
        assert_eq!(
            lanes[ColorGrade::ENABLED_SLOT / 4][ColorGrade::ENABLED_SLOT % 4],
            1.0,
            "the enabled flag was overwritten by a row"
        );
    }

    #[test]
    fn the_lanes_hold_every_slot_and_no_more() {
        let slots = ColorGrade::FIELDS.len() + 1;
        assert_eq!(slots, ColorGrade::SLOTS);
        assert!(
            ColorGrade::LANES * 4 >= ColorGrade::SLOTS,
            "{} lanes cannot carry {} slots",
            ColorGrade::LANES,
            ColorGrade::SLOTS
        );
        assert!(
            (ColorGrade::LANES - 1) * 4 < ColorGrade::SLOTS,
            "the uniform carries a whole lane no slot uses"
        );
        let last = ColorGrade::slot_of(ColorGrade::FIELDS.len() - 1);
        assert!(last < ColorGrade::LANES * 4, "the last row has no lane");
    }

    #[test]
    fn an_identity_grade_packs_the_shader_fast_path() {
        let lanes = ColorGrade::IDENTITY.lanes(0.0);
        assert_eq!(
            lanes[ColorGrade::ENABLED_SLOT / 4][ColorGrade::ENABLED_SLOT % 4],
            0.0
        );
        assert_eq!(
            ColorGrade::IDENTITY.lanes(1.0),
            ColorGrade::default().lanes(1.0)
        );
    }

    #[test]
    fn the_preamble_declares_an_accessor_for_every_row() {
        let wgsl = &*GRADE_WGSL;
        assert!(
            wgsl.contains(&format!("const GRADE_LANES: u32 = {}u;", ColorGrade::LANES)),
            "the shader is not told how many lanes the uniform has"
        );
        for field in ColorGrade::FIELDS {
            assert!(
                wgsl.contains(&format!("fn grade_{field}(")),
                "no shader accessor for {field}"
            );
        }
        assert!(wgsl.contains("fn grade_enabled("));
        assert!(
            wgsl.contains("fn apply_color_grade("),
            "the preamble lost the shader body it prefixes"
        );
    }
}
