//! Regression tests for sequence and clip markers, the Marker Edit dialog,
//! range marker ruler geometry, and the Markers panel colour filter.

use aria_models::{ClipId, MarkerColor, MarkerKind};

use super::premiere_tests::with_clip;
use super::{DialogKind, MarkerEditField};

#[test]
fn pressing_m_twice_opens_marker_edit_dialog_and_commits_in_one_undo_step() {
    let (mut state, _clip) = with_clip();
    // Deselect clip so the marker lands on the sequence ruler at tick 12.
    state.clear_selection();
    state.set_playhead(12);

    state.add_or_edit_marker().expect("first M adds marker");
    assert_eq!(state.dialog, DialogKind::None);
    assert_eq!(state.store.sequence.markers.len(), 1);
    let initial = state.store.sequence.markers[0].clone();
    assert_eq!(initial.tick, 12);
    assert_eq!(initial.name.as_ref(), "Marker 1");
    assert_eq!(initial.duration_ticks, 0);
    assert_eq!(initial.kind, MarkerKind::Comment);

    // Pressing M again at the same playhead opens DialogKind::MarkerEdit
    // populated with the marker's current values.
    state
        .add_or_edit_marker()
        .expect("second M opens MarkerEdit");
    assert_eq!(state.dialog, DialogKind::MarkerEdit);
    assert_eq!(state.marker_edit.marker, initial.id);
    assert_eq!(state.marker_edit.name, "Marker 1");
    assert_eq!(state.marker_edit.duration, "00:00:00:00");
    assert_eq!(state.marker_edit.color, initial.color);
    assert_eq!(state.marker_edit.kind, MarkerKind::Comment);

    // Edit all five fields through the dialog form and commit with Enter
    // (`run_dialog_primary`).
    state.focus_marker_edit_field(Some(MarkerEditField::Name));
    while !state.marker_edit.name.is_empty() {
        state.marker_edit_pop();
    }
    for ch in "Act I".chars() {
        state.marker_edit_push(ch);
    }
    state.focus_marker_edit_field(Some(MarkerEditField::Comment));
    for ch in "Opening beat".chars() {
        state.marker_edit_push(ch);
    }
    state.focus_marker_edit_field(Some(MarkerEditField::Duration));
    while !state.marker_edit.duration.is_empty() {
        state.marker_edit_pop();
    }
    for ch in "00:00:00:10".chars() {
        state.marker_edit_push(ch);
    }
    state.set_marker_edit_color(MarkerColor::Purple);
    state.set_marker_edit_kind(MarkerKind::Chapter);

    assert!(state.run_dialog_primary(), "Enter commits MarkerEdit");
    assert_eq!(state.dialog, DialogKind::None);

    let updated = &state.store.sequence.markers[0];
    assert_eq!(updated.name.as_ref(), "Act I");
    assert_eq!(updated.comment.as_ref(), "Opening beat");
    assert_eq!(updated.duration_ticks, 10);
    assert_eq!(updated.color, MarkerColor::Purple);
    assert_eq!(updated.kind, MarkerKind::Chapter);

    // A single undo step restores all five fields to their pre-dialog values.
    assert!(state.undo(), "undo dialog commit");
    let restored = &state.store.sequence.markers[0];
    assert_eq!(restored.name.as_ref(), "Marker 1");
    assert_eq!(restored.comment.as_ref(), "");
    assert_eq!(restored.duration_ticks, 0);
    assert_eq!(restored.color, initial.color);
    assert_eq!(restored.kind, MarkerKind::Comment);
}

#[test]
fn marker_edit_dialog_refuses_negative_and_out_of_bounds_durations_with_reason() {
    let (mut state, _clip) = with_clip();
    state.clear_selection();
    state.set_playhead(12);
    state.add_marker_at_playhead().expect("add sequence marker");
    let marker_id = state.store.sequence.markers[0].id;

    // 1. Negative duration is refused with a clear reason and leaves the marker intact.
    assert!(state.open_marker_edit(marker_id));
    state.marker_edit.duration = "-00:00:00:05".to_string();
    state.apply_marker_edit();
    assert_eq!(
        state.dialog,
        DialogKind::MarkerEdit,
        "dialog stays open on refused duration"
    );
    let err = state
        .marker_edit
        .error
        .as_ref()
        .expect("refusal reason populated");
    assert!(
        err.contains("non-negative"),
        "expected non-negative refusal reason, got: {err}"
    );
    assert_eq!(state.store.sequence.markers[0].duration_ticks, 0);

    // 2. Duration running past sequence end is refused with a clear reason.
    state.marker_edit.duration = "00:01:00:00".to_string();
    state.apply_marker_edit();
    assert_eq!(state.dialog, DialogKind::MarkerEdit);
    let err = state
        .marker_edit
        .error
        .as_ref()
        .expect("refusal reason populated");
    assert!(
        err.contains("exceeds sequence end"),
        "expected sequence-end refusal reason, got: {err}"
    );
    assert_eq!(state.store.sequence.markers[0].duration_ticks, 0);
}

#[test]
fn clip_marker_attaches_to_selected_clip_and_moves_with_it() {
    let (mut state, clip_id) = with_clip();
    state.select_clip(clip_id);
    state.set_playhead(10);

    // M with a selected clip under the playhead attaches the marker to the clip.
    state.add_or_edit_marker().expect("add clip marker");
    assert_eq!(state.store.sequence.markers.len(), 1);
    let marker = &state.store.sequence.markers[0];
    assert_eq!(marker.clip_id, Some(clip_id.raw()));
    assert_eq!(marker.tick, 10);

    // Moving the clip to timeline tick 24 moves the projected clip marker to 34.
    let track_id = ClipId::from_raw(clip_id.raw());
    let track = state.store.sequence.tracks[0].id;
    state
        .move_clip(track_id, aria_models::TrackId::from_raw(track), 24)
        .expect("move clip");
    assert_eq!(state.store.sequence.markers.len(), 1);
    assert_eq!(state.store.sequence.markers[0].tick, 34);
    assert_eq!(state.store.sequence.markers[0].clip_id, Some(clip_id.raw()));
}

#[test]
fn markers_list_colour_filter_hides_other_colours() {
    let (mut state, _clip) = with_clip();
    state.clear_selection();
    state.set_playhead(4);
    state.add_marker_at_playhead().expect("marker 1 (Blue)");
    state.set_playhead(12);
    state.add_marker_at_playhead().expect("marker 2 (Cyan)");

    assert_eq!(state.visible_markers().len(), 2);

    let first_color = state.store.sequence.markers[0].color;
    let second_color = state.store.sequence.markers[1].color;
    assert_ne!(first_color, second_color);

    state.set_marker_color_filter(Some(first_color));
    let filtered = state.visible_markers();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].color, first_color);

    state.set_marker_color_filter(Some(MarkerColor::Red));
    assert!(state.visible_markers().is_empty());

    state.set_marker_color_filter(None);
    assert_eq!(state.visible_markers().len(), 2);
}
