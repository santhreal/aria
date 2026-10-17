//! Plugin themes: what `register_theme` accepts and refuses, how long a
//! registration lasts, and how a package's `themes/` directory reaches it.
//!
//! The class these close: a theme id that cannot round-trip through the saved
//! `<plugin id>/<theme id>` key, a theme that outlives its plugin, and a
//! package file that is read without its limits. The host's token parse is
//! covered in aria-ui.

use super::*;
use crate::package::{DataPlugin, MANIFEST_FILE, Package, THEMES_DIR};
use crate::plugin::{Plugin, PluginManifest};

const DUSK: &str = "name = \"Dusk\"\n[dark]\naccent = \"#336699\"\n";

struct ThemePlugin {
    manifest: PluginManifest,
    themes: Vec<(&'static str, &'static str)>,
}

impl Plugin for ThemePlugin {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        for (id, text) in &self.themes {
            ctx.register_theme(id, *text)?;
        }
        Ok(())
    }
}

fn plugin(id: &'static str, themes: Vec<(&'static str, &'static str)>) -> ThemePlugin {
    ThemePlugin {
        manifest: PluginManifest::new(id, "Theme Pack", "1.0.0", &[], &[]),
        themes,
    }
}

fn refusal(result: Result<Registration, PluginError>) -> String {
    match result {
        Err(PluginError::ThemeRefused(reason)) => reason,
        Err(other) => panic!("refused as {other:?}, not a theme refusal"),
        Ok(_) => panic!("registered"),
    }
}

#[test]
fn a_mounted_theme_lists_under_its_key_until_the_plugin_unmounts() {
    let ctx = PluginContext::new();
    assert_eq!(ctx.themes_revision(), 0);
    ctx.mount(&plugin("com.example.themes", vec![("dusk", DUSK)]))
        .expect("mounts");
    let mounted = ctx.themes_revision();
    assert!(mounted > 0);
    let themes = ctx.themes();
    assert_eq!(themes.len(), 1);
    assert_eq!(themes[0].key(), "com.example.themes/dusk");
    assert_eq!(&*themes[0].plugin, "Theme Pack");
    assert_eq!(&*themes[0].text, DUSK);

    ctx.unmount("com.example.themes").expect("unmounts");
    assert!(ctx.themes().is_empty());
    assert!(ctx.themes_revision() > mounted);
}

#[test]
fn an_id_is_unique_within_its_plugin_only() {
    let ctx = PluginContext::new();
    let first = plugin("com.example.one", vec![("dusk", DUSK)]);
    ctx.mount(&first).expect("mounts");
    let again = ctx.view(&first.manifest).register_theme("dusk", DUSK);
    assert!(refusal(again).contains("already registered a theme named `dusk`"));
    // The same id from another plugin is a different key.
    ctx.mount(&plugin("com.example.two", vec![("dusk", DUSK)]))
        .expect("a second plugin's dusk mounts");
    let keys: Vec<String> = ctx.themes().iter().map(PluginTheme::key).collect();
    assert_eq!(keys, ["com.example.one/dusk", "com.example.two/dusk"]);
}

#[test]
fn malformed_ids_and_long_texts_are_refused() {
    let ctx = PluginContext::new();
    let longest = "a".repeat(THEME_ID_MAX_BYTES);
    let too_long = "a".repeat(THEME_ID_MAX_BYTES + 1);
    for id in [
        "",
        ".hidden",
        "a/b",
        "a\\b",
        "a b",
        "dusk:",
        "é",
        too_long.as_str(),
    ] {
        let reason = refusal(ctx.register_theme(id, DUSK));
        assert!(reason.contains("theme"), "{id:?}: {reason}");
    }
    for id in ["dusk", "Dusk-2", "dusk_night.v2", longest.as_str()] {
        ctx.register_theme(id, DUSK)
            .unwrap_or_else(|err| panic!("{id:?}: {err}"));
    }
    let full = "#".repeat(THEME_MAX_BYTES);
    ctx.register_theme("full", full.as_str())
        .expect("a text of exactly the limit registers");
    let over = "#".repeat(THEME_MAX_BYTES + 1);
    let reason = refusal(ctx.register_theme("over", over.as_str()));
    assert!(reason.contains(&THEME_MAX_BYTES.to_string()), "{reason}");
}

fn package_dir(scratch: &std::path::Path) -> std::path::PathBuf {
    let dir = scratch.join("com.example.pkg-themes");
    std::fs::create_dir_all(dir.join(THEMES_DIR)).expect("themes dir");
    std::fs::write(
        dir.join(MANIFEST_FILE),
        "[plugin]\nid = \"com.example.pkg-themes\"\nname = \"Packaged Themes\"\nversion = \"0.1.0\"\napi = 1\n",
    )
    .expect("manifest");
    dir
}

#[test]
fn a_package_registers_each_toml_in_its_themes_dir() {
    let scratch = tempfile::tempdir().expect("scratch");
    let dir = package_dir(scratch.path());
    let themes = dir.join(THEMES_DIR);
    std::fs::write(themes.join("night.toml"), "[dark]\nbg = \"#000\"\n").expect("night");
    std::fs::write(themes.join("dusk.toml"), DUSK).expect("dusk");
    std::fs::write(themes.join("notes.txt"), "not a theme").expect("notes");

    let package = Package::open(&dir).expect("opens");
    let ids: Vec<&str> = package
        .themes
        .iter()
        .map(|theme| theme.id.as_str())
        .collect();
    assert_eq!(ids, ["dusk", "night"]);

    let ctx = PluginContext::new();
    ctx.mount(&DataPlugin::new(package)).expect("mounts");
    let keys: Vec<String> = ctx.themes().iter().map(PluginTheme::key).collect();
    assert_eq!(
        keys,
        [
            "com.example.pkg-themes/dusk",
            "com.example.pkg-themes/night"
        ]
    );
    ctx.unmount("com.example.pkg-themes").expect("unmounts");
    assert!(ctx.themes().is_empty());
}

#[test]
fn a_package_theme_file_out_of_bounds_refuses_the_package_naming_the_file() {
    let scratch = tempfile::tempdir().expect("scratch");
    let dir = package_dir(scratch.path());
    let themes = dir.join(THEMES_DIR);

    let badly_named = themes.join("bad name.toml");
    std::fs::write(&badly_named, DUSK).expect("write");
    let err = Package::open(&dir).expect_err("a space in the id");
    assert_eq!(err.path, badly_named);
    std::fs::remove_file(&badly_named).expect("remove");

    let oversized = themes.join("huge.toml");
    std::fs::write(&oversized, "#".repeat(THEME_MAX_BYTES + 1)).expect("write");
    let err = Package::open(&dir).expect_err("larger than the limit");
    assert_eq!(err.path, oversized);
    assert!(err.reason.contains(&THEME_MAX_BYTES.to_string()), "{err}");
    std::fs::remove_file(&oversized).expect("remove");

    let binary = themes.join("binary.toml");
    std::fs::write(&binary, [0xff, 0xfe, 0x00]).expect("write");
    let err = Package::open(&dir).expect_err("not UTF-8");
    assert_eq!(err.path, binary);
}
