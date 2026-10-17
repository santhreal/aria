//! The render-graph fields of `plugin.toml`: the pass tables of an effect's
//! `passes`, and the `[[textures]]` those passes read.

use std::path::PathBuf;

use aria_models::{GraphPass, OutputSize, PassInput, PassStage};
use serde::Deserialize;

use super::{catalog_id, package_path};

/// One pass of a declared effect: the fields of a [`GraphPass`], with its
/// WGSL in a package file.
#[derive(Debug, Clone, PartialEq)]
pub struct DeclaredPass {
    /// The WGSL file, relative to the package.
    pub file: PathBuf,
    /// The name other passes read this one by. Empty names it `pass<index>`.
    pub name: String,
    pub stage: PassStage,
    /// What the pass reads, in binding order. Empty reads the pass before it,
    /// or the source frame for the first pass. A texture input names a
    /// `[[textures]]` id under the package id.
    pub inputs: Vec<PassInput>,
    pub output: OutputSize,
}

impl DeclaredPass {
    /// A fragment pass in `file`, with the default name, inputs and size.
    pub fn file(file: impl Into<PathBuf>) -> Self {
        Self {
            file: file.into(),
            name: String::new(),
            stage: PassStage::Fragment,
            inputs: Vec::new(),
            output: OutputSize::Same,
        }
    }

    /// This pass with `source` as its WGSL.
    pub fn graph_pass(&self, source: String) -> GraphPass {
        GraphPass {
            name: self.name.clone(),
            stage: self.stage,
            inputs: self.inputs.clone(),
            output: self.output,
            source,
        }
    }
}

/// An image the package's effect passes read, registered as a plugin texture
/// when the package mounts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclaredTexture {
    /// The texture id: the declared id under the package id.
    pub id: String,
    /// The image file, relative to the package.
    pub file: PathBuf,
}

/// A `[[textures]]` entry as written.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawTexture {
    id: String,
    file: String,
}

/// A pass table of an effect's `passes` as written.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawGraphPass {
    file: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    stage: PassStage,
    #[serde(default)]
    inputs: Vec<PassInput>,
    #[serde(default)]
    output: OutputSize,
}

/// The `[[textures]]` entries, each id under the package id.
pub(super) fn declared_textures(
    plugin: &str,
    raw: Vec<RawTexture>,
) -> Result<Vec<DeclaredTexture>, String> {
    let mut declared: Vec<DeclaredTexture> = Vec::with_capacity(raw.len());
    for entry in raw {
        let id = catalog_id(plugin, &entry.id)?;
        if declared.iter().any(|held| held.id == id) {
            return Err(format!("{id}: the texture is declared twice"));
        }
        declared.push(DeclaredTexture {
            file: package_path(&id, &entry.file)?,
            id,
        });
    }
    Ok(declared)
}

/// The passes of the effect `effect` of package `plugin`. Each entry of
/// `raw` is a WGSL file path, read as a fragment pass with the default name,
/// inputs and size, or a pass table.
pub(super) fn declared_passes(
    plugin: &str,
    effect: &str,
    raw: Vec<toml::Value>,
    textures: &[DeclaredTexture],
) -> Result<Vec<DeclaredPass>, String> {
    raw.into_iter()
        .enumerate()
        .map(|(index, entry)| declared_pass(plugin, effect, index, entry, textures))
        .collect()
}

fn declared_pass(
    plugin: &str,
    effect: &str,
    index: usize,
    entry: toml::Value,
    textures: &[DeclaredTexture],
) -> Result<DeclaredPass, String> {
    let raw: RawGraphPass = match entry {
        toml::Value::String(file) => return Ok(DeclaredPass::file(package_path(effect, &file)?)),
        table @ toml::Value::Table(_) => table
            .try_into()
            .map_err(|err: toml::de::Error| format!("{effect}: pass {index}: {}", err.message()))?,
        other => {
            return Err(format!(
                "{effect}: pass {index}: expected a WGSL file path or a pass table, found {}",
                other.type_str()
            ));
        }
    };
    let inputs = raw
        .inputs
        .into_iter()
        .map(|input| match input {
            PassInput::Texture(declared) => {
                let id = catalog_id(plugin, &declared)?;
                if textures.iter().any(|texture| texture.id == id) {
                    Ok(PassInput::Texture(id))
                } else {
                    Err(format!(
                        "{effect}: pass {index} reads texture `{declared}`, which no [[textures]] entry declares"
                    ))
                }
            }
            other => Ok(other),
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(DeclaredPass {
        file: package_path(effect, &raw.file)?,
        name: raw.name,
        stage: raw.stage,
        inputs,
        output: raw.output,
    })
}
