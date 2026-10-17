//! Integration gate for every plugin package under `examples/`.
//!
//! # Why
//!
//! Closes the defect class where documentation or example packages under
//! `examples/` drift from the manifest parser (`PackageManifest::parse`),
//! preset validator (`Preset::parse`), icon parser (`CommandIcon::from_svg`),
//! or compositor WGSL validator (`register_declared`). Every directory under
//! `examples/` holding a `plugin.toml` is discovered from the filesystem at
//! run time with no hardcoded directory list, so adding a new example package
//! turns this suite red until `Package::open` succeeds and, for data packages,
//! `DataPlugin` mounts cleanly on a fresh `PluginContext`.
//!
//! What this does not catch: guest runtime logic inside `plugin.wasm` or
//! native `cdylib` callbacks after `Package::open` (covered by the WASM and
//! native ABI test suites).

use std::path::{Path, PathBuf};

use aria_plugin::external::LIBRARY_EXTENSION;
use aria_plugin::package::{
    COMPONENT_FILE, DataPlugin, LIBRARY_DIR, MANIFEST_FILE, Package, Tier, library_name,
};
use aria_plugin::{Contribution, ContributionKind, PluginContext};
use tempfile::TempDir;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonicalize workspace root")
}

/// Discover every directory under `examples_root` that contains `plugin.toml`,
/// sorted by directory path.
fn discover_example_package_dirs(examples_root: &Path) -> Vec<PathBuf> {
    let entries = std::fs::read_dir(examples_root)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", examples_root.display()));
    let mut dirs: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir() && path.join(MANIFEST_FILE).is_file())
        .collect();
    dirs.sort();
    dirs
}

fn find_compiled_cdylib(stem: &str) -> PathBuf {
    let exe = std::env::current_exe().expect("current_exe");
    let deps_dir = exe.parent().expect("test binary parent dir");
    let target_dir = deps_dir.parent().unwrap_or(deps_dir);
    let prefix = if cfg!(target_os = "windows") {
        ""
    } else {
        "lib"
    };
    let exact_name = format!("{prefix}{stem}.{LIBRARY_EXTENSION}");
    let direct = target_dir.join(&exact_name);
    if direct.exists() {
        return direct;
    }
    if let Ok(entries) = std::fs::read_dir(deps_dir) {
        let mut matches: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| {
                p.extension().is_some_and(|ext| ext == LIBRARY_EXTENSION)
                    && p.file_stem().and_then(|s| s.to_str()).is_some_and(|s| {
                        s == format!("{prefix}{stem}") || s.starts_with(&format!("{prefix}{stem}-"))
                    })
            })
            .collect();
        matches.sort();
        if let Some(found) = matches.pop() {
            return found;
        }
    }
    panic!(
        "compiled cdylib `{exact_name}` not found in {} or {}",
        target_dir.display(),
        deps_dir.display()
    );
}

fn copy_dir_recursive(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).expect("create destination dir");
    for entry in std::fs::read_dir(src).expect("read source dir") {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        let file_name = entry.file_name();
        if file_name == "target" {
            continue;
        }
        let target = dst.join(&file_name);
        if path.is_dir() {
            copy_dir_recursive(&path, &target);
        } else {
            std::fs::copy(&path, &target).expect("copy package file");
        }
    }
}

/// Open an example package directory. If the directory is a native crate source
/// tree (`Cargo.toml` present without `plugin.wasm` or `lib/`), stage its files
/// alongside its compiled `cdylib` in `scratch` so `Package::open` checks the
/// complete native package layout.
fn open_example_package(dir: &Path, scratch: &Path) -> Result<Package, String> {
    let cargo_toml = dir.join("Cargo.toml");
    if cargo_toml.is_file()
        && !dir.join(COMPONENT_FILE).is_file()
        && !dir.join(LIBRARY_DIR).is_dir()
    {
        let text = std::fs::read_to_string(&cargo_toml).map_err(|err| err.to_string())?;
        let parsed: toml::Value = toml::from_str(&text).map_err(|err| err.to_string())?;
        let crate_name = parsed
            .get("package")
            .and_then(|pkg| pkg.get("name"))
            .and_then(toml::Value::as_str)
            .ok_or_else(|| format!("{}: missing [package].name", cargo_toml.display()))?;
        let stem = crate_name.replace('-', "_");
        let cdylib = find_compiled_cdylib(&stem);
        let staged = scratch.join(
            dir.file_name()
                .unwrap_or_else(|| std::ffi::OsStr::new("pkg")),
        );
        copy_dir_recursive(dir, &staged);
        let lib_dir = staged.join(LIBRARY_DIR);
        std::fs::create_dir_all(&lib_dir).map_err(|err| err.to_string())?;
        std::fs::copy(&cdylib, lib_dir.join(library_name())).map_err(|err| err.to_string())?;
        return Package::open(&staged).map_err(|err| err.to_string());
    }
    Package::open(dir).map_err(|err| err.to_string())
}

#[test]
fn every_example_package_opens_and_data_package_mounts_exact_contributions() {
    // Link `aria_notes_plugin` so Cargo builds its cdylib before this test runs.
    let _ = aria_notes_plugin::seen_commands();

    let examples_dir = workspace_root().join("examples");
    let dirs = discover_example_package_dirs(&examples_dir);
    assert!(
        dirs.len() >= 3,
        "expected at least 3 example packages under {}, found {:?}",
        examples_dir.display(),
        dirs
    );

    let scratch = TempDir::new().expect("create scratch dir");
    let mut saw_data = false;
    let mut saw_wasm = false;
    let mut saw_native = false;

    for dir in &dirs {
        let pkg = open_example_package(dir, scratch.path()).unwrap_or_else(|err| {
            panic!("example package {} failed to open: {err}", dir.display())
        });
        match &pkg.tier {
            Tier::Data => {
                saw_data = true;
                assert_eq!(pkg.manifest.id, "com.example.data-pack");
                assert_eq!(pkg.manifest.name, "Example Data Pack");

                // Mount through DataPlugin on a fresh PluginContext, matching
                // InstalledPlugins::mount for Tier::Data.
                let ctx = PluginContext::new();
                ctx.mount(&DataPlugin::new(pkg.clone()))
                    .expect("DataPlugin mounts cleanly on a fresh PluginContext");
                assert!(ctx.is_mounted("com.example.data-pack"));

                // Assert exact registered effect.
                let effect = aria_models::effect_spec("com.example.data-pack.warm-tint")
                    .expect("declared effect registered in catalog");
                assert_eq!(effect.label, "Warm Tint");
                assert!(!effect.generator);
                assert_eq!(effect.params.len(), 1);
                assert_eq!(effect.params[0].name, "gain");
                assert_eq!(effect.params[0].default, 1.0);
                assert_eq!(effect.params[0].min, 0.0);
                assert_eq!(effect.params[0].max, 2.0);
                assert!(aria_compositor::renders_effect(
                    "com.example.data-pack.warm-tint"
                ));

                // Assert exact registered generator.
                let generator = aria_models::effect_spec("com.example.data-pack.uv-gradient")
                    .expect("declared generator registered in catalog");
                assert_eq!(generator.label, "UV Gradient");
                assert!(generator.generator);
                assert!(aria_compositor::renders_effect(
                    "com.example.data-pack.uv-gradient"
                ));

                // Assert exact registered transition.
                let transition =
                    aria_models::transition_spec("com.example.data-pack.cross-dissolve")
                        .expect("declared transition registered in catalog");
                assert_eq!(transition.label, "Cross Dissolve");
                assert!(aria_compositor::renders_transition(
                    "com.example.data-pack.cross-dissolve"
                ));

                // Assert exact registered preset.
                let presets = ctx.presets();
                assert_eq!(presets.len(), 1);
                assert_eq!(presets[0].plugin.as_ref(), "Example Data Pack");
                assert_eq!(presets[0].preset.name, "Soft Vignette");
                assert_eq!(presets[0].preset.effects.len(), 1);
                assert_eq!(presets[0].preset.effects[0].effect_id, "vignette");

                // Assert MountedPluginInfo contributions list every item.
                let mounted = ctx.mounted();
                assert_eq!(mounted.len(), 1);
                assert_eq!(
                    mounted[0].contributions,
                    vec![
                        Contribution {
                            kind: ContributionKind::Effect,
                            id: "com.example.data-pack.warm-tint".into(),
                        },
                        Contribution {
                            kind: ContributionKind::Effect,
                            id: "com.example.data-pack.uv-gradient".into(),
                        },
                        Contribution {
                            kind: ContributionKind::Transition,
                            id: "com.example.data-pack.cross-dissolve".into(),
                        },
                        Contribution {
                            kind: ContributionKind::Preset,
                            id: "Soft Vignette".into(),
                        },
                    ]
                );

                // Unmounting removes all registered contributions.
                ctx.unmount("com.example.data-pack")
                    .expect("unmount data package");
                assert!(aria_models::effect_spec("com.example.data-pack.warm-tint").is_none());
                assert!(aria_models::effect_spec("com.example.data-pack.uv-gradient").is_none());
                assert!(
                    aria_models::transition_spec("com.example.data-pack.cross-dissolve").is_none()
                );
                assert!(ctx.presets().is_empty());
            }
            Tier::Wasm { .. } => {
                saw_wasm = true;
            }
            Tier::Native { .. } => {
                saw_native = true;
            }
        }
    }

    assert!(saw_data, "expected a Tier::Data example package");
    assert!(saw_wasm, "expected a Tier::Wasm example package");
    assert!(saw_native, "expected a Tier::Native example package");
}

#[test]
fn discovery_catches_new_broken_example_directory() {
    let fake_examples = TempDir::new().expect("create fake examples root");
    let broken = fake_examples.path().join("broken-plugin");
    std::fs::create_dir_all(&broken).expect("create broken-plugin dir");
    std::fs::write(
        broken.join(MANIFEST_FILE),
        "[plugin]\nid = \"not_reverse_domain\"\nname = \"Broken\"\nversion = \"0.1.0\"\napi = 1\n",
    )
    .expect("write invalid plugin.toml");

    let discovered = discover_example_package_dirs(fake_examples.path());
    assert_eq!(discovered, vec![broken.clone()]);
    let scratch = TempDir::new().expect("scratch");
    let err = open_example_package(&broken, scratch.path()).unwrap_err();
    assert!(
        err.contains("not a reverse-domain id"),
        "expected reverse-domain id refusal, got: {err}"
    );
}
