use serde::{Deserialize, Serialize};

use crate::clip::CropRect;
use crate::error::ModelError;
use crate::param::{Easing, Keyframe, Lerp, Param, SegmentSample};

/// Per-axis scale. Serializes as a bare number when uniform (so old builds
/// and old saves stay interchangeable) and as `[x, y]` when split.
/// Deserializes from either form.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scale2 {
    pub x: f32,
    pub y: f32,
}

impl Scale2 {
    pub const ONE: Self = Self { x: 1.0, y: 1.0 };

    #[inline]
    pub const fn uniform(s: f32) -> Self {
        Self { x: s, y: s }
    }

    /// Exact equality — values come from the same slider unless the user
    /// (or a vec2 keyframe) has split the axes.
    #[inline]
    pub fn is_uniform(self) -> bool {
        self.x == self.y
    }

    /// Geometric mean of the absolute axes. Keeps stroke / blur / style
    /// reference-px widths stable under uniform scale and splits the
    /// difference under stretch (so a `[4, 1]` scale still doubles widths).
    #[inline]
    pub fn isotropic(self) -> f32 {
        (self.x.abs() * self.y.abs()).sqrt()
    }
}

impl Default for Scale2 {
    fn default() -> Self {
        Self::ONE
    }
}

impl From<f32> for Scale2 {
    fn from(s: f32) -> Self {
        Self::uniform(s)
    }
}

impl From<[f32; 2]> for Scale2 {
    fn from([x, y]: [f32; 2]) -> Self {
        Self { x, y }
    }
}

impl From<Scale2> for [f32; 2] {
    fn from(s: Scale2) -> Self {
        [s.x, s.y]
    }
}

impl std::ops::Mul<f32> for Scale2 {
    type Output = Self;
    fn mul(self, rhs: f32) -> Self {
        Self {
            x: self.x * rhs,
            y: self.y * rhs,
        }
    }
}

impl std::ops::MulAssign<f32> for Scale2 {
    fn mul_assign(&mut self, rhs: f32) {
        self.x *= rhs;
        self.y *= rhs;
    }
}

impl Lerp for Scale2 {
    fn lerp(a: Self, b: Self, t: f32) -> Self {
        Self {
            x: f32::lerp(a.x, b.x, t),
            y: f32::lerp(a.y, b.y, t),
        }
    }
}

impl crate::param::Extrapolate for Scale2 {}

impl SegmentSample for Scale2 {
    fn segment_delta(a: &Keyframe<Self>, b: &Keyframe<Self>) -> f32 {
        let dx = b.value.x - a.value.x;
        let dy = b.value.y - a.value.y;
        if dx.abs() >= dy.abs() { dx } else { dy }
    }

    fn segment_distance(a: &Keyframe<Self>, b: &Keyframe<Self>) -> f32 {
        let dx = b.value.x - a.value.x;
        let dy = b.value.y - a.value.y;
        (dx * dx + dy * dy).sqrt()
    }
}

impl Serialize for Scale2 {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if self.is_uniform() {
            serializer.serialize_f32(self.x)
        } else {
            [self.x, self.y].serialize(serializer)
        }
    }
}

impl<'de> Deserialize<'de> for Scale2 {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Wire {
            Uniform(f32),
            Axes([f32; 2]),
        }
        Ok(match Wire::deserialize(deserializer)? {
            Wire::Uniform(s) => Self::uniform(s),
            Wire::Axes([x, y]) => Self { x, y },
        })
    }
}

/// Spatial placement of a clip's content on the canvas (CapCut "Basic"
/// transform: position, anchor, scale, rotation, opacity).
///
/// Coordinates are normalized to the canvas so projects survive canvas-size
/// changes: `position` is the offset of the `anchor_point` from the canvas
/// center as a fraction of canvas width/height (+x right, +y down — screen
/// convention). With the default center anchor this matches the legacy
/// content-center semantics. `anchor_point` is the pivot within the content
/// bounds (0,0 = top-left, 0.5,0.5 = center). `scale` is per-axis with 1.0 =
/// aspect-fit inside the canvas (CapCut's 100%); uniform scales serialize as
/// a bare float for save/compat. `rotation` is degrees clockwise about the
/// anchor.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ClipTransform {
    /// Anchor offset from canvas center, normalized to canvas dimensions.
    /// `[0.0, 0.0]` = anchor on the canvas center; `[0.5, 0.0]` = anchor on
    /// the right canvas edge.
    pub position: [f32; 2],
    /// Pivot within the content bounds, normalized to the placed size
    /// (+x right, +y down). `[0.5, 0.5]` = content center (default).
    #[serde(default = "default_anchor_point")]
    pub anchor_point: [f32; 2],
    /// Per-axis scale; `[1, 1]` aspect-fits the content inside the canvas.
    /// Uniform values serialize as a bare float.
    pub scale: Scale2,
    /// Clockwise rotation in degrees about the anchor.
    pub rotation: f32,
    /// Layer opacity, 0.0 (transparent) ..= 1.0 (opaque).
    pub opacity: f32,
}

fn default_anchor_point() -> [f32; 2] {
    [0.5, 0.5]
}

impl ClipTransform {
    pub const IDENTITY: Self = Self {
        position: [0.0, 0.0],
        anchor_point: [0.5, 0.5],
        scale: Scale2::ONE,
        rotation: 0.0,
        opacity: 1.0,
    };

    pub fn is_identity(&self) -> bool {
        *self == Self::IDENTITY
    }

    /// `Ok` iff every component is finite, both scale axes are positive, and
    /// opacity is within `0..=1` — the invariant
    /// [`crate::Project::set_transform`] enforces before storing.
    pub fn validate(&self) -> Result<(), ModelError> {
        let finite = self.position.iter().all(|v| v.is_finite())
            && self.anchor_point.iter().all(|v| v.is_finite())
            && self.scale.x.is_finite()
            && self.scale.y.is_finite()
            && self.rotation.is_finite()
            && self.opacity.is_finite();
        if !finite {
            return Err(ModelError::InvalidTransform("non-finite component".into()));
        }
        validate_scale(self.scale)?;
        if !(0.0..=1.0).contains(&self.opacity) {
            return Err(ModelError::InvalidTransform(
                "opacity must be in 0..=1".into(),
            ));
        }
        Ok(())
    }
}

impl Default for ClipTransform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

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

/// A value for a [`ClipParam`]: scalar properties take `Scalar`, `position`
/// / per-axis `scale` take `Vec2` (scale also accepts `Scalar` → uniform),
/// color properties (shape fill/stroke) take `Color`, and crop (and any
/// future 4-float rect) take `Rect` as `[x, y, w, h]`. Commands carry this
/// so one command shape serves every param kind.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParamValue {
    Scalar(f32),
    Vec2([f32; 2]),
    Vec3([f32; 3]),
    Color([u8; 4]),
    /// Axis-aligned rect as `[x, y, w, h]` (e.g. [`ClipParam::Crop`]).
    Rect([f32; 4]),
}

impl ParamValue {
    pub(crate) fn scalar(self) -> Result<f32, ModelError> {
        match self {
            ParamValue::Scalar(v) => Ok(v),
            _ => Err(ModelError::InvalidParam("expected a scalar value".into())),
        }
    }

    pub(crate) fn vec2(self) -> Result<[f32; 2], ModelError> {
        match self {
            ParamValue::Vec2(v) => Ok(v),
            _ => Err(ModelError::InvalidParam("expected a vec2 value".into())),
        }
    }

    pub(super) fn color(self) -> Result<[u8; 4], ModelError> {
        match self {
            ParamValue::Color(v) => Ok(v),
            _ => Err(ModelError::InvalidParam("expected a color value".into())),
        }
    }

    pub(crate) fn rect(self) -> Result<[f32; 4], ModelError> {
        match self {
            ParamValue::Rect(v) => Ok(v),
            _ => Err(ModelError::InvalidParam("expected a rect value".into())),
        }
    }
}

impl Lerp for ParamValue {
    fn lerp(a: Self, b: Self, t: f32) -> Self {
        match (a, b) {
            (Self::Scalar(x), Self::Scalar(y)) => Self::Scalar(f32::lerp(x, y, t)),
            (Self::Vec2(x), Self::Vec2(y)) => Self::Vec2(<[f32; 2]>::lerp(x, y, t)),
            (Self::Vec3(x), Self::Vec3(y)) => Self::Vec3(<[f32; 3]>::lerp(x, y, t)),
            (Self::Color(x), Self::Color(y)) => Self::Color(<[u8; 4]>::lerp(x, y, t)),
            (Self::Rect(x), Self::Rect(y)) => Self::Rect([
                f32::lerp(x[0], y[0], t),
                f32::lerp(x[1], y[1], t),
                f32::lerp(x[2], y[2], t),
                f32::lerp(x[3], y[3], t),
            ]),
            (first, _) => first,
        }
    }
}

impl SegmentSample for ParamValue {
    fn segment_sample(a: &Keyframe<Self>, b: &Keyframe<Self>, eased_t: f32) -> Self {
        if let (Self::Vec2(va), Self::Vec2(vb)) = (a.value, b.value) {
            let ka = Keyframe {
                tick: a.tick,
                value: va,
                easing: a.easing,
                tangents: a.tangents,
            };
            let kb = Keyframe {
                tick: b.tick,
                value: vb,
                easing: b.easing,
                tangents: b.tangents,
            };
            return Self::Vec2(<[f32; 2]>::segment_sample(&ka, &kb, eased_t));
        }
        Lerp::lerp(a.value, b.value, eased_t)
    }

    fn segment_delta(a: &Keyframe<Self>, b: &Keyframe<Self>) -> f32 {
        match (a.value, b.value) {
            (Self::Scalar(va), Self::Scalar(vb)) => vb - va,
            (Self::Vec2(va), Self::Vec2(vb)) => {
                let ka = Keyframe {
                    tick: a.tick,
                    value: va,
                    easing: a.easing,
                    tangents: a.tangents,
                };
                let kb = Keyframe {
                    tick: b.tick,
                    value: vb,
                    easing: b.easing,
                    tangents: b.tangents,
                };
                <[f32; 2]>::segment_delta(&ka, &kb)
            }
            (Self::Vec3(va), Self::Vec3(vb)) => {
                let ka = Keyframe {
                    tick: a.tick,
                    value: va,
                    easing: a.easing,
                    tangents: a.tangents,
                };
                let kb = Keyframe {
                    tick: b.tick,
                    value: vb,
                    easing: b.easing,
                    tangents: b.tangents,
                };
                <[f32; 3]>::segment_delta(&ka, &kb)
            }
            (Self::Color(va), Self::Color(vb)) => {
                let ka = Keyframe::new(a.tick, va, a.easing);
                let kb = Keyframe::new(b.tick, vb, b.easing);
                <[u8; 4]>::segment_delta(&ka, &kb)
            }
            (Self::Rect(va), Self::Rect(vb)) => {
                let dx = vb[0] - va[0];
                let dy = vb[1] - va[1];
                let dw = vb[2] - va[2];
                let dh = vb[3] - va[3];
                (dx * dx + dy * dy + dw * dw + dh * dh).sqrt()
            }
            _ => 0.0,
        }
    }

    fn segment_distance(a: &Keyframe<Self>, b: &Keyframe<Self>) -> f32 {
        Self::segment_delta(a, b).abs()
    }
}

/// The animatable spatial placement stored on a clip: each [`ClipTransform`]
/// property as a [`Param`] (M2 keystone). Constant params serialize as bare
/// values, so a never-animated transform is byte-identical to the pre-M2
/// `ClipTransform` JSON and old projects load unchanged.
///
/// Keyframe ticks are clip-relative (offset from the clip's timeline start)
/// at the timeline rate — animation rides along when a clip moves.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnimatedTransform {
    /// Anchor offset from canvas center (see [`ClipTransform::position`]).
    #[serde(default = "default_position_param")]
    pub position: Param<[f32; 2]>,
    /// Pivot within the content bounds (see [`ClipTransform::anchor_point`]).
    #[serde(
        default = "default_anchor_point_param",
        skip_serializing_if = "is_default_anchor_param"
    )]
    pub anchor_point: Param<[f32; 2]>,
    /// Per-axis scale (see [`ClipTransform::scale`]). Uniform constants
    /// serialize as a bare float for pre-M2 / pre-Scale2 save compat.
    #[serde(default = "default_scale_param")]
    pub scale: Param<Scale2>,
    /// Clockwise rotation in degrees (see [`ClipTransform::rotation`]).
    #[serde(default = "default_rotation_param")]
    pub rotation: Param<f32>,
    /// Layer opacity 0..=1 (see [`ClipTransform::opacity`]).
    #[serde(default = "default_opacity_param")]
    pub opacity: Param<f32>,
}

fn default_position_param() -> Param<[f32; 2]> {
    Param::Constant([0.0, 0.0])
}
fn default_anchor_point_param() -> Param<[f32; 2]> {
    Param::Constant([0.5, 0.5])
}
fn is_default_anchor_param(p: &Param<[f32; 2]>) -> bool {
    p.constant() == Some([0.5, 0.5])
}
fn default_scale_param() -> Param<Scale2> {
    Param::Constant(Scale2::ONE)
}
fn default_rotation_param() -> Param<f32> {
    Param::Constant(0.0)
}
fn default_opacity_param() -> Param<f32> {
    Param::Constant(1.0)
}

impl AnimatedTransform {
    /// All-constant identity (centered, aspect-fit, opaque).
    pub fn identity() -> Self {
        Self::from(ClipTransform::IDENTITY)
    }

    /// True iff no property is animated and every constant is the identity.
    pub fn is_identity(&self) -> bool {
        !self.is_animated() && self.sample(0).is_identity()
    }

    /// True iff any property has keyframes.
    pub fn is_animated(&self) -> bool {
        self.position.is_animated()
            || self.anchor_point.is_animated()
            || self.scale.is_animated()
            || self.rotation.is_animated()
            || self.opacity.is_animated()
    }

    /// The transform value at a clip-relative `tick` — the per-frame hot
    /// path (pure, allocation-free).
    pub fn sample(&self, tick: i64) -> ClipTransform {
        self.sample_at(tick as f64)
    }

    /// [`sample`](Self::sample) at a fractional clip-relative tick:
    /// sub-frame animation sampling for export at rates above the timeline
    /// rate (see [`Param::sample_at`]).
    pub fn sample_at(&self, tick: f64) -> ClipTransform {
        ClipTransform {
            position: self.position.sample_at(tick),
            anchor_point: self.anchor_point.sample_at(tick),
            scale: self.scale.sample_at(tick),
            rotation: self.rotation.sample_at(tick),
            opacity: self.opacity.sample_at(tick),
        }
    }

    /// Set every property to a constant, dropping any keyframes.
    pub fn set_constant(&mut self, transform: ClipTransform) {
        self.position.set_constant(transform.position);
        self.anchor_point.set_constant(transform.anchor_point);
        self.scale.set_constant(transform.scale);
        self.rotation.set_constant(transform.rotation);
        self.opacity.set_constant(transform.opacity);
    }

    /// Apply a full-transform edit composing with animation CapCut-style:
    /// animated properties get a keyframe at `tick` (linear easing),
    /// constant properties stay constant. A gesture on a never-animated
    /// clip behaves exactly like the pre-M2 `set_constant`.
    pub fn compose_at(&mut self, transform: ClipTransform, tick: i64) {
        if self.position.is_animated() {
            self.position
                .set_keyframe(tick, transform.position, Easing::Linear);
        } else {
            self.position.set_constant(transform.position);
        }
        if self.anchor_point.is_animated() {
            self.anchor_point
                .set_keyframe(tick, transform.anchor_point, Easing::Linear);
        } else {
            self.anchor_point.set_constant(transform.anchor_point);
        }
        if self.scale.is_animated() {
            self.scale
                .set_keyframe(tick, transform.scale, Easing::Linear);
        } else {
            self.scale.set_constant(transform.scale);
        }
        if self.rotation.is_animated() {
            self.rotation
                .set_keyframe(tick, transform.rotation, Easing::Linear);
        } else {
            self.rotation.set_constant(transform.rotation);
        }
        if self.opacity.is_animated() {
            self.opacity
                .set_keyframe(tick, transform.opacity, Easing::Linear);
        } else {
            self.opacity.set_constant(transform.opacity);
        }
    }

    /// `Ok` iff every stored value (constants and keyframes) passes the
    /// per-property rules [`ClipTransform::validate`] enforces, and every
    /// keyframed param is structurally sound (sorted, non-empty, valid
    /// easings). Used on load and by model mutators.
    pub fn validate(&self) -> Result<(), ModelError> {
        self.position.validate_shape()?;
        self.anchor_point.validate_shape()?;
        self.scale.validate_shape()?;
        self.rotation.validate_shape()?;
        self.opacity.validate_shape()?;
        self.position.for_each_value(validate_position)?;
        self.anchor_point.for_each_value(validate_anchor_point)?;
        self.scale.for_each_value(|v| validate_scale(*v))?;
        self.rotation.for_each_value(|v| validate_rotation(*v))?;
        self.opacity.for_each_value(|v| validate_opacity(*v))?;
        Ok(())
    }
}

/// Accept scalar (uniform) or vec2 (per-axis) for [`ClipParam::Scale`].
pub(crate) fn scale_value(value: ParamValue) -> Result<Scale2, ModelError> {
    match value {
        ParamValue::Scalar(v) => Ok(Scale2::uniform(v)),
        ParamValue::Vec2(v) => Ok(Scale2::from(v)),
        ParamValue::Vec3(_) | ParamValue::Color(_) | ParamValue::Rect(_) => Err(
            ModelError::InvalidParam("expected a scalar or vec2 scale value".into()),
        ),
    }
}

/// Unwrap a rect [`ParamValue`] as a validated [`CropRect`].
pub(crate) fn crop_rect(value: ParamValue) -> Result<CropRect, ModelError> {
    let [x, y, w, h] = value.rect()?;
    let crop = CropRect { x, y, w, h };
    crop.validate()?;
    Ok(crop)
}

pub(crate) fn validate_position(v: &[f32; 2]) -> Result<(), ModelError> {
    if v.iter().all(|c| c.is_finite()) {
        Ok(())
    } else {
        Err(ModelError::InvalidTransform("non-finite component".into()))
    }
}

pub(crate) fn validate_anchor_point(v: &[f32; 2]) -> Result<(), ModelError> {
    if v.iter().all(|c| c.is_finite()) {
        Ok(())
    } else {
        Err(ModelError::InvalidTransform("non-finite anchor".into()))
    }
}

pub(crate) fn validate_scale(v: Scale2) -> Result<(), ModelError> {
    if !v.x.is_finite() || !v.y.is_finite() {
        return Err(ModelError::InvalidTransform("non-finite component".into()));
    }
    if v.x <= 0.0 || v.y <= 0.0 {
        return Err(ModelError::InvalidTransform(
            "scale must be positive".into(),
        ));
    }
    Ok(())
}

pub(crate) fn validate_rotation(v: f32) -> Result<(), ModelError> {
    if v.is_finite() {
        Ok(())
    } else {
        Err(ModelError::InvalidTransform("non-finite component".into()))
    }
}

pub(crate) fn validate_opacity(v: f32) -> Result<(), ModelError> {
    if !v.is_finite() {
        return Err(ModelError::InvalidTransform("non-finite component".into()));
    }
    if !(0.0..=1.0).contains(&v) {
        return Err(ModelError::InvalidTransform(
            "opacity must be in 0..=1".into(),
        ));
    }
    Ok(())
}

impl Default for AnimatedTransform {
    fn default() -> Self {
        Self::identity()
    }
}

impl From<ClipTransform> for AnimatedTransform {
    fn from(t: ClipTransform) -> Self {
        Self {
            position: Param::Constant(t.position),
            anchor_point: Param::Constant(t.anchor_point),
            scale: Param::Constant(t.scale),
            rotation: Param::Constant(t.rotation),
            opacity: Param::Constant(t.opacity),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_transform_validate_rejects_non_positive_and_invalid() {
        assert!(ClipTransform::IDENTITY.validate().is_ok());

        let mut t = ClipTransform::IDENTITY;
        t.scale = Scale2 { x: 0.0, y: 1.0 };
        assert!(t.validate().is_err());

        t.scale = Scale2 { x: 1.0, y: -0.5 };
        assert!(t.validate().is_err());

        t.scale = Scale2 {
            x: f32::NAN,
            y: 1.0,
        };
        assert!(t.validate().is_err());

        t.scale = Scale2::ONE;
        t.opacity = 1.5;
        assert!(t.validate().is_err());

        t.opacity = -0.1;
        assert!(t.validate().is_err());
    }
}
