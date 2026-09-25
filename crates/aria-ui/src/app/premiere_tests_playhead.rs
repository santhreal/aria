//! Playhead: 17 of the cases in [`super`].
//!
//! Split out of `premiere_tests.rs` to stay under the file ceiling. A pure
//! move: only `#[test]` fns live here. Every fixture and helper stays in the
//! parent, reached through `use super::*`, so no two children of this parent
//! reference each other.

use super::*;

#[test]
fn mark_in_and_out_record_the_playhead_and_clear_independently() {
    let (mut state, _) = with_clip();
    state.set_playhead(12);
    state.set_mark_in();
    state.set_playhead(40);
    state.set_mark_out();
    assert_eq!(state.mark_in, Some(12));
    assert_eq!(state.mark_out, Some(40));

    state.clear_mark_in();
    assert_eq!(state.mark_in, None, "clearing In must not touch Out");
    assert_eq!(state.mark_out, Some(40));
    state.clear_mark_out();
    assert_eq!(state.mark_out, None);
}
#[test]
fn marking_out_before_in_does_not_leave_an_inverted_range() {
    let (mut state, _) = with_clip();
    state.set_playhead(40);
    state.set_mark_in();
    state.set_playhead(12);
    // Marking Out behind In must not yield out < in.
    state.set_mark_out();
    if let (Some(inn), Some(out)) = (state.mark_in, state.mark_out) {
        assert!(inn <= out, "in {inn} must not exceed out {out}");
    }

    let (mut state, _) = with_clip();
    state.set_playhead(12);
    state.set_mark_out();
    state.set_playhead(40);
    state.set_mark_in();
    if let (Some(inn), Some(out)) = (state.mark_in, state.mark_out) {
        assert!(inn <= out, "in {inn} must not exceed out {out}");
    }
}
#[test]
fn go_to_in_and_out_move_the_playhead_and_are_inert_when_unset() {
    let (mut state, _) = with_clip();
    state.set_playhead(30);
    state.go_to_in();
    assert_eq!(
        state.playhead.value, 30,
        "no In set: playhead must not move"
    );
    state.go_to_out();
    assert_eq!(
        state.playhead.value, 30,
        "no Out set: playhead must not move"
    );

    state.set_mark_in();
    state.set_playhead(60);
    state.set_mark_out();
    state.set_playhead(0);
    state.go_to_in();
    assert_eq!(state.playhead.value, 30);
    state.go_to_out();
    assert_eq!(state.playhead.value, 60);
}
#[test]
fn trim_to_playhead_pulls_the_chosen_edge_only() {
    let (mut state, clip) = with_clip();
    let (start, end) = clip_bounds(&state, clip);
    let mid = start + (end - start) / 2;
    assert!(mid > start && mid < end, "fixture clip must span a range");

    state.set_playhead(mid);
    state.trim_selected_to_playhead(true).expect("trim head");
    let (new_start, new_end) = clip_bounds(&state, clip);
    assert_eq!(new_start, mid, "Q must move the head to the playhead");
    assert_eq!(new_end, end, "Q must leave the tail alone");

    let mid2 = new_start + (new_end - new_start) / 2;
    state.set_playhead(mid2);
    state.trim_selected_to_playhead(false).expect("trim tail");
    let (after_start, after_end) = clip_bounds(&state, clip);
    assert_eq!(after_start, new_start, "W must leave the head alone");
    assert_eq!(after_end, mid2, "W must move the tail to the playhead");
}
#[test]
fn trim_to_playhead_never_collapses_a_clip() {
    let (mut state, clip) = with_clip();
    let (start, end) = clip_bounds(&state, clip);

    // Playhead at or past the far edge would produce an empty or inverted clip.
    state.set_playhead(end);
    state.trim_selected_to_playhead(true).expect("trim head");
    let (a, b) = clip_bounds(&state, clip);
    assert!(b > a, "head trim must leave a non-empty clip");

    state.set_playhead(start);
    state.trim_selected_to_playhead(false).expect("trim tail");
    let (a, b) = clip_bounds(&state, clip);
    assert!(b > a, "tail trim must leave a non-empty clip");
}
#[test]
fn trim_and_delete_are_inert_with_no_selection() {
    let (mut state, _) = with_clip();
    state.clear_selection();
    let before = clip_count(&state);
    state.trim_selected_to_playhead(true).expect("inert trim");
    state.trim_selected_to_playhead(false).expect("inert trim");
    state.delete_selected().expect("inert delete");
    assert_eq!(clip_count(&state), before);
}
#[test]
fn delete_follows_the_edit_mode_for_every_mode() {
    // Every EditMode must have a defined delete, and both must remove the clip
    // and drop the selection. Adding a mode without a delete path fails here.
    for mode in [EditMode::Ripple, EditMode::Overwrite] {
        let (mut state, clip) = with_clip();
        state.edit_mode = mode;
        let before = clip_count(&state);
        assert!(before > 0);
        state.delete_selected().expect("delete in mode");
        assert_eq!(
            clip_count(&state),
            before - 1,
            "{mode:?} delete must remove the clip"
        );
        assert!(
            !state.selection.contains(&clip),
            "{mode:?} delete must drop the selection"
        );
    }
}
#[test]
fn ripple_delete_closes_the_gap_and_overwrite_leaves_it() {
    // Two clips: deleting the first must pull the second back under Ripple and
    // leave it in place under Overwrite. This is the observable difference.
    let mut starts = Vec::new();
    for mode in [EditMode::Ripple, EditMode::Overwrite] {
        let mut state = AppState::new(PluginContext::new(), scratch()).unwrap();
        state.new_project().unwrap();
        let media = state.import_media(fixture()).expect("import fixture");
        state
            .drop_media_on_timeline(media, None, 0)
            .expect("place first");
        let first_end = state
            .store
            .sequence
            .tracks
            .iter()
            .flat_map(|track| track.clips.iter())
            .map(|clip| clip.timeline_end_ticks)
            .max()
            .expect("first clip");
        state
            .drop_media_on_timeline(media, None, first_end)
            .expect("place second");
        assert_eq!(
            clip_count(&state),
            2,
            "the ripple/overwrite difference is only observable with two clips"
        );
        let first = state
            .store
            .sequence
            .tracks
            .iter()
            .flat_map(|track| track.clips.iter())
            .min_by_key(|clip| clip.timeline_start_ticks)
            .map(|clip| ClipId::from_raw(clip.id))
            .expect("first clip");
        state.edit_mode = mode;
        state.select_clip(first);
        state.delete_selected().expect("delete first");
        let remaining = state
            .store
            .sequence
            .tracks
            .iter()
            .flat_map(|track| track.clips.iter())
            .map(|clip| clip.timeline_start_ticks)
            .min()
            .expect("second clip survives");
        starts.push(remaining);
    }
    assert!(
        starts[0] < starts[1],
        "ripple must close the gap ({}) that overwrite preserves ({})",
        starts[0],
        starts[1]
    );
}
#[test]
fn linked_select_bypass_selects_only_the_clicked_clip() {
    let (mut state, clip) = with_clip();
    // with_link = false is the Alt-held path: never widen the selection.
    state.select_clip_linked(clip, false);
    assert_eq!(state.selection.as_slice(), &[clip]);
    // An unlinked clip selects alone even when link-following is requested.
    state.select_clip_linked(clip, true);
    assert_eq!(
        state.selection.as_slice(),
        &[clip],
        "an unlinked clip must not gather siblings"
    );
}
#[test]
fn added_markers_reach_the_projected_store() {
    let (mut state, _) = with_clip();
    assert!(state.store.sequence.markers.is_empty());
    state.set_playhead(24);
    state.add_marker_at_playhead().expect("add marker");
    let ticks: Vec<i64> = state
        .store
        .sequence
        .markers
        .iter()
        .map(|marker| marker.tick)
        .collect();
    assert_eq!(
        ticks,
        vec![24],
        "the marker must be projected at the playhead, not only stored in the timeline"
    );

    state.set_playhead(48);
    state.add_marker_at_playhead().expect("add second marker");
    assert_eq!(state.store.sequence.markers.len(), 2);
    let ids: Vec<u64> = state
        .store
        .sequence
        .markers
        .iter()
        .map(|marker| marker.id)
        .collect();
    assert_ne!(ids[0], ids[1], "projected markers must keep distinct ids");
}
#[test]
fn markers_and_marks_are_snap_candidates() {
    let (mut state, _) = with_clip();
    state.set_playhead(24);
    state.add_marker_at_playhead().expect("add marker");
    state.set_playhead(60);
    state.set_mark_in();

    let candidates = state.snap_candidates();
    let kinds = |tick: i64| -> Vec<crate::snap::SnapKind> {
        candidates
            .iter()
            .filter(|c| c.tick == tick)
            .map(|c| c.kind)
            .collect()
    };
    assert!(
        kinds(24).contains(&crate::snap::SnapKind::Marker),
        "a marker must be a magnet, and say so: {candidates:?}"
    );
    assert!(
        kinds(60).contains(&crate::snap::SnapKind::MarkIn),
        "an In point must be a magnet, and say so: {candidates:?}"
    );
    let (snapped, guide) = state.snap_at(22, None, 3, false);
    assert_eq!(snapped, 24);
    assert_eq!(guide.expect("fired").kind, crate::snap::SnapKind::Marker);
}
#[test]
fn new_markers_carry_a_name_and_a_colour() {
    let (mut state, _) = with_clip();
    state.set_playhead(10);
    state.add_marker_at_playhead().expect("add");
    state.set_playhead(20);
    state.add_marker_at_playhead().expect("add");
    let names: Vec<&str> = state
        .store
        .sequence
        .markers
        .iter()
        .map(|marker| marker.name.as_str())
        .collect();
    assert_eq!(
        names,
        vec!["Marker 1", "Marker 2"],
        "AddMarker must never write an empty name"
    );
    let colors: Vec<_> = state
        .store
        .sequence
        .markers
        .iter()
        .map(|marker| marker.color)
        .collect();
    assert_ne!(
        colors[0], colors[1],
        "consecutive markers must be distinguishable"
    );
}
#[test]
fn marker_names_do_not_collide_after_a_delete() {
    let (mut state, _) = with_clip();
    for tick in [10, 20, 30] {
        state.set_playhead(tick);
        state.add_marker_at_playhead().expect("add");
    }
    let second = state.store.sequence.markers[1].id;
    state.remove_marker(second).expect("remove");
    assert_eq!(state.store.sequence.markers.len(), 2);
    state.set_playhead(40);
    state.add_marker_at_playhead().expect("add");
    let names: Vec<&str> = state
        .store
        .sequence
        .markers
        .iter()
        .map(|marker| marker.name.as_str())
        .collect();
    assert_eq!(
        names.iter().collect::<std::collections::HashSet<_>>().len(),
        names.len(),
        "a name must not be reused after a delete: {names:?}"
    );
}
#[test]
fn remove_marker_at_playhead_is_inert_when_none_is_near() {
    let (mut state, _) = with_clip();
    state.set_playhead(10);
    state.add_marker_at_playhead().expect("add");
    state.set_playhead(500);
    state.remove_marker_at_playhead().expect("inert");
    assert_eq!(state.store.sequence.markers.len(), 1);
    state.set_playhead(10);
    state.remove_marker_at_playhead().expect("remove");
    assert!(state.store.sequence.markers.is_empty());
}
#[test]
fn rename_and_recolour_keep_the_marker_in_place() {
    let (mut state, _) = with_clip();
    state.set_playhead(18);
    state.add_marker_at_playhead().expect("add");
    let id = state.store.sequence.markers[0].id;
    let color = state.store.sequence.markers[0].color;

    state.rename_marker(id, "Chorus".into()).expect("rename");
    let marker = &state.store.sequence.markers[0];
    assert_eq!(marker.name, "Chorus");
    assert_eq!(marker.tick, 18, "rename must not move the marker");
    assert_eq!(marker.color, color, "rename must not recolour");

    state.cycle_marker_color(id).expect("cycle");
    let marker = &state.store.sequence.markers[0];
    assert_ne!(marker.color, color);
    assert_eq!(marker.name, "Chorus", "recolour must not rename");
    assert_eq!(marker.tick, 18, "recolour must not move");

    state.move_marker(id, 30).expect("move");
    let marker = &state.store.sequence.markers[0];
    assert_eq!(marker.tick, 30);
    assert_eq!(marker.name, "Chorus", "move must not rename");
}
#[test]
fn marker_navigation_is_directional_and_stops_at_the_ends() {
    let (mut state, _) = with_clip();
    for tick in [10, 30, 50] {
        state.set_playhead(tick);
        state.add_marker_at_playhead().expect("add");
    }
    state.set_playhead(0);
    state.go_to_next_marker();
    assert_eq!(state.playhead.value, 10);
    state.go_to_next_marker();
    assert_eq!(state.playhead.value, 30);
    state.go_to_prev_marker();
    assert_eq!(state.playhead.value, 10);
    state.go_to_prev_marker();
    assert_eq!(state.playhead.value, 10, "must not wrap past the first");
    state.set_playhead(50);
    state.go_to_next_marker();
    assert_eq!(state.playhead.value, 50, "must not wrap past the last");
}
#[test]
fn link_needs_two_clips_and_unlink_reverses_it() {
    let (mut state, clip) = with_clip();
    // One clip is not a link group.
    state.select_clip(clip);
    state.link_selected().expect("inert link");
    assert!(
        !state.selection_is_linked(),
        "a single clip must not become a link group"
    );

    // A second clip, placed rather than extracted: the still fixture has no
    // audio to detach, which is itself asserted below.
    let media = state.store.media[0].id;
    let end = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .map(|c| c.timeline_end_ticks)
        .max()
        .expect("first clip");
    state
        .drop_media_on_timeline(MediaId::from_raw(media), None, end)
        .expect("place second");
    let ids: Vec<ClipId> = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .map(|c| ClipId::from_raw(c.id))
        .collect();
    assert_eq!(ids.len(), 2, "two clips are needed to form a link group");

    state.selection = ids.clone();
    state.link_selected().expect("link");
    assert!(state.selection_is_linked(), "the pair must be linked");

    // Selecting one now gathers the other.
    state.select_clip_linked(ids[0], true);
    assert_eq!(
        state.selection.len(),
        2,
        "linked select must gather the pair"
    );
    state.select_clip_linked(ids[0], false);
    assert_eq!(state.selection.len(), 1, "Alt must still bypass the link");

    state.selection = ids;
    state.unlink_selected().expect("unlink");
    assert!(
        !state.selection_is_linked(),
        "unlink must dissolve the group"
    );
}
