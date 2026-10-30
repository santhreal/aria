//! Settings persistence and AI configuration status tests.
//!
//! Only `#[test]` functions are defined here. The tests reach the fixtures and
//! helpers in `settings_tests.rs` through `use super::*`.

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
