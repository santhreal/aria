//! Styling the characters selected in a title from the Character panel.
//!
//! # Why
//!
//! Closes the class where the Character panel styled the whole title while
//! characters were selected in the Viewer, and where its controls read the
//! title's values rather than the selection's. Holds every property a run
//! holds (the face switches, the font, the size, the tracking, the fill) to
//! writing a run over the selection and reading it back, the properties a run
//! does not hold to the whole title, the whole title with nothing selected, and
//! the run through typing and undo.
//!
//! Does not catch the paint of a styled run; that is `aria-render`'s rich
//! text tests.

use aria_models::{ClipId, ClipParam, ClipSource, Generator, TextParam, TextStyle};

use super::super::FAMILIES;
use super::super::text_caret::tests::{type_text, typing};
use crate::app::AppState;
use crate::app::clip_text::TextSwitch;
use crate::keys::fields::CaretKey;

/// The title's style as the project holds it.
fn style(state: &AppState, clip: ClipId) -> TextStyle {
    match &state
        .engine
        .lock()
        .project()
        .clip(clip)
        .expect("title")
        .content
    {
        ClipSource::Generated(Generator::Text { style, .. }) => style.clone(),
        other => panic!("not a title: {other:?}"),
    }
}

/// A title reading "Hello" with "el" selected, and the title.
fn selecting() -> (AppState, ClipId) {
    let (mut state, title, _) = typing();
    type_text(&mut state, "Hello");
    state.title_caret_key(CaretKey::TextStart, false);
    state.title_caret_key(CaretKey::Right, false);
    state.title_caret_key(CaretKey::Right, true);
    state.title_caret_key(CaretKey::Right, true);
    assert_eq!(state.text_style_range(), Some(1..3));
    (state, title)
}

/// Collapse the selection to its end.
fn collapse(state: &mut AppState) {
    state.title_caret_key(CaretKey::Right, false);
    assert_eq!(state.text_style_range(), None);
}

fn bold(state: &AppState) -> bool {
    state
        .selected_text_face()
        .expect("a title")
        .is_on(TextSwitch::Bold)
}

#[test]
fn a_face_switch_styles_the_selection_and_reads_it_back() {
    let (mut state, title) = selecting();
    state.toggle_selected_text_switch(TextSwitch::Bold).unwrap();
    let held = style(&state, title);
    assert!(!held.bold, "the whole title turned bold");
    assert_eq!(held.runs.len(), 1);
    assert_eq!((held.runs[0].start, held.runs[0].end), (1, 3));
    assert_eq!(held.runs[0].bold, Some(true));
    assert!(bold(&state), "the chip reads the title, not the selection");

    // With nothing selected the chip reads, and the switch writes, the title.
    collapse(&mut state);
    assert!(!bold(&state));
    state.toggle_selected_text_switch(TextSwitch::Bold).unwrap();
    let held = style(&state, title);
    assert!(held.bold);
    assert_eq!(held.runs.len(), 1, "a whole-title switch touched the runs");
}

#[test]
fn a_switch_a_run_does_not_hold_styles_the_whole_title() {
    let (mut state, title) = selecting();
    let kerning = style(&state, title).kerning;
    state
        .toggle_selected_text_switch(TextSwitch::Kerning)
        .unwrap();
    let held = style(&state, title);
    assert_eq!(held.kerning, !kerning);
    assert!(held.runs.is_empty());
}

#[test]
fn size_tracking_and_fill_style_the_selection_and_read_it_back() {
    let (mut state, title) = selecting();
    let size = ClipParam::Text {
        param: TextParam::Size,
    };
    let tracking = ClipParam::Text {
        param: TextParam::LetterSpacing,
    };
    let fill = ClipParam::Text {
        param: TextParam::Fill,
    };
    let row = |state: &AppState, param: ClipParam| {
        state
            .param_rows("text")
            .into_iter()
            .find(|row| row.param == param)
            .map(|row| row.value)
    };
    let colour = |state: &AppState| {
        state
            .param_colors("text")
            .into_iter()
            .find(|row| row.param == fill)
            .map(|row| row.value)
    };
    let whole_size = state.param_scalar(size, None).expect("a title has a size");
    let whole_fill = colour(&state).expect("a title has a fill");
    let red = [255, 0, 0, 255];
    assert_ne!(whole_fill, red);

    state.set_param_scalar(size, None, 80.0).unwrap();
    assert_eq!(state.param_scalar(size, None), Some(80.0));
    assert_eq!(row(&state, size), Some(80.0));
    state.nudge_param(size, None, 4.0).unwrap();
    assert_eq!(state.param_scalar(size, None), Some(84.0));
    state.set_param_scalar(tracking, None, 12.0).unwrap();
    assert_eq!(row(&state, tracking), Some(12.0));
    state.set_param_color(fill, red).unwrap();
    assert_eq!(colour(&state), Some(red));

    let held = style(&state, title);
    assert_eq!(held.runs.len(), 1);
    let run = &held.runs[0];
    assert_eq!((run.start, run.end), (1, 3));
    assert_eq!(
        (run.size, run.letter_spacing, run.fill),
        (Some(84.0), Some(12.0), Some(red))
    );

    // The title's own values are untouched and read back once the selection
    // collapses.
    collapse(&mut state);
    assert_eq!(state.param_scalar(size, None), Some(whole_size));
    assert_eq!(colour(&state), Some(whole_fill));
}

#[test]
fn a_run_size_is_clamped_to_the_bound_and_a_broken_value_is_dropped() {
    let (mut state, title) = selecting();
    let size = ClipParam::Text {
        param: TextParam::Size,
    };
    let bound = state
        .key_param(size)
        .and_then(|row| row.spec.range.max)
        .expect("a title's size has a ceiling");
    state.set_param_scalar(size, None, f32::MAX).unwrap();
    assert_eq!(style(&state, title).runs[0].size, Some(bound));
    state.set_param_scalar(size, None, f32::NAN).unwrap();
    assert_eq!(style(&state, title).runs[0].size, Some(bound));
}

#[test]
fn a_font_styles_the_selection_and_the_empty_name_returns_it_to_the_title() {
    let Some(family) = FAMILIES.first().cloned() else {
        return;
    };
    let (mut state, title) = selecting();
    let whole = style(&state, title).font;
    state.set_selected_font(&family).unwrap();
    let held = style(&state, title);
    assert_eq!(held.font, whole, "the whole title changed family");
    assert_eq!(held.runs[0].font.as_deref(), Some(family.as_str()));
    assert_eq!(state.selected_font(), Some(family));
    state.set_selected_font("").unwrap();
    assert_eq!(state.selected_font(), Some(whole.clone()));
    collapse(&mut state);
    assert_eq!(state.selected_font(), Some(whole));
}

#[test]
fn typing_inside_a_styled_run_grows_it_and_undo_restores_it() {
    let (mut state, title) = selecting();
    state.toggle_selected_text_switch(TextSwitch::Bold).unwrap();
    state.title_caret_key(CaretKey::Left, false);
    state.title_caret_key(CaretKey::Right, false);
    assert_eq!(state.text_selection(), (2, 2));
    state.title_push('x');
    let held = style(&state, title);
    assert_eq!((held.runs[0].start, held.runs[0].end), (1, 4));
    state.title_caret_key(CaretKey::Undo, false);
    let held = style(&state, title);
    assert_eq!((held.runs[0].start, held.runs[0].end), (1, 3));
}
