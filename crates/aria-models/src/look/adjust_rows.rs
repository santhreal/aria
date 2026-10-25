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
                "Spread of the tonal range around mid gray.",
            saturation => AdjustSaturation, signed, "adjust-saturation", "Saturation",
                "Color intensity; `-1` is monochrome.",
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
            whites => AdjustWhites, signed, "adjust-whites", "Whites",
                "White point; `+1` takes 0.75 to white, `-1` takes white to 0.8.",
            blacks => AdjustBlacks, signed, "adjust-blacks", "Blacks",
                "Black point; `+1` lifts black to 0.2, `-1` takes 0.25 to black.",
            sharpness => AdjustSharpness, positive, "adjust-sharpness", "Sharpness",
                "Unsharp-mask strength; softening is not supported.",
            vignette => AdjustVignette, positive, "adjust-vignette", "Vignette",
                "Radial darkening from the layer center.",
            vibrance => AdjustVibrance, signed, "adjust-vibrance", "Vibrance",
                "Saturation weighted toward the least saturated colors.",
            faded_film => AdjustFadedFilm, positive, "adjust-faded-film", "Faded Film",
                "Lifts the blacks toward gray and leaves white in place.",
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
            curve_master_shadows => AdjustCurveMasterShadows, signed,
                "adjust-curve-master-shadows", "Master Curve Shadows",
                "RGB curve, every channel, at 1/4 input; ±1 moves the point a quarter.",
            curve_master_midtones => AdjustCurveMasterMidtones, signed,
                "adjust-curve-master-midtones", "Master Curve Midtones",
                "RGB curve, every channel, at 1/2 input; ±1 moves the point a quarter.",
            curve_master_highlights => AdjustCurveMasterHighlights, signed,
                "adjust-curve-master-highlights", "Master Curve Highlights",
                "RGB curve, every channel, at 3/4 input; ±1 moves the point a quarter.",
            curve_red_shadows => AdjustCurveRedShadows, signed,
                "adjust-curve-red-shadows", "Red Curve Shadows",
                "RGB curve, red, at 1/4 input; ±1 moves the point a quarter.",
            curve_red_midtones => AdjustCurveRedMidtones, signed,
                "adjust-curve-red-midtones", "Red Curve Midtones",
                "RGB curve, red, at 1/2 input; ±1 moves the point a quarter.",
            curve_red_highlights => AdjustCurveRedHighlights, signed,
                "adjust-curve-red-highlights", "Red Curve Highlights",
                "RGB curve, red, at 3/4 input; ±1 moves the point a quarter.",
            curve_green_shadows => AdjustCurveGreenShadows, signed,
                "adjust-curve-green-shadows", "Green Curve Shadows",
                "RGB curve, green, at 1/4 input; ±1 moves the point a quarter.",
            curve_green_midtones => AdjustCurveGreenMidtones, signed,
                "adjust-curve-green-midtones", "Green Curve Midtones",
                "RGB curve, green, at 1/2 input; ±1 moves the point a quarter.",
            curve_green_highlights => AdjustCurveGreenHighlights, signed,
                "adjust-curve-green-highlights", "Green Curve Highlights",
                "RGB curve, green, at 3/4 input; ±1 moves the point a quarter.",
            curve_blue_shadows => AdjustCurveBlueShadows, signed,
                "adjust-curve-blue-shadows", "Blue Curve Shadows",
                "RGB curve, blue, at 1/4 input; ±1 moves the point a quarter.",
            curve_blue_midtones => AdjustCurveBlueMidtones, signed,
                "adjust-curve-blue-midtones", "Blue Curve Midtones",
                "RGB curve, blue, at 1/2 input; ±1 moves the point a quarter.",
            curve_blue_highlights => AdjustCurveBlueHighlights, signed,
                "adjust-curve-blue-highlights", "Blue Curve Highlights",
                "RGB curve, blue, at 3/4 input; ±1 moves the point a quarter.",
            hue_sat_red => AdjustHueSatRed, signed, "adjust-hue-sat-red", "Red Saturation",
                "Saturation of reds; `-1` grays them.",
            hue_sat_yellow => AdjustHueSatYellow, signed,
                "adjust-hue-sat-yellow", "Yellow Saturation",
                "Saturation of yellows; `-1` grays them.",
            hue_sat_green => AdjustHueSatGreen, signed, "adjust-hue-sat-green", "Green Saturation",
                "Saturation of greens; `-1` grays them.",
            hue_sat_cyan => AdjustHueSatCyan, signed, "adjust-hue-sat-cyan", "Cyan Saturation",
                "Saturation of cyans; `-1` grays them.",
            hue_sat_blue => AdjustHueSatBlue, signed, "adjust-hue-sat-blue", "Blue Saturation",
                "Saturation of blues; `-1` grays them.",
            hue_sat_magenta => AdjustHueSatMagenta, signed,
                "adjust-hue-sat-magenta", "Magenta Saturation",
                "Saturation of magentas; `-1` grays them.",
            hue_hue_red => AdjustHueHueRed, signed, "adjust-hue-hue-red", "Red Hue",
                "Hue of reds; ±1 turns them 60°.",
            hue_hue_yellow => AdjustHueHueYellow, signed, "adjust-hue-hue-yellow", "Yellow Hue",
                "Hue of yellows; ±1 turns them 60°.",
            hue_hue_green => AdjustHueHueGreen, signed, "adjust-hue-hue-green", "Green Hue",
                "Hue of greens; ±1 turns them 60°.",
            hue_hue_cyan => AdjustHueHueCyan, signed, "adjust-hue-hue-cyan", "Cyan Hue",
                "Hue of cyans; ±1 turns them 60°.",
            hue_hue_blue => AdjustHueHueBlue, signed, "adjust-hue-hue-blue", "Blue Hue",
                "Hue of blues; ±1 turns them 60°.",
            hue_hue_magenta => AdjustHueHueMagenta, signed, "adjust-hue-hue-magenta", "Magenta Hue",
                "Hue of magentas; ±1 turns them 60°.",
            hue_luma_red => AdjustHueLumaRed, signed, "adjust-hue-luma-red", "Red Luma",
                "Luma of reds; ±1 is ±0.25.",
            hue_luma_yellow => AdjustHueLumaYellow, signed, "adjust-hue-luma-yellow", "Yellow Luma",
                "Luma of yellows; ±1 is ±0.25.",
            hue_luma_green => AdjustHueLumaGreen, signed, "adjust-hue-luma-green", "Green Luma",
                "Luma of greens; ±1 is ±0.25.",
            hue_luma_cyan => AdjustHueLumaCyan, signed, "adjust-hue-luma-cyan", "Cyan Luma",
                "Luma of cyans; ±1 is ±0.25.",
            hue_luma_blue => AdjustHueLumaBlue, signed, "adjust-hue-luma-blue", "Blue Luma",
                "Luma of blues; ±1 is ±0.25.",
            hue_luma_magenta => AdjustHueLumaMagenta, signed,
                "adjust-hue-luma-magenta", "Magenta Luma",
                "Luma of magentas; ±1 is ±0.25.",
            hsl_key_hue => AdjustHslKeyHue, signed, "adjust-hsl-key-hue", "Key Hue",
                "Center of the keyed hue range; 0 is red, ±1 is cyan.",
            hsl_key_width => AdjustHslKeyWidth, signed, "adjust-hsl-key-width", "Key Range",
                "Half-width of the keyed hue range: 30° at 0, 60° at `+1`, 0° at `-1`.",
            hsl_hue => AdjustHslHue, signed, "adjust-hsl-hue", "Secondary Hue",
                "Hue of the keyed colors; ±1 turns them 60°.",
            hsl_saturation => AdjustHslSaturation, signed,
                "adjust-hsl-saturation", "Secondary Saturation",
                "Saturation of the keyed colors; `-1` grays them.",
            hsl_luma => AdjustHslLuma, signed, "adjust-hsl-luma", "Secondary Luma",
                "Luma of the keyed colors; ±1 is ±0.25.",
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
