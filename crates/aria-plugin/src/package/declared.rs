//! Registering what a package declares as data: effects, transitions and
//! generators drawn by its WGSL, the textures their passes read, its clip
//! types, the menu rows and default chords that offer its commands, its
//! section in Settings, its workspaces, and its presets and themes.

use std::io::Read as _;
use std::path::{Path, PathBuf};

use aria_compositor::{EffectDefinition, MAX_TEXTURE_SIDE, TransitionDefinition};
use aria_models::{EffectSpec, TransitionSpec};

use super::{DeclaredEffect, DeclaredTexture, Package};
use crate::clip_types::ClipTypeDefinition;
use crate::context::PluginContext;
use crate::error::PluginError;
use crate::plugin::{Plugin, PluginManifest};
use crate::registration::Registration;
use crate::settings_schema::SettingsSection;

/// Largest WGSL file read, in bytes.
const MAX_SHADER_BYTES: u64 = 1 << 20;

/// Largest image file read for a texture, in bytes.
const MAX_TEXTURE_BYTES: u64 = 64 << 20;

/// Register the textures, effects, generators, transitions, clip types, menu
/// entries, key defaults, settings, workspaces, presets and themes `package`
/// declares.
///
/// Call it from [`Plugin::mount`]: the context then holds the registrations
/// for the plugin's lifetime, and the returned list is empty handles. Called
/// outside a mount, the caller holds them and each drops its entry.
///
/// A shader or image file that is missing, outside the package, or larger
/// than its limit (1 MiB of WGSL, 64 MiB of image) fails with the file named,
/// as does an image the decoder does not read or wider or taller than
/// [`MAX_TEXTURE_SIDE`]; a shader the compositor refuses fails with its line
/// and column. Commands themselves are registered by the tier that runs their
/// handlers.
pub fn register_declared(
    ctx: &PluginContext,
    package: &Package,
) -> Result<Vec<Registration>, PluginError> {
    let manifest = &package.manifest;
    let mut held = Vec::with_capacity(
        manifest.textures.len()
            + manifest.effects.len()
            + manifest.generators.len()
            + manifest.transitions.len()
            + manifest.menus.len()
            + manifest.keys.len()
            + package.presets.len()
            + manifest.workspaces.len()
            + package.clip_types().len()
            + package.themes.len()
            + 1,
    );
    for texture in &manifest.textures {
        let image = read_texture(package, texture)?;
        held.push(ctx.register_texture(texture.id.as_str(), &image)?);
    }
    let effects = manifest.effects.iter().map(|effect| (effect, false)).chain(
        manifest
            .generators
            .iter()
            .map(|generator| (generator, true)),
    );
    for (effect, generator) in effects {
        held.push(ctx.register_effect(definition(package, effect, generator)?)?);
    }
    for transition in &manifest.transitions {
        let shader = read_shader(package, &transition.id, &transition.shader)?;
        held.push(ctx.register_transition(TransitionDefinition {
            spec: TransitionSpec {
                id: transition.id.clone(),
                label: transition.name.clone(),
                params: transition.params.clone(),
            },
            shader,
        })?);
    }
    for (clip_type, declared) in package.clip_types().iter().zip(&manifest.clip_types) {
        held.push(match &declared.draws {
            Some(effect) => {
                let EffectDefinition { spec, passes } = definition(package, effect, true)?;
                ctx.define_clip_type(
                    ClipTypeDefinition::new(clip_type.clone(), spec.params, passes)
                        .with_groups(spec.groups)
                        .with_time_dependent(spec.time_dependent),
                )?
            }
            None => ctx.register_clip_type(clip_type.clone())?,
        });
    }
    register_offers(ctx, package, &mut held)?;
    Ok(held)
}

/// Register the menu entries, key defaults, settings section, workspaces,
/// presets and themes `package` declares into `held`.
fn register_offers(
    ctx: &PluginContext,
    package: &Package,
    held: &mut Vec<Registration>,
) -> Result<(), PluginError> {
    let manifest = &package.manifest;
    for entry in &manifest.menus {
        held.push(ctx.register_menu_entry(entry.clone())?);
    }
    for key in &manifest.keys {
        held.push(ctx.register_key_default(key.clone())?);
    }
    if !manifest.settings.is_empty() {
        held.push(ctx.register_settings(SettingsSection {
            plugin: manifest.id.clone(),
            title: manifest.name.clone(),
            fields: manifest.settings.clone(),
        })?);
    }
    for workspace in &manifest.workspaces {
        held.push(ctx.register_workspace(workspace.clone())?);
    }
    for preset in &package.presets {
        held.push(ctx.register_preset(preset.clone())?);
    }
    for theme in &package.themes {
        held.push(ctx.register_theme(&theme.id, theme.text.as_str())?);
    }
    Ok(())
}

fn definition(
    package: &Package,
    effect: &DeclaredEffect,
    generator: bool,
) -> Result<EffectDefinition, PluginError> {
    let passes = effect
        .passes
        .iter()
        .map(|pass| Ok(pass.graph_pass(read_shader(package, &effect.id, &pass.file)?)))
        .collect::<Result<Vec<_>, PluginError>>()?;
    Ok(EffectDefinition {
        spec: EffectSpec {
            id: effect.id.clone(),
            category: effect.category.clone().unwrap_or_default(),
            label: effect.name.clone(),
            params: effect.params.clone(),
            groups: effect.groups.clone(),
            generator,
            stateful: effect.stateful,
            bounds: effect.bounds,
            reads_outside: effect.reads_outside,
            time_dependent: effect.time_dependent,
        },
        passes,
    })
}

/// The failure of the file `relative` of `owner` in `package`.
fn file_failed(package: &Package, owner: &str, relative: &Path, reason: String) -> PluginError {
    PluginError::MountFailed(
        package.manifest.id.clone().into(),
        format!("{owner}: {}: {reason}", relative.display()),
    )
}

/// The canonical path of `relative` inside the package, refusing a path that
/// resolves outside it through a link.
fn inside(package: &Package, owner: &str, relative: &Path) -> Result<PathBuf, PluginError> {
    let failed = |reason: String| file_failed(package, owner, relative, reason);
    let root = package
        .dir
        .canonicalize()
        .map_err(|err| failed(err.to_string()))?;
    let path = package
        .dir
        .join(relative)
        .canonicalize()
        .map_err(|err| failed(err.to_string()))?;
    if !path.starts_with(&root) {
        return Err(failed("resolves outside the package".into()));
    }
    Ok(path)
}

/// The WGSL at `relative` inside the package.
fn read_shader(package: &Package, owner: &str, relative: &Path) -> Result<String, PluginError> {
    let failed = |reason: String| file_failed(package, owner, relative, reason);
    let path = inside(package, owner, relative)?;
    let file = std::fs::File::open(&path).map_err(|err| failed(err.to_string()))?;
    let mut source = String::new();
    file.take(MAX_SHADER_BYTES + 1)
        .read_to_string(&mut source)
        .map_err(|err| failed(err.to_string()))?;
    if source.len() as u64 > MAX_SHADER_BYTES {
        return Err(failed(format!("larger than {MAX_SHADER_BYTES} bytes")));
    }
    Ok(source)
}

/// The image of `texture`, decoded to straight-alpha RGBA8. Its size is read
/// before its pixels, so an image too large to register is never decoded.
fn read_texture(
    package: &Package,
    texture: &DeclaredTexture,
) -> Result<aria_core::RgbaImage, PluginError> {
    let failed = |reason: String| file_failed(package, &texture.id, &texture.file, reason);
    let path = inside(package, &texture.id, &texture.file)?;
    let bytes = std::fs::metadata(&path)
        .map_err(|err| failed(err.to_string()))?
        .len();
    if bytes > MAX_TEXTURE_BYTES {
        return Err(failed(format!("larger than {MAX_TEXTURE_BYTES} bytes")));
    }
    let info = aria_decoder::probe_image(&path).map_err(|err| failed(err.to_string()))?;
    if info.width > MAX_TEXTURE_SIDE || info.height > MAX_TEXTURE_SIDE {
        return Err(failed(format!(
            "{}x{} is larger than {MAX_TEXTURE_SIDE} pixels on a side",
            info.width, info.height
        )));
    }
    aria_decoder::decode_image(&path).map_err(|err| failed(err.to_string()))
}

/// The plugin for a package with no code: mounting it registers the package's
/// declared contributions.
#[derive(Debug)]
pub struct DataPlugin {
    package: Package,
    manifest: PluginManifest,
}

impl DataPlugin {
    pub fn new(package: Package) -> Self {
        let manifest = package.plugin_manifest();
        Self { package, manifest }
    }

    pub fn package(&self) -> &Package {
        &self.package
    }
}

impl Plugin for DataPlugin {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        register_declared(ctx, &self.package).map(drop)
    }
}
