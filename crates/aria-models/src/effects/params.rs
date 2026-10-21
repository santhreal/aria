use serde::{Deserialize, Serialize};

#[cfg(doc)]
use super::EffectSpec;
use super::curve::{
    EFFECT_CURVE_LUT_SAMPLES, EFFECT_GRADIENT_LUT_SAMPLES, EffectCurvePoint, EffectGradientStop,
};
use crate::clip::ParamValue;

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

/// Unit suffix marking an effect parameter measured in canvas pixels.
pub const CANVAS_PX_UNIT: &str = "px";

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

    /// Whether this parameter is measured in canvas pixels (`unit == Some("px")`).
    pub fn is_canvas_px(&self) -> bool {
        self.unit.as_deref() == Some(CANVAS_PX_UNIT)
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
