//! What the context menu's offered rows do when pressed.
//!
//! WHY: closes, for the context menus, the classes the menu bar's census closes
//! in `menu_reach_tests_row`: an offered row that only answers why it did
//! nothing, and a row that records an undo step over nothing, which spends the
//! next Undo on nothing and marks the project unsaved with nothing to save.
//! Every row the census presses is held to these tests with no edit here, in
//! every dressed state that offers it, and again over its own result where the
//! reopened menu still offers it.
//!
//! What this does not catch: a press in a state no stage dresses, a decline
//! worded outside `Decline`, and a repeat of a row the reopened menu no longer
//! offers.

use super::pressed;
use crate::app::decline::Decline;
use crate::app::fixtures;
use crate::app::menu_reach_tests::{history_labels, project_text};
use crate::app::premiere_tests::with_clip;

/// A row the menu offers does not answer with a decline reason: the offer and
/// the action read one state.
#[test]
fn an_offered_context_row_never_answers_with_a_decline() {
    let declined = &pressed().declined;
    assert!(
        declined.is_empty(),
        "offered, then declined:\n{}",
        declined.join("\n")
    );
}

/// A press that records an undo step changes the project.
#[test]
fn a_context_row_that_records_a_step_changes_the_project() {
    let empty = &pressed().empty_steps;
    assert!(
        empty.is_empty(),
        "recorded a step over nothing:\n{}",
        empty.join("\n")
    );
}

/// A row pressed again over its own result changes the project again, as a
/// toggle does, or records nothing.
#[test]
fn a_context_row_over_its_own_result_changes_the_project_or_records_nothing() {
    let repeats = &pressed().repeats;
    assert!(
        repeats.is_empty(),
        "recorded a step over their own result:\n{}",
        repeats.join("\n")
    );
}

/// Linear over a Linear keyframe declines and records nothing, however it is
/// reached: the keyframe menu leaves the row out there, and the action holds
/// the same line for any other caller.
#[test]
fn linear_over_a_linear_keyframe_declines_and_records_nothing() {
    let (mut state, key) = fixtures::keyframe_menu(|_, _, _| {});
    let (steps, text) = (history_labels(&state).len(), project_text(&state));
    state
        .set_keyframe_linear_at(key.clip, key.param, key.tick)
        .expect("a decline is not an error");
    assert_eq!(history_labels(&state).len(), steps, "a step was recorded");
    assert_eq!(project_text(&state), text, "the project changed");
    assert_eq!(
        state.status_message.as_deref(),
        Some(Decline::AlreadyLinear.reason())
    );
}

/// Reset Transform over a picture clip already at the default transform
/// declines and records nothing, from the Viewer menu, which leaves the row out
/// there, or from the inspector's reset.
#[test]
fn reset_transform_at_the_default_declines_and_records_nothing() {
    let (mut state, _) = with_clip();
    assert!(
        !state.selected_transform_resets(),
        "the fixture's clip is at the default"
    );
    let (steps, text) = (history_labels(&state).len(), project_text(&state));
    state
        .reset_selected_transform()
        .expect("a decline is not an error");
    assert_eq!(history_labels(&state).len(), steps, "a step was recorded");
    assert_eq!(project_text(&state), text, "the project changed");
    assert_eq!(
        state.status_message.as_deref(),
        Some(Decline::DefaultTransform.reason())
    );
}
