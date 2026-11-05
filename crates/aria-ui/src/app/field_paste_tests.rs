//! Every key-routed box is one line with a caret and a selection.
//!
//! Closes two classes. The first is "a box that takes only a character at a
//! time": nothing in the app took a paste or an IME commit, so a pasted path,
//! a pasted prompt and every non-Latin input were unreachable. The second is
//! "a box that only appends": every box but the rename bar and the title kept
//! its caret at the end, so the arrows, Home, End, Shift and a selection did
//! nothing in a search, a prompt, a clock or a dialog field. Each case below
//! puts focus in one shipped box and drives it through its row, the table the
//! shell's key handler and input handler read, so a row added without a case
//! turns the suite red, and a row whose box loses its caret goes red here.
//!
//! The box's own rule survives every edit: the clock still refuses letters,
//! a value still takes only what a number is written with, and a name still
//! stops at its cap. A run the rule refuses whole leaves the selection it was
//! typed over in place. Not covered: the drawn caret, which
//! `components_field_tests` measures.

use super::AppState;
use super::premiere_tests::with_clip;
use crate::app::settings_form::SettingsField;
use crate::keys::fields::{self, CaretKey};
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
        id: "aria.field.palette",
        focus: |state| state.open_dialog(crate::app::DialogKind::Palette),
        paste: "export frame",
        expect: "export frame",
    },
    Case {
        id: "aria.field.value",
        focus: super::value_scrub_tests::open_probe_entry,
        // A value box takes the characters a number is written with and drops
        // the rest, and the first one replaces the selected value.
        paste: "12.5 px",
        expect: "12.5",
    },
    Case {
        id: "aria.field.bin-rename",
        focus: |state| {
            // A session starts with no bins: the box under test only exists
            // once there is a folder to rename, so the case makes one.
            state.create_bin().expect("a bin");
            let bin = state.store.bins.first().map(|item| item.id).expect("a bin");
            state.begin_bin_rename(bin);
            clear(state);
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
        id: "aria.field.effects-search",
        focus: AppState::open_effects_panel,
        paste: "gaussian blur",
        expect: "gaussian blur",
    },
    Case {
        id: "aria.field.property-search",
        focus: |state| state.focus_property_search(true),
        paste: "anchor point",
        expect: "anchor point",
    },
    Case {
        id: "aria.field.marker-edit",
        focus: |state| {
            state.add_marker_at_playhead().expect("a marker");
            state.edit_marker_at_playhead();
            state.focus_marker_edit_field(Some(crate::app::MarkerEditField::Name));
            clear(state);
        },
        paste: "Chapter 1",
        expect: "Chapter 1",
    },
    Case {
        id: "aria.field.setting",
        focus: |state| {
            state.focus_setting(Some(SettingsField::AiBaseUrl));
            clear(state);
        },
        paste: "http://localhost:11434/v1",
        expect: "http://localhost:11434/v1",
    },
    Case {
        id: "aria.field.clock",
        focus: |state| {
            state.focus_clock(true);
            clear(state);
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
        id: "aria.field.preset",
        focus: |state| {
            // The name belongs to a clip's stack, so the box only opens once a
            // clip is selected: the field refuses to appear otherwise.
            let clip = first_clip(state);
            state.select_clip(aria_models::ClipId::from_raw(clip));
            state.open_preset_draft();
        },
        paste: "Warm grade",
        expect: "Warm grade",
    },
    Case {
        id: "aria.field.driver",
        focus: |state| {
            // A driver belongs to a property of a selected clip, so the box
            // opens on one: Alt+click adds the driver and opens its source.
            state
                .ctx
                .mount(&aria_plugin::ExpressionsPlugin)
                .expect("mount Expressions");
            let clip = first_clip(state);
            state.select_clip(aria_models::ClipId::from_raw(clip));
            state
                .toggle_driver(aria_models::ClipParam::Rotation)
                .expect("add a driver");
            clear(state);
        },
        paste: "wiggle(2, 15)",
        expect: "wiggle(2, 15)",
    },
    Case {
        id: "aria.field.plugin-panel",
        focus: super::plugin_panels_tests::focus_probe_input,
        paste: "Take 3",
        expect: "Take 3",
    },
    Case {
        id: "aria.field.title",
        focus: |state| {
            // The Type tool places a title with its placeholder selected, so the
            // run replaces the placeholder.
            let [width, height] = state.canvas_size();
            let canvas =
                crate::app::canvas_overlay::Rect::new(0.0, 0.0, width as f32, height as f32);
            state.overlay_frame = crate::app::canvas_overlay::FrameBox {
                container: Some(canvas),
                canvas: Some(canvas),
                layer: None,
            };
            state.tool = crate::types::EditorTool::Type;
            assert!(state.select_layer_at([width as f32 * 0.5, height as f32 * 0.3], false));
        },
        paste: "Night falls",
        expect: "Night falls",
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

/// Empty the focused box as Ctrl+A and Backspace do.
fn clear(state: &mut AppState) {
    let field = fields::focused(state).expect("a box holds focus");
    field.key(state, CaretKey::SelectAll, false);
    field.backspace(state);
    assert_eq!(
        field.text(state),
        "",
        "{} kept text past Ctrl+A, Backspace",
        field.id
    );
}

/// A state with focus in `case`'s box, holding its pasted run.
fn pasted(case: &Case) -> (AppState, fields::TextField) {
    let (mut state, _) = with_clip();
    (case.focus)(&mut state);
    let field = fields::focused(&state).unwrap_or_else(|| panic!("{} did not take focus", case.id));
    assert_eq!(field.id, case.id, "{} focused another box", case.id);
    assert!(fields::insert(&mut state, case.paste), "{}", case.id);
    (state, field)
}

/// The first `count` characters of `text`.
fn head(text: &str, count: usize) -> &str {
    &text[..text
        .char_indices()
        .nth(count)
        .map_or(text.len(), |(at, _)| at)]
}

/// Every shipped box has a paste case. A row added without one is a box whose
/// paste behavior nobody decided, which is the state this row started in.
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

/// A multi-character insert lands whole, in whichever box holds focus, with
/// the caret after it.
#[test]
fn a_pasted_run_lands_whole_in_every_field() {
    for case in CASES {
        let (state, field) = pasted(case);
        assert_eq!(
            field.text(&state),
            case.expect,
            "{} did not take the run whole",
            case.id
        );
        let end = case.expect.chars().count();
        assert_eq!(field.selection(&state), (end, end), "{}", case.id);
    }
}

/// The caret moves inside every box, and typing lands at it rather than at
/// the end: Home, Right, then a digit, which every box's rule admits.
#[test]
fn every_field_types_at_its_caret() {
    for case in CASES {
        let (mut state, field) = pasted(case);
        field.key(&mut state, CaretKey::LineStart, false);
        assert_eq!(field.selection(&state), (0, 0), "{}: Home", case.id);
        field.key(&mut state, CaretKey::Right, false);
        assert!(
            fields::insert(&mut state, "7"),
            "{} refused a digit",
            case.id
        );
        let (first, rest) = case.expect.split_at(head(case.expect, 1).len());
        assert_eq!(
            field.text(&state),
            format!("{first}7{rest}"),
            "{} typed elsewhere",
            case.id
        );
        assert_eq!(field.selection(&state), (2, 2), "{}", case.id);
    }
}

/// Shift extends a selection from the caret in every box, the selection is
/// what Ctrl+C copies, and Backspace takes it whole. Every case pastes at
/// least two characters, so the selection after the first is never empty.
#[test]
fn every_field_selects_with_shift_and_deletes_the_selection() {
    for case in CASES {
        let (mut state, field) = pasted(case);
        field.key(&mut state, CaretKey::LineStart, false);
        field.key(&mut state, CaretKey::Right, false);
        field.key(&mut state, CaretKey::LineEnd, true);
        let end = case.expect.chars().count();
        assert_eq!(field.selection(&state), (1, end), "{}: Shift+End", case.id);
        let (kept, selected) = case.expect.split_at(head(case.expect, 1).len());
        assert_eq!(
            field.selected(&state).as_deref(),
            Some(selected),
            "{}",
            case.id
        );
        field.backspace(&mut state);
        assert_eq!(field.text(&state), kept, "{} kept the selection", case.id);
        assert_eq!(field.selection(&state), (1, 1), "{}", case.id);
    }
}

/// Ctrl+Z in a box takes back its typing and Ctrl+Shift+Z puts it back, in
/// every box. A box that let Ctrl+Z reach the timeline would undo the last
/// edit there and leave the typed text standing.
#[test]
fn every_field_undoes_and_redoes_its_typing() {
    for case in CASES {
        let (mut state, field) = pasted(case);
        field.key(&mut state, CaretKey::Undo, false);
        assert_ne!(
            field.text(&state),
            case.expect,
            "{}: Undo kept the paste",
            case.id
        );
        field.key(&mut state, CaretKey::Redo, false);
        assert_eq!(field.text(&state), case.expect, "{}: Redo", case.id);
    }
}

/// Delete takes the character after the caret, and Ctrl+A selects the whole
/// line, in every box.
#[test]
fn every_field_deletes_forward_and_selects_all() {
    for case in CASES {
        let (mut state, field) = pasted(case);
        field.key(&mut state, CaretKey::LineStart, false);
        field.key(&mut state, CaretKey::Delete, false);
        assert_eq!(
            field.text(&state),
            &case.expect[head(case.expect, 1).len()..],
            "{}: Delete",
            case.id
        );
        field.key(&mut state, CaretKey::SelectAll, false);
        let rest = case.expect.chars().count() - 1;
        assert_eq!(field.selection(&state), (0, rest), "{}: Ctrl+A", case.id);
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
    assert_eq!(field.text(&state), "onetwotab");
}

/// A run a box refuses whole leaves the box as it was, in every box. A bell
/// pasted over a selected name, or a letter typed over the selected timecode,
/// would otherwise delete the selection and put nothing in its place.
#[test]
fn a_run_refused_whole_keeps_the_selection_it_was_typed_over() {
    for case in CASES {
        let (mut state, field) = pasted(case);
        field.key(&mut state, CaretKey::SelectAll, false);
        let before = (field.text(&state).into_owned(), field.selection(&state));
        assert!(
            !fields::insert(&mut state, "\u{7}"),
            "{} took a bell",
            case.id
        );
        let after = (field.text(&state).into_owned(), field.selection(&state));
        assert_eq!(after, before, "{}: a refused run changed the box", case.id);
    }
    let clock = CASES
        .iter()
        .find(|case| case.id == "aria.field.clock")
        .expect("the clock case");
    let (mut state, field) = pasted(clock);
    field.key(&mut state, CaretKey::SelectAll, false);
    assert!(!fields::insert(&mut state, "x"), "the clock took a letter");
    assert_eq!(field.text(&state), clock.expect);
    let end = clock.expect.chars().count();
    assert_eq!(field.selection(&state), (0, end));
}

/// An IME replaces the run it marked. The range is characters, not bytes, so
/// a preedit in a non-Latin script does not leave half of itself behind.
#[test]
fn a_marked_run_is_replaced_not_doubled() {
    let (mut state, _) = with_clip();
    state.focus_assistant_prompt(true);
    fields::insert(&mut state, "にほ");
    let field = fields::focused(&state).expect("prompt");
    assert_eq!(field.text(&state), "にほ");

    // The commit replaces the two marked characters with the finished word.
    assert!(field.replace(&mut state, 0..2, "日本語"));
    assert_eq!(field.text(&state), "日本語");
    assert_eq!(field.selection(&state), (3, 3));
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
    let text = field.text(&state);
    assert!(
        text.chars().count() < long.chars().count(),
        "the cap did not hold: {} characters",
        text.chars().count()
    );
    assert!(text.chars().all(|ch| ch == 'x'));
}
