//! A run of characters lands in every key-routed box.
//!
//! Typing was routed a character at a time, so nothing in the app took a paste
//! or an IME commit: a pasted path, a pasted prompt, and every non-Latin input
//! were unreachable. The run now goes through each row's own `push`, which is
//! why these tests check the box's own rules survive it — the clock still
//! refuses letters and a name still stops at its cap.

use super::AppState;
use super::premiere_tests::with_clip;
use crate::app::settings_form::SettingsField;
use crate::keys::fields;
use crate::types::RenameTarget;

/// How to put focus in one box, what to paste there, and what the box should
/// hold afterwards. The sweep below asserts this covers every shipped row, so a
/// new box turns the suite red until someone says what a paste does to it.
struct Case {
    id: &'static str,
    focus: fn(&mut AppState),
    paste: &'static str,
    expect: &'static str,
}

const CASES: &[Case] = &[
    Case {
        id: "aria.field.rename",
        focus: |state| {
            let clip = first_clip(state);
            state.begin_rename(RenameTarget::Clip(clip));
            state.rename_draft.clear();
        },
        paste: "Opening shot",
        expect: "Opening shot",
    },
    Case {
        id: "aria.field.bin-rename",
        focus: |state| {
            // A session starts with no bins: the box under test only exists
            // once there is a folder to rename, so the case makes one.
            state.create_bin().expect("a bin");
            let bin = state.store.bins.first().map(|item| item.id).expect("a bin");
            state.begin_bin_rename(bin);
            while !(fields::focused(state).expect("bin rename").text)(state).is_empty() {
                state.bin_rename_pop();
            }
        },
        paste: "B roll",
        expect: "B roll",
    },
    Case {
        id: "aria.field.stock-search",
        focus: |state| state.focus_stock_search(true),
        paste: "city at night",
        expect: "city at night",
    },
    Case {
        id: "aria.field.generate-prompt",
        focus: |state| state.focus_generate_prompt(true),
        paste: "a slow pan over rooftops",
        expect: "a slow pan over rooftops",
    },
    Case {
        id: "aria.field.caption-language",
        focus: |state| state.focus_caption_language(true),
        paste: "ja",
        expect: "ja",
    },
    Case {
        id: "aria.field.bin-search",
        focus: |state| state.focus_media_search(true),
        paste: "interview",
        expect: "interview",
    },
    Case {
        id: "aria.field.setting",
        focus: |state| {
            state.focus_setting(Some(SettingsField::AiBaseUrl));
            while !(fields::focused(state).expect("setting").text)(state).is_empty() {
                state.setting_pop();
            }
        },
        paste: "http://localhost:11434/v1",
        expect: "http://localhost:11434/v1",
    },
    Case {
        id: "aria.field.clock",
        focus: |state| {
            state.focus_clock(true);
            while !(fields::focused(state).expect("clock").text)(state).is_empty() {
                state.clock_pop();
            }
        },
        // The clock takes digits and separators only, and a paste is held to the
        // same rule as a keystroke: the letters in "at 00:00:12:00" are dropped.
        paste: "at 00:00:12:00",
        expect: "00:00:12:00",
    },
    Case {
        id: "aria.field.ai-prompt",
        focus: |state| state.focus_assistant_prompt(true),
        paste: "trim the last two seconds",
        expect: "trim the last two seconds",
    },
    Case {
        id: "aria.field.effect-preset",
        focus: |state| {
            // The name belongs to a clip's stack, so the box only opens once a
            // clip is selected: the field refuses to appear otherwise.
            let clip = first_clip(state);
            state.select_clip(aria_models::ClipId::from_raw(clip));
            state.open_effect_preset_draft();
        },
        paste: "Warm grade",
        expect: "Warm grade",
    },
];

fn first_clip(state: &AppState) -> u64 {
    state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .next()
        .expect("a clip")
        .id
}

/// Every shipped box has a paste case. A row added without one is a box whose
/// paste behaviour nobody decided, which is the state this row started in.
#[test]
fn every_shipped_field_has_a_paste_case() {
    for field in fields::SHIPPED_FIELDS {
        assert!(
            CASES.iter().any(|case| case.id == field.id),
            "{} has no paste case",
            field.id
        );
    }
    for case in CASES {
        assert!(
            fields::SHIPPED_FIELDS
                .iter()
                .any(|field| field.id == case.id),
            "{} is not a shipped field",
            case.id
        );
    }
}

/// A multi-character insert lands whole, in whichever box holds focus.
#[test]
fn a_pasted_run_lands_whole_in_every_field() {
    for case in CASES {
        let (mut state, _) = with_clip();
        (case.focus)(&mut state);
        let field =
            fields::focused(&state).unwrap_or_else(|| panic!("{} did not take focus", case.id));
        assert_eq!(field.id, case.id, "{} focused another box", case.id);

        assert!(fields::insert(&mut state, case.paste), "{}", case.id);
        let text = (field.text)(&state);
        assert_eq!(text, case.expect, "{} did not take the run whole", case.id);
    }
}

/// A control character is not text. A pasted path with a trailing newline is a
/// path, and a box has no second line to move to.
#[test]
fn a_pasted_run_drops_control_characters() {
    let (mut state, _) = with_clip();
    state.focus_media_search(true);
    fields::insert(&mut state, "one\ntwo\r\n\ttab\u{7}");
    let field = fields::focused(&state).expect("search");
    assert_eq!((field.text)(&state), "onetwotab");
}

/// An IME replaces the run it marked, which the boxes express as pops. The
/// count is characters, not bytes, so a preedit in a non-Latin script does not
/// leave half of itself behind.
#[test]
fn a_marked_run_is_replaced_not_doubled() {
    let (mut state, _) = with_clip();
    state.focus_assistant_prompt(true);
    fields::insert(&mut state, "にほ");
    let field = fields::focused(&state).expect("prompt");
    assert_eq!((field.text)(&state), "にほ");

    // The commit replaces the two marked characters with the finished word.
    assert!(fields::truncate(&mut state, 2));
    assert_eq!((field.text)(&state), "");
    fields::insert(&mut state, "日本語");
    assert_eq!((field.text)(&state), "日本語");
}

/// With no box focused there is nothing to paste into, and the run must not be
/// dropped into the last box that had focus.
#[test]
fn a_run_with_nothing_focused_is_refused() {
    let (mut state, _) = with_clip();
    state.focus_assistant_prompt(true);
    fields::insert(&mut state, "kept");
    state.focus_assistant_prompt(false);

    assert!(fields::focused(&state).is_none());
    assert!(!fields::insert(&mut state, "lost"));
    assert!(!fields::truncate(&mut state, 3));
    assert_eq!(state.assistant.prompt, "kept");
}

/// A box's length cap holds against a paste. Pasting a page into a clip name
/// must not produce a name the model would refuse.
#[test]
fn a_pasted_run_respects_the_length_cap() {
    let (mut state, _) = with_clip();
    let clip = first_clip(&state);
    state.begin_rename(RenameTarget::Clip(clip));
    state.rename_draft.clear();
    let long = "x".repeat(4096);
    fields::insert(&mut state, &long);
    let field = fields::focused(&state).expect("rename");
    let text = (field.text)(&state);
    assert!(
        text.chars().count() < long.chars().count(),
        "the cap did not hold: {} characters",
        text.chars().count()
    );
    assert!(text.chars().all(|ch| ch == 'x'));
}
