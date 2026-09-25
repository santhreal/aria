//! The grade slider row list: the one place an adjustment is declared.
//!
//! Every surface that names the grade expands this list instead of repeating
//! the fields — the model struct, its `Default`, its save format, the slider
//! table, the parameter axis and its catalog row, the assistant's tool
//! arguments and their lowering, and the UI projection — so a new adjustment
//! is one row here and a row a surface cannot reach does not compile.
//!
//! A row reads:
//!
//! ```text
//! field => LookParam variant, shape, "catalog id", "Label", "what it does",
//! ```
//!
//! - **field** is the struct field, the serde name and the assistant's
//!   argument name.
//! - **variant** is the parameter axis a keyframe is addressed by.
//! - **shape** fixes the neutral value, the range a write is validated
//!   against, and the catalog range the inspector draws:
//!   - `signed` — `[-1, 1]`, neutral `0`.
//!   - `positive` — `[0, 1]`, neutral `0`, one-directional: there is no
//!     softening and no inverse vignette.
//!   - `multiplicative` — `[0, 4]`, neutral `1`: gamma powers, gain scales.
//! - **catalog id** is the stable id a keyframe row, a preset and a test name
//!   the slider by. It is written out rather than derived from the field so
//!   renaming a field cannot silently rewrite a saved id.
//! - **Label** is what a surface shows.
//!
//! Row order is panel order and save order; moving a row moves both.
//!
//! # Expanding the list
//!
//! Pass a macro that takes one repetition of rows:
//!
//! ```ignore
//! macro_rules! count_sliders {
//!     ($($field:ident => $variant:ident, $shape:ident, $id:literal, $label:literal,
//!        $doc:literal,)*) => { [$(stringify!($field),)*].len() };
//! }
//! const HOW_MANY: usize = aria_models::adjust_slider_rows!(count_sliders);
//! ```
//!
//! A surface that needs the neutral or the range either branches on `$shape`
//! in expression position or reads it back from
//! [`ColorAdjustments::SLIDERS`](crate::ColorAdjustments::SLIDERS), which this
//! list builds.
#[macro_export]
macro_rules! adjust_slider_rows {
    ($callback:path) => {
        $callback! {
            brightness => AdjustBrightness, signed, "adjust-brightness", "Brightness",
                "Overall lift of every channel.",
            contrast => AdjustContrast, signed, "adjust-contrast", "Contrast",
                "Spread of the tonal range around mid grey.",
            saturation => AdjustSaturation, signed, "adjust-saturation", "Saturation",
                "Colour intensity; `-1` is monochrome.",
            exposure => AdjustExposure, signed, "adjust-exposure", "Exposure",
                "Stops of light, applied before the tone curve.",
            temperature => AdjustTemperature, signed, "adjust-temperature", "Temperature",
                "Cool (−) to warm (+) white balance.",
            tint => AdjustTint, signed, "adjust-tint", "Tint",
                "Green (−) to magenta (+) white balance.",
            hue => AdjustHue, signed, "adjust-hue", "Hue",
                "Hue rotation; ±1 maps to ±30°.",
            highlights => AdjustHighlights, signed, "adjust-highlights", "Highlights",
                "Lift or compress tones above mid luma.",
            shadows => AdjustShadows, signed, "adjust-shadows", "Shadows",
                "Lift or compress tones below mid luma.",
            sharpness => AdjustSharpness, positive, "adjust-sharpness", "Sharpness",
                "Unsharp-mask strength; softening is not supported.",
            vignette => AdjustVignette, positive, "adjust-vignette", "Vignette",
                "Radial darkening from the layer centre.",
            lift_r => AdjustLiftR, signed, "adjust-lift-r", "Lift Red",
                "Shadow offset, red channel.",
            lift_g => AdjustLiftG, signed, "adjust-lift-g", "Lift Green",
                "Shadow offset, green channel.",
            lift_b => AdjustLiftB, signed, "adjust-lift-b", "Lift Blue",
                "Shadow offset, blue channel.",
            lift_master => AdjustLiftMaster, signed, "adjust-lift-master", "Lift Master",
                "Shadow offset, every channel.",
            gamma_r => AdjustGammaR, multiplicative, "adjust-gamma-r", "Gamma Red",
                "Midtone power, red channel.",
            gamma_g => AdjustGammaG, multiplicative, "adjust-gamma-g", "Gamma Green",
                "Midtone power, green channel.",
            gamma_b => AdjustGammaB, multiplicative, "adjust-gamma-b", "Gamma Blue",
                "Midtone power, blue channel.",
            gamma_master => AdjustGammaMaster, multiplicative, "adjust-gamma-master", "Gamma Master",
                "Midtone power, every channel.",
            gain_r => AdjustGainR, multiplicative, "adjust-gain-r", "Gain Red",
                "Highlight scale, red channel.",
            gain_g => AdjustGainG, multiplicative, "adjust-gain-g", "Gain Green",
                "Highlight scale, green channel.",
            gain_b => AdjustGainB, multiplicative, "adjust-gain-b", "Gain Blue",
                "Highlight scale, blue channel.",
            gain_master => AdjustGainMaster, multiplicative, "adjust-gain-master", "Gain Master",
                "Highlight scale, every channel.",
        }
    };
}

/// Every grade axis as one or-pattern, for a match that answers the whole
/// group at once. Expands from the same rows as the sliders, so a new
/// adjustment joins the group without the match being touched.
#[doc(hidden)]
#[macro_export]
macro_rules! __grade_axes_pattern {
    ($($field:ident => $variant:ident, $shape:ident, $id:literal, $label:literal,
       $doc:literal,)*) => {
        $($crate::LookParam::$variant)|*
    };
}

/// Or-pattern matching every grade slider axis — see
/// [`__grade_axes_pattern`].
#[macro_export]
macro_rules! grade_axes {
    () => {
        $crate::adjust_slider_rows!($crate::__grade_axes_pattern)
    };
}
