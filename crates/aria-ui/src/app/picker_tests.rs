//! Contracts for the one seam every file dialog goes through.
//!
//! The class closed here is "a command that asks the operator for a path cannot
//! be driven, and the window stops redrawing while it waits". Both came from one
//! cause: fifteen call sites opened a blocking native dialog on the thread that
//! draws, so a headless run waited for an answer that never came. Waiting for
//! ever is what a suite cannot recover from — it is a stalled run rather than a
//! failure — so the first assertion here is that an unanswered pick returns.
//!
//! What this does not catch: the dialogs themselves. `aria-dialog` opens them and
//! a native dialog needs a person. What is proved instead is that an answer
//! reaches the state through one seam, in the order the questions were asked,
//! and that cancelling is an answer rather than a second path. A new variant of
//! `Ask` is caught by the compiler, because `Ask::answer` matches exhaustively.

use std::path::PathBuf;

use aria_models::ClipId;

use super::picker::{Answer, Ask, FileSpec};
use super::premiere_tests::with_clip;

/// A path named `name` inside a scratch dir of its own.
///
/// The dir is per name, not per test file: claiming a scratch dir empties it, so
/// two calls sharing one dir would delete each other's staged files.
fn scratch(name: &str) -> PathBuf {
    crate::scratch_pool::scratch(&format!("picker-{name}")).join(name)
}

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/texture.png")
}

/// Report the chosen path through the status line, which is the cheapest thing a
/// continuation can observably change.
fn report(state: &mut super::AppState, answer: Answer) {
    state.status_message = Some(match answer.one() {
        Some(path) => path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string(),
        None => "cancelled".to_string(),
    });
}

fn clip_count(state: &super::AppState) -> usize {
    state
        .store
        .sequence
        .tracks
        .iter()
        .map(|track| track.clips.len())
        .sum()
}

#[test]
fn an_unanswered_pick_returns_instead_of_waiting_for_one() {
    // The defect itself: this used to block until a person answered a native
    // dialog, so a test hung rather than failed. The pick is recorded as asked
    // and the continuation does not run.
    let (mut state, _) = with_clip();
    // What the strip already says. The continuation writes the chosen file's
    // name (or "cancelled") over it, so the observation is that nothing wrote,
    // not that nothing had ever written: placing the fixture's clip posts a
    // status of its own.
    let before = state.status_message.clone();
    state.request_pick(Ask::OpenFile(FileSpec::new("Open project")), report);
    assert_eq!(state.picks_asked(), 1, "the pick was not recorded");
    assert_eq!(
        state.status_message, before,
        "the continuation ran without an answer"
    );
}

#[test]
fn a_supplied_answer_runs_the_continuation_at_once() {
    let (mut state, _) = with_clip();
    state.supply_pick(Answer::file(scratch("chosen.aria")));
    state.request_pick(Ask::OpenFile(FileSpec::new("Open project")), report);
    assert_eq!(state.status_message.as_deref(), Some("chosen.aria"));
    assert_eq!(state.picks_unused(), 0, "the answer was not consumed");
}

#[test]
fn answers_are_used_in_the_order_they_were_supplied() {
    // A template with two slots asks twice. Answers arriving out of order would
    // fill the wrong slot, which is a wrong project rather than an error.
    let (mut state, _) = with_clip();
    state.supply_pick(Answer::file(scratch("first")));
    state.supply_pick(Answer::file(scratch("second")));
    let mut seen = Vec::new();
    for _ in 0..2 {
        state.request_pick(Ask::OpenFile(FileSpec::new("Media")), report);
        seen.push(state.status_message.take().expect("an answer per question"));
    }
    assert_eq!(seen, vec!["first".to_string(), "second".to_string()]);
}

#[test]
fn cancelling_is_an_answer_and_not_a_special_path() {
    let (mut state, _) = with_clip();
    state.supply_pick(Answer::cancelled());
    state.request_pick(
        Ask::OpenFiles(FileSpec::new("Import media")),
        |state, answer| {
            state.status_message = Some(format!("{} chosen", answer.all().len()));
        },
    );
    assert_eq!(state.status_message.as_deref(), Some("0 chosen"));
}

#[test]
fn a_question_answered_no_is_not_permission() {
    // The one confirm in the product deletes a file off disk. "No" and "the
    // dialog was dismissed" are both false, and that is the safety property.
    assert!(!Answer::Confirmed(false).is_yes());
    assert!(Answer::Confirmed(true).is_yes());
    assert!(
        Answer::Confirmed(true).one().is_none(),
        "a yes must not read as a path"
    );
    assert!(Answer::cancelled().one().is_none());
    assert!(Answer::cancelled().all().is_empty());
    assert!(!Answer::file(scratch("x")).is_yes());
}

#[test]
fn a_lut_pick_asks_nothing_when_no_clip_is_selected() {
    // The guard belongs before the pick: asking for a file and then dropping the
    // answer is a dialog the operator filled in for nothing.
    let (mut state, _) = with_clip();
    state.clear_selection();
    state.pick_and_load_lut();
    assert_eq!(state.picks_asked(), 0);
    assert_eq!(state.status_message.as_deref(), Some("Select a clip first"));
}

#[test]
fn a_lut_pick_loads_the_file_the_operator_chose() {
    let (mut state, _) = with_clip();
    let lut = scratch("warm.cube");
    state.supply_pick(Answer::file(lut.clone()));
    state.pick_and_load_lut();
    assert_eq!(state.picks_asked(), 1);
    assert_eq!(
        state.selected_lut().map(|applied| applied.path),
        Some(lut.display().to_string().into()),
        "the chosen LUT is not on the clip"
    );
}

/// A bundle with `slots` replaceable clips, saved to disk.
fn bundle_with_slots(slots: usize, name: &str) -> PathBuf {
    let (mut state, _) = with_clip();
    if slots > 1 {
        let ui = state.selected_ui_clip().expect("the fixture clip");
        let mid = ui.timeline_start_ticks + (ui.timeline_end_ticks - ui.timeline_start_ticks) / 2;
        state.set_playhead(mid);
        state.split_at_playhead().expect("split the fixture clip");
    }
    let ids: Vec<ClipId> = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .map(|clip| ClipId::from_raw(clip.id))
        .collect();
    assert_eq!(ids.len(), slots, "the fixture does not hold {slots} clips");
    for id in ids {
        state.select_clip(id);
        state.toggle_selected_replaceable().expect("mark the slot");
    }
    assert_eq!(state.template_slot_count(), slots);
    let path = scratch(name);
    state
        .save_template(path.clone(), "Fixture".into())
        .expect("save the bundle");
    path
}

#[test]
fn a_two_slot_template_asks_once_per_slot_and_fills_both() {
    // The sequential chain: each slot is asked for only once the last has been
    // answered, so this walks the whole continuation chain rather than one link.
    let bundle = bundle_with_slots(2, "two-slots.ariat");

    let (mut state, _) = with_clip();
    state.supply_pick(Answer::file(fixture()));
    state.supply_pick(Answer::file(fixture()));
    state.fill_and_apply_template(bundle);

    assert_eq!(state.picks_asked(), 2, "one question per slot");
    assert_eq!(state.picks_unused(), 0, "an answer went unused");
    assert_eq!(state.route, crate::types::UiRoute::Editor);
    assert!(
        state.project_path.is_none(),
        "an applied template must not inherit a project path"
    );
    assert_eq!(clip_count(&state), 2, "the filled template did not land");
}

#[test]
fn cancelling_a_slot_applies_the_template_unfilled() {
    let bundle = bundle_with_slots(1, "one-slot.ariat");

    let (mut state, _) = with_clip();
    state.supply_pick(Answer::cancelled());
    state.fill_and_apply_template(bundle);

    assert_eq!(state.picks_asked(), 1);
    assert_eq!(
        state.route,
        crate::types::UiRoute::Editor,
        "a cancelled slot must still open the template"
    );
    assert_eq!(
        clip_count(&state),
        1,
        "the template's own media did not land"
    );
}
