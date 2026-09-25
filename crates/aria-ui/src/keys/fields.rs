//! Key-routed text fields, as data.
//!
//! Every text box in the app is typed into through the key handler rather than a
//! GPUI text field, so each one used to be its own block in the shell: five
//! near-identical `match`es that differed only in which four functions they
//! called. Adding a sixth box meant editing the shell, and the precedence
//! between boxes lived in a `&& rename.is_none()` repeated in every block.
//!
//! A field is a row here instead. The list is precedence order: the first field
//! whose `focused` holds takes the keystroke, so rename outranking a search box
//! is a position in a table rather than a guard in four places.

use crate::app::AppState;

/// One key-routed text field: what holds its focus, and the things a keystroke
/// can mean while it does.
///
/// `submit` takes the state's own context because a field's Enter can start
/// work (a search, a transcription, a generation), not only change state.
#[derive(Clone, Copy)]
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
}

/// Every shipped text box, in precedence order.
///
/// Rename is first because a draft is the newer intention: opening a rename over
/// an open search box means the next key is part of the name. The bare-letter
/// tool shortcuts survive an open box because focus is explicit — a box that was
/// never clicked into holds no keys.
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
        push: AppState::rename_push,
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
        push: AppState::bin_rename_push,
    },
    TextField {
        id: "aria.field.effect-preset",
        focused: AppState::effect_preset_draft_open,
        text: AppState::effect_preset_draft_text,
        escape: AppState::cancel_effect_preset_draft,
        backspace: AppState::effect_preset_draft_pop,
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
        push: AppState::effect_preset_draft_push,
    },
    TextField {
        id: "aria.field.stock-search",
        focused: |state| state.stock_search_focus,
        text: |state| state.stock_query.clone(),
        escape: |state| state.focus_stock_search(false),
        backspace: AppState::stock_query_pop,
        submit: |state, cx| state.run_stock_search(cx),
        history: None,
        push: AppState::stock_query_push,
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
        push: AppState::generate_prompt_push,
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
        push: AppState::caption_language_push,
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
        push: AppState::media_query_push,
    },
    TextField {
        id: "aria.field.setting",
        focused: |state| state.settings_form.focused.is_some(),
        text: |state| state.settings_form.draft.clone(),
        escape: AppState::cancel_setting,
        backspace: AppState::setting_pop,
        submit: |state, _| {
            if let Err(err) = state.commit_setting() {
                state.report_error(err);
            }
        },
        history: None,
        push: AppState::setting_push,
    },
    TextField {
        id: "aria.field.clock",
        focused: AppState::clock_focused,
        text: |state| state.clock_draft.clone().unwrap_or_default(),
        escape: |state| state.focus_clock(false),
        backspace: AppState::clock_pop,
        submit: |state, _| state.commit_clock(),
        history: None,
        push: AppState::clock_push,
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
        push: AppState::assistant_prompt_push,
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
