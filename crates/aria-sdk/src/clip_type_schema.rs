//! The fields of a clip type that draws itself, registered with
//! [`Host::define_clip_type`](crate::Host::define_clip_type) or
//! [`Host::define_cpu_clip_type`](crate::Host::define_cpu_clip_type).

use serde::{Deserialize, Serialize};

use crate::{ClipTypeSpec, EffectGroupSpec, EffectParamSpec, EffectSpec};

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
