//! Effects as data (v1 roadmap M4): a clip carries a list of
//! [`EffectInstance`]s, each `{effect_id, params}`. The model never holds
//! shader code — the compositor owns the WGSL and maps ids to GPU passes.
//!
//! The [`effect_catalog`] here is the validation + UI source of truth
//! (display names, parameter defaults / ranges). It is drift-checked against
//! the compositor's renderable descriptors from `aria-engine`, so the two
//! crates can never disagree on which ids and parameter names exist.

use std::ops::Deref;
use std::sync::{Arc, LazyLock};

use serde::{Deserialize, Serialize};

use crate::Map;
use crate::clip::ParamValue;
use crate::error::ModelError;
use crate::param::{Easing, Param};

/// Value kind for an [`EffectParamSpec`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectParamKind {
    /// Single `f32` scalar in `[min, max]`.
    #[default]
    #[serde(alias = "float")]
    Scalar,
    /// Signed integer in `[int_min, int_max]`.
    Int,
    /// Boolean toggle (`false` / `true`).
    Bool,
    /// Enumerated choice from [`EffectParamSpec::choices`].
    Choice,
    /// RGBA color (`[u8; 4]` or linear `[f32; 4]`).
    Color,
    /// Two-component `[f32; 2]` vector in `[min, max]`.
    #[serde(alias = "point2d", alias = "point_2d")]
    Vec2,
    /// Three-component `[f32; 3]` point in `[min, max]`.
    #[serde(alias = "point3d", alias = "point_3d", alias = "vec3")]
    Point3,
    /// Two-component `[f32; 2]` width/height size in `[min, max]`.
    Size,
    /// Angle in degrees (`f32`) in `[min, max]`.
    Angle,
    /// 1D piecewise-linear response curve sampled as a 64-entry LUT on GPU.
    Curve,
    /// Color gradient stop list sampled as a 64-entry RGBA LUT on GPU.
    Gradient,
    /// Arbitrary UTF-8 text string (CPU / generator / clip-type parameter).
    Text,
    /// Font family identifier (CPU / generator / clip-type parameter).
    Font,
    /// File path reference (CPU / generator / clip-type parameter).
    File,
    /// Timeline layer / clip reference (`ClipId`).
    #[serde(alias = "layer_ref")]
    Layer,
}

/// Number of samples in a GPU curve lookup table.
pub const EFFECT_CURVE_LUT_SAMPLES: usize = 64;

/// Number of samples in a GPU gradient lookup table.
pub const EFFECT_GRADIENT_LUT_SAMPLES: usize = 64;

/// Longest name an [`EffectInstance`] takes, in characters.
pub const EFFECT_NAME_MAX_CHARS: usize = 64;

/// Refuse an instance name that is blank, starts or ends with whitespace, is
/// longer than [`EFFECT_NAME_MAX_CHARS`], or holds a control character.
fn check_effect_name(name: &str) -> Result<(), ModelError> {
    let fault = if name.trim().is_empty() {
        "is blank"
    } else if name.trim() != name {
        "starts or ends with whitespace"
    } else if name.chars().count() > EFFECT_NAME_MAX_CHARS {
        "is longer than the limit"
    } else if name.chars().any(char::is_control) {
        "holds a control character"
    } else {
        return Ok(());
    };
    Err(ModelError::InvalidParam(format!(
        "the effect name {name:?} {fault}; a name is 1 to {EFFECT_NAME_MAX_CHARS} characters of \
         printable text"
    )))
}

impl EffectParamKind {
    /// Stable snake_case identifier used in manifests, WIT, and C ABI JSON.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Scalar => "scalar",
            Self::Int => "int",
            Self::Bool => "bool",
            Self::Choice => "choice",
            Self::Color => "color",
            Self::Vec2 => "vec2",
            Self::Point3 => "point3",
            Self::Size => "size",
            Self::Angle => "angle",
            Self::Curve => "curve",
            Self::Gradient => "gradient",
            Self::Text => "text",
            Self::Font => "font",
            Self::File => "file",
            Self::Layer => "layer",
        }
    }

    /// Parse a kind from its manifest/WIT/ABI identifier.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "scalar" | "float" => Some(Self::Scalar),
            "int" | "integer" => Some(Self::Int),
            "bool" | "boolean" => Some(Self::Bool),
            "choice" | "enum" => Some(Self::Choice),
            "color" | "colour" => Some(Self::Color),
            "vec2" | "point2d" | "point_2d" => Some(Self::Vec2),
            "point3" | "vec3" | "point3d" | "point_3d" => Some(Self::Point3),
            "size" => Some(Self::Size),
            "angle" => Some(Self::Angle),
            "curve" => Some(Self::Curve),
            "gradient" => Some(Self::Gradient),
            "text" | "string" => Some(Self::Text),
            "font" => Some(Self::Font),
            "file" | "path" => Some(Self::File),
            "layer" | "layer_ref" => Some(Self::Layer),
            _ => None,
        }
    }

    /// Whether this parameter kind supports keyframing and property drivers.
    pub const fn is_keyframeable(self) -> bool {
        matches!(
            self,
            Self::Scalar
                | Self::Int
                | Self::Color
                | Self::Vec2
                | Self::Point3
                | Self::Size
                | Self::Angle
        )
    }

    /// Whether this parameter kind validates `min <= max` on `f32` bounds.
    pub const fn has_float_range(self) -> bool {
        matches!(
            self,
            Self::Scalar | Self::Vec2 | Self::Point3 | Self::Size | Self::Angle
        )
    }

    /// Number of `f32` slots this parameter occupies in the shader storage buffer.
    pub const fn shader_floats(self) -> usize {
        match self {
            Self::Scalar | Self::Int | Self::Bool | Self::Choice | Self::Angle | Self::Layer => 1,
            Self::Vec2 | Self::Size => 2,
            Self::Point3 => 3,
            Self::Color => 4,
            Self::Curve => EFFECT_CURVE_LUT_SAMPLES,
            Self::Gradient => EFFECT_GRADIENT_LUT_SAMPLES * 4,
            Self::Text | Self::Font | Self::File => 0,
        }
    }
}

/// UI control display hint for a numeric parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectParamDisplay {
    /// Standard horizontal scrub/slider.
    #[default]
    Slider,
    /// Rotary angle dial (`deg`).
    #[serde(alias = "angle", alias = "dial")]
    AngleDial,
    /// Percentage readout (`0..1` or `0..100` formatted as `%`).
    #[serde(alias = "percent")]
    Percentage,
}

/// One option in a [`EffectParamKind::Choice`] parameter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectChoiceOption {
    /// Stable option identifier stored on the instance.
    pub id: String,
    /// Human-readable option label shown in the dropdown.
    pub label: String,
}

impl EffectChoiceOption {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
        }
    }
}

/// One control point `(x, y)` on a 1D [`EffectParamKind::Curve`] parameter.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EffectCurvePoint {
    pub x: f32,
    pub y: f32,
}

impl EffectCurvePoint {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// Sample a piecewise-linear 1D curve at `t` in `[0.0, 1.0]`.
pub fn sample_effect_curve(points: &[EffectCurvePoint], t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if points.is_empty() {
        return t;
    }
    if points.len() == 1 {
        return points[0].y;
    }
    if t <= points[0].x {
        return points[0].y;
    }
    for pair in points.windows(2) {
        let a = pair[0];
        let b = pair[1];
        if t <= b.x {
            let span = b.x - a.x;
            if span.abs() <= 1e-6 {
                return b.y;
            }
            let u = ((t - a.x) / span).clamp(0.0, 1.0);
            return a.y + (b.y - a.y) * u;
        }
    }
    points[points.len() - 1].y
}

/// Bake a 1D curve into [`EFFECT_CURVE_LUT_SAMPLES`] evenly-spaced samples over `[0, 1]`.
pub fn evaluate_effect_curve_lut(points: &[EffectCurvePoint]) -> [f32; EFFECT_CURVE_LUT_SAMPLES] {
    let mut lut = [0.0_f32; EFFECT_CURVE_LUT_SAMPLES];
    let denom = (EFFECT_CURVE_LUT_SAMPLES - 1) as f32;
    for (i, slot) in lut.iter_mut().enumerate() {
        *slot = sample_effect_curve(points, i as f32 / denom);
    }
    lut
}

/// One color stop on a [`EffectParamKind::Gradient`] parameter.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EffectGradientStop {
    /// Normalized position in `[0.0, 1.0]`.
    #[serde(alias = "position")]
    pub offset: f32,
    /// Linear RGBA color at this stop.
    pub color: [f32; 4],
}

impl EffectGradientStop {
    pub const fn new(offset: f32, color: [f32; 4]) -> Self {
        Self { offset, color }
    }
}

/// Sample a piecewise-linear RGBA gradient at `t` in `[0.0, 1.0]`.
pub fn sample_effect_gradient(stops: &[EffectGradientStop], t: f32) -> [f32; 4] {
    let t = t.clamp(0.0, 1.0);
    if stops.is_empty() {
        return [t, t, t, 1.0];
    }
    if stops.len() == 1 {
        return stops[0].color;
    }
    if t <= stops[0].offset {
        return stops[0].color;
    }
    for pair in stops.windows(2) {
        let a = pair[0];
        let b = pair[1];
        if t <= b.offset {
            let span = b.offset - a.offset;
            if span.abs() <= 1e-6 {
                return b.color;
            }
            let u = ((t - a.offset) / span).clamp(0.0, 1.0);
            return [
                a.color[0] + (b.color[0] - a.color[0]) * u,
                a.color[1] + (b.color[1] - a.color[1]) * u,
                a.color[2] + (b.color[2] - a.color[2]) * u,
                a.color[3] + (b.color[3] - a.color[3]) * u,
            ];
        }
    }
    stops[stops.len() - 1].color
}

/// Bake a gradient into [`EFFECT_GRADIENT_LUT_SAMPLES`] linear RGBA samples over `[0, 1]`.
pub fn evaluate_effect_gradient_lut(
    stops: &[EffectGradientStop],
) -> [[f32; 4]; EFFECT_GRADIENT_LUT_SAMPLES] {
    let mut lut = [[0.0_f32; 4]; EFFECT_GRADIENT_LUT_SAMPLES];
    let denom = (EFFECT_GRADIENT_LUT_SAMPLES - 1) as f32;
    for (i, slot) in lut.iter_mut().enumerate() {
        *slot = sample_effect_gradient(stops, i as f32 / denom);
    }
    lut
}

/// Visibility rule hiding a parameter unless another parameter equals a target value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EffectVisibilityCondition {
    /// Name of the controlling parameter on the same effect or transition.
    pub param: String,
    /// Stringified value the controlling parameter must match.
    pub equals: String,
}

impl<'de> Deserialize<'de> for EffectVisibilityCondition {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Raw {
            param: String,
            #[serde(default)]
            equals: Option<String>,
            #[serde(default)]
            equals_bool: Option<bool>,
            #[serde(default)]
            equals_int: Option<i64>,
            #[serde(default)]
            equals_choice: Option<String>,
        }
        let raw = Raw::deserialize(deserializer)?;
        let equals = raw
            .equals
            .or(raw.equals_choice)
            .or_else(|| raw.equals_bool.map(|b| b.to_string()))
            .or_else(|| raw.equals_int.map(|i| i.to_string()))
            .unwrap_or_default();
        Ok(Self {
            param: raw.param,
            equals,
        })
    }
}

impl EffectVisibilityCondition {
    pub fn new(param: impl Into<String>, equals: impl Into<String>) -> Self {
        Self {
            param: param.into(),
            equals: equals.into(),
        }
    }
}

/// Parameter group section declared on an [`EffectSpec`].
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct EffectGroupSpec {
    /// Stable group identifier referenced by [`EffectParamSpec::group`].
    pub id: String,
    /// Human-readable section heading shown in the inspector.
    pub label: String,
    /// Whether the group section starts collapsed in the inspector.
    #[serde(default)]
    pub collapsed: bool,
}

impl EffectGroupSpec {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            collapsed: false,
        }
    }

    pub fn with_collapsed(mut self, collapsed: bool) -> Self {
        self.collapsed = collapsed;
        self
    }
}

/// Specification of one parameter on an [`EffectSpec`] or [`crate::TransitionSpec`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EffectParamSpec {
    /// Machine-readable parameter key (e.g. `"radius"`).
    pub name: String,
    /// Human-readable label for UI.
    pub label: String,
    /// Value kind (defaults to [`EffectParamKind::Scalar`]).
    #[serde(default)]
    pub kind: EffectParamKind,
    /// Default value for [`EffectParamKind::Scalar`] / [`EffectParamKind::Angle`].
    #[serde(default)]
    pub default: f32,
    /// Minimum allowed value (`Scalar`, `Vec2`, `Point3`, `Size`, `Angle`).
    #[serde(default)]
    pub min: f32,
    /// Maximum allowed value (`Scalar`, `Vec2`, `Point3`, `Size`, `Angle`).
    #[serde(default)]
    pub max: f32,
    /// Optional soft minimum for UI scrubbing before clamping to `min`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub soft_min: Option<f32>,
    /// Optional soft maximum for UI scrubbing before clamping to `max`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub soft_max: Option<f32>,
    /// Optional unit suffix (e.g. `"px"`, `"deg"`, `"%"`, `"s"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    /// Optional control display mode (`slider`, `angle_dial`, `percentage`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display: Option<EffectParamDisplay>,
    /// Default value for [`EffectParamKind::Int`].
    #[serde(default)]
    pub default_int: i64,
    /// Minimum allowed value for [`EffectParamKind::Int`].
    #[serde(default)]
    pub int_min: i64,
    /// Maximum allowed value for [`EffectParamKind::Int`].
    #[serde(default)]
    pub int_max: i64,
    /// Default value for [`EffectParamKind::Bool`].
    #[serde(default)]
    pub default_bool: bool,
    /// Options for [`EffectParamKind::Choice`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<EffectChoiceOption>,
    /// Default selected option id for [`EffectParamKind::Choice`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_choice: Option<String>,
    /// Default value for [`EffectParamKind::Color`] (`[R, G, B, A]` in `0..=255`).
    #[serde(default)]
    pub default_color: [u8; 4],
    /// Optional linear RGBA default for [`EffectParamKind::Color`] (`[f32; 4]`).
    #[serde(
        default,
        alias = "linear_color",
        skip_serializing_if = "Option::is_none"
    )]
    pub default_linear_color: Option<[f32; 4]>,
    /// Default value for [`EffectParamKind::Vec2`] / [`EffectParamKind::Size`].
    #[serde(default)]
    pub default_vec2: [f32; 2],
    /// Default value for [`EffectParamKind::Point3`].
    #[serde(default)]
    pub default_vec3: [f32; 3],
    /// Default control points for [`EffectParamKind::Curve`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub default_curve: Vec<EffectCurvePoint>,
    /// Default color stops for [`EffectParamKind::Gradient`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub default_gradient: Vec<EffectGradientStop>,
    /// Default string for [`EffectParamKind::Text`], [`EffectParamKind::Font`], or [`EffectParamKind::File`].
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub default_text: String,
    /// Optional inspector group id or heading.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// Optional condition controlling when this parameter row is shown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible_when: Option<EffectVisibilityCondition>,
    /// Optional tooltip description shown in the inspector.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tooltip: Option<String>,
}

impl Default for EffectParamSpec {
    fn default() -> Self {
        Self {
            name: String::new(),
            label: String::new(),
            kind: EffectParamKind::Scalar,
            default: 0.0,
            min: 0.0,
            max: 1.0,
            soft_min: None,
            soft_max: None,
            unit: None,
            display: None,
            default_int: 0,
            int_min: 0,
            int_max: 0,
            default_bool: false,
            choices: Vec::new(),
            default_choice: None,
            default_color: [0, 0, 0, 0],
            default_linear_color: None,
            default_vec2: [0.0, 0.0],
            default_vec3: [0.0, 0.0, 0.0],
            default_curve: Vec::new(),
            default_gradient: Vec::new(),
            default_text: String::new(),
            group: None,
            visible_when: None,
            tooltip: None,
        }
    }
}

impl EffectParamSpec {
    /// Build a scalar (`f32`) parameter spec.
    pub fn scalar(
        name: impl Into<String>,
        label: impl Into<String>,
        default: f32,
        min: f32,
        max: f32,
    ) -> Self {
        Self {
            name: name.into(),
            label: label.into(),
            kind: EffectParamKind::Scalar,
            default,
            min,
            max,
            ..Self::default()
        }
    }

    /// Build an integer (`i64`) parameter spec.
    pub fn int(
        name: impl Into<String>,
        label: impl Into<String>,
        default_int: i64,
        int_min: i64,
        int_max: i64,
    ) -> Self {
        Self {
            name: name.into(),
            label: label.into(),
            kind: EffectParamKind::Int,
            default: default_int as f32,
            min: int_min as f32,
            max: int_max as f32,
            default_int,
            int_min,
            int_max,
            ..Self::default()
        }
    }

    /// Build a boolean parameter spec.
    pub fn boolean(name: impl Into<String>, label: impl Into<String>, default_bool: bool) -> Self {
        Self {
            name: name.into(),
            label: label.into(),
            kind: EffectParamKind::Bool,
            default: if default_bool { 1.0 } else { 0.0 },
            default_bool,
            ..Self::default()
        }
    }

    /// Build a choice (dropdown) parameter spec.
    pub fn choice(
        name: impl Into<String>,
        label: impl Into<String>,
        choices: Vec<EffectChoiceOption>,
        default_choice: impl Into<String>,
    ) -> Self {
        let default_choice = default_choice.into();
        let idx = choices
            .iter()
            .position(|c| c.id == default_choice)
            .unwrap_or(0);
        let max = choices.len().saturating_sub(1) as f32;
        Self {
            name: name.into(),
            label: label.into(),
            kind: EffectParamKind::Choice,
            default: idx as f32,
            min: 0.0,
            max,
            choices,
            default_choice: Some(default_choice),
            ..Self::default()
        }
    }

    /// Build an 8-bit RGBA color parameter spec.
    pub fn color(
        name: impl Into<String>,
        label: impl Into<String>,
        default_color: [u8; 4],
    ) -> Self {
        Self {
            name: name.into(),
            label: label.into(),
            kind: EffectParamKind::Color,
            default_color,
            default_linear_color: Some(default_color.map(|c| f32::from(c) / 255.0)),
            ..Self::default()
        }
    }

    /// Build a linear RGBA (`[f32; 4]`) color parameter spec.
    pub fn linear_color(name: impl Into<String>, label: impl Into<String>, rgba: [f32; 4]) -> Self {
        Self {
            name: name.into(),
            label: label.into(),
            kind: EffectParamKind::Color,
            default_color: rgba.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8),
            default_linear_color: Some(rgba),
            ..Self::default()
        }
    }

    /// Build a 2D point (`[f32; 2]`) parameter spec.
    pub fn vec2(
        name: impl Into<String>,
        label: impl Into<String>,
        default_vec2: [f32; 2],
        min: f32,
        max: f32,
    ) -> Self {
        Self {
            name: name.into(),
            label: label.into(),
            kind: EffectParamKind::Vec2,
            min,
            max,
            default_vec2,
            ..Self::default()
        }
    }

    /// Build a 3D point (`[f32; 3]`) parameter spec.
    pub fn point3(
        name: impl Into<String>,
        label: impl Into<String>,
        default_vec3: [f32; 3],
        min: f32,
        max: f32,
    ) -> Self {
        Self {
            name: name.into(),
            label: label.into(),
            kind: EffectParamKind::Point3,
            min,
            max,
            default_vec3,
            ..Self::default()
        }
    }

    /// Build a 2D size (`[f32; 2]`) parameter spec.
    pub fn size(
        name: impl Into<String>,
        label: impl Into<String>,
        default_vec2: [f32; 2],
        min: f32,
        max: f32,
    ) -> Self {
        Self {
            name: name.into(),
            label: label.into(),
            kind: EffectParamKind::Size,
            min,
            max,
            default_vec2,
            unit: Some("px".to_string()),
            ..Self::default()
        }
    }

    /// Build an angle (`f32` degrees) parameter spec.
    pub fn angle(
        name: impl Into<String>,
        label: impl Into<String>,
        default: f32,
        min: f32,
        max: f32,
    ) -> Self {
        Self {
            name: name.into(),
            label: label.into(),
            kind: EffectParamKind::Angle,
            default,
            min,
            max,
            unit: Some("deg".to_string()),
            display: Some(EffectParamDisplay::AngleDial),
            ..Self::default()
        }
    }

    /// Build a 1D curve parameter spec.
    pub fn curve(
        name: impl Into<String>,
        label: impl Into<String>,
        default_curve: Vec<EffectCurvePoint>,
    ) -> Self {
        Self {
            name: name.into(),
            label: label.into(),
            kind: EffectParamKind::Curve,
            default_curve,
            ..Self::default()
        }
    }

    /// Build a color gradient parameter spec.
    pub fn gradient(
        name: impl Into<String>,
        label: impl Into<String>,
        default_gradient: Vec<EffectGradientStop>,
    ) -> Self {
        Self {
            name: name.into(),
            label: label.into(),
            kind: EffectParamKind::Gradient,
            default_gradient,
            ..Self::default()
        }
    }

    /// Build a UTF-8 text parameter spec.
    pub fn text(
        name: impl Into<String>,
        label: impl Into<String>,
        default_text: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            label: label.into(),
            kind: EffectParamKind::Text,
            default_text: default_text.into(),
            ..Self::default()
        }
    }

    /// Build a font family parameter spec.
    pub fn font(
        name: impl Into<String>,
        label: impl Into<String>,
        default_font: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            label: label.into(),
            kind: EffectParamKind::Font,
            default_text: default_font.into(),
            ..Self::default()
        }
    }

    /// Build a file path parameter spec.
    pub fn file(
        name: impl Into<String>,
        label: impl Into<String>,
        default_path: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            label: label.into(),
            kind: EffectParamKind::File,
            default_text: default_path.into(),
            ..Self::default()
        }
    }

    /// Build a layer reference parameter spec.
    pub fn layer(name: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            label: label.into(),
            kind: EffectParamKind::Layer,
            ..Self::default()
        }
    }

    /// Set soft UI slider bounds.
    pub fn with_soft_range(mut self, soft_min: f32, soft_max: f32) -> Self {
        self.soft_min = Some(soft_min);
        self.soft_max = Some(soft_max);
        self
    }

    /// Set the unit suffix (e.g. `"px"`, `"deg"`, `"%"`).
    pub fn with_unit(mut self, unit: impl Into<String>) -> Self {
        self.unit = Some(unit.into());
        self
    }

    /// Set the UI control display hint.
    pub fn with_display(mut self, display: EffectParamDisplay) -> Self {
        self.display = Some(display);
        self
    }

    /// Assign this parameter to an inspector group.
    pub fn with_group(mut self, group: impl Into<String>) -> Self {
        self.group = Some(group.into());
        self
    }

    /// Conditionally show this parameter when another parameter matches `equals`.
    pub fn with_visible_when(
        mut self,
        param: impl Into<String>,
        equals: impl Into<String>,
    ) -> Self {
        self.visible_when = Some(EffectVisibilityCondition::new(param, equals));
        self
    }

    /// Attach a tooltip description.
    pub fn with_tooltip(mut self, tooltip: impl Into<String>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    /// Default color as linear RGBA `[f32; 4]`.
    pub fn default_color_linear(&self) -> [f32; 4] {
        self.default_linear_color
            .unwrap_or_else(|| self.default_color.map(|c| f32::from(c) / 255.0))
    }

    /// Default color as 8-bit RGBA `[u8; 4]`.
    pub fn default_color_u8(&self) -> [u8; 4] {
        if let Some(linear) = self.default_linear_color
            && self.default_color == [0, 0, 0, 0]
        {
            linear.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8)
        } else {
            self.default_color
        }
    }

    /// The default as the value a keyframe of this slot holds, or `None` for a
    /// kind no curve can carry. An integer is held as a scalar, since its curve is
    /// a scalar curve rounded on read.
    pub fn default_param_value(&self) -> Option<ParamValue> {
        Some(match self.kind {
            EffectParamKind::Scalar | EffectParamKind::Angle => ParamValue::Scalar(self.default),
            EffectParamKind::Int => ParamValue::Scalar(self.default_int as f32),
            EffectParamKind::Vec2 | EffectParamKind::Size => ParamValue::Vec2(self.default_vec2),
            EffectParamKind::Point3 => ParamValue::Vec3(self.default_vec3),
            EffectParamKind::Color => ParamValue::Color(self.default_color_u8()),
            EffectParamKind::Bool
            | EffectParamKind::Choice
            | EffectParamKind::Curve
            | EffectParamKind::Gradient
            | EffectParamKind::Text
            | EffectParamKind::Font
            | EffectParamKind::File
            | EffectParamKind::Layer => return None,
        })
    }

    /// Default choice id for [`EffectParamKind::Choice`].
    pub fn default_choice_id(&self) -> Option<&str> {
        self.default_choice
            .as_deref()
            .or_else(|| self.choices.first().map(|c| c.id.as_str()))
    }

    /// Choice index for `choice_id`, or `None` if not in [`Self::choices`].
    pub fn choice_index(&self, choice_id: &str) -> Option<usize> {
        self.choices.iter().position(|c| c.id == choice_id)
    }

    /// The option a stored choice selects: `stored` when it names one of
    /// [`Self::choices`], else the default. A stored id the catalog no longer
    /// lists reads as the default everywhere: in the inspector, in the
    /// sampled index, and in the shader buffer.
    pub fn choice_or_default<'a>(&'a self, stored: Option<&'a str>) -> Option<&'a str> {
        stored
            .filter(|id| self.choice_index(id).is_some())
            .or_else(|| self.default_choice_id())
    }

    /// Default curve control points (identity `(0, 0) -> (1, 1)` when empty).
    pub fn default_curve_points(&self) -> Vec<EffectCurvePoint> {
        if self.default_curve.is_empty() {
            vec![
                EffectCurvePoint::new(0.0, 0.0),
                EffectCurvePoint::new(1.0, 1.0),
            ]
        } else {
            self.default_curve.clone()
        }
    }

    /// Default gradient stops (black-to-white when empty).
    pub fn default_gradient_stops(&self) -> Vec<EffectGradientStop> {
        if self.default_gradient.is_empty() {
            vec![
                EffectGradientStop::new(0.0, [0.0, 0.0, 0.0, 1.0]),
                EffectGradientStop::new(1.0, [1.0, 1.0, 1.0, 1.0]),
            ]
        } else {
            self.default_gradient.clone()
        }
    }
}

/// Append the default shader storage-buffer floats for `params`.
pub fn default_flattened_param_floats(params: &[EffectParamSpec]) -> Vec<f32> {
    let total: usize = params.iter().map(|p| p.kind.shader_floats()).sum();
    let mut out = Vec::with_capacity(total);
    for p in params {
        match p.kind {
            EffectParamKind::Scalar | EffectParamKind::Angle => out.push(p.default),
            EffectParamKind::Int => out.push(p.default_int as f32),
            EffectParamKind::Bool => out.push(if p.default_bool { 1.0 } else { 0.0 }),
            EffectParamKind::Choice => {
                let idx = p
                    .default_choice_id()
                    .and_then(|id| p.choice_index(id))
                    .unwrap_or(0);
                out.push(idx as f32);
            }
            EffectParamKind::Color => out.extend_from_slice(&p.default_color_linear()),
            EffectParamKind::Vec2 | EffectParamKind::Size => out.extend_from_slice(&p.default_vec2),
            EffectParamKind::Point3 => out.extend_from_slice(&p.default_vec3),
            EffectParamKind::Curve => {
                let lut = evaluate_effect_curve_lut(&p.default_curve);
                out.extend_from_slice(&lut);
            }
            EffectParamKind::Gradient => {
                let lut = evaluate_effect_gradient_lut(&p.default_gradient);
                for rgba in lut {
                    out.extend_from_slice(&rgba);
                }
            }
            EffectParamKind::Layer => out.push(0.0),
            EffectParamKind::Text | EffectParamKind::Font | EffectParamKind::File => {}
        }
    }
    out
}

/// Category label used when an [`EffectSpec`] leaves [`EffectSpec::category`] empty.
pub const OTHER_VIDEO_CATEGORY: &str = "Other";

/// Pixel margin adjustments an effect applies to its layer's bounding box.
///
/// Positive values expand the layer bounds outward (e.g. blur or drop shadow
/// overhang); negative values shrink them inward.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct EffectBounds {
    #[serde(default)]
    pub left: f32,
    #[serde(default)]
    pub top: f32,
    #[serde(default)]
    pub right: f32,
    #[serde(default)]
    pub bottom: f32,
}

impl EffectBounds {
    /// Zero margin expansion on all four sides.
    pub const ZERO: Self = Self {
        left: 0.0,
        top: 0.0,
        right: 0.0,
        bottom: 0.0,
    };

    /// Uniform margin expansion on all four sides.
    pub const fn uniform(margin: f32) -> Self {
        Self {
            left: margin,
            top: margin,
            right: margin,
            bottom: margin,
        }
    }

    /// Explicit `(left, top, right, bottom)` margin expansion.
    pub const fn new(left: f32, top: f32, right: f32, bottom: f32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    /// `true` when all four margins are zero.
    pub fn is_zero(&self) -> bool {
        self.left == 0.0 && self.top == 0.0 && self.right == 0.0 && self.bottom == 0.0
    }

    /// Combine two effect bound adjustments in sequence.
    pub fn combine(self, other: Self) -> Self {
        Self {
            left: self.left + other.left,
            top: self.top + other.top,
            right: self.right + other.right,
            bottom: self.bottom + other.bottom,
        }
    }

    /// Apply these margins to a `(center, size)` quad in canvas pixels.
    pub fn apply_to_quad(self, center: [f32; 2], size: [f32; 2]) -> ([f32; 2], [f32; 2]) {
        if self.is_zero() {
            return (center, size);
        }
        let new_w = (size[0] + self.left + self.right).max(0.0);
        let new_h = (size[1] + self.top + self.bottom).max(0.0);
        let new_cx = center[0] + (self.right - self.left) * 0.5;
        let new_cy = center[1] + (self.bottom - self.top) * 0.5;
        ([new_cx, new_cy], [new_w, new_h])
    }
}

/// Specification of a video effect or generator in the catalog.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct EffectSpec {
    /// Stable identifier (e.g. `"gaussian_blur"`).
    pub id: String,
    /// Display name (e.g. `"Gaussian Blur"`).
    pub label: String,
    /// Browser category (e.g. `"Blur"`). Empty falls under [`OTHER_VIDEO_CATEGORY`].
    #[serde(default)]
    pub category: String,
    /// Parameter definitions in display order.
    #[serde(default)]
    pub params: Vec<EffectParamSpec>,
    /// Optional inspector parameter groups in display order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<EffectGroupSpec>,
    /// `true` when this entry synthesizes a frame from `uv`, time, and
    /// parameters alone without reading an input texture.
    #[serde(default)]
    pub generator: bool,
    /// `true` when this effect's output at tick `t` depends on frames before
    /// `t` (temporal feedback, echo, motion trails).
    #[serde(default)]
    pub stateful: bool,
    /// Bounding-box expansion/shrink margins in pixels.
    #[serde(default)]
    pub bounds: EffectBounds,
    /// `true` when the effect samples outside `0.0..=1.0` source UVs.
    #[serde(default, alias = "reads-outside")]
    pub reads_outside: bool,
    /// `true` when the effect output depends on playback time even when
    /// parameters are static.
    #[serde(default, alias = "time-dependent")]
    pub time_dependent: bool,
}

impl EffectSpec {
    /// Browser category label, defaulting to [`OTHER_VIDEO_CATEGORY`] when empty.
    pub fn category(&self) -> &str {
        if self.category.is_empty() {
            OTHER_VIDEO_CATEGORY
        } else {
            &self.category
        }
    }

    /// Look up a parameter specification by name.
    pub fn param(&self, name: &str) -> Option<&EffectParamSpec> {
        self.params.iter().find(|p| p.name == name)
    }

    /// Look up a parameter specification by positional slot index.
    pub fn param_at(&self, index: usize) -> Option<&EffectParamSpec> {
        self.params.get(index)
    }

    /// Look up a group specification by id.
    pub fn group(&self, id: &str) -> Option<&EffectGroupSpec> {
        self.groups.iter().find(|g| g.id == id)
    }

    /// Set bounding-box expansion/shrink margins.
    pub fn with_bounds(mut self, bounds: EffectBounds) -> Self {
        self.bounds = bounds;
        self
    }

    /// Mark whether the effect reads outside the unit UV rectangle.
    pub fn with_reads_outside(mut self, reads_outside: bool) -> Self {
        self.reads_outside = reads_outside;
        self
    }

    /// Mark whether the effect depends on time even with static parameters.
    pub fn with_time_dependent(mut self, time_dependent: bool) -> Self {
        self.time_dependent = time_dependent;
        self
    }
}

/// The shipped effects, read once. Shipped set at `data/effects.toml`.
static SHIPPED: LazyLock<Vec<Arc<EffectSpec>>> = LazyLock::new(|| {
    crate::catalog::load(
        "effects",
        include_str!("../data/effects.toml"),
        |entries: &[EffectSpec]| {
            crate::catalog::unique_ids(entries, |s| &s.id, |s| &s.label)?;
            entries.iter().try_for_each(check_effect_params)
        },
    )
    .into_iter()
    .map(Arc::new)
    .collect()
});

/// The parameter rules every effect follows, shipped or registered: each
/// parameter is named, no name repeats, and no range is inverted.
pub(crate) fn check_effect_params(spec: &EffectSpec) -> Result<(), String> {
    check_params(&spec.id, &spec.params)
}

/// The parameter rules shared by video, transition, and audio effects, for `id`.
pub(crate) fn check_params(id: &str, params: &[EffectParamSpec]) -> Result<(), String> {
    for (index, param) in params.iter().enumerate() {
        if param.name.is_empty() {
            return Err(format!("{id} parameter {index} has no name"));
        }
        if params[..index].iter().any(|p| p.name == param.name) {
            return Err(format!("{id} repeats parameter {}", param.name));
        }
        if param.kind.has_float_range() && param.min > param.max {
            return Err(format!("{id}.{} has an inverted range", param.name));
        }
        if param.kind == EffectParamKind::Int && param.int_min > param.int_max {
            return Err(format!("{id}.{} has an inverted range", param.name));
        }
        if let (Some(smin), Some(smax)) = (param.soft_min, param.soft_max)
            && smin > smax
        {
            return Err(format!("{id}.{} has an inverted soft range", param.name));
        }
        if param.kind == EffectParamKind::Choice && param.choices.is_empty() {
            return Err(format!(
                "{id}.{} choice parameter has no choices",
                param.name
            ));
        }
    }
    Ok(())
}

/// The effects that ship with the app, without registered ones.
pub fn shipped_effect_catalog() -> &'static [Arc<EffectSpec>] {
    &SHIPPED
}

/// Every effect the model knows about now (validation + UI browsing): the
/// shipped set, then effects registered by plugins.
pub fn effect_catalog() -> crate::Catalog<EffectSpec> {
    crate::registered::effect_catalog()
}

/// The catalog entry for `id`, shipped or registered, or `None`.
pub fn effect_spec(id: &str) -> Option<Arc<EffectSpec>> {
    crate::registered::effect_spec(id)
}

/// One parameter of a held spec.
struct ParamRef {
    spec: Arc<EffectSpec>,
    index: usize,
}

impl Deref for ParamRef {
    type Target = EffectParamSpec;

    fn deref(&self) -> &EffectParamSpec {
        &self.spec.params[self.index]
    }
}

/// An effect placed on a clip. Only parameters that differ from their catalog
/// default are stored (others fall back to the default), so a freshly-added
/// effect serializes to just its id and old files that predate a new
/// parameter keep working.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EffectInstance {
    pub effect_id: String,
    /// Explicitly-set scalar / int / angle parameters, keyed by name; constant or keyframed.
    /// Keyframe ticks are clip-relative, like the transform params.
    #[serde(
        default,
        with = "crate::serde_map",
        skip_serializing_if = "Map::is_empty"
    )]
    pub params: Map<String, Param<f32>>,
    /// Explicitly-set 8-bit color parameters (`EffectParamKind::Color`).
    #[serde(
        default,
        with = "crate::serde_map",
        skip_serializing_if = "Map::is_empty"
    )]
    pub color_params: Map<String, Param<[u8; 4]>>,
    /// Explicitly-set linear RGBA (`[f32; 4]`) color parameters (`EffectParamKind::Color`).
    #[serde(
        default,
        with = "crate::serde_map",
        skip_serializing_if = "Map::is_empty"
    )]
    pub linear_color_params: Map<String, Param<[f32; 4]>>,
    /// Explicitly-set 2-d vector / size parameters (`EffectParamKind::Vec2` | `Size`).
    #[serde(
        default,
        with = "crate::serde_map",
        skip_serializing_if = "Map::is_empty"
    )]
    pub vec2_params: Map<String, Param<[f32; 2]>>,
    /// Explicitly-set 3-d point parameters (`EffectParamKind::Point3`).
    #[serde(
        default,
        with = "crate::serde_map",
        skip_serializing_if = "Map::is_empty"
    )]
    pub vec3_params: Map<String, Param<[f32; 3]>>,
    /// Explicitly-set boolean parameters (`EffectParamKind::Bool`).
    #[serde(
        default,
        with = "crate::serde_map",
        skip_serializing_if = "Map::is_empty"
    )]
    pub bool_params: Map<String, bool>,
    /// Explicitly-set choice option ids (`EffectParamKind::Choice`).
    #[serde(
        default,
        with = "crate::serde_map",
        skip_serializing_if = "Map::is_empty"
    )]
    pub choice_params: Map<String, String>,
    /// Explicitly-set 1D curve control points (`EffectParamKind::Curve`).
    #[serde(
        default,
        with = "crate::serde_map",
        skip_serializing_if = "Map::is_empty"
    )]
    pub curve_params: Map<String, Vec<EffectCurvePoint>>,
    /// Explicitly-set gradient stops (`EffectParamKind::Gradient`).
    #[serde(
        default,
        with = "crate::serde_map",
        skip_serializing_if = "Map::is_empty"
    )]
    pub gradient_params: Map<String, Vec<EffectGradientStop>>,
    /// Explicitly-set text / font / file strings (`EffectParamKind::Text` | `Font` | `File`).
    #[serde(
        default,
        with = "crate::serde_map",
        skip_serializing_if = "Map::is_empty"
    )]
    pub text_params: Map<String, String>,
    /// Explicitly-set layer reference raw `ClipId`s (`EffectParamKind::Layer`).
    #[serde(
        default,
        with = "crate::serde_map",
        skip_serializing_if = "Map::is_empty"
    )]
    pub layer_params: Map<String, u64>,
    /// Whether [`Self::effect_id`] was a generator when this instance was placed.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub generator: bool,
    /// Whether the effect is switched off. A bypassed effect keeps its
    /// parameters and its place in the chain and renders nothing.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub bypass: bool,
    /// The name given to this instance, drawn in place of the catalog label.
    /// `None` draws the label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl EffectInstance {
    /// A new instance of `effect_id` with every parameter at its default.
    pub fn new(effect_id: impl Into<String>) -> Self {
        let effect_id = effect_id.into();
        let generator = effect_spec(&effect_id).is_some_and(|s| s.generator);
        Self {
            effect_id,
            params: Map::default(),
            color_params: Map::default(),
            linear_color_params: Map::default(),
            vec2_params: Map::default(),
            vec3_params: Map::default(),
            bool_params: Map::default(),
            choice_params: Map::default(),
            curve_params: Map::default(),
            gradient_params: Map::default(),
            text_params: Map::default(),
            layer_params: Map::default(),
            generator,
            bypass: false,
            name: None,
        }
    }

    /// The name drawn for this instance: its own name, else the catalog
    /// label, else the id when the catalog no longer has the effect.
    pub fn display_name(&self) -> String {
        if let Some(name) = &self.name {
            return name.clone();
        }
        effect_spec(&self.effect_id)
            .map_or_else(|| self.effect_id.clone(), |spec| spec.label.clone())
    }

    /// This instance with every parameter at its catalog default and no
    /// keyframes. The name, the switch and the generator flag are kept.
    pub fn reset(&self) -> Self {
        Self {
            generator: self.generator,
            bypass: self.bypass,
            name: self.name.clone(),
            ..Self::new(self.effect_id.clone())
        }
    }

    /// Refresh [`Self::generator`] from the catalog when the effect is loaded.
    pub fn note_catalog(&mut self) {
        if let Some(spec) = effect_spec(&self.effect_id) {
            self.generator = spec.generator;
        }
    }

    /// Refresh [`Self::generator`] from the catalog.
    pub fn mark_from_catalog(&mut self) {
        self.note_catalog();
    }

    /// Whether this instance was placed as a generator whose catalog entry is now missing.
    pub fn is_missing_generator(&self) -> bool {
        self.generator && effect_spec(&self.effect_id).is_none()
    }

    /// The catalog entry for this instance, or an error if the id is unknown.
    pub fn spec(&self) -> Result<Arc<EffectSpec>, ModelError> {
        effect_spec(&self.effect_id)
            .ok_or_else(|| ModelError::InvalidParam(format!("unknown effect '{}'", self.effect_id)))
    }

    fn param_spec(&self, index: usize) -> Result<ParamRef, ModelError> {
        let spec = self.spec()?;
        if index >= spec.params.len() {
            return Err(ModelError::InvalidParam(format!(
                "effect '{}' has no parameter at index {index}",
                self.effect_id
            )));
        }
        Ok(ParamRef { spec, index })
    }

    fn expect_kind(
        &self,
        pspec: &EffectParamSpec,
        kind: EffectParamKind,
    ) -> Result<(), ModelError> {
        if pspec.kind == kind {
            Ok(())
        } else {
            Err(ModelError::InvalidParam(format!(
                "effect parameter '{}' is {:?}, expected {:?}",
                pspec.name, pspec.kind, kind
            )))
        }
    }

    /// Check whether `pspec` is visible given the current values on `self` at `tick`.
    pub fn is_param_visible(&self, pspec: &EffectParamSpec, spec: &EffectSpec, tick: f64) -> bool {
        let Some(cond) = &pspec.visible_when else {
            return true;
        };
        let Some(other) = spec.param(&cond.param) else {
            return true;
        };
        match other.kind {
            EffectParamKind::Choice => self
                .sample_choice_param(&cond.param)
                .is_some_and(|v| v == cond.equals),
            EffectParamKind::Bool => {
                let target = match cond.equals.as_str() {
                    "true" | "1" => Some(true),
                    "false" | "0" => Some(false),
                    _ => None,
                };
                target.is_some_and(|b| self.sample_bool_param(&cond.param) == Some(b))
            }
            EffectParamKind::Int => cond
                .equals
                .parse::<i64>()
                .ok()
                .is_some_and(|n| self.sample_int_param(&cond.param, tick) == Some(n)),
            EffectParamKind::Scalar | EffectParamKind::Angle => {
                cond.equals.parse::<f32>().ok().is_some_and(|v| {
                    self.sample_param(&cond.param, tick)
                        .is_some_and(|cur| (cur - v).abs() <= 1e-4)
                })
            }
            EffectParamKind::Text | EffectParamKind::Font | EffectParamKind::File => self
                .sample_text_param(&cond.param)
                .is_some_and(|s| s == cond.equals),
            _ => true,
        }
    }

    /// Sampled scalar (`f32`), angle, or integer value of `param` at clip-relative
    /// fractional `tick`, falling back to the catalog default when never set.
    pub fn sample_param(&self, param: &str, tick: f64) -> Option<f32> {
        let spec = self.spec().ok()?;
        let pspec = spec.param(param)?;
        match pspec.kind {
            EffectParamKind::Scalar | EffectParamKind::Angle => Some(
                self.params
                    .get(param)
                    .map(|p| p.sample_at(tick))
                    .unwrap_or(pspec.default),
            ),
            EffectParamKind::Int => Some(
                self.params
                    .get(param)
                    .map(|p| p.sample_at(tick).round())
                    .unwrap_or(pspec.default_int as f32),
            ),
            _ => None,
        }
    }

    /// Sampled integer (`i64`) value of `param` at clip-relative fractional `tick`.
    pub fn sample_int_param(&self, param: &str, tick: f64) -> Option<i64> {
        let spec = self.spec().ok()?;
        let pspec = spec.param(param)?;
        if pspec.kind != EffectParamKind::Int {
            return None;
        }
        Some(
            self.params
                .get(param)
                .map(|p| p.sample_at(tick).round() as i64)
                .unwrap_or(pspec.default_int),
        )
    }

    /// Sampled boolean value of `param`, falling back to the catalog default.
    pub fn sample_bool_param(&self, param: &str) -> Option<bool> {
        let spec = self.spec().ok()?;
        let pspec = spec.param(param)?;
        if pspec.kind != EffectParamKind::Bool {
            return None;
        }
        Some(
            self.bool_params
                .get(param)
                .copied()
                .unwrap_or(pspec.default_bool),
        )
    }

    /// Sampled choice option id of `param`, falling back to the catalog default.
    pub fn sample_choice_param(&self, param: &str) -> Option<String> {
        let spec = self.spec().ok()?;
        let pspec = spec.param(param)?;
        if pspec.kind != EffectParamKind::Choice {
            return None;
        }
        pspec
            .choice_or_default(self.choice_params.get(param).map(String::as_str))
            .map(str::to_owned)
    }

    /// Sampled choice option index of `param` in `pspec.choices`.
    pub fn sample_choice_index(&self, param: &str) -> Option<usize> {
        let spec = self.spec().ok()?;
        let pspec = spec.param(param)?;
        if pspec.kind != EffectParamKind::Choice {
            return None;
        }
        let id = pspec.choice_or_default(self.choice_params.get(param).map(String::as_str))?;
        Some(pspec.choice_index(id).unwrap_or(0))
    }

    /// Sampled 8-bit color value of `param` at clip-relative fractional `tick`.
    pub fn sample_color_param(&self, param: &str, tick: f64) -> Option<[u8; 4]> {
        let spec = self.spec().ok()?;
        let pspec = spec.param(param)?;
        if pspec.kind != EffectParamKind::Color {
            return None;
        }
        if let Some(p) = self.color_params.get(param) {
            return Some(p.sample_at(tick));
        }
        if let Some(p) = self.linear_color_params.get(param) {
            let rgba = p.sample_at(tick);
            return Some(rgba.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8));
        }
        Some(pspec.default_color_u8())
    }

    /// Sampled linear RGBA (`[f32; 4]`) color value of `param` at clip-relative fractional `tick`.
    pub fn sample_linear_color_param(&self, param: &str, tick: f64) -> Option<[f32; 4]> {
        let spec = self.spec().ok()?;
        let pspec = spec.param(param)?;
        if pspec.kind != EffectParamKind::Color {
            return None;
        }
        if let Some(p) = self.linear_color_params.get(param) {
            return Some(p.sample_at(tick));
        }
        if let Some(p) = self.color_params.get(param) {
            let rgba = p.sample_at(tick);
            return Some(rgba.map(|c| f32::from(c) / 255.0));
        }
        Some(pspec.default_color_linear())
    }

    /// Sampled vec2 or size value of `param` at clip-relative fractional `tick`.
    pub fn sample_vec2_param(&self, param: &str, tick: f64) -> Option<[f32; 2]> {
        let spec = self.spec().ok()?;
        let pspec = spec.param(param)?;
        if !matches!(pspec.kind, EffectParamKind::Vec2 | EffectParamKind::Size) {
            return None;
        }
        Some(match self.vec2_params.get(param) {
            Some(p) => p.sample_at(tick),
            None => pspec.default_vec2,
        })
    }

    /// Sampled 3D point (`[f32; 3]`) value of `param` at clip-relative fractional `tick`.
    pub fn sample_vec3_param(&self, param: &str, tick: f64) -> Option<[f32; 3]> {
        let spec = self.spec().ok()?;
        let pspec = spec.param(param)?;
        if pspec.kind != EffectParamKind::Point3 {
            return None;
        }
        Some(match self.vec3_params.get(param) {
            Some(p) => p.sample_at(tick),
            None => pspec.default_vec3,
        })
    }

    /// Sampled 1D curve control points of `param`.
    pub fn sample_curve_param(&self, param: &str) -> Option<Vec<EffectCurvePoint>> {
        let spec = self.spec().ok()?;
        let pspec = spec.param(param)?;
        if pspec.kind != EffectParamKind::Curve {
            return None;
        }
        Some(
            self.curve_params
                .get(param)
                .cloned()
                .unwrap_or_else(|| pspec.default_curve_points()),
        )
    }

    /// Sampled gradient color stops of `param`.
    pub fn sample_gradient_param(&self, param: &str) -> Option<Vec<EffectGradientStop>> {
        let spec = self.spec().ok()?;
        let pspec = spec.param(param)?;
        if pspec.kind != EffectParamKind::Gradient {
            return None;
        }
        Some(
            self.gradient_params
                .get(param)
                .cloned()
                .unwrap_or_else(|| pspec.default_gradient_stops()),
        )
    }

    /// Sampled text, font, or file string of `param`.
    pub fn sample_text_param(&self, param: &str) -> Option<String> {
        let spec = self.spec().ok()?;
        let pspec = spec.param(param)?;
        if !matches!(
            pspec.kind,
            EffectParamKind::Text | EffectParamKind::Font | EffectParamKind::File
        ) {
            return None;
        }
        Some(
            self.text_params
                .get(param)
                .cloned()
                .unwrap_or_else(|| pspec.default_text.clone()),
        )
    }

    /// Sampled layer reference (`ClipId`) of `param` (`None` when unset / `0`).
    pub fn sample_layer_param(&self, param: &str) -> Option<Option<crate::ClipId>> {
        let spec = self.spec().ok()?;
        let pspec = spec.param(param)?;
        if pspec.kind != EffectParamKind::Layer {
            return None;
        }
        let raw = self.layer_params.get(param).copied().unwrap_or(0);
        Some((raw != 0).then(|| crate::ClipId::from_raw(raw)))
    }

    /// Sample all shader-visible parameters in declaration order into a flat `Vec<f32>`.
    pub fn sample_flattened_floats(
        &self,
        tick: f64,
        point: Option<(usize, [f32; 2])>,
    ) -> Result<Vec<f32>, ModelError> {
        let spec = self.spec()?;
        let total: usize = spec.params.iter().map(|p| p.kind.shader_floats()).sum();
        let mut out = Vec::with_capacity(total);
        for (slot, pspec) in spec.params.iter().enumerate() {
            match pspec.kind {
                EffectParamKind::Scalar | EffectParamKind::Angle => {
                    let v = self
                        .params
                        .get(&pspec.name)
                        .map(|p| p.sample_at(tick))
                        .unwrap_or(pspec.default);
                    out.push(v);
                }
                EffectParamKind::Int => {
                    let v = self
                        .params
                        .get(&pspec.name)
                        .map(|p| p.sample_at(tick).round())
                        .unwrap_or(pspec.default_int as f32);
                    out.push(v);
                }
                EffectParamKind::Bool => {
                    let b = self
                        .bool_params
                        .get(&pspec.name)
                        .copied()
                        .unwrap_or(pspec.default_bool);
                    out.push(if b { 1.0 } else { 0.0 });
                }
                EffectParamKind::Choice => {
                    let idx = pspec
                        .choice_or_default(self.choice_params.get(&pspec.name).map(String::as_str))
                        .and_then(|s| pspec.choice_index(s))
                        .unwrap_or(0);
                    out.push(idx as f32);
                }
                EffectParamKind::Color => {
                    let rgba = if let Some(p) = self.linear_color_params.get(&pspec.name) {
                        p.sample_at(tick)
                    } else if let Some(p) = self.color_params.get(&pspec.name) {
                        p.sample_at(tick).map(|c| f32::from(c) / 255.0)
                    } else {
                        pspec.default_color_linear()
                    };
                    out.extend_from_slice(&rgba);
                }
                EffectParamKind::Vec2 | EffectParamKind::Size => {
                    let v = match point {
                        Some((override_slot, override_val)) if override_slot == slot => {
                            override_val
                        }
                        _ => self
                            .vec2_params
                            .get(&pspec.name)
                            .map(|p| p.sample_at(tick))
                            .unwrap_or(pspec.default_vec2),
                    };
                    out.extend_from_slice(&v);
                }
                EffectParamKind::Point3 => {
                    let v = self
                        .vec3_params
                        .get(&pspec.name)
                        .map(|p| p.sample_at(tick))
                        .unwrap_or(pspec.default_vec3);
                    out.extend_from_slice(&v);
                }
                EffectParamKind::Curve => {
                    let pts = self
                        .curve_params
                        .get(&pspec.name)
                        .map(Vec::as_slice)
                        .unwrap_or(&pspec.default_curve);
                    let lut = evaluate_effect_curve_lut(pts);
                    out.extend_from_slice(&lut);
                }
                EffectParamKind::Gradient => {
                    let stops = self
                        .gradient_params
                        .get(&pspec.name)
                        .map(Vec::as_slice)
                        .unwrap_or(&pspec.default_gradient);
                    let lut = evaluate_effect_gradient_lut(stops);
                    for rgba in lut {
                        out.extend_from_slice(&rgba);
                    }
                }
                EffectParamKind::Layer => {
                    let raw = self.layer_params.get(&pspec.name).copied().unwrap_or(0);
                    out.push(raw as f32);
                }
                EffectParamKind::Text | EffectParamKind::Font | EffectParamKind::File => {}
            }
        }
        Ok(out)
    }

    /// Insert or replace a keyframe on a scalar, angle, or integer parameter slot.
    pub fn set_param_keyframe(
        &mut self,
        index: usize,
        tick: i64,
        value: f32,
        easing: Easing,
    ) -> Result<(), ModelError> {
        let pspec = self.param_spec(index)?;
        if !matches!(
            pspec.kind,
            EffectParamKind::Scalar | EffectParamKind::Angle | EffectParamKind::Int
        ) {
            return Err(ModelError::InvalidParam(format!(
                "effect parameter '{}' is {:?}, expected Scalar",
                pspec.name, pspec.kind
            )));
        }
        scalar_or_int_range_check(&pspec, value)?;
        easing.validate()?;
        let stored = if pspec.kind == EffectParamKind::Int {
            value.round()
        } else {
            value
        };
        let default = if pspec.kind == EffectParamKind::Int {
            pspec.default_int as f32
        } else {
            pspec.default
        };
        let name = pspec.name.clone();
        self.params
            .entry(name)
            .or_insert(Param::Constant(default))
            .set_keyframe(tick, stored, easing);
        Ok(())
    }

    /// Insert or replace a keyframe on a color parameter slot.
    pub fn set_color_param_keyframe(
        &mut self,
        index: usize,
        tick: i64,
        value: [u8; 4],
        easing: Easing,
    ) -> Result<(), ModelError> {
        let pspec = self.param_spec(index)?;
        self.expect_kind(&pspec, EffectParamKind::Color)?;
        easing.validate()?;
        let (name, default) = (pspec.name.clone(), pspec.default_color_u8());
        self.linear_color_params.remove(&name);
        self.color_params
            .entry(name)
            .or_insert(Param::Constant(default))
            .set_keyframe(tick, value, easing);
        Ok(())
    }

    /// Insert or replace a keyframe on a linear RGBA (`[f32; 4]`) color parameter slot.
    pub fn set_linear_color_param_keyframe(
        &mut self,
        index: usize,
        tick: i64,
        value: [f32; 4],
        easing: Easing,
    ) -> Result<(), ModelError> {
        let pspec = self.param_spec(index)?;
        self.expect_kind(&pspec, EffectParamKind::Color)?;
        linear_color_check(&pspec, value)?;
        easing.validate()?;
        let (name, default) = (pspec.name.clone(), pspec.default_color_linear());
        self.color_params.remove(&name);
        self.linear_color_params
            .entry(name)
            .or_insert(Param::Constant(default))
            .set_keyframe(tick, value, easing);
        Ok(())
    }

    /// Insert or replace a keyframe on a vec2 or size parameter slot.
    pub fn set_vec2_param_keyframe(
        &mut self,
        index: usize,
        tick: i64,
        value: [f32; 2],
        easing: Easing,
    ) -> Result<(), ModelError> {
        let pspec = self.param_spec(index)?;
        if !matches!(pspec.kind, EffectParamKind::Vec2 | EffectParamKind::Size) {
            return Err(ModelError::InvalidParam(format!(
                "effect parameter '{}' is {:?}, expected Vec2",
                pspec.name, pspec.kind
            )));
        }
        vec2_range_check(&pspec, value)?;
        easing.validate()?;
        let (name, default) = (pspec.name.clone(), pspec.default_vec2);
        self.vec2_params
            .entry(name)
            .or_insert(Param::Constant(default))
            .set_keyframe(tick, value, easing);
        Ok(())
    }

    /// Insert or replace a keyframe on a 3D point (`[f32; 3]`) parameter slot.
    pub fn set_vec3_param_keyframe(
        &mut self,
        index: usize,
        tick: i64,
        value: [f32; 3],
        easing: Easing,
    ) -> Result<(), ModelError> {
        let pspec = self.param_spec(index)?;
        self.expect_kind(&pspec, EffectParamKind::Point3)?;
        vec3_range_check(&pspec, value)?;
        easing.validate()?;
        let (name, default) = (pspec.name.clone(), pspec.default_vec3);
        self.vec3_params
            .entry(name)
            .or_insert(Param::Constant(default))
            .set_keyframe(tick, value, easing);
        Ok(())
    }

    /// Remove the keyframe at exactly `tick` on parameter slot `index`.
    pub fn remove_param_keyframe(&mut self, index: usize, tick: i64) -> Result<(), ModelError> {
        let pspec = self.param_spec(index)?;
        let name = pspec.name.as_str();
        let removed = match pspec.kind {
            EffectParamKind::Scalar | EffectParamKind::Angle | EffectParamKind::Int => self
                .params
                .get_mut(name)
                .is_some_and(|p| p.remove_keyframe(tick)),
            EffectParamKind::Color => {
                let lin = self
                    .linear_color_params
                    .get_mut(name)
                    .is_some_and(|p| p.remove_keyframe(tick));
                let u8_rem = self
                    .color_params
                    .get_mut(name)
                    .is_some_and(|p| p.remove_keyframe(tick));
                lin || u8_rem
            }
            EffectParamKind::Vec2 | EffectParamKind::Size => self
                .vec2_params
                .get_mut(name)
                .is_some_and(|p| p.remove_keyframe(tick)),
            EffectParamKind::Point3 => self
                .vec3_params
                .get_mut(name)
                .is_some_and(|p| p.remove_keyframe(tick)),
            _ => false,
        };
        if removed {
            Ok(())
        } else {
            Err(ModelError::InvalidParam(format!(
                "no keyframe at tick {tick} on {}.{name}",
                self.effect_id
            )))
        }
    }

    /// Replace a scalar, angle, or integer parameter slot with a constant, dropping keyframes.
    pub fn set_param_constant(&mut self, index: usize, value: f32) -> Result<(), ModelError> {
        let pspec = self.param_spec(index)?;
        if !matches!(
            pspec.kind,
            EffectParamKind::Scalar | EffectParamKind::Angle | EffectParamKind::Int
        ) {
            return Err(ModelError::InvalidParam(format!(
                "effect parameter '{}' is {:?}, expected Scalar",
                pspec.name, pspec.kind
            )));
        }
        scalar_or_int_range_check(&pspec, value)?;
        let stored = if pspec.kind == EffectParamKind::Int {
            value.round()
        } else {
            value
        };
        self.params
            .insert(pspec.name.to_string(), Param::Constant(stored));
        Ok(())
    }

    /// Replace an integer parameter slot with a constant `i64`, dropping keyframes.
    pub fn set_int_param_constant(&mut self, index: usize, value: i64) -> Result<(), ModelError> {
        let pspec = self.param_spec(index)?;
        self.expect_kind(&pspec, EffectParamKind::Int)?;
        int_range_check(&pspec, value)?;
        self.params
            .insert(pspec.name.to_string(), Param::Constant(value as f32));
        Ok(())
    }

    /// Set a boolean parameter slot.
    pub fn set_bool_param(&mut self, index: usize, value: bool) -> Result<(), ModelError> {
        let pspec = self.param_spec(index)?;
        self.expect_kind(&pspec, EffectParamKind::Bool)?;
        self.bool_params.insert(pspec.name.to_string(), value);
        Ok(())
    }

    /// Set a choice parameter slot to `choice_id`.
    pub fn set_choice_param(
        &mut self,
        index: usize,
        choice_id: impl Into<String>,
    ) -> Result<(), ModelError> {
        let choice_id = choice_id.into();
        let pspec = self.param_spec(index)?;
        self.expect_kind(&pspec, EffectParamKind::Choice)?;
        choice_check(&pspec, &choice_id)?;
        self.choice_params.insert(pspec.name.to_string(), choice_id);
        Ok(())
    }

    /// Replace a color parameter slot with a constant, dropping keyframes.
    pub fn set_color_param_constant(
        &mut self,
        index: usize,
        value: [u8; 4],
    ) -> Result<(), ModelError> {
        let pspec = self.param_spec(index)?;
        self.expect_kind(&pspec, EffectParamKind::Color)?;
        self.linear_color_params.remove(&pspec.name);
        self.color_params
            .insert(pspec.name.to_string(), Param::Constant(value));
        Ok(())
    }

    /// Replace a color parameter slot with a constant linear RGBA (`[f32; 4]`), dropping keyframes.
    pub fn set_linear_color_param_constant(
        &mut self,
        index: usize,
        value: [f32; 4],
    ) -> Result<(), ModelError> {
        let pspec = self.param_spec(index)?;
        self.expect_kind(&pspec, EffectParamKind::Color)?;
        linear_color_check(&pspec, value)?;
        self.color_params.remove(&pspec.name);
        self.linear_color_params
            .insert(pspec.name.to_string(), Param::Constant(value));
        Ok(())
    }

    /// Replace a vec2 or size parameter slot with a constant, dropping keyframes.
    pub fn set_vec2_param_constant(
        &mut self,
        index: usize,
        value: [f32; 2],
    ) -> Result<(), ModelError> {
        let pspec = self.param_spec(index)?;
        if !matches!(pspec.kind, EffectParamKind::Vec2 | EffectParamKind::Size) {
            return Err(ModelError::InvalidParam(format!(
                "effect parameter '{}' is {:?}, expected Vec2",
                pspec.name, pspec.kind
            )));
        }
        vec2_range_check(&pspec, value)?;
        self.vec2_params
            .insert(pspec.name.to_string(), Param::Constant(value));
        Ok(())
    }

    /// Replace a 3D point parameter slot with a constant `[f32; 3]`, dropping keyframes.
    pub fn set_vec3_param_constant(
        &mut self,
        index: usize,
        value: [f32; 3],
    ) -> Result<(), ModelError> {
        let pspec = self.param_spec(index)?;
        self.expect_kind(&pspec, EffectParamKind::Point3)?;
        vec3_range_check(&pspec, value)?;
        self.vec3_params
            .insert(pspec.name.to_string(), Param::Constant(value));
        Ok(())
    }

    /// Set a 1D curve parameter slot's control points.
    pub fn set_curve_param(
        &mut self,
        index: usize,
        points: Vec<EffectCurvePoint>,
    ) -> Result<(), ModelError> {
        let pspec = self.param_spec(index)?;
        self.expect_kind(&pspec, EffectParamKind::Curve)?;
        curve_check(&pspec, &points)?;
        self.curve_params.insert(pspec.name.to_string(), points);
        Ok(())
    }

    /// Set a gradient parameter slot's color stops.
    pub fn set_gradient_param(
        &mut self,
        index: usize,
        stops: Vec<EffectGradientStop>,
    ) -> Result<(), ModelError> {
        let pspec = self.param_spec(index)?;
        self.expect_kind(&pspec, EffectParamKind::Gradient)?;
        gradient_check(&pspec, &stops)?;
        self.gradient_params.insert(pspec.name.to_string(), stops);
        Ok(())
    }

    /// Set a text, font, or file parameter slot's string value.
    pub fn set_text_param(
        &mut self,
        index: usize,
        value: impl Into<String>,
    ) -> Result<(), ModelError> {
        let pspec = self.param_spec(index)?;
        if !matches!(
            pspec.kind,
            EffectParamKind::Text | EffectParamKind::Font | EffectParamKind::File
        ) {
            return Err(ModelError::InvalidParam(format!(
                "effect parameter '{}' is {:?}, expected Text/Font/File",
                pspec.name, pspec.kind
            )));
        }
        self.text_params
            .insert(pspec.name.to_string(), value.into());
        Ok(())
    }

    /// Set a layer reference parameter slot (`None` clears the reference).
    pub fn set_layer_param(
        &mut self,
        index: usize,
        layer: Option<crate::ClipId>,
    ) -> Result<(), ModelError> {
        let pspec = self.param_spec(index)?;
        self.expect_kind(&pspec, EffectParamKind::Layer)?;
        match layer {
            Some(id) => {
                self.layer_params.insert(pspec.name.to_string(), id.raw());
            }
            None => {
                self.layer_params.remove(&pspec.name);
            }
        }
        Ok(())
    }

    /// Set parameter slot `index` from a [`ParamValue`], dispatching on the
    /// catalog kind.
    pub fn set_param_value_constant(
        &mut self,
        index: usize,
        value: ParamValue,
    ) -> Result<(), ModelError> {
        let pspec = self.param_spec(index)?;
        match pspec.kind {
            EffectParamKind::Scalar | EffectParamKind::Angle | EffectParamKind::Int => {
                self.set_param_constant(index, expect_scalar(value)?)
            }
            EffectParamKind::Color => self.set_color_param_constant(index, expect_color(value)?),
            EffectParamKind::Vec2 | EffectParamKind::Size => {
                self.set_vec2_param_constant(index, expect_vec2(value)?)
            }
            EffectParamKind::Point3 => self.set_vec3_param_constant(index, expect_vec3(value)?),
            _ => Err(ModelError::InvalidParam(format!(
                "effect parameter '{}' ({:?}) is not settable via ParamValue",
                pspec.name, pspec.kind
            ))),
        }
    }

    /// Insert or replace a keyframe from a [`ParamValue`], dispatching on kind.
    pub fn set_param_value_keyframe(
        &mut self,
        index: usize,
        tick: i64,
        value: ParamValue,
        easing: Easing,
    ) -> Result<(), ModelError> {
        let pspec = self.param_spec(index)?;
        match pspec.kind {
            EffectParamKind::Scalar | EffectParamKind::Angle | EffectParamKind::Int => {
                self.set_param_keyframe(index, tick, expect_scalar(value)?, easing)
            }
            EffectParamKind::Color => {
                self.set_color_param_keyframe(index, tick, expect_color(value)?, easing)
            }
            EffectParamKind::Vec2 | EffectParamKind::Size => {
                self.set_vec2_param_keyframe(index, tick, expect_vec2(value)?, easing)
            }
            EffectParamKind::Point3 => {
                self.set_vec3_param_keyframe(index, tick, expect_vec3(value)?, easing)
            }
            _ => Err(ModelError::InvalidParam(format!(
                "effect parameter '{}' ({:?}) does not support keyframes",
                pspec.name, pspec.kind
            ))),
        }
    }

    /// `Ok` iff the id is known, every set parameter names a real slot of the
    /// matching kind, every curve is structurally sound, and every value lies
    /// in range.
    pub fn validate(&self) -> Result<(), ModelError> {
        let spec = self.spec()?;
        if let Some(name) = &self.name {
            check_effect_name(name)?;
        }
        let slot = |name: &str, store: &str, holds: fn(EffectParamKind) -> bool| {
            stored_slot(&spec.params, "effect", &self.effect_id, name, store, holds)
        };
        for (name, param) in &self.params {
            let pspec = slot(name, "params", holds_scalar)?;
            param.validate_shape()?;
            param.for_each_value(|v| scalar_or_int_range_check(pspec, *v))?;
        }
        for (name, param) in &self.color_params {
            slot(name, "color_params", |k| k == EffectParamKind::Color)?;
            param.validate_shape()?;
        }
        for (name, param) in &self.linear_color_params {
            let pspec = slot(name, "linear_color_params", |k| k == EffectParamKind::Color)?;
            param.validate_shape()?;
            param.for_each_value(|v| linear_color_check(pspec, *v))?;
        }
        for (name, param) in &self.vec2_params {
            let pspec = slot(name, "vec2_params", holds_vec2)?;
            param.validate_shape()?;
            param.for_each_value(|v| vec2_range_check(pspec, *v))?;
        }
        for (name, param) in &self.vec3_params {
            let pspec = slot(name, "vec3_params", |k| k == EffectParamKind::Point3)?;
            param.validate_shape()?;
            param.for_each_value(|v| vec3_range_check(pspec, *v))?;
        }
        for name in self.bool_params.keys() {
            slot(name, "bool_params", |k| k == EffectParamKind::Bool)?;
        }
        for (name, choice_id) in &self.choice_params {
            choice_check(
                slot(name, "choice_params", |k| k == EffectParamKind::Choice)?,
                choice_id,
            )?;
        }
        for (name, points) in &self.curve_params {
            curve_check(
                slot(name, "curve_params", |k| k == EffectParamKind::Curve)?,
                points,
            )?;
        }
        for (name, stops) in &self.gradient_params {
            gradient_check(
                slot(name, "gradient_params", |k| k == EffectParamKind::Gradient)?,
                stops,
            )?;
        }
        for name in self.text_params.keys() {
            slot(name, "text_params", holds_text)?;
        }
        for name in self.layer_params.keys() {
            slot(name, "layer_params", |k| k == EffectParamKind::Layer)?;
        }
        Ok(())
    }
}

/// Kinds stored in the scalar map: scalar, angle, and integer.
pub(crate) fn holds_scalar(kind: EffectParamKind) -> bool {
    matches!(
        kind,
        EffectParamKind::Scalar | EffectParamKind::Angle | EffectParamKind::Int
    )
}

/// Kinds stored in the two-component map: vector and size.
pub(crate) fn holds_vec2(kind: EffectParamKind) -> bool {
    matches!(kind, EffectParamKind::Vec2 | EffectParamKind::Size)
}

/// Kinds stored in the string map: text, font, and file.
pub(crate) fn holds_text(kind: EffectParamKind) -> bool {
    matches!(
        kind,
        EffectParamKind::Text | EffectParamKind::Font | EffectParamKind::File
    )
}

/// The catalog spec of the stored parameter `name`.
///
/// Fails when `params` has no parameter of that name, or when its kind is not
/// one that `store`, the field the value was read from, holds. `owner` is
/// `"effect"` or `"transition"` and `id` its catalog id, for the message.
pub(crate) fn stored_slot<'s>(
    params: &'s [EffectParamSpec],
    owner: &str,
    id: &str,
    name: &str,
    store: &str,
    holds: fn(EffectParamKind) -> bool,
) -> Result<&'s EffectParamSpec, ModelError> {
    let pspec = params.iter().find(|p| p.name == name).ok_or_else(|| {
        ModelError::InvalidParam(format!("{owner} '{id}' has no parameter '{name}'"))
    })?;
    if holds(pspec.kind) {
        Ok(pspec)
    } else {
        Err(ModelError::InvalidParam(format!(
            "{owner} parameter '{name}' is {:?}, not stored in {store}",
            pspec.kind
        )))
    }
}

pub(crate) fn choice_check(pspec: &EffectParamSpec, choice_id: &str) -> Result<(), ModelError> {
    if pspec.choice_index(choice_id).is_none() {
        return Err(ModelError::InvalidParam(format!(
            "{} = '{choice_id}' is not a valid choice",
            pspec.name
        )));
    }
    Ok(())
}

pub(crate) fn range_check(pspec: &EffectParamSpec, value: f32) -> Result<(), ModelError> {
    if !value.is_finite() || value < pspec.min || value > pspec.max {
        return Err(ModelError::InvalidParam(format!(
            "{} = {value} out of range [{}, {}]",
            pspec.name, pspec.min, pspec.max
        )));
    }
    Ok(())
}

pub(crate) fn scalar_or_int_range_check(
    pspec: &EffectParamSpec,
    value: f32,
) -> Result<(), ModelError> {
    if pspec.kind == EffectParamKind::Int {
        let min = pspec.int_min as f32;
        let max = pspec.int_max as f32;
        if !value.is_finite() || value < min || value > max {
            return Err(ModelError::InvalidParam(format!(
                "{} = {value} out of range [{}, {}]",
                pspec.name, pspec.int_min, pspec.int_max
            )));
        }
        Ok(())
    } else {
        range_check(pspec, value)
    }
}

pub(crate) fn int_range_check(pspec: &EffectParamSpec, value: i64) -> Result<(), ModelError> {
    if value < pspec.int_min || value > pspec.int_max {
        return Err(ModelError::InvalidParam(format!(
            "{} = {value} out of range [{}, {}]",
            pspec.name, pspec.int_min, pspec.int_max
        )));
    }
    Ok(())
}

pub(crate) fn linear_color_check(
    pspec: &EffectParamSpec,
    value: [f32; 4],
) -> Result<(), ModelError> {
    if value.iter().any(|c| !c.is_finite()) {
        return Err(ModelError::InvalidParam(format!(
            "{} has non-finite linear color component",
            pspec.name
        )));
    }
    Ok(())
}

pub(crate) fn vec2_range_check(pspec: &EffectParamSpec, value: [f32; 2]) -> Result<(), ModelError> {
    for (i, &component) in value.iter().enumerate() {
        if !component.is_finite() || component < pspec.min || component > pspec.max {
            return Err(ModelError::InvalidParam(format!(
                "{}[{i}] = {component} out of range [{}, {}]",
                pspec.name, pspec.min, pspec.max
            )));
        }
    }
    Ok(())
}

pub(crate) fn vec3_range_check(pspec: &EffectParamSpec, value: [f32; 3]) -> Result<(), ModelError> {
    for (i, &component) in value.iter().enumerate() {
        if !component.is_finite() || component < pspec.min || component > pspec.max {
            return Err(ModelError::InvalidParam(format!(
                "{}[{i}] = {component} out of range [{}, {}]",
                pspec.name, pspec.min, pspec.max
            )));
        }
    }
    Ok(())
}

pub(crate) fn curve_check(
    pspec: &EffectParamSpec,
    points: &[EffectCurvePoint],
) -> Result<(), ModelError> {
    if points
        .iter()
        .any(|pt| !pt.x.is_finite() || !pt.y.is_finite())
    {
        return Err(ModelError::InvalidParam(format!(
            "{} has non-finite curve point",
            pspec.name
        )));
    }
    Ok(())
}

pub(crate) fn gradient_check(
    pspec: &EffectParamSpec,
    stops: &[EffectGradientStop],
) -> Result<(), ModelError> {
    if stops
        .iter()
        .any(|s| !s.offset.is_finite() || s.color.iter().any(|c| !c.is_finite()))
    {
        return Err(ModelError::InvalidParam(format!(
            "{} has non-finite gradient stop",
            pspec.name
        )));
    }
    Ok(())
}

fn expect_scalar(value: ParamValue) -> Result<f32, ModelError> {
    match value {
        ParamValue::Scalar(v) => Ok(v),
        _ => Err(ModelError::InvalidParam("expected a scalar value".into())),
    }
}

fn expect_vec2(value: ParamValue) -> Result<[f32; 2], ModelError> {
    match value {
        ParamValue::Vec2(v) => Ok(v),
        _ => Err(ModelError::InvalidParam("expected a vec2 value".into())),
    }
}

fn expect_vec3(value: ParamValue) -> Result<[f32; 3], ModelError> {
    match value {
        ParamValue::Vec3(v) => Ok(v),
        _ => Err(ModelError::InvalidParam("expected a vec3 value".into())),
    }
}

fn expect_color(value: ParamValue) -> Result<[u8; 4], ModelError> {
    match value {
        ParamValue::Color(v) => Ok(v),
        _ => Err(ModelError::InvalidParam("expected a color value".into())),
    }
}

#[cfg(test)]
#[path = "effects_typed_tests.rs"]
pub(crate) mod typed_tests;

#[cfg(test)]
#[path = "effects_name_tests.rs"]
mod name_tests;

#[cfg(test)]
mod tests {
    use super::*;

    /// Every shipped effect names the group it is listed under, and an entry
    /// that names none is listed under Other. A shipped entry added without a
    /// category fails here. A generator is listed under Generators, which has
    /// no folders, so it names none.
    #[test]
    fn every_shipped_effect_names_its_group_and_a_nameless_one_is_other() {
        for spec in shipped_effect_catalog()
            .iter()
            .filter(|spec| !spec.generator)
        {
            assert!(!spec.category.is_empty(), "{} names no category", spec.id);
            assert_eq!(spec.category(), spec.category);
        }
        let spec: EffectSpec = toml::from_str("id = \"x\"\nlabel = \"X\"").expect("parses");
        assert_eq!(spec.category(), OTHER_VIDEO_CATEGORY);
    }

    #[test]
    fn catalog_ids_are_unique() {
        let catalog = effect_catalog();
        let mut ids: Vec<&str> = catalog.iter().map(|s| s.id.as_str()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), catalog.len());
    }

    #[test]
    fn unknown_effect_fails_validation() {
        assert!(EffectInstance::new("nope").validate().is_err());
        assert!(EffectInstance::new("gaussian_blur").validate().is_ok());
    }

    #[test]
    fn sampled_param_falls_back_to_default() {
        let fx = EffectInstance::new("gaussian_blur");
        assert_eq!(fx.sample_param("radius", 0.0), Some(4.0));
        assert_eq!(fx.sample_param("missing", 0.0), None);
    }

    #[test]
    fn out_of_range_constant_rejected() {
        let mut fx = EffectInstance::new("vignette");
        assert!(fx.set_param_constant(0, 2.0).is_err()); // amount max 1.0
        assert!(fx.set_param_constant(0, 0.5).is_ok());
        assert_eq!(fx.sample_param("amount", 0.0), Some(0.5));
    }

    #[test]
    fn keyframe_roundtrip_on_a_param() {
        let mut fx = EffectInstance::new("gaussian_blur");
        fx.set_param_keyframe(0, 0, 0.0, Easing::Linear).unwrap();
        fx.set_param_keyframe(0, 24, 8.0, Easing::Linear).unwrap();
        assert_eq!(fx.sample_param("radius", 12.0), Some(4.0));
        fx.validate().unwrap();
        fx.remove_param_keyframe(0, 24).unwrap();
        // Removing the last-but-one keyframe leaves a constant.
        assert!(fx.remove_param_keyframe(0, 999).is_err());
    }

    #[test]
    fn legacy_scalar_json_still_loads() {
        let fx: EffectInstance =
            serde_json::from_str(r#"{"effect_id":"gaussian_blur","params":[["radius",4.0]]}"#)
                .unwrap();
        assert_eq!(fx.effect_id, "gaussian_blur");
        assert_eq!(fx.sample_param("radius", 0.0), Some(4.0));
        assert!(fx.color_params.is_empty());
        assert!(fx.vec2_params.is_empty());
        fx.validate().unwrap();
    }

    #[test]
    fn color_params_roundtrip() {
        let mut fx = EffectInstance::new("color_overlay");
        fx.set_color_param_constant(0, [10, 20, 30, 40]).unwrap();
        fx.set_vec2_param_constant(1, [0.25, -0.5]).unwrap();
        let json = serde_json::to_string(&fx).unwrap();
        let loaded: EffectInstance = serde_json::from_str(&json).unwrap();
        assert_eq!(
            loaded.sample_color_param("color", 0.0),
            Some([10, 20, 30, 40])
        );
        assert_eq!(loaded.sample_vec2_param("offset", 0.0), Some([0.25, -0.5]));
        loaded.validate().unwrap();
    }

    #[test]
    fn wrong_kind_set_rejected() {
        let mut fx = EffectInstance::new("color_overlay");
        // color slot is Color — scalar setter rejected.
        assert!(fx.set_param_constant(0, 0.5).is_err());
        // offset slot is Vec2 — color setter rejected.
        assert!(fx.set_color_param_constant(1, [1, 2, 3, 4]).is_err());
        // amount slot is Scalar — vec2 setter rejected.
        assert!(fx.set_vec2_param_constant(2, [0.0, 0.0]).is_err());
        // Scalar name stored in color_params fails validate.
        fx.color_params
            .insert("amount".into(), Param::Constant([1, 2, 3, 4]));
        assert!(fx.validate().is_err());
    }

    #[test]
    fn vec2_component_out_of_range_rejected() {
        let mut fx = EffectInstance::new("color_overlay");
        assert!(fx.set_vec2_param_constant(1, [0.0, 1.5]).is_err());
        assert!(fx.set_vec2_param_constant(1, [-1.0, 1.0]).is_ok());
    }

    fn all_15_param_specs() -> Vec<EffectParamSpec> {
        vec![
            EffectParamSpec {
                name: "scalar_p".into(),
                label: "Scalar".into(),
                kind: EffectParamKind::Scalar,
                default: 0.25,
                min: 0.0,
                max: 1.0,
                soft_min: Some(0.1),
                soft_max: Some(0.9),
                unit: Some("px".into()),
                display: Some(EffectParamDisplay::Percentage),
                tooltip: Some("scalar tip".into()),
                ..Default::default()
            },
            EffectParamSpec {
                name: "int_p".into(),
                label: "Int".into(),
                kind: EffectParamKind::Int,
                default_int: 3,
                int_min: 1,
                int_max: 10,
                ..Default::default()
            },
            EffectParamSpec {
                name: "bool_p".into(),
                label: "Bool".into(),
                kind: EffectParamKind::Bool,
                default_bool: false,
                ..Default::default()
            },
            EffectParamSpec {
                name: "choice_p".into(),
                label: "Choice".into(),
                kind: EffectParamKind::Choice,
                choices: vec![
                    EffectChoiceOption::new("fast", "Fast"),
                    EffectChoiceOption::new("quality", "Quality"),
                ],
                default_choice: Some("fast".into()),
                ..Default::default()
            },
            EffectParamSpec {
                name: "color_p".into(),
                label: "Color".into(),
                kind: EffectParamKind::Color,
                default_color: [255, 0, 0, 255],
                ..Default::default()
            },
            EffectParamSpec {
                name: "vec2_p".into(),
                label: "Vec2".into(),
                kind: EffectParamKind::Vec2,
                default_vec2: [0.1, 0.2],
                min: -1.0,
                max: 1.0,
                ..Default::default()
            },
            EffectParamSpec {
                name: "point3_p".into(),
                label: "Point3".into(),
                kind: EffectParamKind::Point3,
                default_vec3: [0.1, 0.2, 0.3],
                min: -1.0,
                max: 1.0,
                ..Default::default()
            },
            EffectParamSpec {
                name: "size_p".into(),
                label: "Size".into(),
                kind: EffectParamKind::Size,
                default_vec2: [640.0, 360.0],
                min: 1.0,
                max: 4096.0,
                ..Default::default()
            },
            EffectParamSpec {
                name: "angle_p".into(),
                label: "Angle".into(),
                kind: EffectParamKind::Angle,
                default: 45.0,
                min: -360.0,
                max: 360.0,
                display: Some(EffectParamDisplay::AngleDial),
                visible_when: Some(EffectVisibilityCondition::new("bool_p", "true")),
                ..Default::default()
            },
            EffectParamSpec {
                name: "curve_p".into(),
                label: "Curve".into(),
                kind: EffectParamKind::Curve,
                default_curve: vec![
                    EffectCurvePoint::new(0.0, 0.0),
                    EffectCurvePoint::new(1.0, 1.0),
                ],
                ..Default::default()
            },
            EffectParamSpec {
                name: "grad_p".into(),
                label: "Gradient".into(),
                kind: EffectParamKind::Gradient,
                default_gradient: vec![
                    EffectGradientStop::new(0.0, [0.0, 0.0, 0.0, 1.0]),
                    EffectGradientStop::new(1.0, [1.0, 1.0, 1.0, 1.0]),
                ],
                ..Default::default()
            },
            EffectParamSpec {
                name: "text_p".into(),
                label: "Text".into(),
                kind: EffectParamKind::Text,
                default_text: "hello".into(),
                ..Default::default()
            },
            EffectParamSpec {
                name: "font_p".into(),
                label: "Font".into(),
                kind: EffectParamKind::Font,
                default_text: "Inter".into(),
                ..Default::default()
            },
            EffectParamSpec {
                name: "file_p".into(),
                label: "File".into(),
                kind: EffectParamKind::File,
                default_text: "lut.cube".into(),
                ..Default::default()
            },
            EffectParamSpec {
                name: "layer_p".into(),
                label: "Layer".into(),
                kind: EffectParamKind::Layer,
                ..Default::default()
            },
        ]
    }

    #[test]
    fn all_15_param_kinds_roundtrip_on_effect_and_transition_and_respect_visibility() {
        let (_effect_reg, _) = crate::register_effect(EffectSpec {
            id: "test.models.all_15_v2".into(),
            label: "All 15".into(),
            category: "Stylize".into(),
            params: all_15_param_specs(),
            groups: vec![EffectGroupSpec {
                id: "adv".into(),
                label: "Advanced".into(),
                collapsed: true,
            }],
            generator: false,
            stateful: false,
            ..Default::default()
        })
        .expect("register test effect");
        let (_tr_reg, _) = crate::register_transition(crate::TransitionSpec {
            id: "test.models.tr_15_v2".into(),
            label: "Tr 15".into(),
            params: all_15_param_specs(),
        })
        .expect("register test transition");

        let mut fx = EffectInstance::new("test.models.all_15_v2");
        let spec = fx.spec().expect("registered spec");
        // Initially `bool_p` is false, so `angle_p` (slot 8) is hidden.
        assert!(!fx.is_param_visible(&spec.params[8], &spec, 0.0));

        fx.set_param_constant(0, 0.75).unwrap();
        fx.set_int_param_constant(1, 7).unwrap();
        fx.set_bool_param(2, true).unwrap();
        fx.set_choice_param(3, "quality").unwrap();
        fx.set_linear_color_param_constant(4, [0.2, 0.4, 0.6, 1.0])
            .unwrap();
        fx.set_vec2_param_constant(5, [-0.5, 0.5]).unwrap();
        fx.set_vec3_param_keyframe(6, 0, [0.0, 0.0, 0.0], Easing::Linear)
            .unwrap();
        fx.set_vec3_param_keyframe(6, 20, [0.4, 0.6, 0.8], Easing::Linear)
            .unwrap();
        fx.set_vec2_param_constant(7, [1920.0, 1080.0]).unwrap();
        fx.set_param_constant(8, 180.0).unwrap();
        fx.set_curve_param(
            9,
            vec![
                EffectCurvePoint::new(0.0, 1.0),
                EffectCurvePoint::new(1.0, 0.0),
            ],
        )
        .unwrap();
        fx.set_gradient_param(
            10,
            vec![
                EffectGradientStop::new(0.0, [1.0, 0.0, 0.0, 1.0]),
                EffectGradientStop::new(1.0, [0.0, 0.0, 1.0, 1.0]),
            ],
        )
        .unwrap();
        fx.set_text_param(11, "updated").unwrap();
        fx.set_text_param(12, "Roboto").unwrap();
        fx.set_text_param(13, "grade.cube").unwrap();
        fx.set_layer_param(14, Some(crate::ClipId::from_raw(42)))
            .unwrap();

        // With `bool_p` = true, `angle_p` is now visible!
        assert!(fx.is_param_visible(&spec.params[8], &spec, 0.0));
        fx.validate().unwrap();

        let json = serde_json::to_string(&fx).unwrap();
        let loaded: EffectInstance = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded, fx);
        assert_eq!(
            loaded.sample_vec3_param("point3_p", 10.0),
            Some([0.2, 0.3, 0.4])
        );
        let floats = loaded.sample_flattened_floats(10.0, None).unwrap();
        let expected_floats: usize = spec.params.iter().map(|p| p.kind.shader_floats()).sum();
        assert_eq!(floats.len(), expected_floats);

        // Verify `clip_params_for` hides `angle_p` when `bool_p` is false and shows it when true.
        let mut clip = crate::Clip::generated(
            crate::Generator::SolidColor {
                rgba: [0, 0, 0, 255],
            },
            crate::TimeRange::at_rate(0, 48, crate::Rational::FPS_24),
        );
        clip.effects.push(loaded);
        let rows_when_true = crate::clip_params_for(&clip);
        assert!(
            rows_when_true.iter().any(|r| r.param
                == crate::ClipParam::Effect {
                    effect: 0,
                    param: 8
                }),
            "angle_p must be present in clip_params_for when bool_p is true"
        );
        clip.effects[0].set_bool_param(2, false).unwrap();
        let rows_when_false = crate::clip_params_for(&clip);
        assert!(
            !rows_when_false.iter().any(|r| r.param
                == crate::ClipParam::Effect {
                    effect: 0,
                    param: 8
                }),
            "angle_p must be omitted from clip_params_for when bool_p is false"
        );

        // Transition round-trip across all 15 kinds.
        let mut tr = crate::Transition::new(
            crate::ClipId::from_raw(1),
            crate::ClipId::from_raw(2),
            "test.models.tr_15_v2",
            12,
        );
        tr.set_param_constant(0, 0.5).unwrap();
        tr.set_int_param(1, 5).unwrap();
        tr.set_bool_param(2, true).unwrap();
        tr.set_choice_param(3, "quality").unwrap();
        tr.set_linear_color_param(4, [0.1, 0.2, 0.3, 1.0]).unwrap();
        tr.set_vec2_param(5, [0.25, -0.25]).unwrap();
        tr.set_vec3_param(6, [0.1, 0.2, 0.3]).unwrap();
        tr.set_vec2_param(7, [1280.0, 720.0]).unwrap();
        tr.set_param_constant(8, 90.0).unwrap();
        tr.set_curve_param(
            9,
            vec![
                EffectCurvePoint::new(0.0, 0.5),
                EffectCurvePoint::new(1.0, 0.5),
            ],
        )
        .unwrap();
        tr.set_gradient_param(
            10,
            vec![
                EffectGradientStop::new(0.0, [0.0, 1.0, 0.0, 1.0]),
                EffectGradientStop::new(1.0, [1.0, 0.0, 1.0, 1.0]),
            ],
        )
        .unwrap();
        tr.set_text_param(11, "tr_text").unwrap();
        tr.set_text_param(12, "Mono").unwrap();
        tr.set_text_param(13, "tr.cube").unwrap();
        tr.set_layer_param(14, Some(crate::ClipId::from_raw(9)))
            .unwrap();
        tr.validate().unwrap();
        let tr_json = serde_json::to_string(&tr).unwrap();
        let tr_loaded: crate::Transition = serde_json::from_str(&tr_json).unwrap();
        assert_eq!(tr_loaded, tr);
        let tr_spec = crate::transition_spec("test.models.tr_15_v2").unwrap();
        assert_eq!(
            tr_loaded.sample_flattened_floats(0.0).unwrap().len(),
            tr_spec
                .params
                .iter()
                .map(|p| p.kind.shader_floats())
                .sum::<usize>()
        );
    }
}
