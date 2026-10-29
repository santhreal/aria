//! Contracts for the `.ariat` bundle round trip.
//!
//! The class closed here is "a template is written but nothing proves it can be
//! read back and applied", which is how a bundle format rots: the writer and
//! the reader drift and nobody notices until a saved template will not open.
//! Every assertion goes through the shipped `AppState` path, not through
//! `aria_models::Template` directly, because the shipped writer is the command,
//! not the model call.
//!
//! Not covered: the file dialogs. `pick_and_save_template` and
//! `pick_and_apply_template` are the dialog wrappers around these functions.

use super::premiere_tests::with_clip;

fn bundle_path() -> std::path::PathBuf {
    crate::scratch_pool::scratch("template").join("bundle.ariat")
}

#[test]
fn a_saved_template_is_readable_and_reports_its_slots() {
    let (mut state, _) = with_clip();
    assert_eq!(
        state.template_slot_count(),
        0,
        "nothing is marked replaceable yet"
    );

    // Mark the fixture clip as a slot, then save.
    state.toggle_selected_replaceable().unwrap();
    assert_eq!(state.template_slot_count(), 1);

    let path = bundle_path();
    state
        .save_template(path.clone(), "Fixture".into())
        .expect("save template");
    assert!(path.exists(), "the bundle was not written");
    assert!(
        state
            .status_message
            .as_deref()
            .is_some_and(|msg| msg.contains("1 slot")),
        "the status line must report the slot count, got {:?}",
        state.status_message
    );

    // The writer's output is readable by the reader, and the slot survives.
    let slots = state.template_slots(&path).expect("read back the bundle");
    assert_eq!(slots.len(), 1, "the saved slot is missing on read-back");
}

#[test]
fn a_slot_hint_becomes_the_prompt_label() {
    let (mut state, _) = with_clip();
    state.toggle_selected_replaceable().unwrap();
    state
        .set_selected_slot_label("Opening shot")
        .expect("set the hint");

    let path = bundle_path();
    state.save_template(path.clone(), "Hinted".into()).unwrap();
    assert_eq!(
        state.template_slots(&path).unwrap(),
        vec!["Opening shot".to_string()],
        "the hint must be what the slot prompt shows"
    );
}

#[test]
fn an_unmarked_project_saves_as_a_template_with_no_slots() {
    // A template with no slot is a starting project, not an error.
    let (mut state, _) = with_clip();
    let path = bundle_path();
    state.save_template(path.clone(), "Plain".into()).unwrap();
    assert!(
        state
            .status_message
            .as_deref()
            .is_some_and(|msg| msg.contains("0 slots")),
        "got {:?}",
        state.status_message
    );
    assert!(state.template_slots(&path).unwrap().is_empty());
}

#[test]
fn saving_a_template_does_not_touch_the_project() {
    let (mut state, _) = with_clip();
    let name = state.store.project_name.clone();
    let dirty = state.store.is_dirty;
    state.project_path = None;

    state.save_template(bundle_path(), "Bundle".into()).unwrap();

    assert_eq!(state.store.project_name, name, "the project was renamed");
    assert_eq!(
        state.store.is_dirty, dirty,
        "saving a template changed the dirty flag"
    );
    assert!(
        state.project_path.is_none(),
        "saving a template became a Save As"
    );
}

#[test]
fn applying_a_template_replaces_the_session_and_opens_the_editor() {
    let (mut state, _) = with_clip();
    state.toggle_selected_replaceable().unwrap();
    let path = bundle_path();
    state
        .save_template(path.clone(), "Reusable".into())
        .unwrap();

    // Walk away from the project: a different name, a stale path, a moved
    // playhead, a live selection.
    state.new_project().unwrap();
    state.project_path = Some(path.with_extension("aria"));
    state.set_playhead(3);
    assert!(
        state
            .store
            .sequence
            .tracks
            .iter()
            .all(|t| t.clips.is_empty())
    );

    // Applying with no picks keeps the template's own sample media, which is how a
    // template previews before it is filled.
    state
        .apply_template(path, Vec::new())
        .expect("apply template");

    assert_eq!(
        state.store.sequence.clip_count(),
        1,
        "the template's clip did not land"
    );
    assert_eq!(state.route, crate::types::UiRoute::Editor);
    assert_eq!(
        state.playhead.value, 0,
        "the playhead must return to the head"
    );
    assert!(state.selection.is_empty(), "a stale selection survived");
    assert!(
        state.project_path.is_none(),
        "an applied template must not inherit the previous project's path"
    );
}

#[test]
fn a_missing_or_malformed_bundle_is_an_error_not_a_panic() {
    let (state, _) = with_clip();
    let missing = bundle_path();
    assert!(state.template_slots(&missing).is_err());

    let junk = bundle_path();
    std::fs::write(&junk, b"not a template").unwrap();
    assert!(state.template_slots(&junk).is_err());
}
