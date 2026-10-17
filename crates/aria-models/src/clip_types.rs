//! Clip types a plugin registers: a named kind of clip whose picture a generator
//! effect draws.
//!
//! A clip of a registered type is a transparent solid with the type's generator
//! first in its effect chain, and [`crate::Clip::clip_type`] holding the type's
//! id. The generator's parameters are the type's data: they are saved with the
//! clip as effect parameters, keyframed like any other, and listed as the type's
//! property rows. The type adds a name, a colour and an icon for the timeline,
//! and the vec2 parameters the Viewer draws as drag handles.
//!
//! The catalog holds no shipped entries: text, shape, adjustment and multicam
//! clips are model variants of their own. A clip whose type is not registered
//! keeps its id and draws what its effect chain draws.

use std::sync::Arc;

use crate::MarkerColor;

/// The most bytes a clip type id may have.
pub const CLIP_TYPE_ID_MAX_BYTES: usize = 128;
/// The most bytes a clip type name may have.
pub const CLIP_TYPE_NAME_MAX_BYTES: usize = 64;
/// The most Viewer handles one clip type may declare.
pub const CLIP_TYPE_MAX_HANDLES: usize = 8;

/// A clip type's glyph: SVG text the registering layer has already parsed and
/// rewritten, and a key that is equal for equal text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipTypeIcon {
    pub svg: Arc<str>,
    pub key: u64,
}

/// One registered clip type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipTypeSpec {
    /// The id saved on each clip of the type. Needs a `.` namespace, such as
    /// `com.example.lower-third`.
    pub id: String,
    /// The name shown in the Generators tab, the palette, the inspector and on
    /// the clip.
    pub label: String,
    /// The effect, registered with `generator: true`, that draws the clip.
    /// Resolved when a clip is placed or drawn, so the type may be registered
    /// before its generator.
    pub generator: String,
    /// The tint of the type's clips on the timeline when the clip has no label
    /// colour of its own.
    pub colour: MarkerColor,
    /// The glyph drawn on the clip and beside the type's name.
    pub icon: Option<ClipTypeIcon>,
    /// Names of the generator's vec2 parameters the Viewer draws as handles. A
    /// value is a point on the clip's layer: `[0, 0]` is the top left corner and
    /// `[1, 1]` the bottom right.
    pub handles: Vec<String>,
}

impl ClipTypeSpec {
    /// A type with no icon and no handles.
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
            icon: None,
            handles: Vec::new(),
        }
    }

    /// Refuse a spec the catalog would refuse past its id namespace and
    /// uniqueness: an id that is too long or holds a space, a blank or long
    /// name, no generator, or an empty, repeated or excess handle.
    pub fn check(&self) -> Result<(), String> {
        check_clip_type(self)
    }

    /// Why the generator cannot draw this type right now, or `None` when it
    /// can: the effect is registered as a generator and every handle names one
    /// of its vec2 parameters.
    pub fn unavailable(&self) -> Option<String> {
        let Some(effect) = crate::effect_spec(&self.generator) else {
            return Some(format!(
                "the generator `{}` is not registered",
                self.generator
            ));
        };
        if !effect.generator {
            return Some(format!(
                "`{}` is an effect, not a generator",
                self.generator
            ));
        }
        self.handles.iter().find_map(|handle| {
            let vec2 = effect
                .params
                .iter()
                .any(|param| param.name == *handle && param.kind == crate::EffectParamKind::Vec2);
            (!vec2).then(|| {
                format!(
                    "the handle `{handle}` names no vec2 parameter of `{}`",
                    self.generator
                )
            })
        })
    }
}

/// Rules a clip type follows before it is listed, beyond the id and label rules
/// every catalog entry follows.
pub(crate) fn check_clip_type(spec: &ClipTypeSpec) -> Result<(), String> {
    let id = &spec.id;
    if id.len() > CLIP_TYPE_ID_MAX_BYTES {
        return Err(format!(
            "{id}: a clip type id is limited to {CLIP_TYPE_ID_MAX_BYTES} bytes"
        ));
    }
    if id.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(format!(
            "{id}: a clip type id has no spaces or control characters"
        ));
    }
    if spec.label.trim().is_empty() {
        return Err(format!("{id} has no name"));
    }
    if spec.label.len() > CLIP_TYPE_NAME_MAX_BYTES {
        return Err(format!(
            "{id}: the name is limited to {CLIP_TYPE_NAME_MAX_BYTES} bytes"
        ));
    }
    if spec.generator.trim().is_empty() {
        return Err(format!("{id} names no generator"));
    }
    if spec.handles.len() > CLIP_TYPE_MAX_HANDLES {
        return Err(format!(
            "{id} declares {} handles; the limit is {CLIP_TYPE_MAX_HANDLES}",
            spec.handles.len()
        ));
    }
    for (index, handle) in spec.handles.iter().enumerate() {
        if handle.is_empty() {
            return Err(format!("{id}: a handle names no parameter"));
        }
        if spec.handles[..index].contains(handle) {
            return Err(format!("{id}: the handle `{handle}` is declared twice"));
        }
    }
    Ok(())
}

/// The registered clip types, in registration order.
pub fn clip_type_catalog() -> crate::Catalog<ClipTypeSpec> {
    crate::registered::clip_type_catalog()
}

/// The registered clip type `id`, or `None`.
pub fn clip_type_spec(id: &str) -> Option<Arc<ClipTypeSpec>> {
    crate::registered::clip_type_spec(id)
}

#[cfg(test)]
#[path = "clip_types_tests.rs"]
mod tests;
