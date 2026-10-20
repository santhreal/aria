//! The rename draft's caret and selection, typed the way the shell routes keys.
//!
//! Closes the class "a rename that opens on the old name and appends to it":
//! the draft opened with its name and a caret after it, so typing a new name
//! produced the old name with the new one stuck on the end. The draft now opens
//! with the name selected. Every edit here goes through the rename row of
//! `keys::fields`, the table the shell's key handler and text-input handler
//! read, so a row that stopped naming the caret functions goes red here rather
//! than in a hand.
//!
//! Covers typing over the selection, collapsing it from either end, Shift
//! extending it by character and by word, Backspace and Delete with and
//! without a selection, Ctrl+A, undo to the opening name and redo, the length
//! cap on a replaced selection, control characters in a paste, and the bar
//! drawing the selection and the caret at its end. Not covered: an IME preedit
//! run, which `shell::text_input` marks through the same `replace` call.

use aria_commands::{Command, EditCommand};

use crate::app::AppState;
use crate::app::rename::RENAME_MAX_LEN;
use crate::keys::fields::{self, Caret, CaretKey};
use crate::types::RenameTarget;

const OPENING: &str = "Night Exterior";

/// Call the project [`OPENING`] through the engine, so the store and the
/// model agree on it.
fn name_project(state: &mut AppState) {
    state
        .apply_command(Command::Edit(EditCommand::SetProjectName {
            name: OPENING.to_string(),
        }))
        .expect("the project takes a name");
}

/// A state renaming its project, which is called [`OPENING`].
fn renaming() -> AppState {
    let (mut state, _) = crate::app::premiere_tests::with_clip();
    name_project(&mut state);
    state.begin_rename(RenameTarget::Project);
    state
}

/// The rename row's caret: the functions the shell calls for its keys.
fn caret(state: &AppState) -> Caret {
    let field = fields::focused(state).expect("the rename bar holds the keys");
    assert_eq!(field.id, "aria.field.rename");
    field.caret.expect("the rename row has a caret")
}

fn key(state: &mut AppState, key: CaretKey, shift: bool) {
    (caret(state).key)(state, key, shift);
}

/// What typing `text` does: the input handler replaces the selection.
fn type_text(state: &mut AppState, text: &str) {
    assert!(fields::insert(state, text), "no box took {text:?}");
}

fn backspace(state: &mut AppState) {
    let field = fields::focused(state).expect("the rename bar holds the keys");
    (field.backspace)(state);
}

/// The draft and its `(anchor, caret)`.
fn draft(state: &AppState) -> (String, (usize, usize)) {
    (state.rename_draft.clone(), (caret(state).selection)(state))
}

#[test]
fn a_rename_opens_on_its_whole_name_selected_and_typing_replaces_it() {
    let mut state = renaming();
    let len = OPENING.chars().count();
    assert_eq!(draft(&state), (OPENING.to_string(), (0, len)));
    assert_eq!((caret(&state).selected)(&state).as_deref(), Some(OPENING));
    type_text(&mut state, "Cut 2");
    assert_eq!(draft(&state), ("Cut 2".to_string(), (5, 5)));
    assert_eq!((caret(&state).selected)(&state), None);
    state.commit_rename().expect("the new name lands");
    assert_eq!(state.store.project_name.to_string(), "Cut 2");
}

#[test]
fn an_arrow_keeps_the_name_and_puts_the_caret_at_that_end() {
    let len = OPENING.chars().count();
    let mut state = renaming();
    key(&mut state, CaretKey::Right, false);
    assert_eq!(draft(&state).1, (len, len));
    type_text(&mut state, " 2");
    assert_eq!(draft(&state).0, "Night Exterior 2");

    let mut state = renaming();
    key(&mut state, CaretKey::Left, false);
    assert_eq!(draft(&state).1, (0, 0));
    type_text(&mut state, "Late ");
    assert_eq!(draft(&state).0, "Late Night Exterior");

    for (end, at) in [(CaretKey::LineEnd, len), (CaretKey::Down, len)] {
        let mut state = renaming();
        key(&mut state, CaretKey::Left, false);
        key(&mut state, end, false);
        assert_eq!(draft(&state).1, (at, at), "{end:?}");
    }
    for start in [CaretKey::LineStart, CaretKey::TextStart, CaretKey::Up] {
        let mut state = renaming();
        key(&mut state, CaretKey::Right, false);
        key(&mut state, start, false);
        assert_eq!(draft(&state).1, (0, 0), "{start:?}");
    }
}

#[test]
fn shift_extends_the_selection_by_character_and_by_word() {
    let len = OPENING.chars().count();
    let mut state = renaming();
    key(&mut state, CaretKey::TextEnd, false);
    key(&mut state, CaretKey::WordLeft, true);
    assert_eq!(
        draft(&state).1,
        (len, 6),
        "Shift+Ctrl+Left selects the last word"
    );
    assert_eq!(
        (caret(&state).selected)(&state).as_deref(),
        Some("Exterior")
    );
    type_text(&mut state, "Interior");
    assert_eq!(draft(&state).0, "Night Interior");

    let mut state = renaming();
    key(&mut state, CaretKey::LineStart, false);
    key(&mut state, CaretKey::Right, true);
    key(&mut state, CaretKey::Right, true);
    assert_eq!(draft(&state).1, (0, 2));
    key(&mut state, CaretKey::Left, true);
    assert_eq!(
        draft(&state).1,
        (0, 1),
        "Shift+Left shrinks from the caret end"
    );
    key(&mut state, CaretKey::WordRight, false);
    assert_eq!(draft(&state).1, (6, 6), "Ctrl+Right stops at the next word");
    key(&mut state, CaretKey::Right, false);
    key(&mut state, CaretKey::SelectAll, false);
    assert_eq!(draft(&state).1, (0, len), "Ctrl+A selects the whole draft");
}

#[test]
fn backspace_and_delete_take_the_selection_or_one_character() {
    let mut state = renaming();
    backspace(&mut state);
    assert_eq!(
        draft(&state),
        (String::new(), (0, 0)),
        "Backspace took the name"
    );

    let mut state = renaming();
    key(&mut state, CaretKey::Delete, false);
    assert_eq!(draft(&state).0, "", "Delete took the name");

    let mut state = renaming();
    key(&mut state, CaretKey::LineStart, false);
    key(&mut state, CaretKey::WordRight, false);
    backspace(&mut state);
    assert_eq!(draft(&state), ("NightExterior".to_string(), (5, 5)));
    key(&mut state, CaretKey::Delete, false);
    assert_eq!(draft(&state), ("Nightxterior".to_string(), (5, 5)));

    let mut state = renaming();
    key(&mut state, CaretKey::LineStart, false);
    backspace(&mut state);
    key(&mut state, CaretKey::TextEnd, false);
    key(&mut state, CaretKey::Delete, false);
    assert_eq!(
        draft(&state).0,
        OPENING,
        "Backspace at the start or Delete at the end removed something"
    );
}

#[test]
fn undo_restores_the_opening_name_and_redo_the_typed_draft() {
    let len = OPENING.chars().count();
    let mut state = renaming();
    let depth = state.history_depth();
    type_text(&mut state, "Day");
    key(&mut state, CaretKey::Undo, false);
    assert_eq!(draft(&state), (OPENING.to_string(), (0, len)));
    key(&mut state, CaretKey::Redo, false);
    assert_eq!(draft(&state), ("Day".to_string(), (3, 3)));
    key(&mut state, CaretKey::Redo, false);
    assert_eq!(
        draft(&state),
        ("Day".to_string(), (3, 3)),
        "a second redo did something"
    );

    key(&mut state, CaretKey::Undo, false);
    type_text(&mut state, "Dusk");
    key(&mut state, CaretKey::Redo, false);
    assert_eq!(
        draft(&state).0,
        "Dusk",
        "typing after an undo kept the redo"
    );
    assert_eq!(
        state.history_depth(),
        depth,
        "the draft's undo reached the edit history"
    );
}

#[test]
fn a_paste_drops_control_characters_and_stops_at_the_cap() {
    let mut state = renaming();
    type_text(&mut state, "Scene\n4\t");
    assert_eq!(draft(&state).0, "Scene4");

    let mut state = renaming();
    key(&mut state, CaretKey::Right, false);
    type_text(&mut state, &"x".repeat(RENAME_MAX_LEN * 2));
    assert_eq!(state.rename_draft.chars().count(), RENAME_MAX_LEN);

    // A full draft still takes a name over its selection: the cap counts what
    // the draft holds after the selection is gone.
    key(&mut state, CaretKey::SelectAll, false);
    type_text(&mut state, "Short");
    assert_eq!(draft(&state), ("Short".to_string(), (5, 5)));
}

#[test]
fn a_draft_set_without_the_caret_holds_the_caret_inside_it() {
    let mut state = renaming();
    state.rename_draft = "ab".into();
    assert_eq!(
        draft(&state).1,
        (0, 2),
        "the caret is read past the end of the draft"
    );
    type_text(&mut state, "c");
    assert_eq!(draft(&state), ("c".to_string(), (1, 1)));
}

/// WHY: the selection is a state that is invisible unless the bar draws it.
/// Opens the bar in a drawn shell, checks the tinted run spans the name with
/// the caret at its right edge, types through the platform input handler and
/// walks the caret with the shell's own key routing, none of which requests a
/// Viewer frame: a name changes no picture.
#[gpui::test]
fn the_bar_draws_the_selection_and_the_caret_at_its_end(cx: &mut gpui::TestAppContext) {
    let (state, visual) = crate::test_support::drawn_shell(cx, "rename-caret");
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            name_project(state);
            state.begin_rename(RenameTarget::Project);
            // A state change made outside the shell notifies nothing, so the
            // window is asked to draw again.
            cx.notify();
        })
    });
    visual.run_until_parked();
    let selection = visual
        .debug_bounds("rename-selection")
        .expect("the opening name is drawn selected");
    let caret = visual
        .debug_bounds("rename-caret")
        .expect("the bar draws its caret");
    assert_eq!(
        caret.left(),
        selection.right(),
        "the caret is not at the selection's end"
    );
    let frames = visual.read(|app| state.read(app).preview_generation);

    visual.simulate_input("D");
    visual.run_until_parked();
    let typed = visual.read(|app| draft(state.read(app)));
    assert_eq!(
        typed,
        ("D".to_string(), (1, 1)),
        "typing did not replace the name"
    );
    assert_eq!(visual.debug_bounds("rename-selection"), None);

    visual.simulate_input("ay");
    visual.simulate_keystrokes("shift-left shift-left");
    visual.run_until_parked();
    let walked = visual.read(|app| draft(state.read(app)));
    assert_eq!(walked, ("Day".to_string(), (3, 1)));
    let selection = visual
        .debug_bounds("rename-selection")
        .expect("Shift+Left draws a selection");
    let caret = visual
        .debug_bounds("rename-caret")
        .expect("the bar draws its caret");
    assert_eq!(
        caret.left(),
        selection.left(),
        "the caret is not at the selection's moving end"
    );
    assert_eq!(
        visual.read(|app| state.read(app).preview_generation),
        frames,
        "a key in the rename bar asked the Viewer for a frame"
    );
}
