//! Settings dialog UI and integration tests.

use crate::app::AppState;
use crate::app::settings_form::SettingsField;
use aria_plugin::PluginContext;

fn make_state() -> AppState {
    let ctx = PluginContext::new();
    let dir = crate::scratch_pool::scratch("settings-dialog-test");
    AppState::new(ctx, dir).expect("state")
}

#[test]
fn filter_subsets_settings_sections() {
    let mut state = make_state();

    // Empty filter matches all sections
    state.settings_form.filter.clear();
    assert!(state.settings_section_visible("Appearance"));
    assert!(state.settings_section_visible("Storage"));
    assert!(state.settings_section_visible("Plugins"));
    assert!(state.settings_section_visible("Keys"));

    // Exact or substring filter
    state.settings_form.filter = "stor".into();
    assert!(state.settings_section_visible("Storage"));
    assert!(!state.settings_section_visible("Appearance"));
    assert!(!state.settings_section_visible("Keys"));

    // Case-insensitive filtering
    state.settings_form.filter = "KEYS".into();
    assert!(state.settings_section_visible("Keys"));
    assert!(!state.settings_section_visible("Storage"));
}

#[test]
fn focus_and_commit_updates_app_state() {
    let mut state = make_state();

    // Focus AiModel field
    state.focus_setting(Some(SettingsField::AiModel));
    assert_eq!(state.settings_form.focused, Some(SettingsField::AiModel));

    // Type draft and commit
    state.settings_form.draft = "claude-3-5-sonnet".into();
    state.commit_setting().expect("commit valid model");

    assert_eq!(state.cloud_settings().ai.model, "claude-3-5-sonnet");

    // Invalid URL validation
    state.focus_setting(Some(SettingsField::AiBaseUrl));
    state.settings_form.draft = "not-a-valid-url".into();
    let err = state.commit_setting().expect_err("invalid URL accepted");
    assert!(err.contains("http://") || err.contains("https://"));
}

#[test]
fn plugin_settings_sections_render_from_services() {
    let state = make_state();
    let sections = state.plugin_settings_sections();
    // Default context has no custom plugin settings, sections is empty
    assert!(sections.is_empty());
}
