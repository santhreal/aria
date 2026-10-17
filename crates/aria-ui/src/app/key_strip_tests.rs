//! The Effect Controls time area: where a strip puts a frame, what a press on
//! it takes, and what a drag, a release, and Escape leave behind.
//!
//! Closes the class "the strip and the timeline disagree about a keyframe or the
//! playhead": every gesture here goes through the state the timeline's property
//! row reads, so each test reads the result back through `keyframes_of` and the
//! playhead rather than through the strip. Does not cover the strip's paint or
//! the shell routing the pointer to it; the drawn test in `inspector_strip_tests`
//! presses the painted strip.

use aria_models::ClipParam;

use super::AppState;
use super::key_strip::{KEY_HIT_PX, KeyStrip, StripMark};
use super::premiere_tests::with_clip;
use super::property_rows::KeyframeIconShape;

/// The strip's place in the window for every test that presses one.
const LEFT: f32 = 100.0;
const WIDTH: f32 = 200.0;

fn strip(start: i64, end: i64, marks: &[i64]) -> KeyStrip {
    KeyStrip {
        clip: aria_models::ClipId::from_raw(1),
        param: ClipParam::Opacity,
        start,
        end,
        playhead: start,
        marks: marks
            .iter()
            .map(|&tick| StripMark {
                tick,
                shape: KeyframeIconShape::Linear,
                selected: false,
            })
            .collect(),
    }
}

/// The window x a frame is drawn at on a strip laid out at `LEFT`, `WIDTH`.
fn x_of(strip: &KeyStrip, tick: i64) -> f32 {
    LEFT + strip.fraction(tick) * WIDTH
}

fn marks(state: &AppState, param: ClipParam) -> Vec<i64> {
    state
        .keyframes_of(param)
        .iter()
        .map(|mark| mark.tick)
        .collect()
}

/// A clip keyed on Opacity at its first frame and twenty frames in, with the
/// playhead parked well clear of both so no drag snaps to it by accident.
fn keyed() -> (AppState, KeyStrip) {
    let (mut state, _) = with_clip();
    let first = state.key_strip(ClipParam::Opacity).expect("opacity strip");
    assert!(
        first.end - first.start >= 60,
        "the fixture clip is too short for these tests: {first:?}"
    );
    state.set_playhead(first.start);
    state
        .add_keyframe_at_playhead(ClipParam::Opacity)
        .expect("first key");
    state.set_playhead(first.start + 20);
    state
        .add_keyframe_at_playhead(ClipParam::Opacity)
        .expect("second key");
    state.set_playhead(first.start + 50);
    let strip = state.key_strip(ClipParam::Opacity).expect("opacity strip");
    (state, strip)
}

#[test]
fn a_strip_maps_the_clip_range_onto_its_width_and_holds_frames_inside_it() {
    let strip = strip(100, 200, &[]);
    assert_eq!(strip.fraction(100), 0.0);
    assert_eq!(strip.fraction(150), 0.5);
    assert_eq!(strip.fraction(200), 1.0);
    assert_eq!(strip.fraction(-5), 0.0, "before the clip pins to its start");
    assert_eq!(strip.fraction(900), 1.0, "past the clip pins to its end");

    assert_eq!(strip.tick_at(0.0), 100);
    assert_eq!(strip.tick_at(0.5), 150);
    assert_eq!(strip.tick_at(1.0), 199, "the end is the clip's last frame");
    assert_eq!(strip.tick_at(-3.0), 100);
    assert_eq!(strip.tick_at(7.0), 199);
    assert_eq!(strip.tick_at(f32::NAN), 100);
    assert_eq!(strip.tick_at(f32::INFINITY), 199);

    // A one-frame clip, and a degenerate one, still answer inside themselves.
    assert_eq!(strip_of(5, 6).tick_at(1.0), 5);
    assert_eq!(strip_of(5, 5).tick_at(0.7), 5);
    assert!(strip_of(5, 5).fraction(5).is_finite());
}

fn strip_of(start: i64, end: i64) -> KeyStrip {
    strip(start, end, &[])
}

#[test]
fn the_playhead_is_drawn_only_while_it_is_on_the_clip() {
    let mut strip = strip(100, 200, &[]);
    strip.playhead = 150;
    assert_eq!(strip.playhead_fraction(), Some(0.5));
    strip.playhead = 100;
    assert_eq!(strip.playhead_fraction(), Some(0.0));
    strip.playhead = 199;
    assert!(strip.playhead_fraction().is_some());
    for off in [99, 200, -1, 10_000] {
        strip.playhead = off;
        assert_eq!(strip.playhead_fraction(), None, "playhead at {off}");
    }
}

#[test]
fn a_press_lands_on_the_nearest_keyframe_within_its_slop_and_on_nothing_past_it() {
    let strip = strip(0, 100, &[10, 14, 60]);
    let slop = 0.05;
    assert_eq!(strip.mark_near(0.10, slop), Some(10));
    assert_eq!(strip.mark_near(0.13, slop), Some(14), "the nearer of two");
    assert_eq!(strip.mark_near(0.62, slop), Some(60));
    assert_eq!(strip.mark_near(0.66, slop), None, "past the slop");
    assert_eq!(strip.mark_near(0.40, slop), None);
    assert_eq!(
        strip_of(0, 100).mark_near(0.1, 1.0),
        None,
        "no keys, no hit"
    );
}

#[test]
fn a_press_off_a_keyframe_scrubs_and_escape_puts_the_playhead_back() {
    let (mut state, strip) = keyed();
    let parked = state.playhead.value;
    let target = strip.start + 35;

    assert!(state.press_key_strip(ClipParam::Opacity, x_of(&strip, target), LEFT, WIDTH, false));
    assert!(state.key_strip_held());
    assert_eq!(state.playhead.value, target, "the press moves the playhead");

    let further = strip.start + 40;
    assert!(state.drag_key_strip(x_of(&strip, further), false));
    assert_eq!(state.playhead.value, further, "the drag scrubs");
    assert!(
        !state.drag_key_strip(x_of(&strip, further), false),
        "the same frame again moves nothing"
    );
    // Dragged off the strip, the scrub holds at the clip's ends.
    assert!(state.drag_key_strip(LEFT + WIDTH * 4.0, false));
    assert_eq!(state.playhead.value, strip.end - 1);

    assert!(state.cancel_key_strip());
    assert!(!state.key_strip_held());
    assert_eq!(state.playhead.value, parked, "Escape returns the playhead");
    assert_eq!(
        marks(&state, ClipParam::Opacity),
        vec![strip.start, strip.start + 20]
    );

    // A released scrub stays where it ended.
    assert!(state.press_key_strip(ClipParam::Opacity, x_of(&strip, target), LEFT, WIDTH, false));
    assert!(state.release_key_strip());
    assert_eq!(state.playhead.value, target);
    assert!(!state.release_key_strip(), "nothing is held");
    assert!(!state.drag_key_strip(LEFT, false), "nothing is held");
    assert!(!state.cancel_key_strip(), "nothing is held");
}

#[test]
fn a_keyframe_dragged_on_the_strip_moves_in_one_undo_step_and_shows_where_it_goes() {
    let (mut state, strip) = keyed();
    let from = strip.start + 20;
    // Pressing a few pixels off the key still takes it.
    let press = x_of(&strip, from) + KEY_HIT_PX - 1.0;
    assert!(state.press_key_strip(ClipParam::Opacity, press, LEFT, WIDTH, false));
    let playhead = state.playhead.value;
    assert_ne!(playhead, from, "a press on a key does not scrub");

    // Thirty frames to the right, measured from where the press landed.
    let to = from + 30;
    let dx = x_of(&strip, to) - x_of(&strip, from);
    assert!(state.drag_key_strip(press + dx, true));
    assert_eq!(state.playhead.value, playhead, "a key drag does not scrub");
    let shown = state.key_strip(ClipParam::Opacity).expect("strip");
    let ticks: Vec<i64> = shown.marks.iter().map(|mark| mark.tick).collect();
    assert_eq!(
        ticks,
        vec![strip.start, to],
        "the strip draws the drag in flight"
    );
    assert!(shown.marks[1].selected, "the pressed key is selected");
    assert!(!shown.marks[0].selected);
    assert_eq!(
        marks(&state, ClipParam::Opacity),
        vec![strip.start, from],
        "nothing is written before the release"
    );

    assert!(state.release_key_strip());
    assert_eq!(marks(&state, ClipParam::Opacity), vec![strip.start, to]);
    assert!(state.undo(), "the move is one undo step");
    assert_eq!(marks(&state, ClipParam::Opacity), vec![strip.start, from]);
}

#[test]
fn a_dragged_keyframe_snaps_to_the_playhead_unless_alt_is_held_and_escape_drops_it() {
    let (mut state, strip) = keyed();
    let from = strip.start + 20;
    let playhead = state.playhead.value;
    let x = x_of(&strip, from);
    assert!(state.press_key_strip(ClipParam::Opacity, x, LEFT, WIDTH, false));

    // One frame short of the playhead, inside the snap distance.
    let near = x_of(&strip, playhead - 1) - x;
    assert!(state.drag_key_strip(x + near, false));
    let snapped = state.key_strip(ClipParam::Opacity).expect("strip");
    assert_eq!(snapped.marks[1].tick, playhead, "snaps to the playhead");
    assert_eq!(state.snap_guide.map(|guide| guide.tick), Some(playhead));

    assert!(state.drag_key_strip(x + near, true));
    let free = state.key_strip(ClipParam::Opacity).expect("strip");
    assert_eq!(
        free.marks[1].tick,
        playhead - 1,
        "Alt places it where the pointer is"
    );
    assert!(state.snap_guide.is_none(), "Alt draws no snap guide");

    let revision = state.engine().lock().revision();
    assert!(state.cancel_key_strip());
    assert_eq!(marks(&state, ClipParam::Opacity), vec![strip.start, from]);
    let after = state.key_strip(ClipParam::Opacity).expect("strip");
    assert_eq!(after.marks[1].tick, from, "Escape drops the drag in flight");
    assert_eq!(
        state.engine().lock().revision(),
        revision,
        "Escape writes nothing"
    );
}

#[test]
fn a_strip_takes_no_press_it_cannot_place() {
    let (mut state, strip) = keyed();
    let x = x_of(&strip, strip.start + 30);
    assert!(!state.press_key_strip(ClipParam::Opacity, x, LEFT, 0.0, false));
    assert!(!state.press_key_strip(ClipParam::Opacity, x, LEFT, -5.0, false));
    assert!(!state.press_key_strip(ClipParam::Opacity, x, LEFT, f32::NAN, false));
    assert!(!state.press_key_strip(ClipParam::Opacity, f32::NAN, LEFT, WIDTH, false));
    assert!(!state.key_strip_held());

    state.clear_selection();
    assert_eq!(
        state.key_strip(ClipParam::Opacity),
        None,
        "nothing is selected"
    );
    assert!(!state.press_key_strip(ClipParam::Opacity, x, LEFT, WIDTH, false));
    assert!(!state.key_strip_held());
}
