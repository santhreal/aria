//! Visual effects, CPU effects, generators, transitions, clip types, driver
//! languages, presets, themes, and workspaces registered by native plugins.

use aria_plugin_abi::{
    ARIA_PARAM_COLOR, ARIA_PARAM_RECT, ARIA_PARAM_SCALAR, ARIA_PARAM_VEC2, ARIA_PARAM_VEC3,
    AriaParamValue,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::Host;
use crate::effect_graph::EffectBounds;

/// Number of uniformly spaced samples in a flattened curve lookup table.
pub const EFFECT_CURVE_LUT_SAMPLES: usize = 64;

/// Number of uniformly spaced RGBA stops in a flattened gradient lookup table.
pub const EFFECT_GRADIENT_LUT_SAMPLES: usize = 64;

/// Value kind of a visual effect or transition parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectParamKind {
    /// An `f32` in `min..=max`.
    #[default]
    Scalar,
    /// An integer in `int_min..=int_max`.
    Int,
    /// A switch.
    Bool,
    /// One option of [`EffectParamSpec::choices`]. Its flattened float is the
    /// option's index.
    Choice,
    /// An RGBA colour. The flattened floats hold its channels in `0.0..=1.0`.
    Color,
    /// Two components in `min..=max`. A clip type's `handles` draws it on the
    /// Viewer.
    Vec2,
    /// Three components in `min..=max`.
    Point3,
    /// Width and height in `min..=max`.
    Size,
    /// Degrees in `min..=max`.
    Angle,
    /// A response curve over `0.0..=1.0`, flattened to
    /// [`EFFECT_CURVE_LUT_SAMPLES`] samples.
    Curve,
    /// A colour gradient over `0.0..=1.0`, flattened to
    /// [`EFFECT_GRADIENT_LUT_SAMPLES`] linear RGBA samples.
    Gradient,
    /// UTF-8 text, read with [`CpuFrameIn::text`]. Shaders do not read it.
    Text,
    /// A font family name, read with [`CpuFrameIn::font`]. Shaders do not read
    /// it.
    Font,
    /// A file path, read with [`CpuFrameIn::file`]. Shaders do not read it.
    File,
    /// A clip on the timeline. A pass that lists it as an input reads that
    /// clip's frame. A CPU effect reads the clip id with [`CpuFrameIn::layer`].
    Layer,
}

impl EffectParamKind {
    /// Number of `f32` elements this parameter occupies in the flattened
    /// shader storage buffer and CPU float slice. `Text`, `Font`, and `File`
    /// occupy none.
    pub const fn flattened_floats(self) -> usize {
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

/// Inspector control presentation for a scalar parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectParamDisplay {
    /// A horizontal scrub slider.
    #[default]
    Slider,
    /// A rotary dial in degrees.
    AngleDial,
    /// The value formatted as a percentage.
    Percentage,
}

/// One option of an [`EffectParamKind::Choice`] parameter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectChoiceOption {
    /// Option id, stored on the effect instance.
    pub id: String,
    /// The text the dropdown lists.
    pub label: String,
}

impl EffectChoiceOption {
    /// An option with id `id`, listed as `label`.
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
        }
    }
}

/// One control point on an [`EffectParamKind::Curve`] parameter (`x, y` in `0.0..=1.0`).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct EffectCurvePoint {
    /// Input, in `0.0..=1.0`.
    pub x: f32,
    /// Output at `x`.
    pub y: f32,
}

impl EffectCurvePoint {
    /// The point `(x, y)`.
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// One colour stop on an [`EffectParamKind::Gradient`] parameter (`position` in `0.0..=1.0`, linear RGBA).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct EffectGradientStop {
    /// Position along the gradient, in `0.0..=1.0`.
    #[serde(alias = "offset")]
    pub position: f32,
    /// Linear RGBA `[r, g, b, a]`.
    pub color: [f32; 4],
}

impl EffectGradientStop {
    /// A stop of `color` at `position`.
    pub const fn new(position: f32, color: [f32; 4]) -> Self {
        Self { position, color }
    }
}

/// Condition under which a parameter row is shown in the inspector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectVisibilityCondition {
    /// Name of the controlling parameter of the same effect or transition.
    pub param: String,
    /// Shows the row while the controlling switch equals this.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub equals_bool: Option<bool>,
    /// Shows the row while the controlling integer equals this.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub equals_int: Option<i32>,
    /// Shows the row while the controlling choice selects this option id. The
    /// host reads `equals_choice`, then `equals_bool`, then `equals_int`, and
    /// uses the first that is set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub equals_choice: Option<String>,
}

impl EffectVisibilityCondition {
    /// Show the row while switch `param` equals `value`.
    pub fn when_bool(param: impl Into<String>, value: bool) -> Self {
        Self {
            param: param.into(),
            equals_bool: Some(value),
            equals_int: None,
            equals_choice: None,
        }
    }

    /// Show the row while integer `param` equals `value`.
    pub fn when_int(param: impl Into<String>, value: i32) -> Self {
        Self {
            param: param.into(),
            equals_bool: None,
            equals_int: Some(value),
            equals_choice: None,
        }
    }

    /// Show the row while choice `param` selects the option `choice`.
    pub fn when_choice(param: impl Into<String>, choice: impl Into<String>) -> Self {
        Self {
            param: param.into(),
            equals_bool: None,
            equals_int: None,
            equals_choice: Some(choice.into()),
        }
    }
}

/// Named collapsible parameter group in an effect or transition inspector.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct EffectGroupSpec {
    /// Group id that parameters name in [`EffectParamSpec::group`].
    pub id: String,
    /// The section heading in Effect Controls.
    pub label: String,
    /// The section starts collapsed.
    #[serde(default)]
    pub collapsed: bool,
}

impl EffectGroupSpec {
    /// An expanded group `id` headed `label`.
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            collapsed: false,
        }
    }

    /// Set whether the section starts collapsed.
    pub fn with_collapsed(mut self, collapsed: bool) -> Self {
        self.collapsed = collapsed;
        self
    }
}

/// One parameter of a visual effect, generator, or transition.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct EffectParamSpec {
    /// Parameter key: unique within the effect and a WGSL identifier. A shader
    /// reads it through `param_<name>()`.
    pub name: String,
    /// The row label.
    pub label: String,
    /// The value kind.
    #[serde(default)]
    pub kind: EffectParamKind,
    /// Default for `Scalar` and `Angle`.
    #[serde(default)]
    pub default: f32,
    /// Lowest value of `Scalar`, `Vec2`, `Point3`, `Size`, and `Angle`.
    #[serde(default)]
    pub min: f32,
    /// Highest value of the kinds `min` bounds. Not below `min`.
    #[serde(default)]
    pub max: f32,
    /// Default for `Color`: RGBA `[r, g, b, a]` in `0..=255`.
    #[serde(default)]
    pub default_color: [u8; 4],
    /// Default for `Vec2` and `Size`.
    #[serde(default)]
    pub default_vec2: [f32; 2],
    /// The control a `Scalar` row draws.
    #[serde(default)]
    pub display: EffectParamDisplay,
    /// Lower end of the scrub range before values clamp to `min`, or `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub soft_min: Option<f32>,
    /// Upper end of the scrub range before values clamp to `max`, or `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub soft_max: Option<f32>,
    /// Unit suffix, such as `px`, `deg`, `%`, or `s`, or `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    /// Tooltip text for the row, or `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tooltip: Option<String>,
    /// Id of the [`EffectGroupSpec`] the row sits in, or `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// Shows the row only while the condition holds, or always when `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible_when: Option<EffectVisibilityCondition>,
    /// The options of a `Choice`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<EffectChoiceOption>,
    /// Default for `Int`.
    #[serde(default)]
    pub default_int: i32,
    /// Lowest value of `Int`.
    #[serde(default)]
    pub int_min: i64,
    /// Highest value of `Int`. Not below `int_min`.
    #[serde(default)]
    pub int_max: i64,
    /// Default for `Bool`.
    #[serde(default)]
    pub default_bool: bool,
    /// Default option id for `Choice`. Empty selects the first option.
    #[serde(default)]
    pub default_choice: String,
    /// Default for `Point3`.
    #[serde(default)]
    pub default_vec3: [f32; 3],
    /// Default control points for `Curve`. Empty is the identity `(0, 0)` to
    /// `(1, 1)`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub default_curve: Vec<EffectCurvePoint>,
    /// Default stops for `Gradient`. Empty is black to white.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub default_gradient: Vec<EffectGradientStop>,
    /// Default for `Text`, `Font`, and `File`.
    #[serde(default)]
    pub default_text: String,
}

impl EffectParamSpec {
    /// Define a scalar parameter with default and inclusive `[min, max]` range.
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
            ..Default::default()
        }
    }

    /// Define an integer parameter with default and inclusive `[min, max]` range.
    pub fn int(
        name: impl Into<String>,
        label: impl Into<String>,
        default_int: i32,
        min: i32,
        max: i32,
    ) -> Self {
        Self {
            name: name.into(),
            label: label.into(),
            kind: EffectParamKind::Int,
            default: default_int as f32,
            default_int,
            int_min: i64::from(min),
            int_max: i64::from(max),
            min: min as f32,
            max: max as f32,
            ..Default::default()
        }
    }

    /// Define a boolean toggle parameter.
    pub fn boolean(name: impl Into<String>, label: impl Into<String>, default_bool: bool) -> Self {
        Self {
            name: name.into(),
            label: label.into(),
            kind: EffectParamKind::Bool,
            default: if default_bool { 1.0 } else { 0.0 },
            default_bool,
            min: 0.0,
            max: 1.0,
            ..Default::default()
        }
    }

    /// Define a choice dropdown parameter.
    pub fn choice(
        name: impl Into<String>,
        label: impl Into<String>,
        choices: Vec<EffectChoiceOption>,
        default_choice: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            label: label.into(),
            kind: EffectParamKind::Choice,
            choices,
            default_choice: default_choice.into(),
            ..Default::default()
        }
    }

    /// Define a 2-d vector parameter with default and inclusive `[min, max]` component bounds.
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
            default: default_vec2[0],
            min,
            max,
            default_vec2,
            ..Default::default()
        }
    }

    /// Define a 3-d point parameter with default and inclusive `[min, max]` component bounds.
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
            default: default_vec3[0],
            min,
            max,
            default_vec3,
            ..Default::default()
        }
    }

    /// Define a 2-d size parameter with default `[w, h]` and inclusive `[min, max]` bounds.
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
            default: default_vec2[0],
            min,
            max,
            default_vec2,
            ..Default::default()
        }
    }

    /// Define an angle parameter in degrees with default and inclusive `[min, max]` range.
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
            display: EffectParamDisplay::AngleDial,
            unit: Some("deg".into()),
            default,
            min,
            max,
            ..Default::default()
        }
    }

    /// Define an RGBA8 colour parameter.
    pub fn color(
        name: impl Into<String>,
        label: impl Into<String>,
        default_color: [u8; 4],
    ) -> Self {
        Self {
            name: name.into(),
            label: label.into(),
            kind: EffectParamKind::Color,
            max: 1.0,
            default_color,
            ..Default::default()
        }
    }

    /// Define a 1-D curve parameter.
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
            ..Default::default()
        }
    }

    /// Define a colour gradient parameter.
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
            ..Default::default()
        }
    }

    /// Define a text string parameter.
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
            ..Default::default()
        }
    }

    /// Define a font family parameter.
    pub fn font(
        name: impl Into<String>,
        label: impl Into<String>,
        default_text: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            label: label.into(),
            kind: EffectParamKind::Font,
            default_text: default_text.into(),
            ..Default::default()
        }
    }

    /// Define a file path parameter.
    pub fn file(
        name: impl Into<String>,
        label: impl Into<String>,
        default_text: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            label: label.into(),
            kind: EffectParamKind::File,
            default_text: default_text.into(),
            ..Default::default()
        }
    }

    /// Define a layer reference parameter (another clip in the sequence).
    pub fn layer(name: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            label: label.into(),
            kind: EffectParamKind::Layer,
            ..Default::default()
        }
    }

    /// Set the scalar display style.
    pub fn with_display(mut self, display: EffectParamDisplay) -> Self {
        self.display = display;
        self
    }

    /// Set the soft range `[soft_min, soft_max]` for interactive slider scrubbing.
    pub fn with_soft_range(mut self, soft_min: f32, soft_max: f32) -> Self {
        self.soft_min = Some(soft_min);
        self.soft_max = Some(soft_max);
        self
    }

    /// Attach a unit suffix (for example `"px"`, `"deg"`, `"%"`).
    pub fn with_unit(mut self, unit: impl Into<String>) -> Self {
        self.unit = Some(unit.into());
        self
    }

    /// Attach an inspector tooltip.
    pub fn with_tooltip(mut self, tooltip: impl Into<String>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    /// Assign the parameter to a named group in [`EffectSpec::groups`].
    pub fn with_group(mut self, group: impl Into<String>) -> Self {
        self.group = Some(group.into());
        self
    }

    /// Show the parameter in the inspector only when `condition` holds.
    pub fn with_visible_when(mut self, condition: EffectVisibilityCondition) -> Self {
        self.visible_when = Some(condition);
        self
    }

    /// Number of `f32` elements this parameter occupies in the flattened float array.
    pub const fn flattened_floats(&self) -> usize {
        self.kind.flattened_floats()
    }
}

/// Specification of a visual effect or generator registered during `mount`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct EffectSpec {
    /// Namespaced id with at least one `.`, not already in the catalog.
    pub id: String,
    /// The name the Effect menu and Effect Controls show. Not empty.
    pub label: String,
    /// The Effect menu submenu. Empty lists the effect under Other. Generators
    /// are not listed in the Effect menu.
    #[serde(default)]
    pub category: String,
    /// Parameters in row order. Flattened values follow this order.
    #[serde(default)]
    pub params: Vec<EffectParamSpec>,
    /// Collapsible parameter sections, in display order.
    #[serde(default)]
    pub groups: Vec<EffectGroupSpec>,
    /// The entry draws a frame without reading an input.
    /// [`Host::register_generator`] sets it.
    #[serde(default)]
    pub generator: bool,
    /// The output at a time depends on frames before it, as feedback and trails
    /// do.
    #[serde(default)]
    pub stateful: bool,
    /// Pixel margins the effect draws past its layer's quad.
    #[serde(default)]
    pub bounds: EffectBounds,
    /// Whether the effect samples its input outside `0.0..=1.0`.
    #[serde(default)]
    pub reads_outside: bool,
    /// Whether the effect reads `aria.time`. An effect that does not is
    /// handed time 0, so its frames at equal parameters are equal.
    #[serde(default)]
    pub time_dependent: bool,
}

impl EffectSpec {
    /// Build an effect specification with `id` and display `label`.
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            category: String::new(),
            params: Vec::new(),
            groups: Vec::new(),
            generator: false,
            stateful: false,
            bounds: EffectBounds::default(),
            reads_outside: false,
            time_dependent: false,
        }
    }

    /// Set the Effect menu and Effects browser category.
    pub fn with_category(mut self, category: impl Into<String>) -> Self {
        self.category = category.into();
        self
    }

    /// Set the ordered parameter specifications.
    pub fn with_params(mut self, params: Vec<EffectParamSpec>) -> Self {
        self.params = params;
        self
    }

    /// Set the collapsible parameter groups.
    pub fn with_groups(mut self, groups: Vec<EffectGroupSpec>) -> Self {
        self.groups = groups;
        self
    }

    /// Mark whether the effect generates its own picture (`generator == true`).
    pub fn with_generator(mut self, generator: bool) -> Self {
        self.generator = generator;
        self
    }

    /// Mark whether the effect holds state across frames.
    pub fn with_stateful(mut self, stateful: bool) -> Self {
        self.stateful = stateful;
        self
    }

    /// Set the pixel margins the effect draws past its layer's quad.
    pub fn with_bounds(mut self, bounds: EffectBounds) -> Self {
        self.bounds = bounds;
        self
    }

    /// Mark whether the effect samples its input outside `0.0..=1.0`.
    pub fn with_reads_outside(mut self, reads_outside: bool) -> Self {
        self.reads_outside = reads_outside;
        self
    }

    /// Mark whether the effect reads `aria.time`.
    pub fn with_time_dependent(mut self, time_dependent: bool) -> Self {
        self.time_dependent = time_dependent;
        self
    }
}

/// Specification of a transition registered during `mount`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct TransitionSpec {
    /// Namespaced id with at least one `.`, not already in the catalog.
    pub id: String,
    /// The name the transition list shows. Not empty.
    pub label: String,
    /// Parameters in row order. A shader reads each through `param_<name>()`.
    #[serde(default)]
    pub params: Vec<EffectParamSpec>,
}

impl TransitionSpec {
    /// Build a transition specification with `id` and display `label`.
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            params: Vec::new(),
        }
    }

    /// Attach ordered transition parameters.
    pub fn with_params(mut self, params: Vec<EffectParamSpec>) -> Self {
        self.params = params;
        self
    }
}

/// IEEE-754 binary16 (`f16`) conversion helpers for [`CpuFrameIn`] and [`CpuFrameOut`].
pub mod f16 {
    /// IEEE-754 binary16 representation of `1.0`.
    pub const ONE: u16 = 0x3c00;

    /// Convert an IEEE-754 binary16 (`f16`) bit pattern to `f32`.
    #[inline]
    pub const fn to_f32(bits: u16) -> f32 {
        let sign = ((bits & 0x8000) as u32) << 16;
        let exp = ((bits >> 10) & 0x1f) as u32;
        let mant = (bits & 0x03ff) as u32;
        if exp == 0 {
            if mant == 0 {
                return f32::from_bits(sign);
            }
            let shift = mant.leading_zeros() - 21;
            let mant = (mant << shift) & 0x03ff;
            let exp = (127 - 15 + 1) - shift;
            f32::from_bits(sign | (exp << 23) | (mant << 13))
        } else if exp == 0x1f {
            f32::from_bits(sign | 0x7f80_0000 | (mant << 13))
        } else {
            f32::from_bits(sign | ((exp + (127 - 15)) << 23) | (mant << 13))
        }
    }

    /// Convert an `f32` to an IEEE-754 binary16 (`f16`) bit pattern (round to
    /// nearest, ties to even).
    #[inline]
    pub const fn from_f32(value: f32) -> u16 {
        let bits = value.to_bits();
        let sign = ((bits >> 16) & 0x8000) as u16;
        let exp = ((bits >> 23) & 0xff) as i32;
        let mant = bits & 0x007f_ffff;
        if exp == 0xff {
            return if mant == 0 {
                sign | 0x7c00
            } else {
                sign | 0x7e00 | ((mant >> 13) as u16)
            };
        }
        let half_exp = exp - 127 + 15;
        if half_exp >= 0x1f {
            return sign | 0x7c00;
        }
        if half_exp <= 0 {
            if half_exp < -10 {
                return sign;
            }
            let full_mant = mant | 0x0080_0000;
            let shift = (14 - half_exp) as u32;
            let round_bit = 1u32 << (shift - 1);
            let mask = round_bit - 1;
            let mut half_mant = (full_mant >> shift) as u16;
            if (full_mant & round_bit) != 0 && ((full_mant & mask) != 0 || (half_mant & 1) != 0) {
                half_mant += 1;
            }
            return sign | half_mant;
        }
        let mut half_mant = (mant >> 13) as u16;
        if (mant & 0x1000) != 0 && ((mant & 0x0fff) != 0 || (half_mant & 1) != 0) {
            half_mant += 1;
        }
        sign | (((half_exp as u16) << 10) + half_mant)
    }
}

/// Input frame and evaluated parameters passed to [`CpuEffect::process`].
///
/// Pixels are packed premultiplied `RGBA16F` (`[u16; 4]` IEEE-754 binary16 bit
/// patterns per pixel, top row first, with no row padding).
#[derive(Debug, Clone, Copy)]
pub struct CpuFrameIn<'a> {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pixels: &'a [u16],
    pub(crate) params: &'a [f32],
    pub(crate) params_json: &'a str,
    pub(crate) spec: Option<&'a EffectSpec>,
    pub(crate) time: f32,
}

impl<'a> CpuFrameIn<'a> {
    /// Frame width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Frame height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Packed premultiplied `RGBA16F` channel values (`width * height * 4`
    /// IEEE-754 binary16 `u16` elements, top row first).
    pub fn pixels(&self) -> &'a [u16] {
        self.pixels
    }

    /// Read pixel `(x, y)` as raw binary16 `[r, g, b, a]`, or `[0, 0, 0, 0]` out of bounds.
    pub fn pixel(&self, x: u32, y: u32) -> [u16; 4] {
        if x >= self.width || y >= self.height {
            return [0; 4];
        }
        let i = ((y as usize) * (self.width as usize) + (x as usize)) * 4;
        [
            self.pixels[i],
            self.pixels[i + 1],
            self.pixels[i + 2],
            self.pixels[i + 3],
        ]
    }

    /// Read pixel `(x, y)` converted from `RGBA16F` to `f32` `[r, g, b, a]`, or
    /// `[0.0, 0.0, 0.0, 0.0]` out of bounds.
    pub fn pixel_f32(&self, x: u32, y: u32) -> [f32; 4] {
        let [r, g, b, a] = self.pixel(x, y);
        [
            f16::to_f32(r),
            f16::to_f32(g),
            f16::to_f32(b),
            f16::to_f32(a),
        ]
    }

    /// Flattened `f32` parameter values in catalog order.
    pub fn params(&self) -> &'a [f32] {
        self.params
    }

    /// The effect instance being rendered, serialized as JSON, or empty when
    /// there is none.
    pub fn params_json(&self) -> &'a str {
        self.params_json
    }

    /// Flattened parameter float at `index`, if present.
    pub fn param_at(&self, index: usize) -> Option<f32> {
        self.params.get(index).copied()
    }

    fn param_slot(&self, name: &str) -> Option<(&'a EffectParamSpec, usize)> {
        let spec = self.spec?;
        let mut offset = 0usize;
        for p in &spec.params {
            if p.name == name {
                return Some((p, offset));
            }
            offset += p.flattened_floats();
        }
        None
    }

    /// Evaluated scalar or angle parameter `name`.
    pub fn scalar(&self, name: &str) -> Option<f32> {
        let (p, offset) = self.param_slot(name)?;
        if !matches!(
            p.kind,
            EffectParamKind::Scalar
                | EffectParamKind::Angle
                | EffectParamKind::Int
                | EffectParamKind::Choice
        ) {
            return None;
        }
        self.params.get(offset).copied().or(Some(p.default))
    }

    /// Evaluated angle parameter `name` in degrees.
    pub fn angle(&self, name: &str) -> Option<f32> {
        let (p, offset) = self.param_slot(name)?;
        if !matches!(p.kind, EffectParamKind::Angle | EffectParamKind::Scalar) {
            return None;
        }
        self.params.get(offset).copied().or(Some(p.default))
    }

    /// Evaluated integer parameter `name`.
    pub fn int(&self, name: &str) -> Option<i32> {
        let (p, offset) = self.param_slot(name)?;
        if !matches!(p.kind, EffectParamKind::Int | EffectParamKind::Scalar) {
            return None;
        }
        self.params
            .get(offset)
            .map(|v| v.round() as i32)
            .or(Some(p.default_int))
    }

    /// Evaluated boolean parameter `name`.
    pub fn boolean(&self, name: &str) -> Option<bool> {
        let (p, offset) = self.param_slot(name)?;
        if p.kind != EffectParamKind::Bool {
            return None;
        }
        self.params
            .get(offset)
            .map(|v| *v >= 0.5)
            .or(Some(p.default_bool))
    }

    /// Evaluated choice parameter `name` as its option id.
    pub fn choice(&self, name: &str) -> Option<String> {
        let (p, offset) = self.param_slot(name)?;
        if p.kind != EffectParamKind::Choice {
            return None;
        }
        if let Some(idx) = self.params.get(offset).map(|v| v.round().max(0.0) as usize)
            && let Some(opt) = p.choices.get(idx)
        {
            return Some(opt.id.clone());
        }
        if !p.default_choice.is_empty() {
            Some(p.default_choice.clone())
        } else {
            p.choices.first().map(|c| c.id.clone())
        }
    }

    /// Evaluated 2-d vector or size parameter `name`.
    pub fn vec2(&self, name: &str) -> Option<[f32; 2]> {
        let (p, offset) = self.param_slot(name)?;
        if !matches!(p.kind, EffectParamKind::Vec2 | EffectParamKind::Size) {
            return None;
        }
        let s = self.params.get(offset..offset + 2)?;
        Some([s[0], s[1]])
    }

    /// Evaluated 2-d size parameter `name`.
    pub fn size(&self, name: &str) -> Option<[f32; 2]> {
        self.vec2(name)
    }

    /// Evaluated 3-d point parameter `name`.
    pub fn point3(&self, name: &str) -> Option<[f32; 3]> {
        let (p, offset) = self.param_slot(name)?;
        if p.kind != EffectParamKind::Point3 {
            return None;
        }
        let s = self.params.get(offset..offset + 3)?;
        Some([s[0], s[1], s[2]])
    }

    /// Evaluated linear RGBA colour parameter `name`.
    pub fn color(&self, name: &str) -> Option<[f32; 4]> {
        let (p, offset) = self.param_slot(name)?;
        if p.kind != EffectParamKind::Color {
            return None;
        }
        let s = self.params.get(offset..offset + 4)?;
        Some([s[0], s[1], s[2], s[3]])
    }

    /// Evaluated 64-sample curve lookup table for parameter `name`.
    pub fn curve_lut(&self, name: &str) -> Option<[f32; EFFECT_CURVE_LUT_SAMPLES]> {
        let (p, offset) = self.param_slot(name)?;
        if p.kind != EffectParamKind::Curve {
            return None;
        }
        let s = self.params.get(offset..offset + EFFECT_CURVE_LUT_SAMPLES)?;
        let mut out = [0.0f32; EFFECT_CURVE_LUT_SAMPLES];
        out.copy_from_slice(s);
        Some(out)
    }

    /// Evaluated 64-stop RGBA gradient lookup table for parameter `name`.
    pub fn gradient_lut(&self, name: &str) -> Option<[[f32; 4]; EFFECT_GRADIENT_LUT_SAMPLES]> {
        let (p, offset) = self.param_slot(name)?;
        if p.kind != EffectParamKind::Gradient {
            return None;
        }
        let s = self
            .params
            .get(offset..offset + EFFECT_GRADIENT_LUT_SAMPLES * 4)?;
        let mut out = [[0.0f32; 4]; EFFECT_GRADIENT_LUT_SAMPLES];
        for (dst, chunk) in out.iter_mut().zip(s.chunks_exact(4)) {
            dst.copy_from_slice(chunk);
        }
        Some(out)
    }

    /// Evaluated text, font, or file parameter `name`: the instance's value, or
    /// the parameter's `default_text` when the instance sets none.
    pub fn text(&self, name: &str) -> Option<String> {
        let (p, _) = self.param_slot(name)?;
        if !matches!(
            p.kind,
            EffectParamKind::Text | EffectParamKind::Font | EffectParamKind::File
        ) {
            return None;
        }
        match self.instance_value("text_params", name) {
            Some(serde_json::Value::String(text)) => Some(text),
            _ => Some(p.default_text.clone()),
        }
    }

    /// Evaluated font family parameter `name`.
    pub fn font(&self, name: &str) -> Option<String> {
        self.text(name)
    }

    /// Evaluated file path parameter `name`.
    pub fn file(&self, name: &str) -> Option<String> {
        self.text(name)
    }

    /// The id of the clip layer parameter `name` references, or `None` when the
    /// parameter references no clip or is not a `Layer` parameter.
    pub fn layer(&self, name: &str) -> Option<u64> {
        let (p, _) = self.param_slot(name)?;
        if p.kind != EffectParamKind::Layer {
            return None;
        }
        self.instance_value("layer_params", name)?.as_u64()
    }

    /// The value the effect instance in [`Self::params_json`] stores for
    /// parameter `name` in its map `field`, which the host writes as a list of
    /// `[name, value]` pairs.
    fn instance_value(&self, field: &str, name: &str) -> Option<serde_json::Value> {
        if self.params_json.is_empty() {
            return None;
        }
        let mut instance: serde_json::Value = serde_json::from_str(self.params_json).ok()?;
        let serde_json::Value::Array(pairs) = instance.get_mut(field)?.take() else {
            return None;
        };
        pairs.into_iter().find_map(|pair| match pair {
            serde_json::Value::Array(mut entry) if entry.len() == 2 && entry[0] == name => {
                entry.pop()
            }
            _ => None,
        })
    }

    /// Clip-relative time in seconds.
    pub fn time(&self) -> f32 {
        self.time
    }
}

/// Output frame buffer written by [`CpuEffect::process`].
///
/// Pixels are packed premultiplied `RGBA16F` (`[u16; 4]` IEEE-754 binary16 bit
/// patterns per pixel, top row first, with no row padding).
#[derive(Debug)]
pub struct CpuFrameOut<'a> {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pixels: &'a mut [u16],
}

impl<'a> CpuFrameOut<'a> {
    /// Frame width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Frame height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Packed premultiplied `RGBA16F` channel values (`width * height * 4`
    /// IEEE-754 binary16 `u16` elements, top row first).
    pub fn pixels(&self) -> &[u16] {
        self.pixels
    }

    /// Mutable packed premultiplied `RGBA16F` channel values (`width * height * 4`
    /// IEEE-754 binary16 `u16` elements, top row first).
    pub fn pixels_mut(&mut self) -> &mut [u16] {
        self.pixels
    }

    /// Write raw binary16 `[r, g, b, a]` at `(x, y)` when inside the frame.
    pub fn set_pixel(&mut self, x: u32, y: u32, rgba: [u16; 4]) {
        if x >= self.width || y >= self.height {
            return;
        }
        let i = ((y as usize) * (self.width as usize) + (x as usize)) * 4;
        self.pixels[i..i + 4].copy_from_slice(&rgba);
    }

    /// Convert `[r, g, b, a]` from `f32` to `RGBA16F` and write it at `(x, y)`
    /// when inside the frame.
    pub fn set_pixel_f32(&mut self, x: u32, y: u32, rgba: [f32; 4]) {
        self.set_pixel(
            x,
            y,
            [
                f16::from_f32(rgba[0]),
                f16::from_f32(rgba[1]),
                f16::from_f32(rgba[2]),
                f16::from_f32(rgba[3]),
            ],
        );
    }

    /// Fill every pixel of the output frame with raw binary16 `rgba`.
    pub fn fill(&mut self, rgba: [u16; 4]) {
        for px in self.pixels.chunks_exact_mut(4) {
            px.copy_from_slice(&rgba);
        }
    }

    /// Convert `rgba` from `f32` to `RGBA16F` and fill every pixel of the output frame.
    pub fn fill_f32(&mut self, rgba: [f32; 4]) {
        self.fill([
            f16::from_f32(rgba[0]),
            f16::from_f32(rgba[1]),
            f16::from_f32(rgba[2]),
            f16::from_f32(rgba[3]),
        ]);
    }
}

/// CPU-evaluated visual effect or generator registered with
/// [`Host::register_cpu_effect`] or [`Host::register_generator`].
pub trait CpuEffect: Send + Sync + 'static {
    /// Process `input` into `output`.
    fn process(
        &self,
        host: &Host,
        input: &CpuFrameIn<'_>,
        output: &mut CpuFrameOut<'_>,
    ) -> Result<(), String>;
}

impl<F> CpuEffect for F
where
    F: Fn(&Host, &CpuFrameIn<'_>, &mut CpuFrameOut<'_>) -> Result<(), String>
        + Send
        + Sync
        + 'static,
{
    fn process(
        &self,
        host: &Host,
        input: &CpuFrameIn<'_>,
        output: &mut CpuFrameOut<'_>,
    ) -> Result<(), String> {
        self(host, input, output)
    }
}

/// A label colour: a clip type's timeline tint and a marker's colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MarkerColor {
    /// The Teal label.
    Teal,
    /// The Blue label.
    Blue,
    /// The Purple label.
    Purple,
    /// The Magenta label.
    Pink,
    /// The Rose label.
    Red,
    /// The Mango label.
    Orange,
    /// The Yellow label.
    Yellow,
    /// The Green label.
    Green,
}

/// Specification of a generator-backed clip type registered during `mount`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClipTypeSpec {
    /// Namespaced id with at least one `.`: at most 128 bytes, no space or
    /// control character, not already in the catalog.
    pub id: String,
    /// The name in the Generators tab, the palette, Effect Controls, and on the
    /// clip: 1 to 64 bytes.
    pub label: String,
    /// Id of the generator that draws the type. Not empty.
    pub generator: String,
    /// The timeline label colour.
    pub colour: MarkerColor,
    /// SVG source of the icon drawn before the clip's name, or `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_svg: Option<String>,
    /// Names of the generator's `Vec2` parameters drawn as Viewer handles: at
    /// most 8, each once.
    #[serde(default)]
    pub handles: Vec<String>,
}

impl ClipTypeSpec {
    /// Build a clip type specification with no icon and no viewer handles.
    pub fn new(
        id: impl Into<String>,
        label: impl Into<String>,
        generator: impl Into<String>,
        colour: MarkerColor,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            generator: generator.into(),
            colour,
            icon_svg: None,
            handles: Vec::new(),
        }
    }

    /// Attach an SVG icon string.
    pub fn with_icon_svg(mut self, svg: impl Into<String>) -> Self {
        self.icon_svg = Some(svg.into());
        self
    }

    /// Declare vec2 generator parameter names drawn as drag handles in the Viewer.
    pub fn with_handles(mut self, handles: Vec<String>) -> Self {
        self.handles = handles;
        self
    }
}

/// Value a clip property holds at one frame.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParamValue {
    /// One value.
    Scalar(f32),
    /// `[x, y]`.
    Vec2([f32; 2]),
    /// `[x, y, z]`.
    Vec3([f32; 3]),
    /// RGBA8 `[r, g, b, a]`.
    Color([u8; 4]),
    /// `[x, y, width, height]`: the left edge, the top edge, the width, and the
    /// height.
    Rect([f32; 4]),
}

impl ParamValue {
    pub(crate) fn from_raw(raw: AriaParamValue) -> Result<Self, String> {
        match raw.kind {
            ARIA_PARAM_SCALAR => Ok(Self::Scalar(raw.x)),
            ARIA_PARAM_VEC2 => Ok(Self::Vec2([raw.x, raw.y])),
            ARIA_PARAM_VEC3 => Ok(Self::Vec3([raw.x, raw.y, raw.z])),
            ARIA_PARAM_COLOR => Ok(Self::Color([raw.r, raw.g, raw.b, raw.a])),
            ARIA_PARAM_RECT => Ok(Self::Rect([raw.x, raw.y, raw.z, raw.w])),
            other => Err(format!("unknown AriaParamValue kind {other}")),
        }
    }

    pub(crate) fn to_raw(self) -> AriaParamValue {
        match self {
            Self::Scalar(x) => AriaParamValue {
                kind: ARIA_PARAM_SCALAR,
                x,
                ..AriaParamValue::default()
            },
            Self::Vec2([x, y]) => AriaParamValue {
                kind: ARIA_PARAM_VEC2,
                x,
                y,
                ..AriaParamValue::default()
            },
            Self::Vec3([x, y, z]) => AriaParamValue {
                kind: ARIA_PARAM_VEC3,
                x,
                y,
                z,
                ..AriaParamValue::default()
            },
            Self::Color([r, g, b, a]) => AriaParamValue {
                kind: ARIA_PARAM_COLOR,
                r,
                g,
                b,
                a,
                ..AriaParamValue::default()
            },
            Self::Rect([x, y, z, w]) => AriaParamValue {
                kind: ARIA_PARAM_RECT,
                x,
                y,
                z,
                w,
                ..AriaParamValue::default()
            },
        }
    }
}

/// Frame variables passed to [`DriverLanguage::evaluate`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DriverVars {
    /// Sequence seconds being evaluated.
    pub time: f64,
    /// Seconds since the clip's first frame.
    pub clip_time: f64,
    /// Sequence frames per second.
    pub frame_rate: f64,
    /// Sequence length in seconds.
    pub duration: f64,
    /// Canvas `[width, height]` in pixels.
    pub canvas: [u32; 2],
    /// The property's keyframed value at `time`.
    pub value: ParamValue,
    /// A number fixed per clip and property, for seeded noise.
    pub seed: u64,
}

/// Specification of a parameter-driver expression language registered during `mount`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DriverLanguageSpec {
    /// ASCII letters, digits, `.`, `-` and `_`. A second registration in one
    /// host is refused.
    pub id: String,
    /// The language's display name.
    pub name: String,
}

impl DriverLanguageSpec {
    /// Build a driver language specification with `id` and display `name`.
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
        }
    }
}

/// Parameter-driver expression evaluator registered with [`Host::register_driver_language`].
pub trait DriverLanguage: Send + Sync + 'static {
    /// Evaluate `expr` for `vars` and return the resulting property value.
    fn evaluate(&self, host: &Host, expr: &str, vars: &DriverVars) -> Result<ParamValue, String>;
}

impl<F> DriverLanguage for F
where
    F: Fn(&Host, &str, &DriverVars) -> Result<ParamValue, String> + Send + Sync + 'static,
{
    fn evaluate(&self, host: &Host, expr: &str, vars: &DriverVars) -> Result<ParamValue, String> {
        self(host, expr, vars)
    }
}

/// Pane that receives keyboard focus when a workspace is applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkspaceFocus {
    /// The Project panel.
    Media,
    /// The Viewer.
    Preview,
    /// The Timeline.
    Timeline,
    /// Effect Controls.
    Inspector,
    /// The assistant.
    Assistant,
    /// The mixer.
    Mixer,
}

/// Pane arrangement a workspace restores.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkspacePanes {
    /// Sidebar width in pixels, above 0 and at most 16384. `None` keeps the
    /// Editing width.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sidebar_width: Option<f32>,
    /// Collapses the sidebar. `None` shows it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sidebar_collapsed: Option<bool>,
    /// Inspector width in pixels, above 0 and at most 16384. `None` keeps the
    /// Editing width.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inspector_width: Option<f32>,
    /// Collapses the inspector. `None` shows it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inspector_collapsed: Option<bool>,
    /// Timeline height in pixels, above 0 and at most 16384. `None` keeps the
    /// Editing height.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeline_height: Option<f32>,
    /// Opens the scope strip. `None` leaves it closed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scopes: Option<bool>,
    /// The pane that takes the keyboard. `None` selects the Timeline.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub focus: Option<WorkspaceFocus>,
    /// A serialized `DockTree` JSON string. `None` derives the layout from the
    /// pane sizes and visibility.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tree: Option<String>,
}

/// Specification of a named workspace registered during `mount`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceSpec {
    /// Unique within the plugin: 1 to 64 ASCII letters, digits, `-`, `_` and
    /// `.`, not starting with `.`.
    pub id: String,
    /// The Window menu row and the Settings row: one line of at most 64 bytes.
    pub name: String,
    /// The arrangement applied.
    #[serde(default)]
    pub panes: WorkspacePanes,
}

impl WorkspaceSpec {
    /// Build a workspace specification with `id` and display `name`.
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            panes: WorkspacePanes::default(),
        }
    }

    /// Set the pane arrangement for the workspace.
    pub fn with_panes(mut self, panes: WorkspacePanes) -> Self {
        self.panes = panes;
        self
    }
}

/// Specification of an animation and effect preset (`.ariapreset`) registered during `mount`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PresetSpec {
    /// The preset format version: 1.
    pub version: u32,
    /// Display name, trimmed, at most 128 characters.
    pub name: String,
    /// Effect instances, at most 64.
    #[serde(default)]
    pub effects: Vec<serde_json::Value>,
    /// Audio effect instances, at most 64.
    #[serde(default)]
    pub audio_effects: Vec<serde_json::Value>,
    /// `{ "id": <clip property row id>, "curve": <constant or keyframes> }`
    /// objects, at most 256, each curve at most 4096 keyframes.
    #[serde(default)]
    pub properties: Vec<serde_json::Value>,
}

impl PresetSpec {
    /// Build a version-1 preset with `name`.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            version: 1,
            name: name.into(),
            effects: Vec::new(),
            audio_effects: Vec::new(),
            properties: Vec::new(),
        }
    }

    /// Append a visual effect instance with default parameters.
    pub fn with_effect(mut self, effect_id: impl Into<String>) -> Self {
        self.effects.push(json!({ "effect_id": effect_id.into() }));
        self
    }

    /// Append a constant scalar property curve (`id` from `clip_param_catalog`, e.g. `"opacity"`).
    pub fn with_scalar_property(mut self, id: impl Into<String>, value: f32) -> Self {
        self.properties.push(json!({
            "id": id.into(),
            "curve": {
                "type": "Constant",
                "value": { "scalar": value }
            }
        }));
        self
    }
}

/// What target an audio analyzer operates on and which menu offers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnalyzerScope {
    /// The selected clip's sound, offered in Clip > Analyze.
    #[default]
    Clip,
    /// The whole sequence mix, offered in Sequence > Analyze.
    Sequence,
    /// The sequence mix from In to Out, or the whole sequence when no range is
    /// marked, offered in Sequence > Analyze.
    InOut,
}

/// Audio analyzer registration metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnalyzerSpec {
    /// Namespaced id with at least one `.`: ASCII letters, digits, `.`, `_` and
    /// `-`.
    pub id: String,
    /// The menu row, palette row, job title, and undo step name. Not blank.
    pub label: String,
    /// An optional grouping label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// What the analyzer reads and which menu lists it.
    #[serde(default)]
    pub scope: AnalyzerScope,
}

impl AnalyzerSpec {
    /// Build an analyzer specification for `id`, `label`, and `scope`.
    pub fn new(id: impl Into<String>, label: impl Into<String>, scope: AnalyzerScope) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            category: None,
            scope,
        }
    }

    /// Attach an optional grouping category.
    #[must_use]
    pub fn with_category(mut self, category: impl Into<String>) -> Self {
        let category = category.into();
        self.category = (!category.trim().is_empty()).then_some(category);
        self
    }
}

/// Ruler or clip marker returned by an [`AnalyzerRun`], in sample frames at the mix rate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnalyzerMarker {
    /// Start position in sample frames.
    pub frame: i64,
    /// Span in sample frames, 0 for a point marker. A span is clamped to the
    /// clip or sequence end.
    #[serde(default)]
    pub duration_frames: i64,
    /// The label beside the marker flag.
    pub name: String,
    /// The marker flag colour.
    pub color: MarkerColor,
}

/// Labelled sample-frame span (such as a detected silence region) returned by an [`AnalyzerRun`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnalyzerRange {
    /// First sample frame of the span.
    pub start: i64,
    /// The sample frame after the span.
    pub end: i64,
    /// Name of the segmentation marker the range becomes. Empty names it
    /// Silence.
    pub label: String,
}

/// Integrated loudness summary returned by an [`AnalyzerRun`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AnalyzerLoudness {
    /// Integrated programme loudness in LUFS (EBU R128, ITU-R BS.1770).
    pub integrated_lufs: f64,
    /// Peak level across channels in dBFS.
    pub true_peak_db: f64,
    /// Loudness range (LRA) in LU.
    pub range_lu: f64,
}

/// Output returned when an [`AnalyzerRun`] finishes.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AnalyzerResult {
    /// Beat positions in sample frames, set as the clip's beat grid. Clip scope
    /// only.
    #[serde(default)]
    pub beats: Vec<i64>,
    /// Markers attached to the clip for clip scope, and sequence markers
    /// otherwise.
    #[serde(default)]
    pub markers: Vec<AnalyzerMarker>,
    /// Spans that become yellow segmentation markers with a duration.
    #[serde(default)]
    pub ranges: Vec<AnalyzerRange>,
    /// A loudness measurement, shown in the job result and on the status line.
    /// It adds no undo step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loudness: Option<AnalyzerLoudness>,
}

/// One running audio analysis session opened by an [`Analyzer`].
pub trait AnalyzerRun: Send {
    /// Feed interleaved `f32` audio samples (`frames * channels`).
    fn push(&mut self, samples: &[f32]) -> Result<(), String>;

    /// Finish analysis and return the [`AnalyzerResult`].
    fn finish(self: Box<Self>) -> Result<AnalyzerResult, String>;
}

/// Audio analyzer registered with [`Host::register_analyzer`](crate::Host::register_analyzer).
pub trait Analyzer: Send + Sync + 'static {
    /// Open a new [`AnalyzerRun`] session for `sample_rate` Hz and `channels` channels.
    fn open(
        &self,
        host: &crate::Host,
        sample_rate: u32,
        channels: u16,
    ) -> Result<Box<dyn AnalyzerRun>, String>;
}
