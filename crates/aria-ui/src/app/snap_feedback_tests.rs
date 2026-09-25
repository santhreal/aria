//! Contracts for what the magnet reports and when it stands down.
//!
//! The class closed here is "a snap is invisible and unavoidable": the guide had
//! no identity, so a lock onto the wrong edge looked like a drag that missed,
//! and the only way to place a clip one frame off a cut was to turn the magnet
//! off in the toolbar and back on again.
//!
//! Not covered: the pointer move that paints the guide, which needs a render
//! surface. `panels::timeline::clip` calls `snap_at` with the Alt modifier and
//! writes the returned candidate to `snap_guide`; everything it decides is here.

use crate::snap::SnapKind;

use super::premiere_tests::with_clip;

/// The fixture clip spans ticks 0..120 on one lane.
const CLIP_END: i64 = 120;

#[test]
fn the_guide_names_the_magnet_it_locked_onto() {
    // The fixture clip's own edges sit on the sequence start and end, so a
    // marker is the one magnet at a tick nothing else claims.
    let (mut state, _) = with_clip();
    state.set_playhead(50);
    state.add_marker_at_playhead().expect("marker");
    state.set_playhead(0);

    let (tick, guide) = state.snap_at(52, None, 4, false);
    assert_eq!(tick, 50);
    assert_eq!(guide.expect("fired").kind, SnapKind::Marker);
}

#[test]
fn the_playhead_outranks_a_clip_edge_at_the_same_distance() {
    let (mut state, _) = with_clip();
    // Two magnets, both two ticks away from the drag: 118 (playhead) and 122.
    state.set_playhead(CLIP_END - 2);
    let (tick, guide) = state.snap_at(CLIP_END - 4, None, 2, false);
    assert_eq!(tick, CLIP_END - 2);
    assert_eq!(guide.expect("fired").kind, SnapKind::Playhead);
}

#[test]
fn the_modifier_suspends_the_magnet_for_one_gesture() {
    let (mut state, clip) = with_clip();
    state.set_playhead(0);

    let (tick, guide) = state.snap_at(CLIP_END - 2, Some(clip.raw() + 1), 4, true);
    assert_eq!(
        tick,
        CLIP_END - 2,
        "a suspended magnet must not move the drag"
    );
    assert!(guide.is_none(), "a suspended magnet must draw no guide");
    assert!(
        state.snap_enabled,
        "suspending a gesture must not change the toolbar toggle"
    );
}

#[test]
fn the_toolbar_toggle_still_switches_the_magnet_off() {
    let (mut state, clip) = with_clip();
    state.set_playhead(0);
    state.snap_enabled = false;

    let (tick, guide) = state.snap_at(CLIP_END - 2, Some(clip.raw() + 1), 4, false);
    assert_eq!(tick, CLIP_END - 2);
    assert!(guide.is_none());
}

#[test]
fn a_drag_outside_the_threshold_reports_nothing() {
    let (mut state, clip) = with_clip();
    state.set_playhead(0);

    let (tick, guide) = state.snap_at(CLIP_END - 40, Some(clip.raw() + 1), 4, false);
    assert_eq!(tick, CLIP_END - 40);
    assert!(guide.is_none());
}

#[test]
fn a_clip_is_not_its_own_magnet() {
    let (mut state, clip) = with_clip();
    state.set_playhead(0);
    state.clear_selection();

    // The sequence end sits on this clip's end, so the tick is still reachable;
    // what must not happen is the clip magneting to the edge it is dragging.
    let all = state.snap_candidates();
    assert!(
        all.iter()
            .any(|c| c.tick == CLIP_END && c.kind == SnapKind::ClipEnd)
    );
    assert!(
        all.iter().any(
            |c| matches!(c.kind, SnapKind::ClipStart | SnapKind::ClipEnd)
                && c.clip == Some(clip.raw())
        ),
        "a clip's edges must say which clip they belong to: {all:?}"
    );
    let (_, guide) = state.snap_at(CLIP_END - 2, Some(clip.raw()), 4, false);
    assert_ne!(guide.map(|g| g.kind), Some(SnapKind::ClipEnd));
}

#[test]
fn a_committed_edit_leaves_no_guide_behind() {
    let (mut state, clip) = with_clip();
    state.snap_guide = Some(crate::snap::SnapCandidate::new(24, SnapKind::Marker));
    state
        .trim_clip(clip, 0, CLIP_END - 10)
        .expect("trim the fixture clip");
    assert!(
        state.snap_guide.is_none(),
        "the guide must not outlive the gesture"
    );
}

#[test]
fn every_magnet_kind_the_state_offers_names_itself() {
    let (mut state, _) = with_clip();
    state.set_playhead(12);
    state.add_marker_at_playhead().expect("marker");
    state.set_mark_in();
    state.set_playhead(48);
    state.set_mark_out();

    let candidates = state.snap_candidates();
    assert!(!candidates.is_empty());
    for candidate in &candidates {
        assert!(
            SnapKind::ALL.contains(&candidate.kind),
            "{candidate:?} is not an enumerated kind"
        );
        assert!(!candidate.kind.label().is_empty());
    }
    let kinds: Vec<SnapKind> = candidates.iter().map(|c| c.kind).collect();
    for expected in [
        SnapKind::Playhead,
        SnapKind::SequenceStart,
        SnapKind::SequenceEnd,
        SnapKind::MarkIn,
        SnapKind::MarkOut,
        SnapKind::Marker,
        SnapKind::ClipStart,
        SnapKind::ClipEnd,
    ] {
        assert!(kinds.contains(&expected), "{expected:?} is not offered");
    }
}
