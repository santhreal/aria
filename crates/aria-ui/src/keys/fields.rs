//! Key-routed text fields, as data.
//!
//! Every text box in the app is a row here rather than a GPUI text field. The
//! key handler routes a box's editing keys (Backspace, Enter, Esc, the arrows,
//! paste) through its row, and the platform input handler in
//! `shell/text_input.rs` routes its typed text through the row's `push`.
//!
//! The list is precedence order: the first field whose `focused` holds takes
//! the keystroke, so rename outranking a search box is a position in a table
//! rather than a guard in every box.

use crate::app::AppState;
use crate::app::settings_form::SettingsField;

/// One key-routed text field: what holds its focus, and the things a keystroke
/// can mean while it does.
///
/// `submit` takes the state's own context because a field's Enter can start
/// work (a search, a transcription, a generation), not only change state.
#[derive(Debug, Clone, Copy)]
pub struct TextField {
    pub id: &'static str,
    pub focused: fn(&AppState) -> bool,
    /// What the box currently holds. An input handler has to be able to read
    /// what it is editing: a platform IME asks for the text around the caret
    /// before it commits, and a preedit run is replaced rather than appended.
    pub text: fn(&AppState) -> String,
    /// Esc: leave the field. Never commits.
    pub escape: fn(&mut AppState),
    pub backspace: fn(&mut AppState),
    pub submit: fn(&mut AppState, &mut gpui::Context<AppState>),
    pub push: fn(&mut AppState, char),
    /// Up/Down: walk this box's own history of submitted values, `-1` toward
    /// older and `1` toward newer.
    ///
    /// `None` for a box with no history, and then the arrows fall through to
    /// whatever they mean outside a text box. A box that recalls is not a
    /// special case in the shell for the same reason the other four keys are
    /// not: the shell routes the key, the row decides what it means.
    pub history: Option<fn(&mut AppState, i32)>,
    /// Tab and Shift+Tab (`true`): move to the next or previous box of the
    /// same kind. `None` for a box that has no neighbours, and then Tab is
    /// not consumed.
    pub tab: Option<fn(&mut AppState, bool)>,
    /// A caret the box moves, and a selection it replaces. `None` for a box
    /// that only appends and pops.
    pub caret: Option<Caret>,
}

/// The caret of a box that has one: the title typed into in the Viewer, and
/// the rename bar.
#[derive(Debug, Clone, Copy)]
pub struct Caret {
    /// Whether a key in the box changes the Viewer's picture, so the Viewer
    /// renders a new frame after each one: a title typed into in the Viewer
    /// does, and a rename draft does not.
    pub redraws: bool,
    /// A movement, selection or history key, and whether Shift is held.
    pub key: fn(&mut AppState, CaretKey, bool),
    /// `(anchor, caret)` in characters.
    pub selection: fn(&AppState) -> (usize, usize),
    /// Replace a range of characters with text.
    pub replace: fn(&mut AppState, std::ops::Range<usize>, &str),
    /// The selected text, `None` with nothing selected.
    pub selected: fn(&AppState) -> Option<String>,
}

/// A key a box with a caret answers on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaretKey {
    Left,
    Right,
    WordLeft,
    WordRight,
    LineStart,
    LineEnd,
    TextStart,
    TextEnd,
    Up,
    Down,
    Delete,
    SelectAll,
    Undo,
    Redo,
}

impl CaretKey {
    /// The caret key a key name and its modifiers mean, `None` for any other
    /// key. Shift is the selection's, except on Ctrl+Shift+Z, which is Redo.
    pub fn of(key: &str, ctrl: bool, shift: bool) -> Option<Self> {
        Some(match (key, ctrl) {
            ("left", false) => Self::Left,
            ("left", true) => Self::WordLeft,
            ("right", false) => Self::Right,
            ("right", true) => Self::WordRight,
            ("home", false) => Self::LineStart,
            ("home", true) => Self::TextStart,
            ("end", false) => Self::LineEnd,
            ("end", true) => Self::TextEnd,
            ("up", false) => Self::Up,
            ("down", false) => Self::Down,
            ("delete", false) => Self::Delete,
            ("a", true) => Self::SelectAll,
            ("z", true) if shift => Self::Redo,
            ("z", true) => Self::Undo,
            ("y", true) => Self::Redo,
            _ => return None,
        })
    }
}

/// Every shipped text box, in precedence order.
///
/// Rename is first because a draft is the newer intention: opening a rename over
/// an open search box means the next key is part of the name. A value readout
/// open as text comes next: it opens from a click, so it is newer than any box
/// that was focused before it. The bare-letter tool shortcuts survive an open
/// box because focus is explicit — a box that was never clicked into holds no
/// keys.
pub const SHIPPED_FIELDS: &[TextField] = &[
    TextField {
        id: "aria.field.rename",
        focused: |state| state.rename.is_some(),
        text: |state| state.rename_draft.clone(),
        escape: AppState::cancel_rename,
        backspace: AppState::rename_pop,
        submit: |state, _| {
            if let Err(err) = state.commit_rename() {
                state.report_error(&err);
            }
        },
        history: None,
        tab: None,
        push: AppState::rename_push,
        caret: Some(Caret {
            redraws: false,
            key: AppState::rename_caret_key,
            selection: AppState::rename_selection,
            replace: AppState::replace_rename_text,
            selected: AppState::rename_selected_text,
        }),
    },
    TextField {
        id: "aria.field.value",
        focused: AppState::value_entry_open,
        text: AppState::value_entry_text,
        escape: AppState::cancel_value_entry,
        backspace: AppState::value_entry_pop,
        submit: |state, cx| {
            // Under playback the next frame presented carries the edit.
            if state.commit_value_entry() && !state.playing {
                let state = cx.entity();
                cx.defer(move |app| crate::app::spawn_preview(state, app));
            }
        },
        history: None,
        tab: Some(AppState::tab_value_entry),
        push: AppState::value_entry_push,
        caret: None,
    },
    TextField {
        id: "aria.field.bin-rename",
        focused: |state| state.bin_rename.is_some(),
        text: |state| {
            state
                .bin_rename
                .as_ref()
                .map(|rename| rename.draft.clone())
                .unwrap_or_default()
        },
        escape: AppState::cancel_bin_rename,
        backspace: AppState::bin_rename_pop,
        submit: |state, _| {
            if let Err(err) = state.commit_bin_rename() {
                state.report_error(&err);
            }
        },
        history: None,
        tab: None,
        push: AppState::bin_rename_push,
        caret: None,
    },
    TextField {
        id: "aria.field.preset",
        focused: AppState::preset_draft_open,
        text: AppState::preset_draft_text,
        escape: AppState::cancel_preset_draft,
        backspace: AppState::preset_draft_pop,
        // The save is the rack's gesture, not a call from here: an empty or
        // duplicate name is refused by the same path the button presses.
        submit: |state, _| {
            if let Err(err) = crate::surfaces::effects_rack::apply(
                state,
                crate::surfaces::effects_rack::Gesture::SavePreset,
            ) {
                state.report_error(&err);
            }
        },
        history: None,
        tab: None,
        push: AppState::preset_draft_push,
        caret: None,
    },
    TextField {
        id: "aria.field.driver",
        focused: AppState::driver_draft_open,
        text: AppState::driver_draft_text,
        escape: AppState::escape_driver_draft,
        backspace: AppState::driver_draft_pop,
        submit: |state, _| {
            if let Err(err) = state.commit_driver_draft() {
                state.report_error(&err);
            }
        },
        history: None,
        tab: None,
        push: AppState::driver_draft_push,
        caret: None,
    },
    TextField {
        id: "aria.field.stock-search",
        focused: |state| state.stock_search_focus,
        text: |state| state.stock_query.clone(),
        escape: |state| state.focus_stock_search(false),
        backspace: AppState::stock_query_pop,
        submit: |state, cx| state.run_stock_search(cx),
        history: None,
        tab: None,
        push: AppState::stock_query_push,
        caret: None,
    },
    TextField {
        id: "aria.field.generate-prompt",
        focused: |state| state.generate.focused,
        text: |state| state.generate.prompt.clone(),
        escape: |state| state.focus_generate_prompt(false),
        backspace: AppState::generate_prompt_pop,
        submit: |state, cx| {
            if let Err(err) = state.submit_generation(cx) {
                state.report_error(err);
            }
        },
        history: None,
        tab: None,
        push: AppState::generate_prompt_push,
        caret: None,
    },
    TextField {
        id: "aria.field.caption-language",
        focused: |state| state.captions.language_focused,
        text: |state| state.captions.language.clone(),
        escape: |state| state.focus_caption_language(false),
        backspace: AppState::caption_language_pop,
        submit: |state, cx| {
            if let Err(err) = state.transcribe_selected(cx) {
                state.report_error(err);
            }
        },
        history: None,
        tab: None,
        push: AppState::caption_language_push,
        caret: None,
    },
    TextField {
        id: "aria.field.bin-search",
        focused: |state| state.media_search_focus,
        text: |state| state.media_query.clone(),
        escape: |state| state.focus_media_search(false),
        backspace: AppState::media_query_pop,
        // A filter has nothing to submit: the list is already filtered on every
        // keystroke, so Enter just puts the keyboard back on the editor.
        submit: |state, _| state.focus_media_search(false),
        history: None,
        tab: None,
        push: AppState::media_query_push,
        caret: None,
    },
    TextField {
        id: "aria.field.effects-search",
        focused: AppState::effects_search_focused,
        text: |state| state.effects_browser.query.clone(),
        escape: |state| state.focus_effects_search(false),
        backspace: AppState::effects_query_pop,
        // The tree is filtered on every keystroke, so Enter returns the
        // keyboard to the editor as the bin search does.
        submit: |state, _| state.focus_effects_search(false),
        history: None,
        tab: None,
        push: AppState::effects_query_push,
        caret: None,
    },
    TextField {
        id: "aria.field.property-search",
        focused: AppState::property_search_focused,
        text: |state| state.property_query().to_string(),
        escape: |state| state.focus_property_search(false),
        backspace: AppState::property_query_pop,
        // The rows are filtered on every keystroke, so Enter returns the
        // keyboard to the editor as the bin search does.
        submit: |state, _| state.focus_property_search(false),
        history: None,
        tab: None,
        push: AppState::property_query_push,
        caret: None,
    },
    TextField {
        id: "aria.field.marker-edit",
        focused: AppState::marker_edit_focused,
        text: AppState::marker_edit_text,
        escape: AppState::escape_marker_edit,
        backspace: AppState::marker_edit_pop,
        submit: |state, _| state.apply_marker_edit(),
        history: None,
        tab: None,
        push: AppState::marker_edit_push,
        caret: None,
    },
    TextField {
        id: "aria.field.setting",
        focused: |state| state.settings_form.focused.is_some(),
        text: |state| state.settings_form.draft.clone(),
        escape: AppState::cancel_setting,
        backspace: AppState::setting_pop,
        submit: |state, cx| {
            // A named key set is recorded in `ui-settings.json`, which the
            // config file's own save does not write.
            let keymap = state.settings_form.focused == Some(SettingsField::KeymapName);
            match state.commit_setting() {
                Ok(()) if keymap => state.persist_settings_later(cx),
                Ok(()) => {}
                Err(err) => state.report_error(err),
            }
        },
        history: None,
        tab: None,
        push: AppState::setting_push,
        caret: None,
    },
    TextField {
        id: "aria.field.clock",
        focused: AppState::clock_focused,
        text: |state| state.clock_draft.clone().unwrap_or_default(),
        escape: |state| state.focus_clock(false),
        backspace: AppState::clock_pop,
        submit: |state, _| state.commit_clock(),
        history: None,
        tab: None,
        push: AppState::clock_push,
        caret: None,
    },
    TextField {
        id: "aria.field.ai-prompt",
        focused: |state| state.assistant.focused,
        text: |state| state.assistant.prompt.clone(),
        escape: |state| state.focus_assistant_prompt(false),
        backspace: AppState::assistant_prompt_pop,
        submit: |state, cx| {
            let prompt = state.assistant.prompt.clone();
            state.send_agent_prompt(prompt, cx);
        },
        history: Some(AppState::recall_agent_prompt),
        tab: None,
        push: AppState::assistant_prompt_push,
        caret: None,
    },
    TextField {
        id: "aria.field.plugin-panel",
        focused: |state| state.plugin_panels.input.is_some(),
        text: AppState::panel_input_text,
        escape: AppState::cancel_panel_input,
        backspace: AppState::panel_input_pop,
        submit: |state, _| state.submit_panel_input(),
        history: None,
        tab: None,
        push: AppState::panel_input_push,
        caret: None,
    },
    // Last: the title stays live while the Type tool holds it, so a box clicked into takes the keys
    // from it.
    TextField {
        id: "aria.field.title",
        focused: |state| state.dialog == crate::app::DialogKind::None && state.text_editing(),
        text: AppState::edited_text,
        escape: AppState::end_text_edit,
        backspace: AppState::title_backspace,
        // A title has more than one line: Enter breaks the line.
        submit: |state, _| {
            let (anchor, caret) = state.text_selection();
            state.replace_title_text(anchor.min(caret)..anchor.max(caret), "\n");
        },
        history: None,
        tab: None,
        push: AppState::title_push,
        caret: Some(Caret {
            redraws: true,
            key: AppState::title_caret_key,
            selection: AppState::text_selection,
            replace: AppState::replace_title_text,
            selected: AppState::edited_selection_text,
        }),
    },
];

/// The field holding focus, or nothing when every box is idle.
pub fn focused(state: &AppState) -> Option<TextField> {
    SHIPPED_FIELDS
        .iter()
        .find(|field| (field.focused)(state))
        .copied()
}

/// Insert a run of characters into the focused box, as if they had been typed.
///
/// This is the whole of paste and of an IME commit: a run is the same event as
/// a keystroke repeated, so it goes through each row's own `push` and keeps that
/// box's own rules — the clock still refuses letters, the bin name still stops
/// at its length cap, and no row needs a second entry point.
///
/// Control characters are dropped. A pasted path with a trailing newline is a
/// path, and a box has no second line to move to.
///
/// Returns whether a box took the text.
pub fn insert(state: &mut AppState, text: &str) -> bool {
    let Some(field) = focused(state) else {
        return false;
    };
    // A box with a caret takes the run over its selection in one edit.
    if let Some(caret) = field.caret {
        let (anchor, at) = (caret.selection)(state);
        (caret.replace)(state, anchor.min(at)..anchor.max(at), text);
        return true;
    }
    for ch in text.chars().filter(|ch| !ch.is_control()) {
        (field.push)(state, ch);
    }
    true
}

/// Remove `count` characters from the end of the focused box, as if Backspace
/// had been pressed that many times. An IME replaces the preedit run it marked,
/// and a box that only appends and pops expresses that as this.
pub fn truncate(state: &mut AppState, count: usize) -> bool {
    let Some(field) = focused(state) else {
        return false;
    };
    for _ in 0..count {
        (field.backspace)(state);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An unnamed or duplicated field is a box the shell cannot describe, and a
    /// duplicate id means two rows claim the same box.
    #[test]
    fn every_field_is_named_once() {
        let mut ids: Vec<&str> = SHIPPED_FIELDS.iter().map(|field| field.id).collect();
        assert!(ids.iter().all(|id| !id.is_empty()));
        let count = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), count, "duplicate field id");
    }

    /// Rename outranks every other box: precedence is this position, and a later
    /// row would mean a name typed over a search box goes into the search.
    #[test]
    fn rename_takes_the_key_before_any_other_field() {
        assert_eq!(SHIPPED_FIELDS[0].id, "aria.field.rename");
    }

    /// No box holds focus on a fresh state, so the tool keys work at startup.
    #[test]
    fn nothing_is_focused_by_default() {
        let state = AppState::new(
            aria_plugin::PluginContext::new(),
            crate::scratch_pool::scratch("fields"),
        )
        .expect("state");
        assert!(focused(&state).is_none());
    }
}
