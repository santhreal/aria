//! Regression tests for sequence and clip markers, the Marker Edit dialog,
//! range marker ruler geometry, and the Markers panel colour filter.

use std::collections::BTreeSet;

use aria_engine::EngineError;
use aria_models::{ClipId, MarkerColor, MarkerKind, RationalTime};

use super::decline::Decline;
use super::premiere_tests::with_clip;
use super::{AppState, DialogKind, MarkerEditField};
use crate::chrome::menu_items::items_of;
use crate::surfaces::settings_pick::RATES;
use crate::types::AppMenu;

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
        state.press_backspace();
    }
    for ch in "Act I".chars() {
        state.type_key(ch);
    }
    state.focus_marker_edit_field(Some(MarkerEditField::Comment));
    for ch in "Opening beat".chars() {
        state.type_key(ch);
    }
    state.focus_marker_edit_field(Some(MarkerEditField::Duration));
    while !state.marker_edit.duration.is_empty() {
        state.press_backspace();
    }
    for ch in "00:00:00:10".chars() {
        state.type_key(ch);
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
    state.marker_edit.duration.set("-00:00:00:05");
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
    state.marker_edit.duration.set("00:01:00:00");
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

/// A marker row that acts on the marker under the playhead is offered exactly
/// when its command acts, and the window it reaches is one second of wall
/// clock at every rate the Sequence Settings dialog offers.
///
/// The class closed: Clear Marker, Edit Marker… and Move Nearest Marker to
/// Playhead were offered whenever the sequence held any marker while their
/// commands act only on one within a second of the playhead, so a live row did
/// nothing; and a second counted as the rate's numerator, which at 29.97 fps
/// reached a marker 1001 seconds away and held a frozen frame as long. The rows
/// are found by the reason they give with a marker out of reach, so a Markers
/// row that takes the playhead guard without a command here is red. Not
/// covered: the Markers panel's buttons, which act on a marker by id.
#[test]
fn a_playhead_marker_row_is_offered_exactly_when_its_command_acts() {
    type Run = fn(&mut AppState) -> Result<(), EngineError>;
    let runs: [(&str, Run); 3] = [
        ("seq-marker-remove", |state| {
            state.remove_marker_at_playhead()
        }),
        ("seq-marker-rename", |state| {
            state.edit_marker_at_playhead();
            Ok(())
        }),
        ("seq-marker-snap", |state| {
            crate::commands::reach::snap_marker(state);
            Ok(())
        }),
    ];
    let reason = Decline::NoMarkerAtPlayhead.reason();
    for rate in RATES {
        let (mut probe, _) = with_clip();
        probe
            .set_sequence_rate(rate)
            .expect("the dialog's rate applies");
        let second = probe.one_second();
        let wall = RationalTime::new(second, rate).seconds();
        let frame = RationalTime::new(1, rate).seconds();
        assert!(
            (1.0..1.0 + frame).contains(&wall),
            "one second at {rate:?} is {second} ticks, {wall} s"
        );
        // A marker two seconds in, on the five-second still, with the playhead
        // `offset` ticks from it.
        let dress = |offset: i64| {
            let (mut state, _) = with_clip();
            state.clear_selection();
            state
                .set_sequence_rate(rate)
                .expect("the dialog's rate applies");
            let mark = 2 * second;
            state.set_playhead(mark);
            state.add_marker_at_playhead().expect("a marker lands");
            state.set_playhead(mark + offset);
            assert_eq!(
                state.playhead.value,
                mark + offset,
                "the still is too short to hold the playhead {offset} ticks from the marker at {rate:?}"
            );
            state
        };
        let far = dress(second + 1);
        let guarded: BTreeSet<&str> = items_of(AppMenu::Markers, &far)
            .iter()
            .filter(|row| row.blocked_by(&far) == Some(reason))
            .map(|row| row.id)
            .collect();
        assert_eq!(
            guarded,
            runs.iter().map(|(id, _)| *id).collect::<BTreeSet<_>>(),
            "the Markers rows that wait for a marker at the playhead drifted from the commands here"
        );
        for (id, run) in runs {
            for offset in [-second - 1, -second, -1, 1, second, second + 1] {
                let mut state = dress(offset);
                let row = items_of(AppMenu::Markers, &state)
                    .into_iter()
                    .find(|row| row.id == id)
                    .unwrap_or_else(|| panic!("the Markers menu lost {id}"));
                let offered = row.blocked_by(&state).is_none();
                assert_eq!(
                    offered,
                    offset.abs() <= second,
                    "{id} at {rate:?}, {offset} ticks from the marker"
                );
                let before = (state.history_depth(), state.dialog);
                state.status_message = None;
                run(&mut state).unwrap_or_else(|err| panic!("{id} failed: {err}"));
                let acted = (state.history_depth(), state.dialog) != before;
                assert_eq!(
                    acted, offered,
                    "{id} at {rate:?}, {offset} ticks from the marker: offered {offered}, acted {acted}"
                );
                if !offered {
                    assert_eq!(
                        state.status_message.as_deref(),
                        Some(reason),
                        "{id} declined in other words than its row"
                    );
                }
            }
        }
    }
}

/// A Markers row withheld because the sequence holds no marker declines in
/// that row's words when it runs anyway, as from a key, and changes nothing.
///
/// The class closed: Clear All Markers said "No markers to clear" and Go to
/// Next Marker "No later marker" under a row whose tooltip said the sequence
/// has no markers. The rows are found by the reason they give on a sequence
/// with no marker, so a row that takes the guard without a command here is red.
/// Not covered: a row offered over markers that still finds none to act on,
/// such as Go to Next Marker past the last.
#[test]
fn a_markers_row_withheld_for_no_markers_declines_in_its_rows_words() {
    type Run = fn(&mut AppState) -> Result<(), EngineError>;
    let runs: [(&str, Run); 3] = [
        ("seq-marker-next", |state| {
            state.go_to_next_marker();
            Ok(())
        }),
        ("seq-marker-prev", |state| {
            state.go_to_prev_marker();
            Ok(())
        }),
        ("marker-clear-all", |state| state.clear_all_markers()),
    ];
    let (mut state, _) = with_clip();
    state.set_playhead(state.store.sequence.duration_ticks / 2);
    let reason = Decline::NoMarkers.reason();
    let withheld: BTreeSet<&str> = items_of(AppMenu::Markers, &state)
        .iter()
        .filter(|row| row.blocked_by(&state) == Some(reason))
        .map(|row| row.id)
        .collect();
    assert_eq!(
        withheld,
        runs.iter().map(|(id, _)| *id).collect::<BTreeSet<_>>(),
        "the Markers rows withheld for no markers drifted from the commands here"
    );
    for (id, run) in runs {
        let before = (state.history_depth(), state.dialog, state.playhead.value);
        state.status_message = None;
        run(&mut state).unwrap_or_else(|err| panic!("{id} failed instead of declining: {err}"));
        assert_eq!(
            state.status_message.as_deref(),
            Some(reason),
            "{id} declined in other words than its row"
        );
        assert_eq!(
            (state.history_depth(), state.dialog, state.playhead.value),
            before,
            "{id} changed the project, the playhead or the dialog"
        );
    }
}
