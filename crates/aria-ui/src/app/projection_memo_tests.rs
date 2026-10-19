//! When a poll re-projects, and when it must not.
//!
//! The class these close: a memo that serves a store the engine has moved past.
//! `sync_from_engine` runs on every gesture frame, so it compares the inputs
//! before projecting; the risk that buys is a stale store nobody can see is
//! stale. One test proves the memo exists, and one test per key field proves
//! that field still invalidates it — including the three that move without the
//! project moving at all: a proxy substitution, a save, and a plugin changing a
//! catalog the project reads. The playhead is the
//! exception and has its own contract: it invalidates nothing, because the
//! projection carries it as a value.
//!
//! What they do not catch: a *new* projection input added without a matching
//! key field. Nothing mechanical can, which is why the key is built from the
//! same values in the same order as the projection, one call apart.

use std::sync::Arc;

use aria_commands::{Command, EditCommand, ProjectCommand};
use aria_models::{
    ClipTypeSpec, MarkerColor, MediaId, RationalTime, TrackKind, register_clip_type,
};

use gpui::SharedString;

use crate::app::AppState;
use crate::app::premiere_tests::with_clip;

#[test]
fn an_unchanged_poll_keeps_the_projection() {
    let (mut state, _) = with_clip();
    state.sync_from_engine();

    let drawn = Arc::clone(&state.store);
    state.sync_from_engine();
    assert!(
        Arc::ptr_eq(&drawn, &state.store),
        "a poll with nothing to report rebuilt the projection"
    );

    // A gesture polls per frame; the memo has to survive the tenth poll, not
    // just the second.
    for _ in 0..10 {
        state.sync_from_engine();
    }
    assert!(
        Arc::ptr_eq(&drawn, &state.store),
        "repeated idle polls rebuilt the projection"
    );
}

#[test]
fn an_edit_reprojects() {
    let (mut state, _) = with_clip();
    state.sync_from_engine();
    let drawn = Arc::clone(&state.store);
    let tracks = state.store.sequence.tracks.len();

    // Applied on the engine directly, so the only thing that can carry the
    // change into the store is the poll under test.
    state
        .engine
        .lock()
        .apply(Command::Edit(EditCommand::AddTrack {
            kind: TrackKind::Video,
            name: "V2".into(),
            index: None,
            pinned: true,
        }))
        .expect("add a track");
    state.sync_from_engine();

    assert!(
        !Arc::ptr_eq(&drawn, &state.store),
        "an edit did not reach the projection"
    );
    assert_eq!(state.store.sequence.tracks.len(), tracks + 1);
}

#[test]
fn a_selection_change_reprojects() {
    let (mut state, clip) = with_clip();
    state.sync_from_engine();
    let drawn = Arc::clone(&state.store);

    state.selection.clear();
    state.sync_from_engine();

    assert!(
        !Arc::ptr_eq(&drawn, &state.store),
        "a cleared selection did not reach the projection"
    );
    assert!(
        state.store.selection.is_empty(),
        "the projection still marks {clip:?} selected"
    );
}

#[test]
fn selection_commands_update_the_store_without_reprojecting() {
    let (mut state, clip) = with_clip();
    state.sync_from_engine();
    let projections = state.projections;
    assert!(
        state
            .store
            .sequence
            .tracks
            .iter()
            .flat_map(|track| &track.clips)
            .any(|projected| projected.id == clip.raw() && projected.selected)
    );

    state.clear_selection();
    assert_eq!(
        state.projections, projections,
        "clearing selection reprojected unchanged model data"
    );
    assert!(state.store.selection.is_empty());
    assert!(state.selected.is_none());
    assert!(
        state
            .store
            .sequence
            .tracks
            .iter()
            .flat_map(|track| &track.clips)
            .all(|projected| !projected.selected)
    );

    state.select_clip(clip);
    assert_eq!(
        state.projections, projections,
        "selecting a clip reprojected unchanged model data"
    );
    assert_eq!(state.store.selection, [clip.raw()]);
    assert!(state.selected.is_some());
}

/// A playhead move reaches the store and stops there: the projection carries
/// the tick as a value, so re-walking the project would produce the store it
/// already has. What is sampled at the playhead is re-taken instead, which
/// `app::scrub_stale_tests` holds to a full re-projection.
#[test]
fn a_playhead_move_reaches_the_store_without_a_reprojection() {
    let (mut state, _) = with_clip();
    state.sync_from_engine();
    let projections = state.projections;
    let rate = state.store.sequence.frame_rate;

    state.playhead = RationalTime::new(state.playhead.value + 5, rate);
    state.sync_from_engine();

    assert_eq!(state.store.playhead_ticks, state.playhead.value);
    assert_eq!(
        state.projections, projections,
        "a moved playhead rebuilt a store that was already right"
    );
}

#[test]
fn a_proxy_substitution_reprojects() {
    let (mut state, _) = with_clip();
    state.sync_from_engine();
    let drawn = Arc::clone(&state.store);
    let media = MediaId::from_raw(state.store.media.first().expect("a pool item").id);
    assert!(!state.store.media[0].proxied);

    // Which media decode through a proxy is renderer state: it never touches
    // the project, so it cannot move the engine revision. The pool row states
    // the substitution, so the memo has to watch it separately.
    state
        .engine
        .lock()
        .set_media_proxy(media, state.data_dir.join("stand-in.mp4"));
    state.sync_from_engine();

    assert!(
        !Arc::ptr_eq(&drawn, &state.store),
        "a proxy substitution did not reach the projection"
    );
    assert!(
        state.store.media[0].proxied,
        "the pool row does not state the substitution"
    );
}

#[test]
fn saving_reprojects_although_the_project_did_not_change() {
    let (mut state, _) = with_clip();
    state.sync_from_engine();
    let drawn = Arc::clone(&state.store);
    let revision = state.engine.lock().revision();
    assert!(state.store.is_dirty, "an imported clip is unsaved work");

    let path = state.data_dir.join("memo.aria");
    state
        .engine
        .lock()
        .apply(Command::Project(ProjectCommand::Save { path }))
        .expect("save the project");
    assert_eq!(
        state.engine.lock().revision(),
        revision,
        "a save is not an edit; it must not move the revision"
    );
    state.sync_from_engine();

    assert!(
        !Arc::ptr_eq(&drawn, &state.store),
        "a save did not reach the projection"
    );
    assert!(
        !state.store.is_dirty,
        "the title bar still reads the session as unsaved"
    );
}

#[test]
fn the_first_poll_projects() {
    let (mut state, _) = with_clip();
    // The fixture already drew a store through its edits; drop the memo to the
    // state a freshly built `AppState` is in and prove a cold poll projects
    // rather than serving whatever was there.
    state.projected = None;
    let drawn = Arc::clone(&state.store);
    state.sync_from_engine();

    assert!(
        !Arc::ptr_eq(&drawn, &state.store),
        "a poll with no memo served the old projection"
    );
}

/// The clip type name the store shows on `clip`.
fn type_label(state: &AppState, clip: aria_models::ClipId) -> Option<SharedString> {
    state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| &track.clips)
        .find(|ui| ui.id == clip.raw())
        .and_then(|ui| ui.clip_type.as_ref())
        .map(|clip_type| clip_type.label.clone())
}

#[test]
fn a_plugin_changing_a_catalog_the_project_uses_reprojects() {
    // Registers and drops a clip type every other library test can list.
    crate::test_support::catalogs::own_process(|| {
        let (mut state, clip) = with_clip();
        let id = "test.memo.used";
        state
            .engine
            .lock()
            .apply(Command::Edit(EditCommand::SetClipType {
                clip,
                clip_type: Some(id.into()),
            }))
            .expect("a clip type");
        state.sync_from_engine();
        let drawn = Arc::clone(&state.store);
        assert_eq!(
            type_label(&state, clip).as_deref(),
            Some(id),
            "a type no plugin lists is shown by its id"
        );

        // Listing the type moves no project revision.
        let held = register_clip_type(ClipTypeSpec::new(
            id,
            "Memo type",
            "color_overlay",
            MarkerColor::Blue,
        ))
        .expect("the type registers")
        .0;
        state.sync_from_engine();
        assert!(
            !Arc::ptr_eq(&drawn, &state.store),
            "a registration the clip reads did not reach the projection"
        );
        assert_eq!(type_label(&state, clip).as_deref(), Some("Memo type"));

        drop(held);
        state.sync_from_engine();
        assert_eq!(
            type_label(&state, clip).as_deref(),
            Some(id),
            "a dropped type is still shown by its name"
        );
    });
}

#[test]
fn a_catalog_the_project_does_not_use_keeps_the_projection() {
    // Registers and drops a clip type every other library test can list.
    crate::test_support::catalogs::own_process(|| {
        let (mut state, _) = with_clip();
        state.sync_from_engine();
        let drawn = Arc::clone(&state.store);
        let _held = register_clip_type(ClipTypeSpec::new(
            "test.memo.unused",
            "Unused memo type",
            "color_overlay",
            MarkerColor::Green,
        ))
        .expect("the type registers");
        state.sync_from_engine();
        assert!(
            Arc::ptr_eq(&drawn, &state.store),
            "a registration the project reads nothing of rebuilt the projection"
        );
    });
}

#[test]
fn report_error_formats_various_error_types() {
    let (mut state, _) = with_clip();
    assert_eq!(state.last_error, None);

    state.report_error("A string slice error");
    assert_eq!(state.last_error.as_deref(), Some("A string slice error"));

    state.report_error(String::from("An owned string error"));
    assert_eq!(state.last_error.as_deref(), Some("An owned string error"));

    let engine_err = aria_engine::EngineError::MissingMedia("clip-123".into());
    state.report_error(engine_err);
    assert!(state.last_error.is_some());
    assert!(state.last_error.as_ref().unwrap().contains("clip-123"));
}
