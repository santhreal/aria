//! Key-routed text fields, as data.
//!
//! Every text box in the app is a row here rather than a GPUI text field. The
//! key handler routes a box's editing keys (Backspace, Delete, Enter, Esc, the
//! arrows, Home and End, select all, undo, copy, cut and paste) through its
//! row, and the platform input handler in `shell/text_input.rs` routes its
//! typed text and IME runs through the row too.
//!
//! The list is precedence order: the first field whose `focused` holds takes
//! the keystroke, so rename outranking a search box is a position in a table
//! rather than a guard in every box.

use std::borrow::Cow;
use std::ops::Range;

use crate::app::AppState;
use crate::app::line_text::{LineRule, LineText};
use crate::app::settings_form::SettingsField;

/// One key-routed text field: what holds its focus, and the things a keystroke
/// can mean while it does.
#[derive(Debug, Clone, Copy)]
pub struct TextField {
    pub id: &'static str,
    pub focused: fn(&AppState) -> bool,
    /// Esc: leave the field. Never commits.
    pub escape: fn(&mut AppState),
    /// Enter.
    pub submit: Submit,
    /// Up/Down: step through the list the box keeps, `-1` up and `1` down:
    /// the prompts it sent, or the rows its text filters.
    ///
    /// `None` for a box with no list, and then Up and Down move the caret to
    /// the ends of the line.
    pub step: Option<fn(&mut AppState, i32)>,
    /// Tab and Shift+Tab (`true`): move to the next or previous box of the
    /// same form, in the order the form draws them. Returns whether the
    /// picture can have changed, so the Viewer renders a new frame: a value
    /// committed on the way out does. `None` for a box that has no
    /// neighbors, and then Tab is not consumed.
    pub tab: Option<fn(&mut AppState, bool) -> bool>,
    /// Where the box's text is held.
    pub store: Store,
}

/// What Enter does in a box.
#[derive(Debug, Clone, Copy)]
pub enum Submit {
    /// Runs on the state with its own context, because Enter can start work
    /// (a search, a transcription, a generation), not only change state.
    State(fn(&mut AppState, &mut gpui::Context<AppState>)),
    /// Runs the palette's highlighted command. A command can start playback,
    /// which takes the window, so the shell runs it.
    PaletteCommand,
}

/// Where a box's text is held.
#[derive(Debug, Clone, Copy)]
pub enum Store {
    /// One line, as a [`LineText`].
    Line(Line),
    /// The title typed into in the Viewer: the lines of a text clip, so every
    /// edit draws a new frame of the Viewer.
    Title,
}

/// The text of a one-line box and what the box takes.
#[derive(Debug, Clone, Copy)]
pub struct Line {
    /// The text, `None` while the box is closed.
    pub get: fn(&AppState) -> Option<&LineText>,
    pub get_mut: fn(&mut AppState) -> Option<&mut LineText>,
    /// What the box takes: its cap and its characters.
    pub rule: fn(&AppState) -> LineRule,
    /// What follows an edit that changed the text, such as a filter applied on
    /// every keystroke or the end of a walk through the history.
    pub changed: Option<fn(&mut AppState)>,
}

impl Line {
    /// Run `edit` on the text with the box's rule, then the box's follow-up
    /// when the text changed. Returns whether it changed.
    fn edit(
        self,
        state: &mut AppState,
        edit: impl FnOnce(&mut LineText, LineRule) -> bool,
    ) -> bool {
        let rule = (self.rule)(state);
        let changed = (self.get_mut)(state).is_some_and(|text| edit(text, rule));
        if changed && let Some(follow) = self.changed {
            follow(state);
        }
        changed
    }
}

impl TextField {
    /// What the box holds.
    pub fn text(self, state: &AppState) -> Cow<'_, str> {
        match self.store {
            Store::Line(line) => Cow::Borrowed((line.get)(state).map_or("", LineText::as_str)),
            Store::Title => Cow::Owned(state.edited_text()),
        }
    }

    /// `(anchor, caret)` in characters.
    pub fn selection(self, state: &AppState) -> (usize, usize) {
        match self.store {
            Store::Line(line) => (line.get)(state).map_or((0, 0), LineText::selection),
            Store::Title => state.text_selection(),
        }
    }

    /// The selected text, `None` with nothing selected.
    pub fn selected(self, state: &AppState) -> Option<String> {
        match self.store {
            Store::Line(line) => (line.get)(state)?.selected().map(str::to_owned),
            Store::Title => state.edited_selection_text(),
        }
    }

    /// Whether an edit in the box changes the Viewer's picture, so the Viewer
    /// renders a new frame after each one.
    pub fn redraws(self) -> bool {
        matches!(self.store, Store::Title)
    }

    /// Replace characters `range` with what the box takes of `text`, and put
    /// the caret after it: a keystroke, a paste, or an IME run. Returns
    /// whether the text changed.
    pub fn replace(self, state: &mut AppState, range: Range<usize>, text: &str) -> bool {
        match self.store {
            Store::Line(line) => line.edit(state, |line, rule| line.replace(range, text, rule)),
            Store::Title => state.replace_title_text(range, text),
        }
    }

    /// Backspace: the selection, or the character before the caret.
    pub fn backspace(self, state: &mut AppState) {
        match self.store {
            Store::Line(line) => {
                line.edit(state, |line, _| line.backspace());
            }
            Store::Title => state.title_backspace(),
        }
    }

    /// A caret key, with `extend` for Shift held.
    pub fn key(self, state: &mut AppState, key: CaretKey, extend: bool) {
        match self.store {
            Store::Line(line) => {
                line.edit(state, |line, _| line.key(key, extend));
            }
            Store::Title => state.title_caret_key(key, extend),
        }
    }
}

/// A key a box answers on its own: a caret movement, a selection, a delete,
/// or an undo.
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
/// an open search box means the next key is part of the name. The palette comes
/// next: it opens over every panel, so a box focused before it waits under it.
/// A value readout open as text follows: it opens from a click, so it is newer
/// than any box that was focused before it. The bare-letter tool shortcuts
/// survive an open box because focus is explicit — a box that was never
/// clicked into holds no keys.
pub const SHIPPED_FIELDS: &[TextField] = &[
    TextField {
        id: "aria.field.rename",
        focused: |state| state.rename.is_some(),
        escape: AppState::cancel_rename,
        submit: Submit::State(|state, _| {
            if let Err(err) = state.commit_rename() {
                state.report_error(&err);
            }
        }),
        step: None,
        tab: None,
        store: Store::Line(Line {
            get: |state| Some(&state.rename_draft),
            get_mut: |state| Some(&mut state.rename_draft),
            rule: |_| crate::app::rename::RENAME_RULE,
            changed: None,
        }),
    },
    TextField {
        id: "aria.field.palette",
        focused: |state| state.dialog == crate::app::DialogKind::Palette,
        escape: |state| state.open_dialog(crate::app::DialogKind::None),
        submit: Submit::PaletteCommand,
        step: Some(|state, delta| state.palette_move(delta as isize)),
        tab: None,
        store: Store::Line(Line {
            get: |state| Some(&state.palette_query),
            get_mut: |state| Some(&mut state.palette_query),
            rule: |_| LineRule::ANY,
            // The highlight returns to the first hit: the row under it is not
            // the row it was.
            changed: Some(AppState::palette_restart),
        }),
    },
    TextField {
        id: "aria.field.value",
        focused: AppState::value_entry_open,
        escape: AppState::cancel_value_entry,
        submit: Submit::State(|state, cx| {
            // In a dialog, Enter is the dialog's Enter: its primary action
            // applies the typed value with the rest of the form.
            if state.value_scrubs().entry_drafts() && state.run_dialog_primary() {
                return;
            }
            // Under playback the next frame presented carries the edit.
            let redraws = state.value_scrubs().redraws() && !state.playing;
            if state.commit_value_entry() && redraws {
                let state = cx.entity();
                cx.defer(move |app| crate::app::spawn_preview(state, app));
            }
        }),
        step: None,
        tab: Some(AppState::tab_value_entry),
        store: Store::Line(Line {
            get: AppState::value_entry_line,
            get_mut: AppState::value_entry_line_mut,
            rule: AppState::value_entry_rule,
            changed: None,
        }),
    },
    TextField {
        id: "aria.field.bin-rename",
        focused: |state| state.bin_rename.is_some(),
        escape: AppState::cancel_bin_rename,
        submit: Submit::State(|state, _| {
            if let Err(err) = state.commit_bin_rename() {
                state.report_error(&err);
            }
        }),
        step: None,
        tab: None,
        store: Store::Line(Line {
            get: |state| state.bin_rename.as_ref().map(|rename| &rename.draft),
            get_mut: |state| state.bin_rename.as_mut().map(|rename| &mut rename.draft),
            rule: |_| crate::app::bins::BIN_NAME_RULE,
            changed: None,
        }),
    },
    TextField {
        id: "aria.field.preset",
        focused: AppState::preset_draft_open,
        escape: AppState::cancel_preset_draft,
        // The save is the rack's gesture, not a call from here: an empty or
        // duplicate name is refused by the same path the button presses.
        submit: Submit::State(|state, _| {
            if let Err(err) = crate::surfaces::effects_rack::apply(
                state,
                crate::surfaces::effects_rack::Gesture::SavePreset,
            ) {
                state.report_error(&err);
            }
        }),
        step: None,
        tab: None,
        store: Store::Line(Line {
            get: AppState::preset_draft_name,
            get_mut: AppState::preset_draft_name_mut,
            rule: |_| crate::app::presets::PRESET_NAME_RULE,
            changed: None,
        }),
    },
    TextField {
        id: "aria.field.driver",
        focused: AppState::driver_draft_open,
        escape: AppState::escape_driver_draft,
        submit: Submit::State(|state, _| {
            if let Err(err) = state.commit_driver_draft() {
                state.report_error(&err);
            }
        }),
        step: None,
        tab: None,
        store: Store::Line(Line {
            get: |state| state.driver_draft().map(|draft| &draft.text),
            get_mut: AppState::driver_draft_text_mut,
            rule: |_| crate::app::drivers::DRIVER_RULE,
            changed: None,
        }),
    },
    TextField {
        id: "aria.field.stock-search",
        focused: |state| state.stock_search_focus,
        escape: |state| state.focus_stock_search(false),
        submit: Submit::State(|state, cx| state.run_stock_search(cx)),
        step: None,
        tab: None,
        store: Store::Line(Line {
            get: |state| Some(&state.stock_query),
            get_mut: |state| Some(&mut state.stock_query),
            rule: |_| LineRule::ANY,
            changed: None,
        }),
    },
    TextField {
        id: "aria.field.generate-prompt",
        focused: |state| state.generate.focused,
        escape: |state| state.focus_generate_prompt(false),
        submit: Submit::State(|state, cx| {
            if let Err(err) = state.submit_generation(cx) {
                state.report_error(err);
            }
        }),
        step: None,
        tab: None,
        store: Store::Line(Line {
            get: |state| Some(&state.generate.prompt),
            get_mut: |state| Some(&mut state.generate.prompt),
            rule: |_| LineRule::ANY,
            changed: None,
        }),
    },
    TextField {
        id: "aria.field.caption-language",
        focused: |state| state.captions.language_focused,
        escape: |state| state.focus_caption_language(false),
        submit: Submit::State(|state, cx| {
            if let Err(err) = state.transcribe_selected(cx) {
                state.report_error(err);
            }
        }),
        step: None,
        tab: None,
        store: Store::Line(Line {
            get: |state| Some(&state.captions.language),
            get_mut: |state| Some(&mut state.captions.language),
            rule: |_| LineRule::ANY,
            changed: None,
        }),
    },
    TextField {
        id: "aria.field.bin-search",
        focused: |state| state.media_search_focus,
        escape: |state| state.focus_media_search(false),
        // A filter has nothing to submit: the list is already filtered on every
        // keystroke, so Enter just puts the keyboard back on the editor.
        submit: Submit::State(|state, _| state.focus_media_search(false)),
        step: None,
        tab: None,
        store: Store::Line(Line {
            get: |state| Some(&state.media_query),
            get_mut: |state| Some(&mut state.media_query),
            rule: |_| crate::app::media::MEDIA_QUERY_RULE,
            changed: None,
        }),
    },
    TextField {
        id: "aria.field.effects-search",
        focused: AppState::effects_search_focused,
        escape: |state| state.focus_effects_search(false),
        // The tree is filtered on every keystroke, so Enter returns the
        // keyboard to the editor as the bin search does.
        submit: Submit::State(|state, _| state.focus_effects_search(false)),
        step: None,
        tab: None,
        store: Store::Line(Line {
            get: |state| Some(&state.effects_browser.query),
            get_mut: |state| Some(&mut state.effects_browser.query),
            rule: |_| crate::app::effects_browser::QUERY_RULE,
            changed: None,
        }),
    },
    TextField {
        id: "aria.field.property-search",
        focused: AppState::property_search_focused,
        escape: |state| state.focus_property_search(false),
        // The rows are filtered on every keystroke, so Enter returns the
        // keyboard to the editor as the bin search does.
        submit: Submit::State(|state, _| state.focus_property_search(false)),
        step: None,
        tab: None,
        store: Store::Line(Line {
            get: |state| Some(state.property_query()),
            get_mut: |state| Some(state.property_query_mut()),
            rule: |_| crate::app::effects_browser::QUERY_RULE,
            changed: None,
        }),
    },
    TextField {
        id: "aria.field.marker-edit",
        focused: AppState::marker_edit_focused,
        escape: AppState::escape_marker_edit,
        submit: Submit::State(|state, _| state.apply_marker_edit()),
        step: None,
        tab: Some(AppState::tab_marker_edit),
        store: Store::Line(Line {
            get: AppState::marker_edit_line,
            get_mut: AppState::marker_edit_line_mut,
            rule: AppState::marker_edit_rule,
            changed: None,
        }),
    },
    TextField {
        id: "aria.field.setting",
        focused: |state| state.settings_form.focused.is_some(),
        escape: AppState::cancel_setting,
        submit: Submit::State(|state, cx| {
            // A named key set is recorded in `ui-settings.json`, which the
            // config file's own save does not write.
            let keymap = state.settings_form.focused == Some(SettingsField::KeymapName);
            match state.commit_setting() {
                Ok(()) if keymap => state.persist_settings_later(cx),
                Ok(()) => {}
                Err(err) => state.report_error(err),
            }
        }),
        step: None,
        tab: Some(AppState::tab_setting),
        store: Store::Line(Line {
            get: |state| Some(&state.settings_form.draft),
            get_mut: |state| Some(&mut state.settings_form.draft),
            rule: |_| LineRule::ANY,
            // The filter box narrows the sections as it is typed into.
            changed: Some(AppState::live_filter),
        }),
    },
    TextField {
        id: "aria.field.clock",
        focused: AppState::clock_focused,
        escape: |state| state.focus_clock(false),
        submit: Submit::State(|state, _| state.commit_clock()),
        step: None,
        tab: None,
        store: Store::Line(Line {
            get: |state| state.clock_draft.as_ref(),
            get_mut: |state| state.clock_draft.as_mut(),
            rule: |_| crate::app::clock::CLOCK_RULE,
            changed: None,
        }),
    },
    TextField {
        id: "aria.field.ai-prompt",
        focused: |state| state.assistant.focused,
        escape: |state| state.focus_assistant_prompt(false),
        // Enter is Send: a slash command expands here as it does on the button.
        submit: Submit::State(AppState::send_composer_prompt),
        step: Some(AppState::recall_agent_prompt),
        tab: None,
        store: Store::Line(Line {
            get: |state| Some(&state.assistant.prompt),
            get_mut: |state| Some(&mut state.assistant.prompt),
            rule: |_| LineRule::ANY,
            // An edit makes the prompt the composer's own, so the next Up
            // starts from the newest sent prompt again.
            changed: Some(|state| state.assistant.recall = None),
        }),
    },
    TextField {
        id: "aria.field.plugin-panel",
        focused: |state| state.plugin_panels.input.is_some(),
        escape: AppState::cancel_panel_input,
        submit: Submit::State(|state, _| state.submit_panel_input()),
        step: None,
        tab: None,
        store: Store::Line(Line {
            get: |state| state.plugin_panels.input.as_ref().map(|input| &input.draft),
            get_mut: |state| {
                state
                    .plugin_panels
                    .input
                    .as_mut()
                    .map(|input| &mut input.draft)
            },
            rule: |_| crate::app::plugin_panels::INPUT_RULE,
            changed: None,
        }),
    },
    // Last: the title stays live while the Type tool holds it, so a box clicked into takes the keys
    // from it.
    TextField {
        id: "aria.field.title",
        focused: |state| state.dialog == crate::app::DialogKind::None && state.text_editing(),
        escape: AppState::end_text_edit,
        // A title has more than one line: Enter breaks the line.
        submit: Submit::State(|state, _| {
            let (anchor, caret) = state.text_selection();
            state.replace_title_text(anchor.min(caret)..anchor.max(caret), "\n");
        }),
        step: None,
        tab: None,
        store: Store::Title,
    },
];

/// The field holding focus, or nothing when every box is idle.
pub fn focused(state: &AppState) -> Option<TextField> {
    SHIPPED_FIELDS
        .iter()
        .find(|field| (field.focused)(state))
        .copied()
}

/// Type a run of characters over the focused box's selection, as one edit.
///
/// This is the whole of paste and of an IME commit: the run goes through the
/// box's own rule, so the clock still refuses letters and the bin name still
/// stops at its length cap, and no row needs a second entry point.
///
/// Control characters are dropped. A pasted path with a trailing newline is a
/// path, and a one-line box has no second line to move to.
///
/// Returns whether the box's text changed.
pub fn insert(state: &mut AppState, text: &str) -> bool {
    let Some(field) = focused(state) else {
        return false;
    };
    let (anchor, caret) = field.selection(state);
    field.replace(state, anchor.min(caret)..anchor.max(caret), text)
}

/// Keystrokes as a test presses them: into the box that holds focus, through
/// its row, as the key handler and the input handler route them.
#[cfg(test)]
impl AppState {
    /// Type `ch` into the box that holds focus. With no box focused the key
    /// is not text, and nothing changes.
    pub(crate) fn type_key(&mut self, ch: char) {
        insert(self, ch.encode_utf8(&mut [0; 4]));
    }

    /// Press Backspace in the box that holds focus.
    pub(crate) fn press_backspace(&mut self) {
        if let Some(field) = focused(self) {
            field.backspace(self);
        }
    }
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
