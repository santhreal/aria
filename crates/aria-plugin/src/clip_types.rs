//! Clip types: a named kind of clip whose picture a generator draws.
//!
//! The catalog is [`aria_models::clip_type_catalog`], so the editor's
//! projection reads a clip's type name and colour without a plugin context.
//! This module registers into it on a plugin's behalf: the entry is listed
//! under the plugin's contributions and removed when the plugin unmounts, and
//! an icon is parsed and rewritten here, so the catalog never holds SVG text
//! that has not been through [`CommandIcon::from_svg`].
//!
//! A type is registered in one of two forms. [`PluginContext::register_clip_type`]
//! lists a type drawn by a generator registered on its own.
//! [`PluginContext::define_clip_type`] takes the type's fields and its
//! renderer together: the host registers a generator under the type's id whose
//! parameters are the fields, and one registration holds both entries.

use std::sync::Arc;

use aria_compositor::{
    CpuEffect, CpuEffectDefinition, CpuFrameIn, CpuFrameOut, EffectDefinition, EffectParams,
    EffectRender,
};
use aria_models::{
    ClipTypeIcon, ClipTypeSpec, EffectGroupSpec, EffectParamSpec, EffectSpec, GraphPass,
};

use crate::command_icon::CommandIcon;
use crate::context::PluginContext;
use crate::error::PluginError;
use crate::registration::{Contribution, ContributionKind, Registration};

impl CommandIcon {
    /// This icon as a clip type's glyph.
    pub fn to_clip_type_icon(&self) -> ClipTypeIcon {
        ClipTypeIcon {
            svg: std::sync::Arc::from(self.svg()),
            key: self.key(),
        }
    }
}

/// A clip type with the fields each of its clips stores and what draws it.
///
/// Each field is an effect parameter of any kind. A clip of the type stores
/// its field values in the project, keyframes them where the kind does, edits
/// each through one undo step, and shows them as its rows in the inspector's
/// Effects block and in the timeline's Effects group.
#[derive(Clone)]
pub struct ClipTypeDefinition {
    /// The type. `generator` is empty or the type's own id; the host sets it
    /// to the id.
    pub spec: ClipTypeSpec,
    /// The fields, in row order. A handle names a `Vec2` field.
    pub fields: Vec<EffectParamSpec>,
    /// Collapsible groups that fields name in `group`.
    pub groups: Vec<EffectGroupSpec>,
    /// Whether the picture changes with time when the fields do not. The
    /// renderer of a type that does not is handed time 0.
    pub time_dependent: bool,
    /// What draws the picture.
    pub render: ClipTypeRender,
}

impl std::fmt::Debug for ClipTypeDefinition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClipTypeDefinition")
            .field("spec", &self.spec)
            .field("fields", &self.fields)
            .field("groups", &self.groups)
            .field("time_dependent", &self.time_dependent)
            .field("render", &self.render)
            .finish()
    }
}

impl ClipTypeDefinition {
    /// A type with `fields`, no groups, drawn by `render`, whose picture
    /// changes only with its fields.
    pub fn new(
        spec: ClipTypeSpec,
        fields: Vec<EffectParamSpec>,
        render: impl Into<ClipTypeRender>,
    ) -> Self {
        Self {
            spec,
            fields,
            groups: Vec::new(),
            time_dependent: false,
            render: render.into(),
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
}

/// What draws a defined clip type.
#[derive(Clone)]
pub enum ClipTypeRender {
    /// WGSL passes reading the fields as `aria.params`, on the terms of an
    /// [`EffectDefinition`] whose spec is a generator.
    Passes(Vec<GraphPass>),
    /// A CPU callback handed a transparent frame of the clip's size.
    Cpu(Arc<dyn CpuEffect>),
}

impl std::fmt::Debug for ClipTypeRender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Passes(passes) => f.debug_tuple("Passes").field(passes).finish(),
            Self::Cpu(_) => f.write_str("Cpu(..)"),
        }
    }
}

impl ClipTypeRender {
    /// A CPU renderer: `render` receives the clip's field values, the time in
    /// seconds into the clip, and a transparent frame of the clip's size.
    pub fn cpu(render: impl ClipRender + 'static) -> Self {
        Self::Cpu(Arc::new(Drawn(render)))
    }
}

impl From<Vec<GraphPass>> for ClipTypeRender {
    fn from(passes: Vec<GraphPass>) -> Self {
        Self::Passes(passes)
    }
}

/// What draws a clip type on the CPU.
///
/// `frame` holds premultiplied `RGBA16F` pixels, starts transparent, and has
/// the clip's layer size. Returning `Err(reason)` leaves the clip transparent
/// for that frame and reports a recoverable fault; a panic fails the plugin
/// for the session.
pub trait ClipRender: Send + Sync {
    /// Draw the clip with its `fields` at `time` seconds into the clip.
    fn render(
        &self,
        fields: &EffectParams<'_>,
        time: f32,
        frame: &mut CpuFrameOut<'_>,
    ) -> Result<(), String>;
}

impl<F> ClipRender for F
where
    F: Fn(&EffectParams<'_>, f32, &mut CpuFrameOut<'_>) -> Result<(), String> + Send + Sync,
{
    fn render(
        &self,
        fields: &EffectParams<'_>,
        time: f32,
        frame: &mut CpuFrameOut<'_>,
    ) -> Result<(), String> {
        self(fields, time, frame)
    }
}

/// A [`ClipRender`] as the generator effect the compositor runs.
struct Drawn<R>(R);

impl<R: ClipRender> CpuEffect for Drawn<R> {
    fn process(
        &self,
        _input: CpuFrameIn<'_>,
        output: &mut CpuFrameOut<'_>,
        params: &EffectParams<'_>,
        time: f32,
    ) -> Result<(), String> {
        self.0.render(params, time, output)
    }
}

/// Parse and rewrite `spec.icon`, refusing one [`CommandIcon::from_svg`] does
/// not read.
fn rewrite_icon(spec: &mut ClipTypeSpec) -> Result<(), PluginError> {
    if let Some(icon) = &spec.icon {
        let parsed = CommandIcon::from_svg(icon.svg.as_bytes())
            .map_err(|reason| PluginError::ClipTypeRefused(format!("{}: {reason}", spec.id)))?;
        spec.icon = Some(parsed.to_clip_type_icon());
    }
    Ok(())
}

impl PluginContext {
    /// Add `spec` to the clip type catalog. The Generators tab and the palette
    /// offer it, and placing it puts a transparent solid on the timeline with
    /// `spec.generator` first in its chain and the type's id on the clip, as one
    /// undo step.
    ///
    /// Held on the terms of [`register_effect`](Self::register_effect). A
    /// `spec.icon` is parsed again and its key recomputed. Refused when the icon
    /// is not an SVG [`CommandIcon::from_svg`] reads, or the catalog refuses the
    /// spec (see [`aria_models::register_clip_type`]). The generator may be
    /// registered later: a type whose generator is missing is listed and cannot
    /// be placed until it is registered.
    pub fn register_clip_type(&self, mut spec: ClipTypeSpec) -> Result<Registration, PluginError> {
        rewrite_icon(&mut spec)?;
        let contribution = Contribution::new(ContributionKind::ClipType, spec.id.clone());
        let (entry, _) =
            aria_models::register_clip_type(spec).map_err(PluginError::ClipTypeRefused)?;
        Ok(self.retain_if_mounting(Registration::holding(contribution, entry)))
    }

    /// Add a clip type that declares its fields and draws itself.
    ///
    /// The host registers a generator under the type's id, named after the
    /// type, whose parameters are `definition.fields`, drawn by
    /// `definition.render`; the type names it as its generator. The returned
    /// registration holds both on the terms of
    /// [`register_effect`](Self::register_effect), and the Plugins list shows
    /// one clip type. The Generators tab and the palette list the type, not
    /// its generator.
    ///
    /// Refused as [`PluginError::ClipTypeRefused`] when `spec.generator` names
    /// another effect, a handle names no `Vec2` field, or the type is refused
    /// on the terms of [`register_clip_type`](Self::register_clip_type); as
    /// [`PluginError::RegisterRefused`] when the fields or the passes are
    /// refused on the terms of [`register_effect`](Self::register_effect).
    pub fn define_clip_type(
        &self,
        definition: ClipTypeDefinition,
    ) -> Result<Registration, PluginError> {
        let ClipTypeDefinition {
            mut spec,
            fields,
            groups,
            time_dependent,
            render,
        } = definition;
        if !spec.generator.is_empty() && spec.generator != spec.id {
            return Err(PluginError::ClipTypeRefused(format!(
                "{}: a clip type with its own fields draws itself; its generator is empty or `{}`, not `{}`",
                spec.id, spec.id, spec.generator
            )));
        }
        spec.generator.clone_from(&spec.id);
        rewrite_icon(&mut spec)?;
        let effect = EffectSpec {
            id: spec.id.clone(),
            label: spec.label.clone(),
            params: fields,
            groups,
            generator: true,
            time_dependent,
            ..EffectSpec::default()
        };
        let (listed, entry) =
            aria_models::register_clip_type(spec).map_err(PluginError::ClipTypeRefused)?;
        let render = match render {
            ClipTypeRender::Passes(passes) => EffectRender::Shader(EffectDefinition {
                spec: effect,
                passes,
            }),
            ClipTypeRender::Cpu(draw) => EffectRender::Cpu(CpuEffectDefinition {
                spec: effect,
                effect: draw,
            }),
        };
        let drawn = aria_compositor::register_effect_with_sinks(
            render,
            Some(self.effect_fault_sink()),
            self.frame_cost_sink(),
        )?;
        if let Some(reason) = entry.unavailable() {
            return Err(PluginError::ClipTypeRefused(format!(
                "{}: {reason}",
                entry.id
            )));
        }
        let contribution = Contribution::new(ContributionKind::ClipType, entry.id.clone());
        Ok(self.retain_if_mounting(Registration::holding(contribution, (listed, drawn))))
    }
}

#[cfg(test)]
#[path = "clip_types_tests.rs"]
mod tests;
