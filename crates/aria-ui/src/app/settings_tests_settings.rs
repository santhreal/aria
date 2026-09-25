//! Settings: 4 of the cases in [`super`].
//!
//! Split out of `settings_tests.rs` to stay under the file ceiling. A pure
//! move: only `#[test]` fns live here. Every fixture and helper stays in the
//! parent, reached through `use super::*`, so no two children of this parent
//! reference each other.

use super::*;

/// The AI status is `aria-ai`'s own verdict, and it names what is missing.
#[test]
fn the_ai_status_names_what_is_missing_and_then_reads_configured() {
    let (mut state, _) = with_clip();
    state
        .set_ai_source(AiSource::Local)
        .expect("save the source");
    let status = state.ai_status();
    assert_ne!(
        status, "configured",
        "an empty AI config read as configured"
    );
    assert!(status.contains("base URL"), "{status}");

    retype(
        &mut state,
        SettingsField::AiBaseUrl,
        "http://localhost:11434/v1",
    );
    state.commit_setting().expect("save the URL");
    retype(
        &mut state,
        SettingsField::AiModel,
        aria_ai::catalog::DEFAULT_LOCAL_MODEL,
    );
    state.commit_setting().expect("save the model");
    assert_eq!(
        state.ai_status(),
        "configured",
        "a complete local config still reads as unconfigured"
    );

    // A key named through an unset variable is reported, not silently treated as absent.
    retype(
        &mut state,
        SettingsField::AiKeyEnv,
        "ARIA_KEY_THAT_IS_NOT_SET",
    );
    state.commit_setting().expect("save the variable name");
    let status = state.ai_status();
    assert!(
        status.contains("ARIA_KEY_THAT_IS_NOT_SET"),
        "an unset key variable was not reported: {status}"
    );
}
/// The filter narrows the sections by name, never writes to the config, and an empty
/// filter shows everything.
#[test]
fn the_settings_filter_narrows_sections_without_touching_the_config() {
    let (mut state, _) = with_clip();
    let sections = [
        "Appearance",
        "AI",
        "Provider keys",
        "Storage",
        "Caches",
        "Transcription",
        "Cloud",
        "Application",
    ];
    assert!(
        sections
            .iter()
            .all(|title| state.settings_section_visible(title)),
        "an empty filter hid a section"
    );

    let before = std::fs::read_to_string(state.config_path()).unwrap_or_default();
    retype(&mut state, SettingsField::Filter, "cach");
    assert!(state.settings_section_visible("Caches"));
    assert!(!state.settings_section_visible("Appearance"));
    // Case does not matter: nobody types a section name with its capital.
    state.cancel_setting();
    retype(&mut state, SettingsField::Filter, "STORAGE");
    assert!(state.settings_section_visible("Storage"));
    state.commit_setting().expect("the filter commits");
    assert!(
        state.settings_section_visible("Storage"),
        "the filter was lost on commit"
    );
    assert_eq!(
        std::fs::read_to_string(state.config_path()).unwrap_or_default(),
        before,
        "the filter was written to the config"
    );

    state.focus_setting(Some(SettingsField::Filter));
    state.cancel_setting();
    assert!(
        sections
            .iter()
            .all(|title| state.settings_section_visible(title)),
        "Esc did not clear the filter"
    );
}
/// Every override key the settings table knows has a cache to belong to, and every disk
/// cache has a key to persist under.
///
/// This is the pairing that goes stale in silence: a cache added to `aria-storage` with
/// no `[storage.paths]` key can be redirected in the dialog and forget the redirect at
/// the next launch.
#[test]
fn every_disk_cache_has_a_settings_key_and_the_reverse() {
    for key in StoragePathOverrides::KEYS {
        let id = CacheId::parse(key).unwrap_or_else(|_| panic!("{key} is not a cache id"));
        assert_eq!(
            id.descriptor().kind,
            CacheKind::Disk,
            "{key} names a cache with no directory"
        );
    }
    let mut overrides = StoragePathOverrides::default();
    for id in CacheId::ALL {
        let slot = overrides.slot(id.as_str());
        match id.descriptor().kind {
            CacheKind::Disk => {
                assert!(
                    slot.is_some(),
                    "{} cannot be overridden in config.toml",
                    id.as_str()
                );
            }
            CacheKind::Memory => assert!(
                slot.is_none(),
                "{} has a path setting but lives in memory",
                id.as_str()
            ),
        }
    }
}
/// A plugin declares a setting, the dialog offers it, and the value round-trips
/// through the file the settings plugin owns.
///
/// The class closed: a plugin could read and write `config.toml` but had no way to
/// show a field, so its configuration was hand-editing. The assertion is on the
/// whole path — declared, listed, focused, typed, committed, read back — because
/// each half alone would pass with the other missing.
#[test]
fn a_plugin_declared_setting_is_offered_and_round_trips() {
    const FIELDS: &[aria_plugin::SettingSpec] = &[aria_plugin::SettingSpec {
        key: "notes.directory",
        label: "Notes directory",
        placeholder: "the project directory",
        secret: false,
    }];
    let ctx = aria_plugin::PluginContext::new();
    let _ = crate::plugins::mount_all(&ctx, &crate::scratch_pool::scratch("settings-schema"));
    let schema = aria_plugin::settings_schema::schema_of(&ctx)
        .expect("the settings plugin registers the schema registry");
    schema.declare(aria_plugin::SettingsSection {
        plugin: "example.notes",
        title: "Notes",
        fields: FIELDS,
    });

    let mut state =
        AppState::new(ctx, crate::scratch_pool::scratch("plugin-settings")).expect("state");
    let sections = state.plugin_settings_sections();
    assert_eq!(
        sections.iter().map(|s| s.title).collect::<Vec<_>>(),
        vec!["Notes"],
        "a declared section is not offered"
    );

    let field = SettingsField::Plugin(FIELDS[0]);
    assert_eq!(field.label(), "Notes directory");
    assert_eq!(state.setting_draft(field).0, "", "a fresh key has no value");
    retype(&mut state, field, "/tmp/notes");
    state.commit_setting().expect("a plugin value is savable");
    assert_eq!(state.settings_form.focused, None);
    assert_eq!(
        state.setting_draft(field).0,
        "/tmp/notes",
        "the value did not reach the settings service"
    );

    // Clearing it stores nothing rather than an empty string, so the plugin's own
    // default applies again.
    retype(&mut state, field, "");
    state.commit_setting().expect("clearing is savable");
    assert_eq!(state.setting_draft(field).0, "");
}
