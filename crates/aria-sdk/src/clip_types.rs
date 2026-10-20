//! Generator-backed clip types: the [`ClipTypeSpec`] a plugin registers, the
//! [`ClipTypeSchema`] of a type whose clips store their own fields, and the
//! label colours of clip types and markers.

use serde::{Deserialize, Serialize};

use crate::{EffectGroupSpec, EffectParamSpec, EffectSpec};

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

/// The fields each clip of a type stores, as effect parameters of any kind.
///
/// A clip of the type stores its field values in the project, keyframes them
/// where the kind does, edits each through one undo step, and shows them as
/// its rows in the inspector and on the timeline. The host registers a
/// generator under the type's id whose parameters are the fields.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ClipTypeSchema {
    /// The fields, in row order. A handle names a `Vec2` field.
    #[serde(default)]
    pub fields: Vec<EffectParamSpec>,
    /// Collapsible groups that fields name in `group`.
    #[serde(default)]
    pub groups: Vec<EffectGroupSpec>,
    /// Whether the picture changes with time when the fields do not. A type
    /// that does not is drawn at time 0.
    #[serde(default)]
    pub time_dependent: bool,
}

impl ClipTypeSchema {
    /// A schema of `fields`, with no groups, whose picture changes only with
    /// its fields.
    pub fn new(fields: Vec<EffectParamSpec>) -> Self {
        Self {
            fields,
            groups: Vec::new(),
            time_dependent: false,
        }
    }

    /// Set the collapsible field groups.
    pub fn with_groups(mut self, groups: Vec<EffectGroupSpec>) -> Self {
        self.groups = groups;
        self
    }

    /// Mark whether the picture changes with time when the fields do not.
    pub fn with_time_dependent(mut self, time_dependent: bool) -> Self {
        self.time_dependent = time_dependent;
        self
    }

    /// The generator the host registers for the type `spec`: what a
    /// [`CpuFrameIn`](crate::CpuFrameIn) reads fields by name from.
    pub(crate) fn generator_spec(&self, spec: &ClipTypeSpec) -> EffectSpec {
        EffectSpec::new(spec.id.as_str(), spec.label.as_str())
            .with_params(self.fields.clone())
            .with_groups(self.groups.clone())
            .with_generator(true)
            .with_time_dependent(self.time_dependent)
    }
}
