//! `[[clip_types]]`: the kinds of clip a package declares, each drawn by a
//! generator the package declares or by its own `shader` or `passes`.

use aria_models::{
    ClipTypeSpec, EffectBounds, EffectGroupSpec, EffectParamKind, EffectParamSpec, MarkerColor,
};
use serde::Deserialize;

use super::{
    DeclaredClipType, DeclaredTexture, RawEffect, catalog_id, declared_effect, package_path,
};

/// A `[[clip_types]]` entry as written.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawClipType {
    id: String,
    name: String,
    #[serde(default)]
    generator: Option<String>,
    colour: MarkerColor,
    #[serde(default)]
    icon: Option<String>,
    #[serde(default)]
    handles: Vec<String>,
    #[serde(default)]
    shader: Option<String>,
    #[serde(default)]
    passes: Vec<toml::Value>,
    #[serde(default)]
    params: Vec<EffectParamSpec>,
    #[serde(default)]
    groups: Vec<EffectGroupSpec>,
    #[serde(default)]
    time_dependent: bool,
}

/// The `[[clip_types]]` entries, each id under the package id and checked on
/// the catalog's terms. A type with its own `shader` or `passes` is drawn by
/// a generator registered under its id, so it claims that id among the
/// package's effect ids in `taken`, and each handle names one of its `vec2`
/// params.
pub(super) fn declared_clip_types(
    plugin: &str,
    raw: Vec<RawClipType>,
    textures: &[DeclaredTexture],
    taken: &mut Vec<String>,
) -> Result<Vec<DeclaredClipType>, String> {
    let mut declared: Vec<DeclaredClipType> = Vec::with_capacity(raw.len());
    for entry in raw {
        let id = catalog_id(plugin, &entry.id)?;
        if declared.iter().any(|held| held.spec.id == id) {
            return Err(format!("{id}: the clip type is declared twice"));
        }
        let icon = entry
            .icon
            .as_deref()
            .map(|icon| package_path(&id, icon))
            .transpose()?;
        let draws_itself = entry.shader.is_some() || !entry.passes.is_empty();
        let generator = match (entry.generator, draws_itself) {
            (Some(generator), false) => {
                if !entry.params.is_empty() || !entry.groups.is_empty() || entry.time_dependent {
                    return Err(format!(
                        "{id}: `params`, `groups` and `time_dependent` belong to a clip type with its own `shader` or `passes`"
                    ));
                }
                catalog_id(plugin, &generator)?
            }
            (None, true) => id.clone(),
            (Some(_), true) => {
                return Err(format!(
                    "{id}: declares `generator` and its own `shader` or `passes`; use one"
                ));
            }
            (None, false) => {
                return Err(format!(
                    "{id}: declares no `generator`, `shader` or `passes`"
                ));
            }
        };
        let spec = ClipTypeSpec {
            generator,
            label: entry.name,
            colour: entry.colour,
            icon: None,
            handles: entry.handles,
            id,
        };
        spec.check()
            .map_err(|fault| format!("[[clip_types]] {fault}"))?;
        let draws = if draws_itself {
            if let Some(handle) = spec.handles.iter().find(|handle| {
                !entry
                    .params
                    .iter()
                    .any(|param| param.name == **handle && param.kind == EffectParamKind::Vec2)
            }) {
                return Err(format!(
                    "{}: the handle `{handle}` names no vec2 param",
                    spec.id
                ));
            }
            let raw = RawEffect {
                id: spec.id.clone(),
                name: spec.label.clone(),
                category: None,
                shader: entry.shader,
                passes: entry.passes,
                params: entry.params,
                groups: entry.groups,
                stateful: false,
                bounds: EffectBounds::ZERO,
                reads_outside: false,
                time_dependent: entry.time_dependent,
            };
            Some(declared_effect(plugin, raw, textures, taken)?)
        } else {
            None
        };
        declared.push(DeclaredClipType { spec, icon, draws });
    }
    Ok(declared)
}
