//! The magnet index built from the standard workload's projection.
//!
//! What these defend: the index holds every magnet the projection carries and
//! no others, and every [`SnapKind`] has something that produces it. What it
//! costs to build and to query is measured in `tests/frame_budget.rs`, a test
//! binary of its own.

use aria_models::{Rational, RationalTime};

use crate::snap::{SnapKind, index_for};
use crate::{project_to_ui, workload};

fn playhead() -> RationalTime {
    RationalTime::new(1_000, Rational::FPS_24)
}

#[test]
fn the_magnet_index_holds_every_magnet_the_projection_carries() {
    let store = project_to_ui(&workload::standard(), playhead(), &[]);
    let clips: usize = store
        .sequence
        .tracks
        .iter()
        .map(|track| track.clips.len())
        .sum();
    let beats: usize = store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .map(|clip| clip.beat_ticks.len())
        .sum();
    let expected = 2 + store.sequence.markers.len() + clips * 2 + beats;
    assert_eq!(
        index_for(&store).rows().len(),
        expected,
        "the index is the sequence bounds, the markers, both edges of every clip, and every beat"
    );
}

/// Fail-by-default: every [`SnapKind`] is produced either by the index or as a
/// loose magnet passed to `nearest`. A tenth kind that nothing builds turns
/// this red instead of shipping a magnet that can never fire.
#[test]
fn every_snap_kind_has_a_producer() {
    let store = project_to_ui(&workload::standard(), playhead(), &[]);
    let index = index_for(&store);
    // The three the editor supplies per query, because they move without a
    // projection (`AppState::loose_magnets`).
    let loose = [SnapKind::Playhead, SnapKind::MarkIn, SnapKind::MarkOut];
    // The one a keyframe drag in the property rows builds from the dragged
    // clip's own keys (`AppState::update_timeline_keyframe_drag`): those are
    // not edges of the sequence, so no clip drag snaps to them.
    let keyframe_drag = [SnapKind::Keyframe];
    for kind in SnapKind::ALL {
        let produced = loose.contains(&kind)
            || keyframe_drag.contains(&kind)
            || index.rows().iter().any(|row| row.kind == kind);
        assert!(
            produced,
            "{kind:?} is a magnet nothing builds: add it to the index or to the loose set"
        );
    }
}
