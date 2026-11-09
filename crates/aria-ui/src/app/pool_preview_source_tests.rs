//! Insert and Overwrite on the selected pool items, with no Source tab open.
//!
//! The class these close: a placement of several pool items that half-lands.
//! The items land end to end from the playhead as one undo step named for the
//! edit, and when the edit rejects one item, the items placed before it are
//! taken back, so the lanes and History read as they did before the press.
//! Both modes are asserted. Not covered: which lane each channel takes, which
//! `source_drop_tests` and `source_patch_tests` assert.

use std::path::PathBuf;

use aria_models::{TrackId, TrackKind};
use aria_plugin::PluginContext;

use crate::app::AppState;
use crate::scratch_pool::scratch;
use crate::types::EditMode;

const MODES: [EditMode; 2] = [EditMode::Ripple, EditMode::Overwrite];

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name)
}

/// A new sequence with a picture lane and a sound lane, the playhead at 0,
/// and the silent 3 s shot then the 2 s shot with sound selected in the pool,
/// in that order.
fn state() -> (AppState, TrackId, TrackId) {
    let mut state = AppState::new(PluginContext::new(), scratch("poolplace")).expect("state");
    state.new_project().expect("project");
    let silent = state
        .import_media(fixture("fixture-640x360-24fps-3s.mp4"))
        .expect("import the silent shot");
    let sounding = state
        .import_media(fixture("fixture-640x360-24fps-2s-av.mp4"))
        .expect("import the shot with sound");
    let picture = TrackId::from_raw(state.store.sequence.tracks[0].id);
    let sound = state
        .ensure_kind_track(TrackKind::Audio)
        .expect("a sound lane");
    state.pool_selection.replace(silent.raw());
    state.pool_selection.toggle(sounding.raw());
    assert_eq!(state.pool_selection.ids(), [silent.raw(), sounding.raw()]);
    state.set_playhead(0);
    (state, picture, sound)
}

fn spans(state: &AppState, lane: TrackId) -> Vec<(i64, i64)> {
    let mut spans: Vec<(i64, i64)> = state
        .store
        .sequence
        .tracks
        .iter()
        .find(|track| track.id == lane.raw())
        .expect("the lane")
        .clips
        .iter()
        .map(|clip| (clip.timeline_start_ticks, clip.timeline_end_ticks))
        .collect();
    spans.sort_unstable();
    spans
}

#[test]
fn every_selected_item_lands_end_to_end_as_one_undo_step() {
    for mode in MODES {
        let (mut state, picture, sound) = state();
        let steps = state.history_entries().len();
        state.place_pool_selection(mode).expect("place");

        let shown = spans(&state, picture);
        assert_eq!(shown.len(), 2, "{mode:?}: both pictures land");
        assert_eq!(
            shown[0].0, 0,
            "{mode:?}: the first item lands at the playhead"
        );
        assert_eq!(
            shown[0].1, shown[1].0,
            "{mode:?}: the second item starts where the first ends"
        );
        assert_eq!(
            spans(&state, sound),
            [shown[1]],
            "{mode:?}: the sound lands under its picture"
        );
        let status = match mode {
            EditMode::Ripple => "Inserted 2 items",
            _ => "Overwrote with 2 items",
        };
        assert_eq!(state.status_message.as_deref(), Some(status));
        let history = state.history_entries();
        assert_eq!(history.len(), steps + 1, "{mode:?}: one undo step");
        assert_eq!(
            history.last().map(|step| step.label.as_str()),
            Some("Place media"),
            "{mode:?}"
        );

        assert!(state.undo(), "{mode:?}: Undo");
        assert_eq!(
            (spans(&state, picture), spans(&state, sound)),
            (vec![], vec![]),
            "{mode:?}: one Undo takes every item back"
        );
    }
}

/// The sound lane is locked, so the edit rejects the shot with sound after
/// the silent shot has landed.
#[test]
fn an_item_the_edit_rejects_takes_the_items_placed_before_it_back() {
    for mode in MODES {
        let (mut state, picture, sound) = state();
        state
            .set_track_locked(sound, true)
            .expect("lock the sound lane");
        let steps = state.history_entries().len();

        assert!(
            state.place_pool_selection(mode).is_err(),
            "{mode:?}: the locked sound lane rejects the shot with sound"
        );
        assert_eq!(
            spans(&state, picture),
            Vec::<(i64, i64)>::new(),
            "{mode:?}: the silent shot is taken back"
        );
        assert_eq!(
            state.history_entries().len(),
            steps,
            "{mode:?}: History reads as before the press"
        );

        // The silent shot alone lands: the rejection is the second item's.
        let first = state.pool_selection.ids()[0];
        state.pool_selection.replace(first);
        state
            .place_pool_selection(mode)
            .expect("place the silent shot");
        assert_eq!(spans(&state, picture).len(), 1, "{mode:?}");
    }
}
