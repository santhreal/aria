//! Settings dialog UI and integration tests.

use crate::app::settings_form::{SettingsField, SettingsSection, format_agent_capabilities};
use crate::app::{AppState, DialogKind};
use aria_plugin::PluginContext;

fn make_state() -> AppState {
    let ctx = PluginContext::new();
    let dir = crate::scratch_pool::scratch("settings-dialog-test");
    AppState::new(ctx, dir).expect("state")
}

/// A state with the key bindings and the plugin settings registry mounted.
fn mounted_state() -> AppState {
    let dir = crate::scratch_pool::scratch("settings-dialog-sections");
    let ctx = PluginContext::new();
    ctx.mount(&crate::plugins::SettingsPlugin {
        path: dir.join("settings.toml"),
    })
    .expect("the settings plugin mounts");
    ctx.mount(&crate::plugins::UiPlugin)
        .expect("the ui plugin needs nothing");
    AppState::new(ctx, dir).expect("state")
}

#[test]
fn filter_subsets_settings_sections() {
    let mut state = make_state();

    // Empty filter matches all sections
    state.settings_form.filter.clear();
    assert!(
        SettingsSection::ALL
            .iter()
            .all(|section| state.settings_section_visible(section.title()))
    );

    // A filter that is part of a title shows each section containing it.
    state.settings_form.filter = "stor".into();
    assert!(state.settings_section_visible("Storage"));
    assert!(!state.settings_section_visible("Appearance"));
    let keys = SettingsSection::ProviderKeys.title();
    let shortcuts = SettingsSection::KeyboardShortcuts.title();
    state.settings_form.filter = "key".into();
    assert!(state.settings_section_visible(keys));
    assert!(state.settings_section_visible(shortcuts));

    // Case-insensitive filtering
    state.settings_form.filter = "SHORTCUT".into();
    assert!(state.settings_section_visible(shortcuts));
    assert!(!state.settings_section_visible(keys));
}

/// What Settings shows under its current filter: the titles of the sections
/// it shows, shipped then declared, and how many key bindings it lists.
fn shown(
    state: &AppState,
    declared: &[std::sync::Arc<aria_plugin::SettingsSection>],
) -> (Vec<String>, usize) {
    let titles = SettingsSection::ALL
        .iter()
        .map(|section| section.title())
        .chain(declared.iter().map(|section| section.title.as_str()))
        .filter(|title| state.settings_section_visible(title))
        .map(str::to_owned)
        .collect();
    let listed = state
        .listed_binding_rows()
        .map_or(0, |listing| listing.rows.len());
    (titles, listed)
}

/// Run the shipped binding `id` against the state.
fn run_binding(state: &mut AppState, id: &str) {
    let binding = crate::keys::bindings::SHIPPED_BINDINGS
        .iter()
        .find(|binding| binding.id == id)
        .unwrap_or_else(|| panic!("{id} is a shipped binding"));
    let crate::keys::KeyBody::Plain(run) = binding.run else {
        panic!("{id} runs against the state alone");
    };
    run(state);
}

/// WHY: Edit > Keyboard Shortcuts opened Settings under the filter `Keys`,
/// which the title Provider keys contains too, so the item opened two
/// sections where its name promises one, and Ctrl+, after it kept that
/// filter. Every opener now passes a `SettingsSection`, a filter that is a
/// shipped section's whole title shows that section alone, and Preferences
/// clears the filter.
///
/// Opens each section of `SettingsSection::ALL` and reads every title the
/// dialog shows: the shipped ones, a plugin section whose title contains the
/// opened one, and the key bindings, which list only under Keyboard
/// Shortcuts. Then presses Ctrl+Alt+K and Ctrl+, through their bindings. Not
/// covered: the drawn dialog, which tests the same predicate for each
/// section it draws, and the menu rows, which call the same opener.
#[test]
fn opening_a_section_shows_that_section_alone() {
    let mut state = mounted_state();
    let _lookalikes: Vec<aria_plugin::Registration> = SettingsSection::ALL
        .iter()
        .enumerate()
        .map(|(ix, section)| {
            state
                .ctx
                .register_settings(aria_plugin::SettingsSection {
                    plugin: format!("example.lookalike{ix}"),
                    title: format!("{} extras", section.title()),
                    fields: Vec::new(),
                })
                .expect("the section registers")
        })
        .collect();
    let declared = state.plugin_settings_sections();
    for section in SettingsSection::ALL {
        let lookalike = format!("{} extras", section.title());
        assert!(
            declared.iter().any(|held| held.title == lookalike),
            "{lookalike} was not declared"
        );
    }
    let total = state.binding_rows().len();
    assert!(total > 0, "no key binding is mounted");
    let alone = |section: SettingsSection| {
        let listed = if section == SettingsSection::KeyboardShortcuts {
            total
        } else {
            0
        };
        (vec![section.title().to_owned()], listed)
    };

    for opened in SettingsSection::ALL {
        state.open_settings_at(opened);
        assert_eq!(state.dialog, DialogKind::Settings);
        assert_eq!(
            shown(&state, &declared),
            alone(opened),
            "{} opened",
            opened.title()
        );
        state.settings_form.filter = opened.title().to_lowercase();
        assert_eq!(
            shown(&state, &declared),
            alone(opened),
            "under the filter {:?}",
            state.settings_form.filter
        );
    }

    run_binding(&mut state, "key-keyboard-shortcuts");
    assert_eq!(state.dialog, DialogKind::Settings);
    assert_eq!(
        shown(&state, &declared),
        alone(SettingsSection::KeyboardShortcuts),
        "Ctrl+Alt+K opened"
    );
    run_binding(&mut state, "key-settings");
    let every: Vec<String> = SettingsSection::ALL
        .iter()
        .map(|section| section.title().to_owned())
        .chain(declared.iter().map(|section| section.title.clone()))
        .collect();
    assert_eq!(
        shown(&state, &declared),
        (every, total),
        "Ctrl+, after Ctrl+Alt+K opened"
    );
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

/// WHY: The AI `Can do` row and Settings count strings previously printed
/// literal `"(s)"` (`"67 edit(s), 5 app tool(s) of which 2 look and 1 ask
/// first, 0 plugin tool(s)"`). Every count slot must inflect both its noun and
/// its verb for singular (`1`) and plural (`0`, `2+`) values without `"(s)"`.
#[test]
fn agent_capabilities_inflects_nouns_and_verbs_for_every_count() {
    assert_eq!(
        format_agent_capabilities(67, 5, 2, 1, 0),
        "67 edits, 5 app tools (2 read-only, 1 asks first), 0 plugin tools"
    );
    assert_eq!(
        format_agent_capabilities(1, 1, 1, 2, 1),
        "1 edit, 1 app tool (1 read-only, 2 ask first), 1 plugin tool"
    );
    assert_eq!(
        format_agent_capabilities(0, 0, 0, 0, 2),
        "0 edits, 0 app tools (0 read-only, 0 ask first), 2 plugin tools"
    );

    let state = make_state();
    let live = state.agent_capabilities();
    assert!(
        !live.contains("(s)"),
        "live Can do row contains `(s)`: {live}"
    );
}
