//! Which clip property a parameter command addresses.
//!
//! [`ClipParam`] and its per-axis sub-enums, each declared together with a
//! census of its variants, plus the classification that places each variant
//! in the animatable-property registry.

use serde::{Deserialize, Serialize};

#[cfg(doc)]
use super::ParamValue;

/// Declare [`ClipParam`] together with a census of its variants.
///
/// A variant that carries a payload cannot be listed as a plain unit value the
/// way `param_enum!`'s `ALL` lists one, so each variant names one
/// representative value of itself and the census is emitted from that same
/// list. A variant cannot be added without a census entry, which is what stops
/// [`ClipParam::CENSUS`] from going quietly short while a new property ships
/// uncovered.
macro_rules! clip_param_enum {
    (
        $(#[$enum_meta:meta])*
        pub enum $name:ident {
            $(
                $(#[$variant_meta:meta])*
                $variant:ident $({ $($field:ident : $fty:ty),+ $(,)? })? => $census:expr,
            )+
        }
    ) => {
        $(#[$enum_meta])*
        pub enum $name {
            $(
                $(#[$variant_meta])*
                $variant $({ $($field: $fty),+ })?,
            )+
        }

        impl $name {
            /// One value of every variant, in declaration order.
            ///
            /// A payload variant stands for itself: the classification in
            /// [`ClipParam::class`] is per variant, so one sample covers the
            /// variant whatever its payload.
            pub const CENSUS: &'static [Self] = &[$($census),+];
        }
    };
}

clip_param_enum! {
/// Which animatable clip property a parameter command addresses. Grows as
/// later milestones make more properties animatable (effect params, volume).
///
/// [`ClipParam::class`] declares what each variant is: a property addressed
/// directly, a wrapper over one of the property axes, or a parameter the
/// animatable-property registry holds no row for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClipParam {
    Position => ClipParam::Position,
    AnchorPoint => ClipParam::AnchorPoint,
    Scale => ClipParam::Scale,
    Rotation => ClipParam::Rotation,
    Opacity => ClipParam::Opacity,
    /// The clip's crop window (kept-region rect in content fractions).
    /// Routed to [`crate::Clip::crop`] instead of the transform. Always contains a
    /// [`ParamValue::Rect`] `[x, y, w, h]`.
    Crop => ClipParam::Crop,
    /// The clip's playback-rate ramp (M2 speed curves). Animates the
    /// instantaneous speed *multiplier* over the clip's normalized span
    /// (`speed_curve`), not the clip transform — its keyframe ticks live in
    /// `0..=`[`crate::SPEED_CURVE_SCALE`], and editing it re-derives the clip's
    /// timeline duration. Always carries a [`ParamValue::Scalar`].
    Speed => ClipParam::Speed,
    /// The clip's audio gain envelope (M8 volume envelopes). Routed to the
    /// clip's `volume: Param<f32>` instead of the transform, so the same
    /// keyframe commands draw volume automation and ducking writes ordinary
    /// volume keyframes. Media-backed clips only. Always carries a
    /// [`ParamValue::Scalar`] in `0..=`[`crate::MAX_CLIP_VOLUME`].
    Volume => ClipParam::Volume,
    /// The clip's stereo pan envelope. Routed to the clip's `pan: Param<f32>`
    /// instead of the transform. Media-backed clips only (same target rule as
    /// [`ClipParam::Volume`] — video clips with sound can pan too). Always
    /// carries a [`ParamValue::Scalar`] in `−1..=1`.
    Pan => ClipParam::Pan,
    /// A parameter of one of the clip's effects (M4): `effect` is the index
    /// into [`crate::Clip::effects`], `param` the catalog slot. Routed to the
    /// effect's typed param maps instead of the transform. Carries
    /// [`ParamValue::Scalar`], [`ParamValue::Color`], or [`ParamValue::Vec2`]
    /// matching the catalog slot's [`crate::EffectParamKind`].
    Effect {
        effect: u32,
        param: u32,
    } => ClipParam::Effect { effect: 0, param: 0 },
    /// An animatable property of a [`crate::Generator::Shape`] clip. Routed to the
    /// generator's own `Param`s instead of the transform, so the same
    /// keyframe commands animate shape geometry and colors. Scalar
    /// properties carry [`ParamValue::Scalar`]; `Fill`/`StrokeColor` carry
    /// [`ParamValue::Color`].
    Shape {
        param: ShapeParam,
    } => ClipParam::Shape { param: ShapeParam::Width },
    /// An animatable property of a [`crate::Generator::Text`] clip's visual style.
    /// Scalar properties carry [`ParamValue::Scalar`]; color properties carry
    /// [`ParamValue::Color`].
    Text {
        param: TextParam,
    } => ClipParam::Text { param: TextParam::Size },
    /// An animatable property of the clip's color look. Structural look
    /// selections (filter id, LUT path, chroma color) remain ordinary clip
    /// edits.
    Look {
        param: LookParam,
    } => ClipParam::Look { param: LookParam::FilterIntensity },
    /// An animatable property of one of the clip's masks: `mask` is the index
    /// into [`crate::Clip::masks`]. The shape, mode, inversion and path remain
    /// ordinary mask edits.
    Mask {
        mask: u32,
        param: MaskParam,
    } => ClipParam::Mask { mask: 0, param: MaskParam::Feather },
    /// An animatable property of the clip's layer styles. Enabling/removing a
    /// style block remains a structural edit (`SetClipLayerStyles`).
    Style {
        param: StyleParam,
    } => ClipParam::Style { param: StyleParam::ShadowColor },
}
}

/// Declare a unit-variant parameter enum together with its census.
///
/// The census (`ALL`) is emitted from the same variant list as the enum, so a
/// new property appears in it by construction. Everything downstream that has
/// to cover every property — the catalog in
/// [`crate::clip_param_catalog`], the wire schema, the keyframe strip, the
/// conformance suites — iterates `ALL` rather than restating the list, which is
/// what makes a forgotten property a red test instead of a silent hole.
macro_rules! param_enum {
    (
        $(#[$enum_meta:meta])*
        pub enum $name:ident {
            $(
                $(#[$variant_meta:meta])*
                $variant:ident,
            )+
        }
    ) => {
        $(#[$enum_meta])*
        pub enum $name {
            $(
                $(#[$variant_meta])*
                $variant,
            )+
        }

        impl $name {
            /// Every variant, in declaration order.
            ///
            /// Generated beside the enum, so a variant cannot exist without
            /// appearing here.
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];
        }
    };
}

param_enum! {
/// The animatable properties of a [`crate::Generator::Shape`] (see
/// [`ClipParam::Shape`]). Structural knobs — the shape kind, polygon sides,
/// star points, path points — are not animatable; they change through
/// `SetGenerator`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShapeParam {
    /// Shape box width (reference px). Scalar.
    Width,
    /// Shape box height (reference px). Scalar.
    Height,
    /// Corner rounding (reference px). Scalar; rect/polygon/star only honor
    /// it visually but it may be set on any shape.
    CornerRadius,
    /// Star inner-vertex radius fraction. Scalar; star shapes only.
    InnerRatio,
    /// Fill color. Color.
    Fill,
    /// Stroke color. Color; requires the shape to have a stroke.
    StrokeColor,
    /// Stroke width (reference px). Scalar; requires the shape to have a
    /// stroke.
    StrokeWidth,
}
}

param_enum! {
/// The animatable visual treatment properties of a [`crate::Generator::Text`] clip.
/// Font selection and layout structure remain ordinary generator edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextParam {
    /// Glyph size (reference px). Scalar.
    Size,
    /// Glyph fill color. Color.
    Fill,
    /// Extra space between glyphs (reference px). Scalar.
    LetterSpacing,
    /// Extra space between lines (reference px). Scalar.
    LineSpacing,
    /// Glyph stroke width (reference px). Scalar.
    StrokeWidth,
    /// Glyph stroke color. Color.
    StrokeColor,
    /// Text shadow blur radius (reference px). Scalar.
    ShadowBlur,
    /// Text shadow offset distance (reference px). Scalar.
    ShadowDistance,
    /// Text shadow color. Color.
    ShadowColor,
    /// Background card color. Color; requires a background block.
    BackgroundColor,
    /// Background card corner rounding (`0` … `1`). Scalar; requires a
    /// background block.
    BackgroundRadius,
}
}

param_enum! {
/// The animatable properties in a clip's color look, chroma key and track
/// matte. Every one carries [`ParamValue::Scalar`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LookParam {
    /// Colour filter strength (`0` … `1`). Scalar; requires a filter.
    FilterIntensity,
    /// LUT strength (`0` … `1`). Scalar; requires a LUT.
    LutIntensity,
    /// Grade brightness. Scalar.
    AdjustBrightness,
    /// Grade contrast. Scalar.
    AdjustContrast,
    /// Grade saturation. Scalar.
    AdjustSaturation,
    /// Grade exposure. Scalar.
    AdjustExposure,
    /// Grade temperature. Scalar.
    AdjustTemperature,
    /// Grade tint. Scalar.
    AdjustTint,
    /// Grade hue rotation. Scalar.
    AdjustHue,
    /// Grade highlight recovery. Scalar.
    AdjustHighlights,
    /// Grade shadow lift. Scalar.
    AdjustShadows,
    /// Grade white point. Scalar.
    AdjustWhites,
    /// Grade black point. Scalar.
    AdjustBlacks,
    /// Grade sharpness. Scalar.
    AdjustSharpness,
    /// Grade vignette strength. Scalar.
    AdjustVignette,
    /// Grade vibrance. Scalar.
    AdjustVibrance,
    /// Grade faded film strength. Scalar.
    AdjustFadedFilm,
    /// Grade lift red channel offset. Scalar.
    AdjustLiftR,
    /// Grade lift green channel offset. Scalar.
    AdjustLiftG,
    /// Grade lift blue channel offset. Scalar.
    AdjustLiftB,
    /// Grade lift master offset. Scalar.
    AdjustLiftMaster,
    /// Grade gamma red channel power. Scalar.
    AdjustGammaR,
    /// Grade gamma green channel power. Scalar.
    AdjustGammaG,
    /// Grade gamma blue channel power. Scalar.
    AdjustGammaB,
    /// Grade gamma master power. Scalar.
    AdjustGammaMaster,
    /// Grade gain red channel scale. Scalar.
    AdjustGainR,
    /// Grade gain green channel scale. Scalar.
    AdjustGainG,
    /// Grade gain blue channel scale. Scalar.
    AdjustGainB,
    /// Grade gain master scale. Scalar.
    AdjustGainMaster,
    /// Grade RGB curve, every channel, shadows point. Scalar.
    AdjustCurveMasterShadows,
    /// Grade RGB curve, every channel, midtones point. Scalar.
    AdjustCurveMasterMidtones,
    /// Grade RGB curve, every channel, highlights point. Scalar.
    AdjustCurveMasterHighlights,
    /// Grade RGB curve, red, shadows point. Scalar.
    AdjustCurveRedShadows,
    /// Grade RGB curve, red, midtones point. Scalar.
    AdjustCurveRedMidtones,
    /// Grade RGB curve, red, highlights point. Scalar.
    AdjustCurveRedHighlights,
    /// Grade RGB curve, green, shadows point. Scalar.
    AdjustCurveGreenShadows,
    /// Grade RGB curve, green, midtones point. Scalar.
    AdjustCurveGreenMidtones,
    /// Grade RGB curve, green, highlights point. Scalar.
    AdjustCurveGreenHighlights,
    /// Grade RGB curve, blue, shadows point. Scalar.
    AdjustCurveBlueShadows,
    /// Grade RGB curve, blue, midtones point. Scalar.
    AdjustCurveBlueMidtones,
    /// Grade RGB curve, blue, highlights point. Scalar.
    AdjustCurveBlueHighlights,
    /// Grade hue curve, saturation of reds. Scalar.
    AdjustHueSatRed,
    /// Grade hue curve, saturation of yellows. Scalar.
    AdjustHueSatYellow,
    /// Grade hue curve, saturation of greens. Scalar.
    AdjustHueSatGreen,
    /// Grade hue curve, saturation of cyans. Scalar.
    AdjustHueSatCyan,
    /// Grade hue curve, saturation of blues. Scalar.
    AdjustHueSatBlue,
    /// Grade hue curve, saturation of magentas. Scalar.
    AdjustHueSatMagenta,
    /// Grade hue curve, hue of reds. Scalar.
    AdjustHueHueRed,
    /// Grade hue curve, hue of yellows. Scalar.
    AdjustHueHueYellow,
    /// Grade hue curve, hue of greens. Scalar.
    AdjustHueHueGreen,
    /// Grade hue curve, hue of cyans. Scalar.
    AdjustHueHueCyan,
    /// Grade hue curve, hue of blues. Scalar.
    AdjustHueHueBlue,
    /// Grade hue curve, hue of magentas. Scalar.
    AdjustHueHueMagenta,
    /// Grade hue curve, luma of reds. Scalar.
    AdjustHueLumaRed,
    /// Grade hue curve, luma of yellows. Scalar.
    AdjustHueLumaYellow,
    /// Grade hue curve, luma of greens. Scalar.
    AdjustHueLumaGreen,
    /// Grade hue curve, luma of cyans. Scalar.
    AdjustHueLumaCyan,
    /// Grade hue curve, luma of blues. Scalar.
    AdjustHueLumaBlue,
    /// Grade hue curve, luma of magentas. Scalar.
    AdjustHueLumaMagenta,
    /// Grade HSL secondary key centre. Scalar.
    AdjustHslKeyHue,
    /// Grade HSL secondary key width. Scalar.
    AdjustHslKeyWidth,
    /// Grade HSL secondary hue turn. Scalar.
    AdjustHslHue,
    /// Grade HSL secondary saturation. Scalar.
    AdjustHslSaturation,
    /// Grade HSL secondary luma offset. Scalar.
    AdjustHslLuma,
    /// Chroma-key tolerance (`0` … `1`). Scalar; requires a chroma key.
    ChromaStrength,
    /// Chroma-key shadow retention (`0` … `1`). Scalar; requires a chroma key.
    ChromaShadow,
    /// Track-matte edge softness (`0` … `1`). Scalar; requires a track matte.
    MatteSoftness,
}
}

param_enum! {
/// An animatable property of one of the clip's masks (see [`ClipParam::Mask`]).
///
/// [`MaskParam::Center`] and [`MaskParam::Size`] carry [`ParamValue::Vec2`];
/// the rest carry [`ParamValue::Scalar`]. The shape, mode, inversion and path
/// are not animatable; they change through `SetClipMask`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaskParam {
    /// Edge softness (`0` … `1`). Scalar.
    Feather,
    /// Centre offset as a fraction of layer size. Vec2.
    Center,
    /// Size as a fraction of layer size. Vec2.
    Size,
    /// Rotation in degrees (clockwise). Scalar.
    Rotation,
    /// Rectangle corner rounding (`0` … `1`). Scalar.
    Roundness,
    /// How much of the mask's result replaces the stack above it (`0` … `1`).
    /// Scalar.
    Opacity,
    /// Edge offset in reference pixels, outward when positive. Scalar.
    Expansion,
}
}

param_enum! {
/// An animatable property of the clip's [`crate::LayerStyles`].
///
/// `*Color` params carry [`ParamValue::Color`]; [`StyleParam::ShadowOffset`]
/// carries [`ParamValue::Vec2`]; the rest carry [`ParamValue::Scalar`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StyleParam {
    /// Shadow color. Color.
    ShadowColor,
    /// Shadow offset in reference pixels. Vec2.
    ShadowOffset,
    /// Shadow blur radius in reference pixels. Scalar.
    ShadowBlur,
    /// Glow color. Color.
    GlowColor,
    /// Glow radius in reference pixels. Scalar.
    GlowRadius,
    /// Glow intensity (`0` … `4`). Scalar.
    GlowIntensity,
    /// Outline color. Color.
    OutlineColor,
    /// Outline width in reference pixels. Scalar.
    OutlineWidth,
    /// Background plate color. Color.
    BackgroundColor,
    /// Background padding in reference pixels. Scalar.
    BackgroundPadding,
    /// Background corner radius in reference pixels. Scalar.
    BackgroundRadius,
}
}

param_enum! {
/// A property axis: a sub-enum of clip properties that one [`ClipParam`]
/// wrapper variant addresses as a group.
///
/// An axis carries no row of its own. The animatable-property registry holds
/// one row per member of the sub-enum, so a surface enumerates the members
/// rather than the wrapper.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ParamAxis {
    /// [`LookParam`]: colour grade, chroma key, track matte.
    Look,
    /// [`StyleParam`]: shadow, glow, outline, background plate.
    Style,
    /// [`TextParam`]: the visual treatment of a text generator.
    Text,
    /// [`ShapeParam`]: the geometry and colours of a shape generator.
    Shape,
}
}

impl ParamAxis {
    /// Every [`ClipParam`] this axis covers, one per member of its sub-enum,
    /// in the sub-enum's declaration order.
    ///
    /// Read from the sub-enum's own `ALL`, so a property added there is
    /// covered here without a second list to maintain.
    pub fn params(self) -> impl Iterator<Item = ClipParam> {
        type Slices = (
            &'static [LookParam],
            &'static [StyleParam],
            &'static [TextParam],
            &'static [ShapeParam],
        );
        let (look, style, text, shape): Slices = match self {
            Self::Look => (LookParam::ALL, &[], &[], &[]),
            Self::Style => (&[], StyleParam::ALL, &[], &[]),
            Self::Text => (&[], &[], TextParam::ALL, &[]),
            Self::Shape => (&[], &[], &[], ShapeParam::ALL),
        };
        look.iter()
            .map(|&param| ClipParam::Look { param })
            .chain(style.iter().map(|&param| ClipParam::Style { param }))
            .chain(text.iter().map(|&param| ClipParam::Text { param }))
            .chain(shape.iter().map(|&param| ClipParam::Shape { param }))
    }
}

/// Why the animatable-property registry holds no row for a [`ClipParam`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ParamExclusion {
    /// The parameter names a slot on one of the clip's effects. Effect rows
    /// are per effect id: [`crate::effect_catalog`] declares the slots and
    /// [`crate::clip_params_for`] builds a row for each slot of each effect a
    /// clip carries, so the static catalog holds none.
    EffectSlot,
    /// A speed ramp is keyframed on its own normalized tick scale
    /// (`0..=`[`crate::SPEED_CURVE_SCALE`]) through `set_clip_speed_curve`, and
    /// editing it re-derives the clip's timeline duration. It is not reachable
    /// through `set_param_keyframe`, so it is not a property in the registry's
    /// sense.
    SpeedCurve,
    /// The parameter names a property of one of the clip's masks. Mask rows
    /// are per mask: [`crate::clip_params_for`] builds a row for each property
    /// of each mask a clip carries, so the static catalog holds none.
    MaskSlot,
}

/// What a [`ClipParam`] variant addresses, and so where its rows in the
/// animatable-property registry ([`crate::clip_param_catalog`]) are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClipParamClass {
    /// One property of the clip, addressed directly. The catalog holds exactly
    /// one row for the variant, under this id.
    Direct(&'static str),
    /// A wrapper over a property axis. The rows belong to the axis's members,
    /// one each; the wrapper names no property by itself.
    Delegated(ParamAxis),
    /// No row, for the reason carried here.
    Excluded(ParamExclusion),
}
