//! The property selectors the keyframe and constant commands address.
//!
//! Beside [`super`] rather than inside it only for length: these are the same
//! wire DTOs, re-exported through the same `pub use`. Split here because the
//! selector tree is the one part of the parameter wire that grows every time a
//! new animatable property ships, and it grows independently of the commands
//! that carry it.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// An animatable clip property the keyframe commands can address.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WireClipParam {
    /// Canvas placement of the content center (vec2: use the `position`
    /// argument, not `value`).
    Position,
    /// Canvas point around which the clip scales and rotates (vec2: use the
    /// `position` argument, not `value`).
    AnchorPoint,
    /// Scale (1.0 = fit inside the canvas). Use `value` for a uniform number
    /// or `position` as `[x, y]` for per-axis.
    Scale,
    /// Clockwise rotation in degrees.
    Rotation,
    /// Layer opacity 0.0–1.0.
    Opacity,
    /// Kept-region crop window as content fractions (rect: use the `rect`
    /// argument `[x, y, w, h]`, not `value`). Visual clips only.
    Crop,
    /// Audio gain envelope (0.0 = mute, 1.0 = unchanged, up to 10.0 boost).
    /// Keyframing it draws volume automation; this is how you fade audio in
    /// or out over time or duck music under a voice. Media-backed clips only.
    Volume,
    /// Stereo pan envelope (−1.0 = full left, 0.0 = center, +1.0 = full
    /// right). Keyframing it draws balance automation. Media-backed clips
    /// only (including video clips with sound — same target rule as volume).
    Pan,
    /// Playback-rate ramp (scalar: 1.0 = normal speed). Media-backed clips
    /// only; keyframes use normalized positions within the clip.
    Speed,
    /// A parameter of an effect already on the clip. `index` is the
    /// effect-chain position and `param` is its catalog name, as in
    /// `set_effect_param`. Use `value` for scalars, `position` for vec2
    /// params, and `rgba` for color params.
    Effect { index: u32, param: String },
    /// A visual property of a generated shape clip.
    Shape { param: WireShapeParam },
    /// A visual style property of a generated text clip.
    Text { param: WireTextParam },
    /// A scalar property of the clip's color look.
    Look { param: WireLookParam },
    /// A property of one mask in the clip's mask stack. `index` is the
    /// mask's position (0 = top), as in `set_clip_mask`. `center` and `size`
    /// use `position`; the rest use `value`.
    Mask { index: u32, param: WireMaskParam },
    /// A property of the clip's layer-quad styles (shadow/glow/outline/
    /// background). Color params use `rgba`; `shadow_offset` uses
    /// `position`; the rest use scalar `value`.
    Style { param: WireStyleParam },
}

/// Animatable properties of a generated shape clip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WireShapeParam {
    Width,
    Height,
    CornerRadius,
    InnerRatio,
    Fill,
    StrokeColor,
    StrokeWidth,
}

/// Animatable visual-style properties of a generated text clip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WireTextParam {
    Size,
    Fill,
    LetterSpacing,
    LineSpacing,
    StrokeWidth,
    StrokeColor,
    ShadowBlur,
    ShadowDistance,
    ShadowColor,
    BackgroundColor,
    BackgroundRadius,
}

/// Animatable scalar properties of a clip's color look.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WireLookParam {
    FilterIntensity,
    LutIntensity,
    AdjustBrightness,
    AdjustContrast,
    AdjustSaturation,
    AdjustExposure,
    AdjustTemperature,
    AdjustTint,
    AdjustHue,
    AdjustHighlights,
    AdjustShadows,
    AdjustWhites,
    AdjustBlacks,
    AdjustSharpness,
    AdjustVignette,
    AdjustVibrance,
    AdjustFadedFilm,
    /// Lift / gamma / gain, per channel and master: the colour wheels. Lift is
    /// a signed offset (-1 … 1, 0 neutral); gamma and gain are multiplicative
    /// (0 … 4, 1 neutral).
    AdjustLiftR,
    AdjustLiftG,
    AdjustLiftB,
    AdjustLiftMaster,
    AdjustGammaR,
    AdjustGammaG,
    AdjustGammaB,
    AdjustGammaMaster,
    AdjustGainR,
    AdjustGainG,
    AdjustGainB,
    AdjustGainMaster,
    /// RGB curves, hue curves and the HSL secondary: signed (-1 … 1, 0
    /// neutral).
    AdjustCurveMasterShadows,
    AdjustCurveMasterMidtones,
    AdjustCurveMasterHighlights,
    AdjustCurveRedShadows,
    AdjustCurveRedMidtones,
    AdjustCurveRedHighlights,
    AdjustCurveGreenShadows,
    AdjustCurveGreenMidtones,
    AdjustCurveGreenHighlights,
    AdjustCurveBlueShadows,
    AdjustCurveBlueMidtones,
    AdjustCurveBlueHighlights,
    AdjustHueSatRed,
    AdjustHueSatYellow,
    AdjustHueSatGreen,
    AdjustHueSatCyan,
    AdjustHueSatBlue,
    AdjustHueSatMagenta,
    AdjustHueHueRed,
    AdjustHueHueYellow,
    AdjustHueHueGreen,
    AdjustHueHueCyan,
    AdjustHueHueBlue,
    AdjustHueHueMagenta,
    AdjustHueLumaRed,
    AdjustHueLumaYellow,
    AdjustHueLumaGreen,
    AdjustHueLumaCyan,
    AdjustHueLumaBlue,
    AdjustHueLumaMagenta,
    AdjustHslKeyHue,
    AdjustHslKeyWidth,
    AdjustHslHue,
    AdjustHslSaturation,
    AdjustHslLuma,
    ChromaStrength,
    ChromaShadow,
    MatteSoftness,
}

/// Animatable properties of one mask in a clip's mask stack.
///
/// `center` / `size` carry vec2 values; the rest are scalars.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WireMaskParam {
    /// Edge softness, 0 … 1.
    Feather,
    /// Centre offset as a fraction of the layer size.
    Center,
    /// Size as a fraction of the layer size.
    Size,
    /// Rotation in degrees, clockwise.
    Rotation,
    /// Rectangle corner rounding, 0 … 1.
    Roundness,
    /// How much of the mask's result replaces the coverage above it, 0 … 1.
    Opacity,
    /// Edge offset in reference pixels, outward when positive.
    Expansion,
}

/// Animatable properties of a clip's layer-quad styles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WireStyleParam {
    ShadowColor,
    ShadowOffset,
    ShadowBlur,
    GlowColor,
    GlowRadius,
    GlowIntensity,
    OutlineColor,
    OutlineWidth,
    BackgroundColor,
    BackgroundPadding,
    BackgroundRadius,
}
