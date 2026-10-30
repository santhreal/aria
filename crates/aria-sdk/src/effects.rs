//! Visual effects, generators, and transitions registered by native plugins,
//! and the parameters they declare.

use serde::{Deserialize, Serialize};

use crate::EffectBounds;

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
    /// UTF-8 text, read with [`CpuFrameIn::text`](crate::CpuFrameIn::text). Shaders do not read it.
    Text,
    /// A font family name, read with [`CpuFrameIn::font`](crate::CpuFrameIn::font). Shaders do not read
    /// it.
    Font,
    /// A file path, read with [`CpuFrameIn::file`](crate::CpuFrameIn::file). Shaders do not read it.
    File,
    /// A clip on the timeline. A pass that lists it as an input reads that
    /// clip's frame. A CPU effect reads the clip id with [`CpuFrameIn::layer`](crate::CpuFrameIn::layer).
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

/// How the inspector prints a number parameter. A number row draws a value
/// track between the parameter's bounds whatever its display.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectParamDisplay {
    /// The value and its unit.
    #[default]
    Slider,
    /// The value in degrees, to a tenth.
    AngleDial,
    /// The stored fraction as a whole percent.
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    /// Highest value of the kinds `min` bounds. Not below `min`. `1.0` when
    /// unset.
    #[serde(default = "default_max")]
    pub max: f32,
    /// Default for `Color`: RGBA `[r, g, b, a]` in `0..=255`.
    #[serde(default)]
    pub default_color: [u8; 4],
    /// Default for `Vec2` and `Size`.
    #[serde(default)]
    pub default_vec2: [f32; 2],
    /// How a `Scalar` row prints its value.
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

impl Default for EffectParamSpec {
    fn default() -> Self {
        Self {
            name: String::new(),
            label: String::new(),
            kind: EffectParamKind::default(),
            default: 0.0,
            min: 0.0,
            max: DEFAULT_MAX,
            default_color: [0; 4],
            default_vec2: [0.0; 2],
            display: EffectParamDisplay::default(),
            soft_min: None,
            soft_max: None,
            unit: None,
            tooltip: None,
            group: None,
            visible_when: None,
            choices: Vec::new(),
            default_int: 0,
            int_min: 0,
            int_max: 0,
            default_bool: false,
            default_choice: String::new(),
            default_vec3: [0.0; 3],
            default_curve: Vec::new(),
            default_gradient: Vec::new(),
            default_text: String::new(),
        }
    }
}

/// The `max` of a parameter that sets none: the top of a unit range, as on the
/// host.
const DEFAULT_MAX: f32 = 1.0;

/// [`DEFAULT_MAX`], for a parameter whose JSON omits `max`.
fn default_max() -> f32 {
    DEFAULT_MAX
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
    /// [`Host::register_generator`](crate::Host::register_generator) sets it.
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
