//! `plugin.toml`: what a package declares, read without running any of its code.

use std::path::{Component, Path, PathBuf};

use aria_models::{ClipTypeSpec, EffectBounds, EffectGroupSpec, EffectParamSpec};
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};

use super::API_MAJOR;
use crate::activation::{Activation, ActivationKind};
use crate::enablement::Enablement;
use crate::keys::{Chord, KeyDefault, KeyRoute};
use crate::menus::{MenuCommand, MenuPath};
use crate::service::PanelLocation;
use crate::settings_schema::{SettingKind, SettingSpec};
use crate::workspaces::{WorkspaceFocus, WorkspacePanes, WorkspaceSpec};

#[path = "manifest_clip_types.rs"]
mod clip_types;
#[path = "manifest_passes.rs"]
mod passes;

pub use passes::{DeclaredPass, DeclaredTexture};

/// A parsed and checked `plugin.toml`.
#[derive(Debug, Clone, PartialEq)]
pub struct PackageManifest {
    /// Reverse-domain id, unique across installed plugins.
    pub id: String,
    pub name: String,
    /// Semantic version.
    pub version: String,
    /// The API major version the package is built against.
    pub api: u32,
    pub publisher: String,
    pub description: String,
    pub license: String,
    /// Plugins that must be mounted first, each with the versions it accepts.
    pub requires: Vec<Requirement>,
    /// Activation events. Empty means the plugin loads at start.
    pub activation: Vec<Activation>,
    pub permissions: Permissions,
    /// Video effects drawn by the package's WGSL.
    pub effects: Vec<DeclaredEffect>,
    /// Transitions drawn by the package's WGSL.
    pub transitions: Vec<DeclaredTransition>,
    /// Generators: effects that draw a picture of their own.
    pub generators: Vec<DeclaredEffect>,
    /// Palette commands. Their handlers come from the package's code.
    pub commands: Vec<DeclaredCommand>,
    /// Docked panels, listed before the package loads. Their views come from
    /// the package's code.
    pub panels: Vec<DeclaredPanel>,
    /// Menu rows offering the commands: each `[[commands]]` `menu`, then each
    /// `[[menus]]` entry.
    pub menus: Vec<MenuCommand>,
    /// Default chords for the commands: each `[[commands]]` `key`, then each
    /// `[[keys]]` entry.
    pub keys: Vec<KeyDefault>,
    /// The fields of the package's section in Settings, each key under the
    /// package id.
    pub settings: Vec<SettingSpec>,
    /// Named pane arrangements offered in the Window menu.
    pub workspaces: Vec<WorkspaceSpec>,
    /// Kinds of clip a generator of the package draws.
    pub clip_types: Vec<DeclaredClipType>,
    /// Images the package's effect passes read, registered before its
    /// effects.
    pub textures: Vec<DeclaredTexture>,
}

/// A clip type the package declares.
#[derive(Debug, Clone, PartialEq)]
pub struct DeclaredClipType {
    /// The type, with its id and generator under the package id and no icon.
    pub spec: ClipTypeSpec,
    /// The icon's SVG file, relative to the package.
    pub icon: Option<PathBuf>,
    /// The generator a type with its own `shader` or `passes` is drawn by,
    /// registered under the type's id with the type's `params` as its fields.
    /// `None` for a type drawn by the generator its `generator` names.
    pub draws: Option<DeclaredEffect>,
}

/// A plugin this one needs, and the versions of it that satisfy the need.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Requirement {
    pub id: String,
    pub versions: VersionReq,
}

impl Requirement {
    /// Whether `version` of the required plugin satisfies this requirement.
    /// A version that is not semantic satisfies only a requirement of any
    /// version.
    pub fn admits(&self, version: &str) -> bool {
        match Version::parse(version) {
            Ok(version) => self.versions.matches(&version),
            Err(_) => self.versions == VersionReq::STAR,
        }
    }
}

/// What a package may reach beyond its own storage, settings, logging, events
/// and contributions, which need no permission.
///
/// Serialized for a host's record of what it granted; a field this build does
/// not know is refused, so a record from a newer build grants nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Permissions {
    /// Reading the project; with `ReadWrite`, proposing transactions.
    pub project: Access,
    /// Probing and decoding the project's media.
    pub media: Access,
    /// Creating GPU resources and submitting passes.
    pub gpu: bool,
    /// Hosts the package may connect to.
    pub net: Vec<String>,
    /// File system scopes beyond the package and its data directory.
    pub fs: Vec<FsScope>,
    /// Executables the package may spawn.
    pub process: Vec<String>,
    /// Reading the clipboard.
    pub clipboard: bool,
}

/// How far a permission reaches.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Access {
    #[default]
    None,
    Read,
    ReadWrite,
}

/// One file system scope a package may read and write.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FsScope {
    /// The open project's folder.
    Project,
    /// Paths the user picks in a host dialog.
    Pick,
    /// One absolute path and everything under it.
    Path(PathBuf),
}

/// An effect or generator the package draws with its own WGSL.
#[derive(Debug, Clone, PartialEq)]
pub struct DeclaredEffect {
    /// The catalog id: the declared id under the package id.
    pub id: String,
    pub name: String,
    pub category: Option<String>,
    /// The passes, in declaration order: one WGSL file each, with the graph
    /// fields of a [`aria_models::GraphPass`].
    pub passes: Vec<DeclaredPass>,
    pub params: Vec<EffectParamSpec>,
    pub groups: Vec<EffectGroupSpec>,
    pub stateful: bool,
    /// Pixels the effect draws beyond its layer's edges.
    pub bounds: EffectBounds,
    /// Whether the effect samples its source outside `0.0..=1.0`.
    pub reads_outside: bool,
    /// Whether the effect's frame changes with time when its parameters do
    /// not. A pass of an effect that is not reads `aria.time` as zero.
    pub time_dependent: bool,
}

/// A transition the package draws with its own WGSL.
#[derive(Debug, Clone, PartialEq)]
pub struct DeclaredTransition {
    /// The catalog id: the declared id under the package id.
    pub id: String,
    pub name: String,
    /// The WGSL file, relative to the package.
    pub shader: PathBuf,
    pub params: Vec<EffectParamSpec>,
}

/// A palette command, listed before the package loads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclaredCommand {
    pub id: String,
    pub title: String,
    /// The default chord on the editor, such as `Ctrl+Alt+L`.
    pub key: Option<String>,
    /// The menu path, such as `Sequence/Color Tools`.
    pub menu: Option<String>,
    /// The icon's SVG file, relative to the package.
    pub icon: Option<PathBuf>,
    /// The editor state the command needs.
    pub enablement: Option<Enablement>,
}

/// A docked panel, listed before the package loads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclaredPanel {
    pub id: String,
    pub title: String,
    /// The sidebar, the inspector, or the timeline area.
    pub location: PanelLocation,
}

/// A `[[panels]]` entry as written.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPanel {
    id: String,
    title: String,
    location: String,
}

/// A `[[commands]]` entry as written.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCommand {
    id: String,
    title: String,
    #[serde(default)]
    key: Option<String>,
    #[serde(default)]
    menu: Option<String>,
    #[serde(default)]
    icon: Option<String>,
    #[serde(default)]
    enablement: Option<String>,
}

/// A `[[menus]]` entry: one more place a declared command is offered.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMenu {
    command: String,
    menu: String,
}

/// A `[[keys]]` entry: a declared command's default chord on one route.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawKey {
    command: String,
    key: String,
    #[serde(default)]
    route: Option<String>,
}

/// A `[[settings]]` field.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSetting {
    key: String,
    label: String,
    #[serde(rename = "type", default)]
    kind: Option<String>,
    #[serde(default)]
    placeholder: String,
    #[serde(default)]
    default: Option<toml::Value>,
    #[serde(default)]
    min: Option<f64>,
    #[serde(default)]
    max: Option<f64>,
    #[serde(default)]
    step: Option<f64>,
    #[serde(default)]
    options: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFile {
    plugin: RawPlugin,
    #[serde(default)]
    activation: RawActivation,
    #[serde(default)]
    permissions: RawPermissions,
    #[serde(default)]
    effects: Vec<RawEffect>,
    #[serde(default)]
    transitions: Vec<RawTransition>,
    #[serde(default)]
    generators: Vec<RawEffect>,
    #[serde(default)]
    commands: Vec<RawCommand>,
    #[serde(default)]
    panels: Vec<RawPanel>,
    #[serde(default)]
    menus: Vec<RawMenu>,
    #[serde(default)]
    keys: Vec<RawKey>,
    #[serde(default)]
    settings: Vec<RawSetting>,
    #[serde(default)]
    workspaces: Vec<RawWorkspace>,
    #[serde(default)]
    clip_types: Vec<clip_types::RawClipType>,
    #[serde(default)]
    textures: Vec<passes::RawTexture>,
}

/// A `[[workspaces]]` entry: an id, a name, and the fields of
/// [`WorkspacePanes`] beside them. Spelled out rather than flattened, because
/// serde does not refuse an unknown field through `flatten`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawWorkspace {
    id: String,
    name: String,
    sidebar_width: Option<f32>,
    sidebar_collapsed: Option<bool>,
    inspector_width: Option<f32>,
    inspector_collapsed: Option<bool>,
    timeline_height: Option<f32>,
    scopes: Option<bool>,
    focus: Option<WorkspaceFocus>,
    tree: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPlugin {
    id: String,
    name: String,
    version: String,
    api: RawApi,
    #[serde(default)]
    publisher: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    license: String,
    #[serde(default)]
    requires: Vec<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RawApi {
    Number(i64),
    Text(String),
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawActivation {
    #[serde(default)]
    on: Vec<String>,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPermissions {
    #[serde(default)]
    project: Access,
    #[serde(default)]
    media: Access,
    #[serde(default)]
    gpu: bool,
    #[serde(default)]
    net: Vec<String>,
    #[serde(default)]
    fs: Vec<String>,
    #[serde(default)]
    process: RawProcess,
    #[serde(default)]
    clipboard: bool,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RawProcess {
    Flag(bool),
    List(Vec<String>),
}

impl Default for RawProcess {
    fn default() -> Self {
        Self::Flag(false)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEffect {
    id: String,
    name: String,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    shader: Option<String>,
    #[serde(default)]
    passes: Vec<toml::Value>,
    #[serde(default)]
    params: Vec<EffectParamSpec>,
    #[serde(default)]
    groups: Vec<EffectGroupSpec>,
    #[serde(default)]
    stateful: bool,
    #[serde(default)]
    bounds: EffectBounds,
    #[serde(default)]
    reads_outside: bool,
    #[serde(default)]
    time_dependent: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTransition {
    id: String,
    name: String,
    shader: String,
    #[serde(default)]
    params: Vec<EffectParamSpec>,
}

/// The `[[panels]]` locations, as `plugin.toml` spells them.
const PANEL_LOCATIONS: [(&str, PanelLocation); 3] = [
    ("sidebar", PanelLocation::Sidebar),
    ("inspector", PanelLocation::Inspector),
    ("timeline", PanelLocation::Timeline),
];

impl PackageManifest {
    /// Parse and check the text of a `plugin.toml`.
    pub fn parse(text: &str) -> Result<Self, String> {
        let raw: RawFile = toml::from_str(text).map_err(|err| err.to_string())?;
        let RawPlugin {
            id,
            name,
            version,
            api,
            publisher,
            description,
            license,
            requires,
        } = raw.plugin;
        check_plugin_id(&id)?;
        if name.trim().is_empty() {
            return Err(format!("{id}: the plugin needs a name"));
        }
        Version::parse(&version)
            .map_err(|err| format!("{id}: version {version:?} is not semantic: {err}"))?;
        let api = api_major(&id, api)?;
        let requires = requires
            .iter()
            .map(|raw| requirement(&id, raw))
            .collect::<Result<Vec<_>, _>>()?;
        let permissions = permissions(&id, raw.permissions)?;

        let textures = passes::declared_textures(&id, raw.textures)?;
        let mut catalog_ids = Vec::new();
        let mut effects = Vec::with_capacity(raw.effects.len());
        for effect in raw.effects {
            effects.push(declared_effect(&id, effect, &textures, &mut catalog_ids)?);
        }
        let mut generators = Vec::with_capacity(raw.generators.len());
        for generator in raw.generators {
            generators.push(declared_effect(
                &id,
                generator,
                &textures,
                &mut catalog_ids,
            )?);
        }
        let mut transitions = Vec::with_capacity(raw.transitions.len());
        for transition in raw.transitions {
            let qualified = catalog_id(&id, &transition.id)?;
            if transition.name.trim().is_empty() {
                return Err(format!("{qualified}: the transition needs a name"));
            }
            claim(&mut catalog_ids, &qualified)?;
            transitions.push(DeclaredTransition {
                shader: package_path(&qualified, &transition.shader)?,
                id: qualified,
                name: transition.name,
                params: transition.params,
            });
        }
        let mut commands: Vec<DeclaredCommand> = Vec::with_capacity(raw.commands.len());
        let mut menus = Vec::new();
        let mut keys = Vec::new();
        for command in raw.commands {
            if command.id.trim().is_empty() {
                return Err(format!("{id}: a command needs an id"));
            }
            if command.title.trim().is_empty() {
                return Err(format!("{}: the command needs a title", command.id));
            }
            if commands.iter().any(|held| held.id == command.id) {
                return Err(format!("{}: the command is declared twice", command.id));
            }
            if let Some(menu) = &command.menu {
                place(&mut menus, &command.id, menu)?;
            }
            if let Some(key) = &command.key {
                bind(&mut keys, &command.id, key, None)?;
            }
            let icon = command
                .icon
                .as_deref()
                .map(|icon| package_path(&command.id, icon))
                .transpose()?;
            let enablement = command
                .enablement
                .as_deref()
                .map(|source| {
                    Enablement::parse(source).map_err(|err| format!("{}: {err}", command.id))
                })
                .transpose()?;
            commands.push(DeclaredCommand {
                id: command.id,
                title: command.title,
                key: command.key,
                menu: command.menu,
                icon,
                enablement,
            });
        }
        let command_ids: Vec<&str> = commands.iter().map(|command| command.id.as_str()).collect();
        for menu in &raw.menus {
            declared(&command_ids, &menu.command, "menu")?;
            place(&mut menus, &menu.command, &menu.menu)?;
        }
        for key in &raw.keys {
            declared(&command_ids, &key.command, "key")?;
            bind(&mut keys, &key.command, &key.key, key.route.as_deref())?;
        }
        let mut settings: Vec<SettingSpec> = Vec::with_capacity(raw.settings.len());
        for setting in raw.settings {
            let spec = setting_spec(&id, setting)?;
            if settings.iter().any(|held| held.key == spec.key) {
                return Err(format!("{}: the setting is declared twice", spec.key));
            }
            settings.push(spec);
        }
        let mut workspaces: Vec<WorkspaceSpec> = Vec::with_capacity(raw.workspaces.len());
        for entry in raw.workspaces {
            let spec = WorkspaceSpec {
                id: entry.id,
                name: entry.name,
                panes: WorkspacePanes {
                    sidebar_width: entry.sidebar_width,
                    sidebar_collapsed: entry.sidebar_collapsed,
                    inspector_width: entry.inspector_width,
                    inspector_collapsed: entry.inspector_collapsed,
                    timeline_height: entry.timeline_height,
                    scopes: entry.scopes,
                    focus: entry.focus,
                    tree: entry.tree,
                },
            };
            if let Some(fault) = crate::workspaces::spec_fault(&spec) {
                return Err(format!("[[workspaces]] {fault}"));
            }
            if workspaces.iter().any(|held| held.id == spec.id) {
                return Err(format!("{}: the workspace is declared twice", spec.id));
            }
            workspaces.push(spec);
        }
        let clip_types =
            clip_types::declared_clip_types(&id, raw.clip_types, &textures, &mut catalog_ids)?;
        let panels = declared_panels(&id, raw.panels)?;
        let setting_keys: Vec<&str> = settings.iter().map(|spec| spec.key.as_str()).collect();
        let panel_ids: Vec<&str> = panels.iter().map(|panel| panel.id.as_str()).collect();
        let mut activation = Vec::with_capacity(raw.activation.on.len());
        for event in &raw.activation.on {
            let event = activation_event(&id, event, &command_ids, &panel_ids, &setting_keys)?;
            if !activation.contains(&event) {
                activation.push(event);
            }
        }

        Ok(Self {
            id,
            name,
            version,
            api,
            publisher,
            description,
            license,
            requires,
            activation,
            permissions,
            effects,
            transitions,
            generators,
            commands,
            panels,
            menus,
            keys,
            settings,
            workspaces,
            clip_types,
            textures,
        })
    }

    /// Whether the package loads at start: it lists no activation events, or
    /// lists `startup`.
    pub fn loads_at_start(&self) -> bool {
        self.activation.is_empty() || self.activation.contains(&Activation::Startup)
    }
}

/// Refuse a `[[menus]]` or `[[keys]]` entry for a command `[[commands]]` does not
/// declare.
fn declared(commands: &[&str], command: &str, what: &str) -> Result<(), String> {
    if commands.contains(&command) {
        return Ok(());
    }
    Err(format!(
        "{command}: a {what} names a command no [[commands]] entry declares"
    ))
}

fn place(menus: &mut Vec<MenuCommand>, command: &str, menu: &str) -> Result<(), String> {
    let entry = MenuCommand {
        command: command.to_owned(),
        path: MenuPath::parse(menu).map_err(|reason| format!("{command}: {reason}"))?,
    };
    if menus.contains(&entry) {
        return Err(format!(
            "{command}: the menu {} is declared twice",
            entry.path
        ));
    }
    menus.push(entry);
    Ok(())
}

fn bind(
    keys: &mut Vec<KeyDefault>,
    command: &str,
    key: &str,
    route: Option<&str>,
) -> Result<(), String> {
    let chord = Chord::parse(key).ok_or_else(|| {
        format!("{command}: key {key:?} is not a chord such as `Ctrl+Alt+L` on a key Aria can bind")
    })?;
    let route = match route {
        None => KeyRoute::Editor,
        Some(text) => KeyRoute::parse(text).ok_or_else(|| {
            format!("{command}: route {text:?} is not `editor`, `timeline`, `viewer`, or `home`")
        })?,
    };
    if keys
        .iter()
        .any(|held| held.command == command && held.route == route)
    {
        return Err(format!(
            "{command}: the command has two {} keys",
            route.label()
        ));
    }
    keys.push(KeyDefault {
        command: command.to_owned(),
        chord,
        route,
    });
    Ok(())
}

/// A `[[settings]]` field, its key under the package id and its default in
/// stored form.
fn setting_spec(plugin: &str, raw: RawSetting) -> Result<SettingSpec, String> {
    let key = catalog_id(plugin, &raw.key)?;
    let kind_name = raw.kind.as_deref().unwrap_or("text");
    let numeric = raw.min.is_some() || raw.max.is_some() || raw.step.is_some();
    if numeric && kind_name != "number" {
        return Err(format!("{key}: min, max and step belong to a number"));
    }
    if !raw.options.is_empty() && kind_name != "choice" {
        return Err(format!("{key}: options belong to a choice"));
    }
    let kind = match kind_name {
        "text" => SettingKind::Text,
        "secret" => SettingKind::Secret,
        "toggle" => SettingKind::Toggle,
        "number" => SettingKind::Number {
            min: raw.min.unwrap_or(f64::MIN),
            max: raw.max.unwrap_or(f64::MAX),
            step: raw.step.unwrap_or(1.0),
        },
        "choice" => SettingKind::Choice(raw.options),
        other => {
            return Err(format!(
                "{key}: type {other:?} is not text, secret, toggle, number or choice"
            ));
        }
    };
    let default = match raw.default {
        None => None,
        Some(toml::Value::String(text)) => Some(text),
        Some(toml::Value::Boolean(flag)) => Some(flag.to_string()),
        Some(toml::Value::Integer(number)) => Some(number.to_string()),
        Some(toml::Value::Float(number)) => Some(number.to_string()),
        Some(other) => {
            return Err(format!(
                "{key}: default {other} is not text, a number or true or false"
            ));
        }
    };
    let mut spec = SettingSpec {
        key,
        label: raw.label,
        placeholder: raw.placeholder,
        kind,
        default,
    };
    if let Some(problem) = spec.problem() {
        return Err(problem);
    }
    // `problem` accepted the default, so `check` returns its stored form.
    let stored = spec
        .default
        .as_deref()
        .and_then(|text| spec.kind.check(text).ok())
        .filter(|text| !text.is_empty());
    spec.default = stored;
    Ok(spec)
}

/// A reverse-domain id: dot-separated labels of lowercase ASCII letters,
/// digits and `-`, at least two labels. `aria.` belongs to shipped plugins.
fn check_plugin_id(id: &str) -> Result<(), String> {
    let labels: Vec<&str> = id.split('.').collect();
    let well_formed = labels.len() >= 2
        && labels.iter().all(|label| {
            !label.is_empty()
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        });
    if !well_formed {
        return Err(format!(
            "plugin id {id:?} is not a reverse-domain id such as `com.example.tools`"
        ));
    }
    if labels[0] == "aria" {
        return Err(format!(
            "plugin id {id:?}: `aria.` is reserved for shipped plugins"
        ));
    }
    Ok(())
}

fn api_major(id: &str, api: RawApi) -> Result<u32, String> {
    let major = match api {
        RawApi::Number(number) => u32::try_from(number).ok(),
        RawApi::Text(text) => text.trim().parse::<u32>().ok(),
    };
    match major {
        Some(API_MAJOR) => Ok(API_MAJOR),
        Some(other) => Err(format!(
            "{id}: built for plugin API {other}; this Aria loads API {API_MAJOR}"
        )),
        None => Err(format!("{id}: api is not a major version number")),
    }
}

/// `"<id> <versions>"`, or a bare id for any version.
fn requirement(id: &str, raw: &str) -> Result<Requirement, String> {
    let raw = raw.trim();
    let (required, versions) = raw
        .split_once(char::is_whitespace)
        .map_or((raw, ""), |(required, versions)| {
            (required, versions.trim())
        });
    let well_formed = !required.is_empty()
        && required.contains('.')
        && required
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.');
    if !well_formed {
        return Err(format!("{id}: requires {raw:?}, which is not a plugin id"));
    }
    if required == id {
        return Err(format!("{id}: requires itself"));
    }
    let versions = if versions.is_empty() {
        VersionReq::STAR
    } else {
        VersionReq::parse(versions).map_err(|err| format!("{id}: requires {raw:?}: {err}"))?
    };
    Ok(Requirement {
        id: required.to_owned(),
        versions,
    })
}

/// Read one `[activation] on` event. `command:`, `panel:` and `setting:` must
/// name a row the package declares; a setting key is read under the package
/// id, as `[[settings]]` keys are.
fn activation_event(
    id: &str,
    event: &str,
    commands: &[&str],
    panels: &[&str],
    settings: &[&str],
) -> Result<Activation, String> {
    let parsed = Activation::parse(event).map_err(|reason| format!("{id}: {reason}"))?;
    let (rows, what) = match parsed.kind() {
        ActivationKind::Command => (commands, "[[commands]]"),
        ActivationKind::Panel => (panels, "[[panels]]"),
        ActivationKind::Setting => {
            let key = parsed.argument().unwrap_or_default();
            let qualified = catalog_id(id, key)?;
            if settings.contains(&qualified.as_str()) {
                return Ok(Activation::Setting(qualified));
            }
            (settings, "[[settings]]")
        }
        ActivationKind::Startup
        | ActivationKind::File
        | ActivationKind::Effect
        | ActivationKind::Transition
        | ActivationKind::Generator => return Ok(parsed),
    };
    if rows.contains(&parsed.argument().unwrap_or_default()) {
        return Ok(parsed);
    }
    Err(format!(
        "{id}: activation event {event:?} names no {what} entry of this package"
    ))
}

fn declared_panels(id: &str, raw: Vec<RawPanel>) -> Result<Vec<DeclaredPanel>, String> {
    let mut panels: Vec<DeclaredPanel> = Vec::with_capacity(raw.len());
    for panel in raw {
        if panel.id.trim().is_empty() {
            return Err(format!("{id}: a panel needs an id"));
        }
        if panel.title.trim().is_empty() {
            return Err(format!("{}: the panel needs a title", panel.id));
        }
        if panels.iter().any(|held| held.id == panel.id) {
            return Err(format!("{}: the panel is declared twice", panel.id));
        }
        let Some((_, location)) = PANEL_LOCATIONS
            .iter()
            .find(|(spelling, _)| *spelling == panel.location)
        else {
            let spellings: Vec<&str> = PANEL_LOCATIONS
                .iter()
                .map(|(spelling, _)| *spelling)
                .collect();
            return Err(format!(
                "{}: location {:?} is not one of {}",
                panel.id,
                panel.location,
                spellings.join(", ")
            ));
        };
        panels.push(DeclaredPanel {
            id: panel.id,
            title: panel.title,
            location: *location,
        });
    }
    Ok(panels)
}

fn permissions(id: &str, raw: RawPermissions) -> Result<Permissions, String> {
    for host in &raw.net {
        let bare = !host.is_empty()
            && host
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.' || b == b'*');
        if !bare {
            return Err(format!(
                "{id}: net host {host:?} is not a host name, such as `api.example.com`"
            ));
        }
    }
    let fs = raw
        .fs
        .iter()
        .map(|scope| match scope.as_str() {
            "project" => Ok(FsScope::Project),
            "pick" => Ok(FsScope::Pick),
            path if Path::new(path).is_absolute() => Ok(FsScope::Path(PathBuf::from(path))),
            other => Err(format!(
                "{id}: fs scope {other:?} is not `project`, `pick`, or an absolute path"
            )),
        })
        .collect::<Result<Vec<_>, _>>()?;
    let process = match raw.process {
        RawProcess::Flag(false) => Vec::new(),
        RawProcess::Flag(true) => {
            return Err(format!(
                "{id}: process = true names no executable; list the executables instead"
            ));
        }
        RawProcess::List(names) => {
            if let Some(empty) = names.iter().find(|name| name.trim().is_empty()) {
                return Err(format!("{id}: process lists an empty name {empty:?}"));
            }
            names
        }
    };
    Ok(Permissions {
        project: raw.project,
        media: raw.media,
        gpu: raw.gpu,
        net: raw.net,
        fs,
        process,
        clipboard: raw.clipboard,
    })
}

/// The catalog id for a declared one: under the package id unless it already
/// is.
fn catalog_id(plugin: &str, declared: &str) -> Result<String, String> {
    let well_formed = !declared.is_empty()
        && declared
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'));
    if !well_formed {
        return Err(format!(
            "{plugin}: id {declared:?} holds characters other than ASCII letters, digits, `-`, `_` and `.`"
        ));
    }
    let prefix = format!("{plugin}.");
    if declared.starts_with(&prefix) {
        Ok(declared.to_owned())
    } else {
        Ok(format!("{prefix}{declared}"))
    }
}

fn claim(taken: &mut Vec<String>, id: &str) -> Result<(), String> {
    if taken.iter().any(|held| held == id) {
        return Err(format!("{id}: declared twice"));
    }
    taken.push(id.to_owned());
    Ok(())
}

fn declared_effect(
    plugin: &str,
    raw: RawEffect,
    textures: &[DeclaredTexture],
    taken: &mut Vec<String>,
) -> Result<DeclaredEffect, String> {
    let id = catalog_id(plugin, &raw.id)?;
    if raw.name.trim().is_empty() {
        return Err(format!("{id}: the effect needs a name"));
    }
    claim(taken, &id)?;
    let passes = match (raw.shader, raw.passes.is_empty()) {
        (Some(shader), true) => vec![DeclaredPass::file(package_path(&id, &shader)?)],
        (None, false) => passes::declared_passes(plugin, &id, raw.passes, textures)?,
        (Some(_), false) => {
            return Err(format!(
                "{id}: declares both `shader` and `passes`; use one"
            ));
        }
        (None, true) => return Err(format!("{id}: declares no `shader` or `passes`")),
    };
    Ok(DeclaredEffect {
        id,
        name: raw.name,
        category: raw.category,
        passes,
        params: raw.params,
        groups: raw.groups,
        stateful: raw.stateful,
        bounds: raw.bounds,
        reads_outside: raw.reads_outside,
        time_dependent: raw.time_dependent,
    })
}

/// A path inside the package: relative, and never through `..`.
fn package_path(owner: &str, raw: &str) -> Result<PathBuf, String> {
    let path = Path::new(raw);
    let mut inside = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => inside.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(format!("{owner}: {raw:?} is not a path inside the package"));
            }
        }
    }
    if inside.as_os_str().is_empty() {
        return Err(format!("{owner}: {raw:?} names no file"));
    }
    Ok(inside)
}
