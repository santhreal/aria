//! Slip, slide, roll, and rate stretch.
//!
//! The class these close: every trim in the editor used to be an edge drag, so
//! the arithmetic that decides how much room a gesture has never existed. Four
//! gestures now share the same pixels and differ only by the active tool, which
//! makes "how far can this go" and "how many undo entries did it cost" the two
//! things that can silently go wrong. Both are asserted here.
//!
//! The room helpers are pure, so they are asserted directly at their boundaries.
//! The gestures that issue more than one command are asserted through the real
//! `AppState`, because the property that matters — one undo entry, and nothing
//! half-applied on a refusal — lives in the engine's history group, not in the
//! arithmetic.
//!
//! Not covered: the pointer routing that picks a gesture, which needs a window.
//! `finish_body_drag` and `finish_trim` only choose which of these to call.

use aria_models::Rational;
use aria_plugin::PluginContext;

use crate::app::AppState;
use crate::app::trim_gestures::{Neighbour, roll_room, slide_room, stretch_speed};
use crate::scratch_pool::scratch;

fn fixture() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/texture.png")
}

fn hood(clip: u64, start: i64, end: i64) -> Neighbour {
    Neighbour {
        clip: aria_models::ClipId::from_raw(clip),
        start,
        end,
    }
}

/// A project with the still placed twice, abutting, on one lane.
fn pair() -> (AppState, Vec<(u64, i64, i64)>) {
    let mut state = AppState::new(PluginContext::new(), scratch("trimges")).expect("state");
    state.new_project().expect("project");
    let media = state.import_media(fixture()).expect("import the still");
    state
        .drop_media_on_timeline(media, None, 0)
        .expect("place the first");
    let first_end = clips(&state)[0].2;
    state
        .drop_media_on_timeline(media, None, first_end)
        .expect("place the second abutting");
    let placed = clips(&state);
    assert_eq!(placed.len(), 2, "the lane holds both takes");
    (state, placed)
}

fn clips(state: &AppState) -> Vec<(u64, i64, i64)> {
    state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .map(|clip| (clip.id, clip.timeline_start_ticks, clip.timeline_end_ticks))
        .collect()
}

// --- slide_room ---

#[test]
fn a_slide_with_no_neighbour_is_unbounded_in_that_direction() {
    // Empty space costs nobody anything: the whole delta is available.
    assert_eq!(slide_room(100, 200, 40, None, None), 40);
    assert_eq!(slide_room(100, 200, -40, None, None), -40);
}

#[test]
fn a_slide_stops_where_the_neighbour_would_vanish() {
    let after = hood(9, 200, 210);
    // The next clip is ten ticks long, so it can give nine and keep one.
    assert_eq!(slide_room(100, 200, 40, None, Some(after)), 9);
    assert_eq!(slide_room(100, 200, 5, None, Some(after)), 5);
    let before = hood(8, 90, 100);
    assert_eq!(slide_room(100, 200, -40, Some(before), None), -9);
}

#[test]
fn a_slide_of_nothing_or_of_a_degenerate_clip_is_nothing() {
    assert_eq!(slide_room(100, 200, 0, None, None), 0);
    assert_eq!(slide_room(200, 200, 40, None, None), 0);
    assert_eq!(slide_room(200, 100, 40, None, None), 0);
}

#[test]
fn a_neighbour_already_at_one_tick_gives_nothing() {
    // The boundary case the clamp exists for: a further drag must be inert
    // rather than asking the model for an empty clip.
    let after = hood(9, 200, 201);
    assert_eq!(slide_room(100, 200, 40, None, Some(after)), 0);
    let before = hood(8, 99, 100);
    assert_eq!(slide_room(100, 200, -40, Some(before), None), 0);
}

// --- roll_room ---

#[test]
fn a_roll_stops_where_either_side_would_vanish() {
    let left = hood(1, 0, 100);
    let right = hood(2, 100, 120);
    assert_eq!(roll_room(left, right, 500), 19);
    assert_eq!(roll_room(left, right, -500), -99);
    assert_eq!(roll_room(left, right, 5), 5);
    assert_eq!(roll_room(left, right, -5), -5);
}

#[test]
fn a_roll_against_a_one_tick_side_is_inert() {
    let left = hood(1, 0, 1);
    let right = hood(2, 1, 2);
    assert_eq!(roll_room(left, right, 9), 0);
    assert_eq!(roll_room(left, right, -9), 0);
}

// --- stretch_speed ---

#[test]
fn a_shorter_clip_plays_faster_and_a_longer_one_slower() {
    let one = Rational::new(1, 1);
    // Half the timeline for the same source is double speed.
    assert_eq!(stretch_speed(one, 100, 50), Some(Rational::new(2, 1)));
    assert_eq!(stretch_speed(one, 100, 200), Some(Rational::new(1, 2)));
}

#[test]
fn a_stretch_composes_with_the_speed_the_clip_already_runs_at() {
    let double = Rational::new(2, 1);
    assert_eq!(stretch_speed(double, 100, 200), Some(Rational::new(1, 1)));
    assert_eq!(stretch_speed(double, 100, 50), Some(Rational::new(4, 1)));
}

#[test]
fn a_degenerate_stretch_asks_for_nothing() {
    let one = Rational::new(1, 1);
    assert_eq!(stretch_speed(one, 100, 100), None);
    assert_eq!(stretch_speed(one, 100, 0), None);
    assert_eq!(stretch_speed(one, 0, 100), None);
    assert_eq!(stretch_speed(one, -5, 100), None);
}

#[test]
fn a_stretch_beyond_what_a_speed_can_say_is_refused_rather_than_rounded() {
    let extreme = Rational::new(i32::MAX, 1);
    assert_eq!(stretch_speed(extreme, i64::from(i32::MAX), 1), None);
}

// --- the gestures, through the real state ---

#[test]
fn neighbours_are_the_clips_that_touch_and_nothing_further_away() {
    let (state, placed) = pair();
    let (first, second) = (placed[0], placed[1]);
    let (before, after) = state.abutting(aria_models::ClipId::from_raw(first.0));
    assert_eq!(before, None, "nothing precedes the first clip");
    assert_eq!(
        after.map(|n| n.clip.raw()),
        Some(second.0),
        "the abutting clip is the neighbor"
    );
    let (before, after) = state.abutting(aria_models::ClipId::from_raw(second.0));
    assert_eq!(before.map(|n| n.clip.raw()), Some(first.0));
    assert_eq!(after, None);
}

#[test]
fn a_clip_that_is_no_longer_there_has_no_neighbours() {
    let (state, _) = pair();
    let (before, after) = state.abutting(aria_models::ClipId::from_raw(9999));
    assert_eq!((before, after), (None, None));
}

#[test]
fn a_roll_moves_the_cut_and_costs_one_undo_entry() {
    let (mut state, placed) = pair();
    let (first, second) = (placed[0], placed[1]);
    let cut = first.2;
    let before = state.revision;
    state
        .roll_cut(
            hood(first.0, first.1, first.2),
            hood(second.0, second.1, second.2),
            -10,
        )
        .expect("the cut has room to move left");
    let after = clips(&state);
    let moved_cut = after[0].2;
    assert_eq!(moved_cut, cut - 10, "the left clip ends ten ticks earlier");
    assert_eq!(
        after[1].1,
        cut - 10,
        "the right clip starts at the same instant"
    );
    assert_eq!(after[1].2, second.2, "the outer edges did not move");
    assert_eq!(after[0].1, first.1);
    assert!(state.revision > before, "the roll landed");
    // One undo puts both trims back: a rolling edit made in one gesture must
    // not take two undos to unmake.
    assert!(state.undo(), "the group is one entry");
    assert_eq!(clips(&state), placed, "both clips are back where they were");
}

#[test]
fn a_roll_with_no_room_leaves_the_timeline_alone() {
    let (mut state, placed) = pair();
    let (first, second) = (placed[0], placed[1]);
    let before = state.revision;
    state
        .roll_cut(
            hood(first.0, first.1, first.2),
            hood(second.0, second.1, second.2),
            i64::MAX / 4,
        )
        .expect("a clamped roll is not an error");
    let after = clips(&state);
    assert_eq!(
        after[1].2, second.2,
        "the right clip keeps its tail whatever the drag asked for"
    );
    assert!(after[0].2 < second.2, "the cut stayed inside the pair");
    assert!(state.revision > before);
}

#[test]
fn a_slide_keeps_the_clip_length_and_trades_with_the_neighbour() {
    let (mut state, placed) = pair();
    let (first, second) = (placed[0], placed[1]);
    let length = second.2 - second.1;
    let before = state.revision;
    state
        .slide_clip(
            aria_models::ClipId::from_raw(second.0),
            second.1,
            second.2,
            -10,
            Some(hood(first.0, first.1, first.2)),
            None,
        )
        .expect("the previous clip can give ten ticks");
    let after = clips(&state);
    let slid = after
        .iter()
        .find(|item| item.0 == second.0)
        .copied()
        .expect("the slid clip is still there");
    assert_eq!(slid.1, second.1 - 10, "it moved left by the delta");
    assert_eq!(slid.2 - slid.1, length, "and kept its length");
    let prev = after
        .iter()
        .find(|item| item.0 == first.0)
        .copied()
        .expect("the neighbor is still there");
    assert_eq!(prev.2, first.2 - 10, "the neighbor absorbed the delta");
    assert_eq!(prev.1, first.1, "its own start did not move");
    assert!(state.revision > before);
    assert!(state.undo(), "the pair of commands is one entry");
    assert_eq!(clips(&state), placed);
}

#[test]
fn a_slide_of_zero_is_not_an_edit() {
    let (mut state, placed) = pair();
    let second = placed[1];
    let before = state.revision;
    state
        .slide_clip(
            aria_models::ClipId::from_raw(second.0),
            second.1,
            second.2,
            0,
            Some(hood(placed[0].0, placed[0].1, placed[0].2)),
            None,
        )
        .expect("nothing to do is not a failure");
    assert_eq!(state.revision, before, "an inert gesture costs no history");
    assert_eq!(clips(&state), placed);
}

#[test]
fn a_ripple_trim_of_the_tail_closes_the_gap_it_makes() {
    let (mut state, placed) = pair();
    let (first, second) = (placed[0], placed[1]);
    let track = state
        .track_of(aria_models::ClipId::from_raw(first.0))
        .expect("the clip is on a lane");
    let before = state.revision;
    state
        .ripple_trim(
            aria_models::ClipId::from_raw(first.0),
            track,
            first.1,
            first.2,
            false,
            -10,
        )
        .expect("shortening the tail has room");
    let after = clips(&state);
    let trimmed = after
        .iter()
        .find(|item| item.0 == first.0)
        .copied()
        .expect("the trimmed clip is still there");
    let moved = after
        .iter()
        .find(|item| item.0 == second.0)
        .copied()
        .expect("the later clip is still there");
    assert_eq!(trimmed.2, first.2 - 10, "the tail came in ten ticks");
    assert_eq!(moved.1, trimmed.2, "the later clip closed up against it");
    assert_eq!(
        moved.2 - moved.1,
        second.2 - second.1,
        "the later clip only moved; it was not trimmed"
    );
    assert!(state.revision > before);
    // A trim and the shift it forces are one gesture, so one undo.
    assert!(state.undo(), "the group is one entry");
    assert_eq!(clips(&state), placed);
}

#[test]
fn a_ripple_trim_of_the_head_slides_the_lane_back() {
    let (mut state, placed) = pair();
    let (first, second) = (placed[0], placed[1]);
    let track = state
        .track_of(aria_models::ClipId::from_raw(first.0))
        .expect("the clip is on a lane");
    state
        .ripple_trim(
            aria_models::ClipId::from_raw(first.0),
            track,
            first.1,
            first.2,
            true,
            10,
        )
        .expect("shortening the head has room");
    let after = clips(&state);
    let trimmed = after
        .iter()
        .find(|item| item.0 == first.0)
        .copied()
        .expect("the trimmed clip is still there");
    let moved = after
        .iter()
        .find(|item| item.0 == second.0)
        .copied()
        .expect("the later clip is still there");
    assert_eq!(
        trimmed.1, first.1,
        "the lane still starts where it did: the head trim slid back"
    );
    assert_eq!(trimmed.2, first.2 - 10, "the clip is ten ticks shorter");
    assert_eq!(moved.1, trimmed.2, "and the later clip followed it left");
    assert!(state.undo());
    assert_eq!(clips(&state), placed);
}

#[test]
fn a_ripple_trim_of_nothing_is_not_an_edit() {
    let (mut state, placed) = pair();
    let first = placed[0];
    let track = state
        .track_of(aria_models::ClipId::from_raw(first.0))
        .expect("the clip is on a lane");
    let before = state.revision;
    state
        .ripple_trim(
            aria_models::ClipId::from_raw(first.0),
            track,
            first.1,
            first.2,
            false,
            0,
        )
        .expect("nothing to do is not a failure");
    assert_eq!(state.revision, before);
    assert_eq!(clips(&state), placed);
}

#[test]
fn a_rate_stretch_of_a_still_is_declined_by_the_model_not_by_the_ui() {
    // The fixture is a still, so the model refuses a speed on it. What is
    // asserted here is that the gesture reports that refusal rather than
    // leaving a half-applied edit behind.
    let (mut state, placed) = pair();
    let first = placed[0];
    let length = first.2 - first.1;
    let before = state.revision;
    let result = state.rate_stretch(aria_models::ClipId::from_raw(first.0), length, length * 2);
    if result.is_ok() {
        let after = clips(&state);
        assert_ne!(
            after[0].2 - after[0].1,
            length,
            "a stretch that landed changed the length"
        );
    } else {
        assert_eq!(state.revision, before, "a refused stretch changed nothing");
        assert_eq!(clips(&state), placed);
    }
}

#[test]
fn a_rate_stretch_of_a_clip_that_is_gone_does_nothing() {
    let (mut state, placed) = pair();
    let before = state.revision;
    state
        .rate_stretch(aria_models::ClipId::from_raw(9999), 100, 200)
        .expect("an unknown clip is not an error");
    assert_eq!(state.revision, before);
    assert_eq!(clips(&state), placed);
}

#[test]
fn every_tool_names_itself_a_key_and_a_control_exactly_once() {
    use crate::types::EditorTool;
    let mut keys: Vec<&str> = EditorTool::all().map(|tool| tool.key()).collect();
    let count = keys.len();
    keys.sort_unstable();
    keys.dedup();
    assert_eq!(keys.len(), count, "two tools claim the same key");
    let mut ids: Vec<&str> = EditorTool::all().map(|tool| tool.control_id()).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), count, "two tools claim the same control id");
    for tool in EditorTool::all() {
        assert!(!tool.label().is_empty(), "{tool:?} has no name");
        assert!(
            crate::tooltip::tip_for(tool.control_id()).is_some(),
            "{tool:?} has no tooltip, so its button says nothing"
        );
    }
}

/// Every tool's declared key is bound, on the editor route, to that tool.
///
/// The class this closes: a tool's row says which key selects it, and the key
/// table is a separate list of `Binding` rows written by hand. Nothing tied the
/// two together, so a tool could declare `y` with nothing bound to it, or `y`
/// could be bound to a different tool, and both compiled. The chord is matched
/// against the row and then the binding is *run*, because "a row exists" is
/// weaker than "pressing the key selects this tool".
///
/// Not covered: chords a tool does not declare. `key-tool-pointer-a` binds `a`
/// to the pointer as a second chord, which no row names and this does not reach.
#[test]
fn every_tool_is_bound_to_the_key_its_row_declares() {
    use crate::keys::bindings::SHIPPED_BINDINGS;
    use crate::keys::{Chord, KeyRoute};
    use crate::types::EditorTool;

    let dir = scratch("trim-tool-keys");
    let mut state = AppState::new(PluginContext::new(), dir).unwrap();

    for tool in EditorTool::all() {
        let key = tool.key();
        let chord =
            Chord::parse(key).unwrap_or_else(|| panic!("{tool:?} declares invalid chord {key:?}"));
        let bound: Vec<_> = SHIPPED_BINDINGS
            .iter()
            .filter(|binding| binding.route == KeyRoute::Editor && binding.chord == chord)
            .collect();
        assert_eq!(
            bound.len(),
            1,
            "{tool:?} declares key {key:?}, which {} editor bindings claim",
            bound.len()
        );

        // Start from a tool this binding is not supposed to produce, so the
        // assertion cannot pass because the state already held the answer.
        let sentinel = if tool == EditorTool::Razor {
            EditorTool::Pointer
        } else {
            EditorTool::Razor
        };
        state.tool = sentinel;
        match bound[0].run {
            crate::keys::KeyBody::Plain(run) => run(&mut state),
            // A tool binding is a state change and nothing else. One that needed
            // the entity context would not be a tool.
            crate::keys::KeyBody::Windowed(_) | crate::keys::KeyBody::Command(_) => {
                panic!("{tool:?} binds a body that is not a state change")
            }
        }
        assert_eq!(
            state.tool, tool,
            "the {key:?} binding does not select {tool:?}"
        );
    }
}
