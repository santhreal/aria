//! Plugin packages: a directory holding `plugin.toml` and the package's code
//! and data.
//!
//! ```text
//! color-tools/
//!   plugin.toml
//!   plugin.wasm            WASM tier, or
//!   lib/linux-x86_64.so    native tier, one library per platform
//!   shaders/*.wgsl
//!   textures/*.png         images effect passes read, as `[[textures]]` declares
//!   presets/*.ariapreset
//!   themes/*.toml
//! ```
//!
//! Reading a package reads its manifest and checks which files are present.
//! No code in the package runs. A package with neither `plugin.wasm` nor `lib/`
//! is data only: its declared effects, transitions and generators register
//! from their WGSL with no plugin code at all.

use std::borrow::Cow;
use std::path::{Path, PathBuf};

use crate::command_icon::{CommandIcon, ICON_MAX_BYTES};
use crate::commands::CommandSpec;
use crate::plugin::{Names, PluginManifest};

mod archive;
mod declared;
mod grants;
mod install;
mod manifest;

pub use archive::{
    ARCHIVE_EXTENSION, MAX_ARCHIVE_ENTRIES, MAX_UNPACKED_BYTES, SOURCE_ENTRIES, is_archive, pack,
    unpack,
};
pub use declared::{DataPlugin, register_declared};
pub use install::{Staged, stage, uninstall};
pub use manifest::{
    Access, DeclaredCommand, DeclaredEffect, DeclaredPanel, DeclaredPass, DeclaredTexture,
    DeclaredTransition, FsScope, PackageManifest, Permissions, Requirement,
};

/// The plugin API major version this host loads.
pub const API_MAJOR: u32 = 1;

/// The manifest at the root of every package.
pub const MANIFEST_FILE: &str = "plugin.toml";

/// The component of a WASM package.
pub const COMPONENT_FILE: &str = "plugin.wasm";

/// The directory holding a native package's libraries.
pub const LIBRARY_DIR: &str = "lib";

/// The directory holding a package's `.ariapreset` preset files.
pub const PRESETS_DIR: &str = "presets";

/// The directory holding a package's `.toml` theme files.
pub const THEMES_DIR: &str = "themes";

/// Largest `plugin.toml` read, in bytes.
const MAX_MANIFEST_BYTES: u64 = 1 << 20;

/// How a package's code runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tier {
    /// A shared library on the stable C ABI, for this platform.
    Native { library: PathBuf },
    /// A WebAssembly component.
    Wasm { component: PathBuf },
    /// No code: only declared contributions.
    Data,
}

/// A package read from disk.
#[derive(Debug, Clone, PartialEq)]
pub struct Package {
    /// The package directory.
    pub dir: PathBuf,
    pub manifest: PackageManifest,
    pub tier: Tier,
    /// The declared commands' icons, read and parsed, by command id.
    icons: Vec<(String, CommandIcon)>,
    /// The declared clip types, each with its icon read and parsed.
    clip_types: Vec<aria_models::ClipTypeSpec>,
    /// The package's validated `.ariapreset` files from `presets/`, sorted by path.
    pub presets: Vec<aria_models::Preset>,
    /// The package's theme files from `themes/`, sorted by path.
    pub themes: Vec<PackageTheme>,
}

/// One `themes/<id>.toml` file of a package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageTheme {
    /// The file name without `.toml`.
    pub id: String,
    /// The file's text. The host reads its tokens.
    pub text: String,
}

/// Why a package was not read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{}: {reason}", path.display())]
pub struct PackageError {
    /// The package directory or file.
    pub path: PathBuf,
    pub reason: String,
}

impl PackageError {
    fn new(path: &Path, reason: impl Into<String>) -> Self {
        Self {
            path: path.to_owned(),
            reason: reason.into(),
        }
    }
}

/// The library file name a native package ships for this platform, such as
/// `linux-x86_64.so`.
pub fn library_name() -> String {
    format!(
        "{}-{}.{}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        std::env::consts::DLL_EXTENSION
    )
}

impl Package {
    /// Read the package in `dir`.
    pub fn open(dir: &Path) -> Result<Self, PackageError> {
        let manifest_path = dir.join(MANIFEST_FILE);
        let text =
            read_manifest(&manifest_path).map_err(|reason| PackageError::new(dir, reason))?;
        let manifest = PackageManifest::parse(&text)
            .map_err(|reason| PackageError::new(&manifest_path, reason))?;
        let tier = tier(dir).map_err(|reason| PackageError::new(dir, reason))?;
        if tier == Tier::Data && !manifest.commands.is_empty() {
            return Err(PackageError::new(
                dir,
                format!(
                    "declares commands but holds no code: add {COMPONENT_FILE} or {LIBRARY_DIR}/{}",
                    library_name()
                ),
            ));
        }
        let mut icons = Vec::new();
        for command in &manifest.commands {
            let Some(icon) = &command.icon else {
                continue;
            };
            let path = dir.join(icon);
            let parsed = read_icon(&path).map_err(|reason| {
                PackageError::new(&path, format!("the icon of {}: {reason}", command.id))
            })?;
            icons.push((command.id.clone(), parsed));
        }
        let mut clip_types = Vec::with_capacity(manifest.clip_types.len());
        for declared in &manifest.clip_types {
            let mut spec = declared.spec.clone();
            if let Some(icon) = &declared.icon {
                let path = dir.join(icon);
                let parsed = read_icon(&path).map_err(|reason| {
                    PackageError::new(&path, format!("the icon of {}: {reason}", spec.id))
                })?;
                spec.icon = Some(parsed.to_clip_type_icon());
            }
            clip_types.push(spec);
        }
        let presets = read_presets(dir)?;
        let themes = read_themes(dir)?;
        Ok(Self {
            dir: dir.to_owned(),
            manifest,
            tier,
            icons,
            clip_types,
            presets,
            themes,
        })
    }

    /// The declared clip types, in declaration order, each with its icon.
    pub fn clip_types(&self) -> &[aria_models::ClipTypeSpec] {
        &self.clip_types
    }

    /// The spec a command the package's code registers as `id` is listed
    /// under: `title`, with the icon and enablement condition its
    /// `[[commands]]` entry declares.
    pub fn command_spec(&self, id: String, title: String) -> CommandSpec {
        let enablement = self
            .manifest
            .commands
            .iter()
            .find(|command| command.id == id)
            .and_then(|command| command.enablement.clone());
        let icon = self
            .icons
            .iter()
            .find(|(owner, _)| *owner == id)
            .map(|(_, icon)| icon.clone());
        CommandSpec {
            id,
            title,
            icon,
            enablement,
        }
    }

    /// The manifest the host mounts this package under.
    ///
    /// `uses` lists the services the permissions grant: `ProjectHost` for
    /// `project`, `DecoderService` and `AudioHost` for `media`, `GpuContext` and
    /// `RenderHost` for `gpu`. Settings need no permission.
    pub fn plugin_manifest(&self) -> PluginManifest {
        let manifest = &self.manifest;
        let permissions = &manifest.permissions;
        let mut uses = vec!["SettingsService".to_owned(), "SettingsSchema".to_owned()];
        if permissions.project != Access::None {
            uses.push("ProjectHost".to_owned());
        }
        if permissions.media != Access::None {
            uses.push("DecoderService".to_owned());
            uses.push("AudioHost".to_owned());
        }
        if permissions.gpu {
            uses.push("GpuContext".to_owned());
            uses.push("RenderHost".to_owned());
        }
        PluginManifest {
            id: Cow::Owned(manifest.id.clone()),
            name: Cow::Owned(manifest.name.clone()),
            version: Cow::Owned(manifest.version.clone()),
            requires: Names::Owned(
                manifest
                    .requires
                    .iter()
                    .map(|requirement| requirement.id.clone())
                    .collect(),
            ),
            uses: Names::Owned(uses),
        }
    }

    /// The package's own storage: `<data root>/plugin-data/<id>`.
    pub fn data_dir(&self, data_root: &Path) -> PathBuf {
        data_root.join("plugin-data").join(&self.manifest.id)
    }
}

fn read_manifest(path: &Path) -> Result<String, String> {
    use std::io::Read as _;

    let file = std::fs::File::open(path).map_err(|err| match err.kind() {
        std::io::ErrorKind::NotFound => format!("no {MANIFEST_FILE}"),
        _ => format!("{MANIFEST_FILE}: {err}"),
    })?;
    let mut text = String::new();
    file.take(MAX_MANIFEST_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(|err| format!("{MANIFEST_FILE}: {err}"))?;
    if text.len() as u64 > MAX_MANIFEST_BYTES {
        return Err(format!(
            "{MANIFEST_FILE} is larger than {MAX_MANIFEST_BYTES} bytes"
        ));
    }
    Ok(text)
}

fn read_icon(path: &Path) -> Result<CommandIcon, String> {
    use std::io::Read as _;

    let file = std::fs::File::open(path).map_err(|err| err.to_string())?;
    let mut bytes = Vec::new();
    file.take(ICON_MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|err| err.to_string())?;
    if bytes.len() > ICON_MAX_BYTES {
        return Err(format!("the icon is larger than {ICON_MAX_BYTES} bytes"));
    }
    CommandIcon::from_svg(&bytes)
}

/// The files in `<dir>/<sub>` with extension `ext`, sorted by path, each read
/// whole. A missing directory holds none; a file larger than `max` bytes fails
/// with its path.
fn data_files(
    dir: &Path,
    sub: &str,
    ext: &str,
    max: u64,
) -> Result<Vec<(PathBuf, Vec<u8>)>, PackageError> {
    use std::io::Read as _;

    let data_dir = dir.join(sub);
    if !data_dir.exists() {
        return Ok(Vec::new());
    }
    if !data_dir.is_dir() {
        return Err(PackageError::new(
            &data_dir,
            format!("{sub} must be a directory"),
        ));
    }
    let entries = std::fs::read_dir(&data_dir)
        .map_err(|err| PackageError::new(&data_dir, err.to_string()))?;
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|err| PackageError::new(&data_dir, err.to_string()))?;
        let path = entry.path();
        if path.extension().is_some_and(|found| found == ext) {
            paths.push(path);
        }
    }
    paths.sort();
    let mut files = Vec::with_capacity(paths.len());
    for path in paths {
        let file =
            std::fs::File::open(&path).map_err(|err| PackageError::new(&path, err.to_string()))?;
        let mut bytes = Vec::new();
        file.take(max + 1)
            .read_to_end(&mut bytes)
            .map_err(|err| PackageError::new(&path, err.to_string()))?;
        if bytes.len() as u64 > max {
            return Err(PackageError::new(&path, format!("larger than {max} bytes")));
        }
        files.push((path, bytes));
    }
    Ok(files)
}

fn read_presets(dir: &Path) -> Result<Vec<aria_models::Preset>, PackageError> {
    let files = data_files(
        dir,
        PRESETS_DIR,
        aria_models::PRESET_FILE_EXTENSION,
        aria_models::PRESET_MAX_BYTES,
    )?;
    let mut presets = Vec::with_capacity(files.len());
    for (path, bytes) in files {
        let preset = aria_models::Preset::parse(&bytes)
            .map_err(|err| PackageError::new(&path, err.to_string()))?;
        if presets
            .iter()
            .any(|existing: &aria_models::Preset| existing.name.eq_ignore_ascii_case(&preset.name))
        {
            return Err(PackageError::new(
                &path,
                format!("duplicate preset name `{}` in package", preset.name),
            ));
        }
        presets.push(preset);
    }
    Ok(presets)
}

fn read_themes(dir: &Path) -> Result<Vec<PackageTheme>, PackageError> {
    let files = data_files(dir, THEMES_DIR, "toml", crate::THEME_MAX_BYTES as u64)?;
    let mut themes = Vec::with_capacity(files.len());
    for (path, bytes) in files {
        let id = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .ok_or_else(|| PackageError::new(&path, "the file name is not UTF-8"))?
            .to_owned();
        if let Some(fault) = crate::themes::id_fault(&id) {
            return Err(PackageError::new(&path, fault));
        }
        let text = String::from_utf8(bytes)
            .map_err(|_| PackageError::new(&path, "a theme file is UTF-8 text"))?;
        themes.push(PackageTheme { id, text });
    }
    Ok(themes)
}

/// Which tier the files in `dir` make the package.
fn tier(dir: &Path) -> Result<Tier, String> {
    let component = dir.join(COMPONENT_FILE);
    let libraries = dir.join(LIBRARY_DIR);
    match (component.is_file(), libraries.is_dir()) {
        (true, true) => Err(format!(
            "holds both {COMPONENT_FILE} and {LIBRARY_DIR}/; a package is one tier"
        )),
        (true, false) => Ok(Tier::Wasm { component }),
        (false, false) => Ok(Tier::Data),
        (false, true) => {
            let name = library_name();
            let library = libraries.join(&name);
            if library.is_file() {
                return Ok(Tier::Native { library });
            }
            let mut found: Vec<String> = std::fs::read_dir(&libraries)
                .map_err(|err| format!("{LIBRARY_DIR}/: {err}"))?
                .filter_map(Result::ok)
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect();
            found.sort();
            Err(if found.is_empty() {
                format!("{LIBRARY_DIR}/ is empty; this platform loads {LIBRARY_DIR}/{name}")
            } else {
                format!(
                    "no {LIBRARY_DIR}/{name} for this platform; the package holds {}",
                    found.join(", ")
                )
            })
        }
    }
}

/// Read every package under `dir`, sorted by path.
///
/// A missing `dir` holds no packages. A subdirectory is a package. A plugin
/// library or an `.ariaplugin` archive outside a package directory is an
/// error naming what to do instead. Other files and hidden entries are not
/// packages and are passed over.
pub fn discover(dir: &Path) -> Vec<Result<Package, PackageError>> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(err) => return vec![Err(PackageError::new(dir, err.to_string()))],
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            !path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with('.'))
        })
        .collect();
    paths.sort();
    paths
        .into_iter()
        .filter_map(|path| {
            if path.is_dir() {
                return Some(Package::open(&path));
            }
            let extension = path
                .extension()
                .map(|ext| ext.to_string_lossy().to_ascii_lowercase());
            match extension.as_deref() {
                Some("ariaplugin") => Some(Err(PackageError::new(
                    &path,
                    "an archive is not loaded in place; install it from the Plugins panel",
                ))),
                Some("so" | "dylib" | "dll") => Some(Err(PackageError::new(
                    &path,
                    format!(
                        "a plugin library loads from a package: put it at \
                         <package>/{LIBRARY_DIR}/{} beside a {MANIFEST_FILE}",
                        library_name()
                    ),
                ))),
                _ => None,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests;

#[cfg(test)]
#[path = "command_tests.rs"]
mod command_tests;

#[cfg(test)]
#[path = "graph_tests.rs"]
mod graph_tests;
