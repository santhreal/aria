//! Typing into a title in the Viewer with the Type tool.
//!
//! # Why
//!
//! Closes the class where the Type tool created a title but could not edit
//! one: a press on a title placed no caret, the keys never reached its text,
//! and nothing selected a range. Holds the caret to the boundaries the renderer
//! lays the glyphs out at (a press, a drag, Shift+press, and the line keys read
//! them), the selection keys, typing over a selection, one undo step per typing
//! burst, the empty title, and the key routing that makes a stale edit inert.
//!
//! Does not catch window event delivery or the paint of the caret, which need
//! a render surface. The caret against glyph pixels is `aria-render`'s
//! `text_caret_placement`.

use aria_models::{ClipId, ClipSource, Generator};

use super::*;
use crate::app::canvas_overlay::{FrameBox, Rect};
use crate::app::premiere_tests::with_clip;
use crate::keys::fields;
use crate::types::EditorTool;

/// A state typing into a new title placed on the picture, its placeholder
/// selected, with the Viewer drawing the canvas one to one, and the clip the
/// picture belongs to.
pub(in crate::app) fn typing() -> (AppState, ClipId, ClipId) {
    let (mut state, picture) = with_clip();
    let [width, height] = state.canvas_size();
    let canvas = Rect::new(0.0, 0.0, width as f32, height as f32);
    state.overlay_frame = FrameBox {
        container: Some(canvas),
        canvas: Some(canvas),
        layer: None,
    };
    state.tool = EditorTool::Type;
    assert!(state.select_layer_at([width as f32 * 0.5, height as f32 * 0.3], false));
    let title = state.selected_clip().expect("the new title is selected");
    assert_ne!(title, picture);
    (state, title, picture)
}

/// The title's text as the project holds it.
fn content(state: &AppState, clip: ClipId) -> String {
    match &state
        .engine
        .lock()
        .project()
        .clip(clip)
        .expect("title")
        .content
    {
        ClipSource::Generated(Generator::Text { content, .. }) => content.clone(),
        other => panic!("not a title: {other:?}"),
    }
}

pub(in crate::app) fn type_text(state: &mut AppState, text: &str) {
    for ch in text.chars() {
        state.title_push(ch);
    }
}

/// The title's boundaries as the renderer measured them.
fn carets(state: &AppState) -> Vec<TitleCaret> {
    state
        .selected
        .as_ref()
        .and_then(|detail| detail.carets.as_deref())
        .expect("a title on screen has carets")
        .to_vec()
}

/// The middle of a boundary, in window pixels: the canvas is drawn one to one.
fn middle(caret: &TitleCaret) -> [f32; 2] {
    [
        0.5 * (caret.top[0] + caret.bottom[0]),
        0.5 * (caret.top[1] + caret.bottom[1]),
    ]
}

#[test]
fn typing_replaces_the_placeholder_a_new_title_starts_with() {
    let (mut state, title, _) = typing();
    assert_eq!(state.text_selection(), (0, 5));
    assert_eq!(
        fields::focused(&state).map(|field| field.id),
        Some("aria.field.title")
    );
    type_text(&mut state, "Hi");
    assert_eq!(content(&state, title), "Hi");
    assert_eq!(state.text_selection(), (2, 2));
}

#[test]
fn a_typing_burst_is_one_undo_step_and_a_caret_move_ends_it() {
    let (mut state, title, _) = typing();
    type_text(&mut state, "Hey");
    state.title_caret_key(CaretKey::Left, false);
    state.title_push('!');
    assert_eq!(content(&state, title), "He!y");
    state.title_caret_key(CaretKey::Undo, false);
    assert_eq!(content(&state, title), "Hey");
    state.title_caret_key(CaretKey::Undo, false);
    assert_eq!(content(&state, title), "Title");
    state.title_caret_key(CaretKey::Redo, false);
    assert_eq!(content(&state, title), "Hey");
    // The caret stays inside the text the history step left.
    let (anchor, caret) = state.text_selection();
    assert!(anchor <= 3 && caret <= 3, "{anchor} {caret}");
}

#[test]
fn shift_extends_the_selection_and_a_plain_arrow_collapses_it() {
    let (mut state, _, _) = typing();
    type_text(&mut state, "Hello there");
    state.title_caret_key(CaretKey::Left, true);
    state.title_caret_key(CaretKey::Left, true);
    assert_eq!(state.text_selection(), (11, 9));
    state.title_caret_key(CaretKey::Left, false);
    assert_eq!(state.text_selection(), (9, 9));
    state.title_caret_key(CaretKey::Right, true);
    state.title_caret_key(CaretKey::Right, false);
    assert_eq!(state.text_selection(), (10, 10));
    state.title_caret_key(CaretKey::WordLeft, true);
    assert_eq!(state.text_selection(), (10, 6));
    state.title_caret_key(CaretKey::WordLeft, false);
    assert_eq!(state.text_selection(), (0, 0));
    state.title_caret_key(CaretKey::WordRight, false);
    assert_eq!(state.text_selection(), (6, 6));
    state.title_caret_key(CaretKey::TextEnd, false);
    assert_eq!(state.text_selection(), (11, 11));
    state.title_caret_key(CaretKey::SelectAll, false);
    assert_eq!(state.text_selection(), (0, 11));
}

#[test]
fn the_line_keys_follow_the_lines_the_title_is_laid_out_in() {
    let (mut state, title, _) = typing();
    type_text(&mut state, "ab");
    let (anchor, caret) = state.text_selection();
    state.replace_title_text(anchor..caret, "\n");
    type_text(&mut state, "cd");
    assert_eq!(content(&state, title), "ab\ncd");
    assert_eq!(state.text_selection(), (5, 5));
    // From the end of the second line, up lands at the end of the first: a
    // count of characters would land anywhere but here.
    state.title_caret_key(CaretKey::Up, false);
    assert_eq!(state.text_selection(), (2, 2));
    state.title_caret_key(CaretKey::LineStart, false);
    assert_eq!(state.text_selection(), (0, 0));
    state.title_caret_key(CaretKey::Down, true);
    assert_eq!(state.text_selection(), (0, 3));
    state.title_caret_key(CaretKey::LineEnd, false);
    assert_eq!(state.text_selection(), (5, 5));
    // Past the last line is the end of the text, before the first its start.
    state.title_caret_key(CaretKey::Down, false);
    assert_eq!(state.text_selection(), (5, 5));
    state.title_caret_key(CaretKey::TextStart, false);
    state.title_caret_key(CaretKey::Up, false);
    assert_eq!(state.text_selection(), (0, 0));
}

#[test]
fn backspace_and_delete_take_the_selection_or_one_character() {
    let (mut state, title, _) = typing();
    type_text(&mut state, "abc");
    state.title_backspace();
    assert_eq!(content(&state, title), "ab");
    state.title_caret_key(CaretKey::Left, false);
    state.title_caret_key(CaretKey::Delete, false);
    assert_eq!(content(&state, title), "a");
    // Nothing after the caret: Delete is inert, and so is Backspace before
    // the first character.
    state.title_caret_key(CaretKey::Delete, false);
    state.title_caret_key(CaretKey::TextStart, false);
    state.title_backspace();
    assert_eq!(content(&state, title), "a");
    state.title_caret_key(CaretKey::SelectAll, false);
    state.title_backspace();
    assert_eq!(content(&state, title), "");
    // An empty title draws nothing, and still has a caret to type at.
    let drawn = state.drawn_text_caret().expect("still typing");
    assert!(drawn.caret.is_some());
    state.title_push('z');
    assert_eq!(content(&state, title), "z");
}

#[test]
fn a_press_on_a_title_places_the_caret_at_the_boundary_under_it() {
    let (mut state, title, picture) = typing();
    type_text(&mut state, "Wide text");
    let stops = carets(&state);
    assert_eq!(stops.len(), 10, "{stops:?}");
    assert!(
        stops.windows(2).all(|pair| pair[0].top[0] < pair[1].top[0]),
        "one line, left to right: {stops:?}"
    );
    assert!(state.select_layer_at(middle(&stops[3]), false));
    assert_eq!(state.text_selection(), (3, 3));
    assert!(state.text_dragging());
    assert!(state.drag_text_caret(middle(&stops[7])));
    assert_eq!(state.text_selection(), (3, 7));
    state.end_text_drag();
    assert!(!state.text_dragging());
    assert!(state.select_layer_at(middle(&stops[1]), true));
    assert_eq!(state.text_selection(), (3, 1));

    // A press on the title with another clip selected selects it, at the
    // boundary under the pointer.
    state.select_clip(picture);
    assert!(!state.text_editing());
    assert!(state.select_layer_at(middle(&stops[5]), false));
    assert_eq!(state.selected_clip(), Some(title));
    assert_eq!(state.text_selection(), (5, 5));

    // Off the title, the press places a new one.
    let [width, height] = state.canvas_size();
    assert!(state.select_layer_at([width as f32 * 0.1, height as f32 * 0.9], false));
    assert_ne!(state.selected_clip(), Some(title));
    assert_eq!(state.text_selection(), (0, 5));
}

#[test]
fn the_drawn_selection_has_one_cell_per_selected_character() {
    let (mut state, _, _) = typing();
    let drawn = state.drawn_text_caret().expect("typing");
    assert_eq!(drawn.selection.len(), 5);
    assert!(drawn.caret.is_some());
    state.title_caret_key(CaretKey::Right, false);
    let drawn = state.drawn_text_caret().expect("typing");
    assert!(drawn.selection.is_empty());
    let stops = carets(&state);
    let end = stops.iter().find(|stop| stop.index == 5).expect("end");
    assert_eq!(drawn.caret, Some([end.top, end.bottom]));
}

#[test]
fn a_pasted_run_replaces_the_selection_in_one_step() {
    let (mut state, title, _) = typing();
    assert!(fields::insert(&mut state, "Yo\tu"));
    assert_eq!(content(&state, title), "You");
    assert!(state.undo());
    assert_eq!(content(&state, title), "Title");
}

#[test]
fn an_edit_is_live_only_with_the_type_tool_on_its_selected_title() {
    let (mut state, title, picture) = typing();
    state.tool = EditorTool::Pointer;
    assert!(fields::focused(&state).is_none());
    state.title_push('x');
    assert_eq!(content(&state, title), "Title");
    state.tool = EditorTool::Type;
    state.select_clip(picture);
    assert!(fields::focused(&state).is_none());
    state.select_clip(title);
    assert!(fields::focused(&state).is_some());
    state.end_text_edit();
    assert!(fields::focused(&state).is_none());
    assert!(state.drawn_text_caret().is_none());
}

#[test]
fn a_caret_at_a_wrap_is_drawn_at_the_start_of_the_next_line() {
    let stop = |index, line| TitleCaret {
        index,
        line,
        top: [index as f32, line as f32],
        bottom: [index as f32, line as f32 + 1.0],
    };
    let stops = [stop(0, 0), stop(1, 0), stop(2, 0), stop(2, 1), stop(3, 1)];
    assert_eq!(caret_stop(&stops, 2).map(|stop| stop.line), Some(1));
    assert_eq!(caret_stop(&stops, 1).map(|stop| stop.line), Some(0));
    // An index the layout draws no boundary for sits on the one before it.
    assert_eq!(caret_stop(&stops, 9).map(|stop| stop.index), Some(3));
    assert_eq!(vertical(&stops, 1, true), Some(2));
    assert_eq!(vertical(&stops, 3, false), Some(2));
    assert_eq!(vertical(&stops, 0, false), None);
}

#[test]
fn word_keys_stop_at_the_starts_of_words() {
    let text = "one  two three";
    assert_eq!(word_right(text, 0), 5);
    assert_eq!(word_right(text, 5), 9);
    assert_eq!(word_right(text, 9), 14);
    assert_eq!(word_left(text, 14), 9);
    assert_eq!(word_left(text, 9), 5);
    assert_eq!(word_left(text, 4), 0);
    assert_eq!(word_left(text, 0), 0);
    // Characters, not bytes.
    assert_eq!(word_right("é è", 0), 2);
    assert_eq!(byte_at("é è", 2), 3);
}
