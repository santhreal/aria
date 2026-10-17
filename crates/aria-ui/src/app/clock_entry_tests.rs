//! Typing a timecode without clicking the clock.
//!
//! The class these close: numeric entry is one field with three meanings. A
//! bare timecode seeks; a leading keypad `+` or `-` moves the selected clips,
//! trims the selected edge, or moves the playhead when nothing is selected.
//! Each meaning is asserted here, with the boundaries a plausible bug crosses:
//! a companion moved twice, adjacent clips moved in the order that collides, a
//! move pushed past the sequence start, several clips taking several undo
//! steps, and a digit opening the field over a panel that does not own the
//! playhead.
//!
//! Not covered: the shell's key-down hook itself, which needs a window. It
//! calls `type_into_clock` with the key name and stops the event when that
//! returns true; the field route then delivers the rest of the typing.

use aria_models::{ClipId, MediaId, TrackId, TrackKind};
use aria_plugin::PluginContext;

use crate::app::AppState;
use crate::app::clock::ClockSite;
use crate::scratch_pool::scratch;
use crate::types::{ActivePanel, TrimSide};

/// One picture lane and one audio lane, with the 2 s, 24 fps shot with sound
/// placed as two linked pairs: (24, 72) and, abutting it, (72, 120).
fn two_pairs() -> (AppState, TrackId, TrackId, [ClipId; 2]) {
    let mut state = AppState::new(PluginContext::new(), scratch("clockentry")).expect("state");
    state.new_project().expect("project");
    let shot = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/fixture-640x360-24fps-3s-av.mp4");
    let media: MediaId = state.import_media(shot).expect("import the shot");
    let picture = TrackId::from_raw(state.store.sequence.tracks[0].id);
    let sound = state
        .add_lane_of_kind(TrackKind::Audio, None)
        .expect("an audio lane");
    for at in [24, 72] {
        state
            .drop_media_on_timeline(media, Some(picture), at)
            .expect("drop");
    }
    let mut placed = picture_clips(&state, picture);
    placed.sort_by_key(|&(_, start, _)| start);
    let ids = [placed[0].0, placed[1].0];
    state.active_panel = ActivePanel::Timeline;
    state.clear_selection();
    (state, picture, sound, ids)
}

fn picture_clips(state: &AppState, lane: TrackId) -> Vec<(ClipId, i64, i64)> {
    state
        .store
        .sequence
        .tracks
        .iter()
        .find(|track| track.id == lane.raw())
        .expect("the lane")
        .clips
        .iter()
        .map(|clip| {
            (
                ClipId::from_raw(clip.id),
                clip.timeline_start_ticks,
                clip.timeline_end_ticks,
            )
        })
        .collect()
}

fn spans(state: &AppState, lane: TrackId) -> Vec<(i64, i64)> {
    let mut spans: Vec<(i64, i64)> = picture_clips(state, lane)
        .into_iter()
        .map(|(_, start, end)| (start, end))
        .collect();
    spans.sort_unstable();
    spans
}

/// Press `key` over the focused panel, type `rest` into the field it opens,
/// and press Enter.
fn type_entry(state: &mut AppState, key: &str, rest: &str) {
    assert!(
        state.type_into_clock(key, false),
        "{key} over the {:?} panel did not open the clock",
        state.active_panel
    );
    for ch in rest.chars() {
        state.clock_push(ch);
    }
    state.commit_clock();
    assert!(!state.clock_focused(), "Enter releases the field");
}

#[test]
fn a_digit_over_the_timeline_seeks_to_the_typed_timecode() {
    let (mut state, ..) = two_pairs();
    type_entry(&mut state, "1", "2");
    assert_eq!(state.store.playhead_ticks, 12);

    type_entry(&mut state, "1", ".00");
    assert_eq!(state.store.playhead_ticks, 24, "`.` separates fields");
    assert_eq!(
        state.status_message.as_deref(),
        Some("Playhead 00:00:01:00")
    );
    assert!(state.last_error.is_none());
}

#[test]
fn only_the_timeline_and_the_program_view_take_typed_digits() {
    let (mut state, ..) = two_pairs();
    for (panel, key, shift, opens) in [
        (ActivePanel::Timeline, "0", false, true),
        (ActivePanel::Timeline, "add", false, true),
        (ActivePanel::Timeline, "subtract", true, true),
        (ActivePanel::Preview, "7", false, true),
        (ActivePanel::Media, "1", false, false),
        (ActivePanel::Inspector, "1", false, false),
        (ActivePanel::Mixer, "1", false, false),
        (ActivePanel::Ai, "1", false, false),
        (ActivePanel::Timeline, "1", true, false),
        (ActivePanel::Timeline, "a", false, false),
        (ActivePanel::Timeline, "-", false, false),
        (ActivePanel::Timeline, "enter", false, false),
        (ActivePanel::Timeline, "12", false, false),
    ] {
        state.clock_draft = None;
        state.active_panel = panel;
        assert_eq!(
            state.type_into_clock(key, shift),
            opens,
            "{key} (shift {shift}) over {panel:?}"
        );
        let expected = match key {
            "add" => Some("+"),
            "subtract" => Some("-"),
            _ if opens => Some(key),
            _ => None,
        };
        assert_eq!(
            state.clock_draft.as_deref(),
            expected,
            "{key} over {panel:?}"
        );
        // The field under the keys is the one drawn with the draft.
        let site = if panel == ActivePanel::Timeline {
            ClockSite::Timeline
        } else {
            ClockSite::Viewer
        };
        assert!(
            !opens || state.clock_field(site).0,
            "{key} over {panel:?} opened the other playhead field"
        );
    }

    // A field already open takes the key as text, not as a fresh start.
    state.active_panel = ActivePanel::Timeline;
    state.focus_clock(true);
    let seeded = state.clock_draft.clone();
    assert!(!state.type_into_clock("5", false));
    assert_eq!(state.clock_draft, seeded);
}

#[test]
fn the_source_tab_keeps_digits_off_the_sequence_clock() {
    let (mut state, ..) = two_pairs();
    let media = state.store.media[0].id;
    state.select_pool_item(media);
    state.active_panel = ActivePanel::Media;
    assert!(state.toggle_source_monitor(), "the Source tab opens");
    state.active_panel = ActivePanel::Preview;
    assert!(!state.type_into_clock("1", false));
    assert!(!state.clock_focused());
}

#[test]
fn a_sign_moves_the_playhead_when_nothing_is_selected() {
    let (mut state, picture, _, _) = two_pairs();
    let before = spans(&state, picture);
    state.set_playhead(24);

    type_entry(&mut state, "add", "12");
    assert_eq!(state.store.playhead_ticks, 36);
    type_entry(&mut state, "subtract", "1.00");
    assert_eq!(state.store.playhead_ticks, 12);
    type_entry(&mut state, "subtract", "100");
    assert_eq!(state.store.playhead_ticks, 0, "the playhead stops at zero");
    assert_eq!(spans(&state, picture), before, "no clip moved");
}

#[test]
fn a_sign_moves_every_selected_clip_once_as_one_undo_step() {
    let (mut state, picture, sound, [first, second]) = two_pairs();
    // The first pair's sound is selected beside its picture: a companion the
    // move must not carry twice.
    let first_sound = state.engine.lock().project().move_companions(first);
    state.select_clip(first);
    for clip in first_sound {
        state.add_clip_to_selection(clip);
    }
    state.add_clip_to_selection(second);
    state.set_playhead(0);

    // The pairs abut, so moving the first before the second collides.
    type_entry(&mut state, "add", "12");
    assert_eq!(spans(&state, picture), vec![(36, 84), (84, 132)]);
    assert_eq!(spans(&state, sound), vec![(36, 84), (84, 132)]);
    assert_eq!(state.store.playhead_ticks, 0, "a move leaves the playhead");
    assert_eq!(
        state.status_message.as_deref(),
        Some("Moved 2 clips 12 frames later")
    );

    type_entry(&mut state, "subtract", "6");
    assert_eq!(spans(&state, picture), vec![(30, 78), (78, 126)]);
    assert_eq!(spans(&state, sound), vec![(30, 78), (78, 126)]);

    assert!(state.undo());
    assert_eq!(spans(&state, picture), vec![(36, 84), (84, 132)]);
    assert!(state.undo());
    assert_eq!(
        spans(&state, picture),
        vec![(24, 72), (72, 120)],
        "one undo step per typed move"
    );
    assert_eq!(spans(&state, sound), vec![(24, 72), (72, 120)]);
}

#[test]
fn a_move_stops_at_the_sequence_start() {
    let (mut state, picture, _, [first, second]) = two_pairs();
    state.select_clip(first);
    state.add_clip_to_selection(second);

    type_entry(&mut state, "subtract", "100");
    assert_eq!(
        spans(&state, picture),
        vec![(0, 48), (48, 96)],
        "the selection keeps its spacing and stops at zero"
    );

    type_entry(&mut state, "subtract", "5");
    assert_eq!(spans(&state, picture), vec![(0, 48), (48, 96)]);
    assert_eq!(
        state.status_message.as_deref(),
        Some("The selection is already at the sequence start")
    );
    assert!(state.undo());
    assert_eq!(
        spans(&state, picture),
        vec![(24, 72), (72, 120)],
        "the refused move added no undo step"
    );
}

#[test]
fn a_sign_trims_the_selected_edge_in_the_edit_mode() {
    // Overwrite mode: the head moves and the gap it leaves stays.
    let (mut state, picture, _, [first, _]) = two_pairs();
    state.edit_mode = crate::types::EditMode::Overwrite;
    state.select_edge(first, TrimSide::In);
    type_entry(&mut state, "add", "6");
    assert_eq!(spans(&state, picture), vec![(30, 72), (72, 120)]);

    // Ripple mode, the default: the lane closes up behind the trimmed head.
    let (mut state, picture, _, [first, _]) = two_pairs();
    assert_eq!(state.edit_mode, crate::types::EditMode::Ripple);
    state.select_edge(first, TrimSide::In);
    type_entry(&mut state, "add", "6");
    assert_eq!(spans(&state, picture), vec![(24, 66), (66, 114)]);
}

#[test]
fn a_typo_keeps_the_selection_where_it_was() {
    let (mut state, picture, _, [first, _]) = two_pairs();
    state.select_clip(first);
    type_entry(&mut state, "add", "1:99");
    assert_eq!(spans(&state, picture), vec![(24, 72), (72, 120)]);
    assert!(
        state
            .last_error
            .as_deref()
            .is_some_and(|err| err.contains("+1:99 is not a time.")),
        "{:?}",
        state.last_error
    );
}
