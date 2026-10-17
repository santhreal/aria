//! `aria plugin new` — scaffold a new WASM or native plugin source package.

use std::fs;
use std::path::{Path, PathBuf};

use aria_plugin::package::{MANIFEST_FILE, PackageManifest};

use crate::args::Args;
use crate::fail::Failure;

pub(crate) const FLAGS: &[&str] = &["tier", "dir", "sdk"];

const HOST_WIT: &str = include_str!("../../../aria-plugin-wasm/wit/plugin.wit");
const EFFECT_WGSL: &str = include_str!("templates/effect.wgsl");
const MANIFEST_TPL: &str = include_str!("templates/plugin.toml.tpl");
const CARGO_TPL: &str = include_str!("templates/Cargo.toml.tpl");
const WASM_LIB_TPL: &str = include_str!("templates/wasm_lib.rs.tpl");
const NATIVE_LIB_TPL: &str = include_str!("templates/native_lib.rs.tpl");

/// Target execution tier for a newly created plugin package.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NewTier {
    Wasm,
    Native,
}

impl NewTier {
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::Wasm => "wasm",
            Self::Native => "native",
        }
    }

    pub(crate) fn parse(raw: &str) -> Result<Self, Failure> {
        match raw {
            "wasm" => Ok(Self::Wasm),
            "native" => Ok(Self::Native),
            _ => Err(Failure::Usage(format!(
                "--tier wants wasm or native, got \"{raw}\""
            ))),
        }
    }
}

pub(crate) fn run(args: &Args) -> Result<(), Failure> {
    let Some(id) = args.positional.get(1) else {
        return Err(Failure::Usage("a plugin id is required".into()));
    };
    if let Some(extra) = args.positional.get(2) {
        return Err(Failure::Usage(format!("unexpected argument \"{extra}\"")));
    }

    let tier = match args.value("tier")? {
        Some(raw) => NewTier::parse(raw)?,
        None => NewTier::Wasm,
    };
    let parent = match args.value("dir")? {
        Some(raw) => PathBuf::from(raw),
        None => PathBuf::from("."),
    };
    let sdk = args.value("sdk")?.map(PathBuf::from);

    let created = create_package(id, tier, &parent, sdk.as_deref())?;
    println!("created      {}", created.display());
    println!("next         aria plugin dev {}", created.display());
    println!("             aria plugin pack {}", created.display());
    Ok(())
}

/// Create `<parent>/<id>` with a manifest, sample effect, sample command, and
/// crate sources for `tier`.
pub(crate) fn create_package(
    id: &str,
    tier: NewTier,
    parent: &Path,
    sdk: Option<&Path>,
) -> Result<PathBuf, Failure> {
    let slug = id.rsplit('.').next().unwrap_or(id);
    let display_name = display_name_from_slug(slug);
    let shader_rel = format!("shaders/{slug}.wgsl");
    let command_id = format!("{id}.run");
    let command_title = format!("Run {display_name}");

    let manifest_text = render_manifest(
        id,
        &display_name,
        slug,
        &shader_rel,
        &command_id,
        &command_title,
    );
    // Validate the id and manifest through `PackageManifest::parse` before
    // touching the filesystem so `aria-plugin` remains the single owner of id
    // and manifest rules.
    PackageManifest::parse(&manifest_text).map_err(Failure::Failed)?;

    let target_dir = parent.join(id);
    if target_dir.exists() || fs::symlink_metadata(&target_dir).is_ok() {
        return Err(Failure::Failed(format!(
            "{}: already exists",
            target_dir.display()
        )));
    }

    let dependency_line = match tier {
        NewTier::Wasm => {
            if let Some(sdk_path) = sdk {
                let _ = sdk_path
                    .canonicalize()
                    .map_err(|err| Failure::Failed(format!("{}: {err}", sdk_path.display())))?;
            }
            "wit-bindgen = \"0.51\"".to_owned()
        }
        NewTier::Native => {
            let built_tree_sdk = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../aria-sdk"));
            let spec = resolve_sdk_dependency_with(sdk, built_tree_sdk)?;
            format!("aria-sdk = {spec}")
        }
    };

    let crate_name = crate_name_from_slug(slug);
    let cargo_toml = CARGO_TPL
        .replace("__CRATE_NAME__", &crate_name)
        .replace("__TIER__", tier.key())
        .replace("__DEPENDENCY_LINE__", &dependency_line);

    let lib_rs = match tier {
        NewTier::Wasm => WASM_LIB_TPL,
        NewTier::Native => NATIVE_LIB_TPL,
    }
    .replace("__PLUGIN_ID__", id)
    .replace("__COMMAND_ID__", &command_id)
    .replace("__COMMAND_TITLE__", &command_title);

    let gitignore = match tier {
        NewTier::Wasm => "target/\n*.ariaplugin\nplugin.wasm\n",
        NewTier::Native => "target/\n*.ariaplugin\nlib/\n",
    };

    write_file(&target_dir.join(MANIFEST_FILE), &manifest_text)?;
    write_file(&target_dir.join("Cargo.toml"), &cargo_toml)?;
    write_file(&target_dir.join(".gitignore"), gitignore)?;
    write_file(&target_dir.join("src/lib.rs"), &lib_rs)?;
    write_file(&target_dir.join(&shader_rel), EFFECT_WGSL)?;
    if tier == NewTier::Wasm {
        write_file(&target_dir.join("wit/plugin.wit"), HOST_WIT)?;
    }

    Ok(target_dir)
}

/// Resolve the `aria-sdk` dependency table for a native plugin's `Cargo.toml`.
///
/// Order:
/// 1. `--sdk PATH` (canonicalized path dependency).
/// 2. The `aria-sdk` crate in the source tree this binary was compiled from,
///    when it still exists at run time.
/// 3. Git dependency on the workspace repository URL (`branch = "main"`).
pub(crate) fn resolve_sdk_dependency_with(
    sdk_arg: Option<&Path>,
    built_tree_sdk: &Path,
) -> Result<String, Failure> {
    if let Some(sdk_path) = sdk_arg {
        let canonical = sdk_path
            .canonicalize()
            .map_err(|err| Failure::Failed(format!("{}: {err}", sdk_path.display())))?;
        if !canonical.join("Cargo.toml").is_file() {
            return Err(Failure::Failed(format!(
                "{}: no Cargo.toml found for --sdk",
                canonical.display()
            )));
        }
        return Ok(format!("{{ path = {:?} }}", canonical.to_string_lossy()));
    }
    if built_tree_sdk.join("Cargo.toml").is_file() {
        let resolved = built_tree_sdk
            .canonicalize()
            .unwrap_or_else(|_| built_tree_sdk.to_path_buf());
        return Ok(format!("{{ path = {:?} }}", resolved.to_string_lossy()));
    }
    Ok(format!(
        "{{ git = {:?}, branch = \"main\" }}",
        env!("CARGO_PKG_REPOSITORY")
    ))
}

fn display_name_from_slug(slug: &str) -> String {
    let words: Vec<String> = slug
        .split('-')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => {
                    let mut word = String::with_capacity(part.len());
                    word.push(first.to_ascii_uppercase());
                    word.push_str(chars.as_str());
                    word
                }
                None => String::new(),
            }
        })
        .collect();
    if words.is_empty() {
        slug.to_owned()
    } else {
        words.join(" ")
    }
}

fn crate_name_from_slug(slug: &str) -> String {
    if slug.bytes().next().is_some_and(|b| b.is_ascii_digit()) {
        format!("plugin-{slug}")
    } else {
        slug.to_owned()
    }
}

fn render_manifest(
    id: &str,
    display_name: &str,
    effect_id: &str,
    shader_rel: &str,
    command_id: &str,
    command_title: &str,
) -> String {
    MANIFEST_TPL
        .replace("__PLUGIN_ID__", id)
        .replace("__PLUGIN_NAME__", display_name)
        .replace("__EFFECT_ID__", effect_id)
        .replace("__SHADER_PATH__", shader_rel)
        .replace("__COMMAND_ID__", command_id)
        .replace("__COMMAND_TITLE__", command_title)
}

fn write_file(path: &Path, contents: &str) -> Result<(), Failure> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .map_err(|err| Failure::Failed(format!("{}: {err}", parent.display())))?;
    }
    fs::write(path, contents).map_err(|err| Failure::Failed(format!("{}: {err}", path.display())))
}
