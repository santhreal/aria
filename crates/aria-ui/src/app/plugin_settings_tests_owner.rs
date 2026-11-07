//! The settings plugin and the session share one `config.toml`.
//!
//! The class closed here: two writers of one file where one writes a stale
//! copy over the other. The settings plugin read the file once at mount and
//! saved that copy whole, so a plugin field saved after **Move storage…**,
//! **Redirect…**, or a quota step put the old root, cache path, or quota back,
//! and the plugin went on reading the values from mount. The shipped plugin
//! list also pointed it at the default profile's file whatever profile the
//! session runs on. Every session writer below, and one write to the file
//! that skips the session's reload, runs before a plugin save, and the file
//! and the plugin's reader are both checked after it.
//!
//! Not caught: two processes saving the file at once. For a key both changed,
//! the last save wins.

use std::path::Path;

use aria_plugin::{PluginContext, Registration, SettingKind, SettingsService};
use aria_settings::{DEFAULT_DOWNLOAD_QUOTA_MIB, Settings};

use super::{declare, spec};
use crate::app::AppState;

const KEY: &str = "example.notes.dir";

/// A session whose settings plugin serves the session's own `config.toml`,
/// as the shipped plugin list mounts it, with one plugin field declared.
fn session() -> (AppState, Registration) {
    let dir = crate::scratch_pool::scratch("settings-owner");
    let ctx = PluginContext::new();
    let plugin = crate::plugins::builtin_by_id("aria.settings", &dir).expect("a settings plugin");
    ctx.mount(plugin.as_ref())
        .expect("the settings plugin mounts");
    let state = AppState::new(ctx, dir).expect("state");
    let section = declare(&state, vec![spec(KEY, "Folder", SettingKind::Text, None)]);
    (state, section)
}

fn on_disk(state: &AppState) -> Settings {
    aria_settings::load(&state.config_path()).expect("the file loads")
}

/// A session write, and whether a loaded file still holds it.
type Writer = (
    &'static str,
    fn(&mut AppState, &Path) -> Result<(), String>,
    fn(&Settings, &Path) -> bool,
);

fn writers() -> [Writer; 4] {
    [
        (
            "Move storage…",
            |state, dir| state.relocate_storage_root(dir.join("root-b")),
            |file, dir| file.storage.root.as_deref() == Some(dir.join("root-b").as_path()),
        ),
        (
            "Redirect…",
            |state, dir| state.set_cache_override("proxies", dir.join("own-proxies")),
            |file, dir| {
                file.storage.paths.get("proxies") == Some(dir.join("own-proxies").as_path())
            },
        ),
        (
            "quota step",
            |state, _| state.set_download_quota_mib(DEFAULT_DOWNLOAD_QUOTA_MIB + 512),
            |file, _| file.storage.download_quota_mib == DEFAULT_DOWNLOAD_QUOTA_MIB + 512,
        ),
        (
            "a write that skips the reload",
            |state, _| {
                let path = state.config_path();
                let mut file = aria_settings::load(&path)?;
                file.cloud.base_url = "https://elsewhere.example".into();
                Ok(aria_settings::save(&path, &file)?)
            },
            |file, _| file.cloud.base_url == "https://elsewhere.example",
        ),
    ]
}

#[test]
fn a_plugin_save_keeps_every_session_write() {
    for (name, write, holds) in writers() {
        let (mut state, _section) = session();
        let dir = state.data_dir.clone();
        write(&mut state, &dir).unwrap_or_else(|err| panic!("{name}: {err}"));
        assert!(
            holds(&on_disk(&state), &dir),
            "{name} did not reach the file"
        );

        state
            .set_plugin_setting(KEY, "notes")
            .expect("a plugin save");

        let file = on_disk(&state);
        assert!(holds(&file, &dir), "a plugin save wrote over {name}");
        assert_eq!(file.plugin_value(KEY), Some("notes"), "after {name}");
    }
}

#[test]
fn a_plugin_reads_what_the_session_wrote_since_mount() {
    let (mut state, _section) = session();
    let settings = state.ctx.get::<dyn SettingsService>();
    assert_eq!(
        settings.get("storage.download_quota_mib"),
        Some(serde_json::json!(DEFAULT_DOWNLOAD_QUOTA_MIB))
    );
    // An unsaved plugin write stays readable across the session's write.
    settings.set(KEY, serde_json::Value::String("draft".into()));
    state
        .set_download_quota_mib(DEFAULT_DOWNLOAD_QUOTA_MIB + 512)
        .expect("a quota step");
    assert_eq!(
        settings.get("storage.download_quota_mib"),
        Some(serde_json::json!(DEFAULT_DOWNLOAD_QUOTA_MIB + 512)),
        "the plugin reads the quota from mount"
    );
    assert_eq!(
        settings.get(KEY),
        Some(serde_json::Value::String("draft".into())),
        "re-reading the file dropped an unsaved plugin write"
    );
}

#[test]
fn the_shipped_settings_plugin_serves_the_session_profile() {
    let dir = crate::scratch_pool::scratch("settings-profile");
    let mut file = Settings::default();
    file.cloud.base_url = "https://profile.example".into();
    aria_settings::save(&aria_settings::config_in(&dir), &file).expect("write the config");
    let ctx = PluginContext::new();
    let plugin = crate::plugins::builtin_by_id("aria.settings", &dir).expect("a settings plugin");
    ctx.mount(plugin.as_ref())
        .expect("the settings plugin mounts");
    assert_eq!(
        ctx.get::<dyn SettingsService>().get("cloud.base_url"),
        Some(serde_json::Value::String("https://profile.example".into())),
        "the plugin serves another profile's config"
    );
}
