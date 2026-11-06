//! Tab walks a form's boxes in the order the form draws them.
//!
//! Closes the class "a form with several boxes whose Tab does nothing": the
//! Marker dialog's Name, Comment and Duration boxes and the Settings
//! dialog's typed fields took no Tab, so moving between them took the
//! pointer. `TABS` states what Tab does in every shipped box, so a box added
//! without a decision turns the suite red. Each walking form is driven
//! through its row's `tab`, the entry the shell's key handler calls: forward
//! across the wrap and back across it, and each box it lands on holds its
//! text selected, so typing replaces it.
//!
//! The value readouts walk by what was painted, which
//! `value_scrub_tests::tab_commits_and_opens_the_neighboring_readout`
//! drives, and a plugin section's typed fields are walked in
//! `plugin_settings_tests`. Not covered: the drawn order, which each form
//! reads from the constant its ring reads (`MarkerEditField::ORDER`,
//! `SettingsField::AI`).

use super::AppState;
use super::premiere_tests::with_clip;
use crate::app::settings_form::SettingsField;
use crate::keys::fields::{self, TextField};

/// A form Tab walks: how to focus its first box, which box holds focus, and
/// the boxes in the order the form draws them.
struct Ring {
    focus: fn(&mut AppState),
    at: fn(&AppState) -> Option<&'static str>,
    boxes: &'static [&'static str],
}

/// What Tab does in one shipped box.
enum Tab {
    /// Moves through the boxes of the box's form.
    Walks(Ring),
    /// Moves to the readout painted beside the open one.
    Painted,
    /// The box has no neighbors, so Tab is not consumed.
    Alone,
}

const TABS: &[(&str, Tab)] = &[
    ("aria.field.rename", Tab::Alone),
    ("aria.field.palette", Tab::Alone),
    ("aria.field.value", Tab::Painted),
    ("aria.field.bin-rename", Tab::Alone),
    ("aria.field.preset", Tab::Alone),
    ("aria.field.driver", Tab::Alone),
    ("aria.field.stock-search", Tab::Alone),
    ("aria.field.generate-prompt", Tab::Alone),
    ("aria.field.caption-language", Tab::Alone),
    ("aria.field.bin-search", Tab::Alone),
    ("aria.field.effects-search", Tab::Alone),
    ("aria.field.property-search", Tab::Alone),
    (
        "aria.field.marker-edit",
        Tab::Walks(Ring {
            focus: |state| {
                state.clear_selection();
                state.add_marker_at_playhead().expect("a marker");
                state.edit_marker_at_playhead();
                // Every box holds text, so a selection on arrival is visible.
                state.marker_edit.comment.set("Opening beat");
            },
            at: |state| state.marker_edit.focused.map(|field| field.caption().0),
            boxes: &["Name", "Comment", "Duration (timecode)"],
        }),
    ),
    (
        "aria.field.setting",
        Tab::Walks(Ring {
            focus: |state| {
                state.open_settings();
                save_ai_fields(state);
                state.focus_setting(Some(SettingsField::AiBaseUrl));
            },
            at: |state| state.settings_form.focused.map(SettingsField::id),
            boxes: &["ai-base-url", "ai-model", "ai-key", "ai-key-env"],
        }),
    ),
    ("aria.field.clock", Tab::Alone),
    ("aria.field.ai-prompt", Tab::Alone),
    // A panel's one text event is Submitted, which is Enter's: a Tab that sent
    // it could start the plugin's action.
    ("aria.field.plugin-panel", Tab::Alone),
    ("aria.field.title", Tab::Alone),
];

/// A value in each AI field, saved through the dialog's own commit.
const AI_VALUES: [(SettingsField, &str); 4] = [
    (SettingsField::AiBaseUrl, "http://localhost:11434/v1"),
    (SettingsField::AiModel, "llama3"),
    (SettingsField::AiKey, "sk-test"),
    (SettingsField::AiKeyEnv, "ARIA_TEST_KEY"),
];

fn save_ai_fields(state: &mut AppState) {
    for (field, value) in AI_VALUES {
        state.focus_setting(Some(field));
        state.settings_form.draft.set(value);
        state.commit_setting().expect("a valid AI value saves");
    }
}

/// Every shipped box states what Tab does, and its row agrees: a box that
/// walks takes the key, and a box alone leaves it.
#[test]
fn every_shipped_box_states_what_tab_does() {
    for field in fields::SHIPPED_FIELDS {
        let Some((_, tab)) = TABS.iter().find(|(id, _)| *id == field.id) else {
            panic!("{} states nothing about Tab", field.id);
        };
        assert_eq!(
            field.tab.is_some(),
            !matches!(tab, Tab::Alone),
            "{}: its row and its decision disagree about Tab",
            field.id
        );
    }
    for (id, _) in TABS {
        assert!(
            fields::SHIPPED_FIELDS.iter().any(|field| field.id == *id),
            "{id} is not a shipped field"
        );
    }
}

/// Press Tab, or Shift+Tab with `back`, in the focused box as the shell does,
/// and check where it landed: still `field`, on box `to` of the ring, with the
/// box's text selected whole. A move between boxes changes no picture.
fn press(state: &mut AppState, field: TextField, ring: &Ring, back: bool, to: usize) {
    let tab = field.tab.expect("a walking box takes Tab");
    assert!(
        !tab(state, back),
        "{}: a move between boxes asked for a frame",
        field.id
    );
    let focused = fields::focused(state).map(|field| field.id);
    assert_eq!(focused, Some(field.id), "{}: Tab left the form", field.id);
    assert_eq!(
        (ring.at)(state),
        Some(ring.boxes[to]),
        "{}: Tab (back {back}) landed on the wrong box",
        field.id
    );
    let text = field.text(state).chars().count();
    assert!(text > 0, "{}: {} holds no text", field.id, ring.boxes[to]);
    assert_eq!(
        field.selection(state),
        (0, text),
        "{}: {} was not selected on arrival",
        field.id,
        ring.boxes[to]
    );
}

/// Tab moves through every box of a walking form in draw order and wraps to
/// the first; Shift+Tab walks the same boxes backward across the same wrap.
#[test]
fn tab_walks_each_form_in_draw_order_and_wraps_both_ways() {
    for (id, tab) in TABS {
        let Tab::Walks(ring) = tab else { continue };
        let (mut state, _) = with_clip();
        (ring.focus)(&mut state);
        let field = fields::focused(&state).unwrap_or_else(|| panic!("{id} took no focus"));
        assert_eq!(field.id, *id, "{id}: another box took focus");
        assert_eq!((ring.at)(&state), Some(ring.boxes[0]), "{id}: first box");
        let len = ring.boxes.len();
        for step in 1..=len {
            press(&mut state, field, ring, false, step % len);
        }
        for step in (0..len).rev() {
            press(&mut state, field, ring, true, step);
        }
    }
}

/// Tab between the Marker dialog's boxes applies nothing: the marker keeps
/// its name until Save, and the typed name waits in its box.
#[test]
fn tab_in_the_marker_dialog_applies_nothing_before_save() {
    let (mut state, _) = with_clip();
    state.clear_selection();
    state.add_marker_at_playhead().expect("a marker");
    state.edit_marker_at_playhead();
    for ch in "Act I".chars() {
        state.type_key(ch);
    }
    assert!(!state.tab_marker_edit(false));
    assert_eq!(state.store.sequence.markers[0].name, "Marker 1");
    assert_eq!(state.marker_edit.name, "Act I");
    assert_eq!(
        state.marker_edit.focused,
        Some(crate::app::MarkerEditField::Comment)
    );
}

/// Tab saves a Settings field on its way out, as Enter does, and a value the
/// field refuses keeps the focus and the typed text so it can be fixed. Every
/// shipped field outside the AI section is alone in its section: Tab there
/// moves nowhere and saves nothing.
#[test]
fn tab_saves_a_settings_field_and_holds_a_refused_one() {
    let (mut state, _) = with_clip();
    state.focus_setting(Some(SettingsField::AiModel));
    state.settings_form.draft.set("mistral");
    assert!(!state.tab_setting(false));
    assert_eq!(state.settings_form.focused, Some(SettingsField::AiKey));
    assert_eq!(state.setting_draft(SettingsField::AiModel).0, "mistral");

    state.focus_setting(Some(SettingsField::AiBaseUrl));
    state.settings_form.draft.set("localhost:11434");
    assert!(!state.tab_setting(false));
    assert_eq!(
        state.settings_form.focused,
        Some(SettingsField::AiBaseUrl),
        "a refused value moved on"
    );
    assert_eq!(state.settings_form.draft, "localhost:11434");

    // A field alone in its section keeps the focus and its draft and saves
    // nothing: Enter saves it, and a Tab in Save keys as would write a key set.
    let alone = SettingsField::ALL
        .into_iter()
        .filter(|field| !SettingsField::AI.contains(field));
    for field in alone {
        let typed = if field == SettingsField::CloudBaseUrl {
            "https://tabbed.example"
        } else {
            "tabbed"
        };
        let saved = state.setting_draft(field).0;
        assert_ne!(saved, typed);
        state.focus_setting(Some(field));
        state.settings_form.draft.set(typed);
        assert!(!state.tab_setting(false));
        assert_eq!(state.settings_form.focused, Some(field), "{}", field.id());
        assert_eq!(state.settings_form.draft, typed, "{}", field.id());
        state.focus_setting(None);
        assert_eq!(
            state.setting_draft(field).0,
            saved,
            "Tab saved {}",
            field.id()
        );
    }
}
