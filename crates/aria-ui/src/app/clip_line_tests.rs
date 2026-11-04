//! Dragging the line a clip bar draws.
//!
//! The class closed here is "a drag on the line writes a value other than the
//! one the bar drew under the pointer": travel compounded across moves rather
//! than measured from the press, a gain moved in linear amplitude where the
//! bar draws decibels, a press between two keyframes moving one of them, a
//! drag landing as several undo steps, or Escape keeping what the drag wrote.
//! Not covered: where the canvas places the line and the dots; the canvas
//! tests in `panels/timeline/clip/bar_tests.rs` press the painted line.

use aria_commands::{Command, EditCommand};
use aria_models::{ClipId, ClipParam, Param, RationalTime};

use super::{AppState, LinePress, LineTravel};
use crate::app::premiere_tests::{with_clip, with_media};
use crate::types::Taper;

/// The band the tests drag on: 40 px from the bottom of the scale to its top.
const BAND: f32 = 40.0;

fn model_param(state: &AppState, clip: ClipId, param: ClipParam) -> Param<f32> {
    let engine = state.engine.lock();
    let model = engine
        .project()
        .clip(clip)
        .expect("the clip is in the project");
    match param {
        ClipParam::Opacity => model.transform.opacity.clone(),
        ClipParam::Volume => model.volume.clone(),
        ClipParam::Rotation => model.transform.rotation.clone(),
        other => panic!("no test reader for {other:?}"),
    }
}

fn constant(state: &AppState, clip: ClipId, param: ClipParam) -> f32 {
    model_param(state, clip, param)
        .constant()
        .unwrap_or_else(|| panic!("{param:?} is not animated"))
}

fn keys(state: &AppState, clip: ClipId, param: ClipParam) -> Vec<(i64, f32)> {
    model_param(state, clip, param)
        .keyframes()
        .iter()
        .map(|key| (key.tick, key.value))
        .collect()
}

fn line_scale(state: &AppState, clip: ClipId) -> Taper {
    state
        .ui_clip_ref(clip)
        .and_then(|ui| ui.line.as_ref())
        .expect("the bar draws a line")
        .scale
}

fn key(state: &mut AppState, clip: ClipId, param: ClipParam, tick: i64, value: f32) {
    let at = RationalTime::new(tick, state.store.sequence.frame_rate);
    state
        .apply_command(Command::Edit(EditCommand::SetParamKeyframe {
            clip,
            param,
            at,
            value: aria_models::ParamValue::Scalar(value),
            easing: aria_models::Easing::Linear,
            tangents: None,
        }))
        .expect("the keyframe is accepted");
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

#[test]
fn a_drag_is_measured_from_the_press_and_lands_as_one_undo_step() {
    let (mut state, clip) = with_clip();
    assert_eq!(constant(&state, clip, ClipParam::Opacity), 1.0);
    assert_eq!(
        line_scale(&state, clip),
        Taper::Linear { min: 0.0, max: 1.0 }
    );

    assert!(state.press_clip_line(clip, LinePress::Line(6), BAND));
    assert!(state.clip_line_held());
    assert!(state.drag_clip_line(LineTravel::down(10.0)));
    assert!(close(constant(&state, clip, ClipParam::Opacity), 0.75));
    // A second move rewrites the whole travel rather than adding to the first.
    assert!(state.drag_clip_line(LineTravel::down(20.0)));
    assert!(close(constant(&state, clip, ClipParam::Opacity), 0.5));
    let readout = state
        .trim_feedback
        .as_ref()
        .expect("a drag shows its value")
        .delta_label
        .clone();
    assert_eq!(readout.as_ref(), "50%");
    // The pointer resting where it was writes nothing.
    assert!(!state.drag_clip_line(LineTravel::down(20.0)));

    assert!(state.release_clip_line());
    assert!(!state.clip_line_held());
    assert!(state.trim_feedback.is_none());
    assert!(close(constant(&state, clip, ClipParam::Opacity), 0.5));

    assert!(state.undo(), "the drag is on the undo stack");
    assert_eq!(constant(&state, clip, ClipParam::Opacity), 1.0);
    assert!(
        state.ui_clip_ref(clip).is_some(),
        "one undo takes back the drag and nothing before it"
    );
}

#[test]
fn a_drag_clamps_to_the_property_range() {
    let (mut state, clip) = with_clip();
    assert!(state.press_clip_line(clip, LinePress::Line(0), BAND));
    assert!(state.drag_clip_line(LineTravel::down(-400.0)));
    assert_eq!(constant(&state, clip, ClipParam::Opacity), 1.0);
    assert!(state.drag_clip_line(LineTravel::down(400.0)));
    assert_eq!(constant(&state, clip, ClipParam::Opacity), 0.0);
    assert!(state.release_clip_line());
}

#[test]
fn escape_puts_the_line_back_where_the_press_found_it() {
    let (mut state, clip) = with_clip();
    assert!(state.press_clip_line(clip, LinePress::Line(0), BAND));
    assert!(state.drag_clip_line(LineTravel::down(30.0)));
    assert!(close(constant(&state, clip, ClipParam::Opacity), 0.25));

    assert!(state.cancel_clip_line());
    assert!(!state.clip_line_held());
    assert!(state.trim_feedback.is_none());
    assert_eq!(constant(&state, clip, ClipParam::Opacity), 1.0);
    assert!(!state.release_clip_line(), "nothing is held after Escape");
}

#[test]
fn a_gain_line_moves_in_decibels() {
    let (mut state, clip) = with_media(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/fixture-sine-48k-2s.wav"),
    );
    let scale = line_scale(&state, clip);
    let Taper::Gain { max } = scale else {
        panic!("an audio clip's volume line draws a gain, got {scale:?}");
    };
    assert_eq!(constant(&state, clip, ClipParam::Volume), 1.0);

    // A sixth of the band down is a sixth of the scale's decibel span.
    let travel = BAND / 6.0;
    assert!(state.press_clip_line(clip, LinePress::Line(0), BAND));
    assert!(state.drag_clip_line(LineTravel::down(travel)));
    assert!(state.release_clip_line());
    let span_db = 20.0 * max.log10() - aria_analysis::GAIN_FLOOR_DB;
    let after_db = 20.0 * constant(&state, clip, ClipParam::Volume).log10();
    assert!(
        (after_db - (-span_db / 6.0)).abs() < 1e-2,
        "unity moved to {after_db} dB, expected {} dB",
        -span_db / 6.0
    );
}

#[test]
fn a_press_between_two_keyframes_moves_both_and_a_press_on_one_moves_only_it() {
    let (mut state, clip) = with_clip();
    key(&mut state, clip, ClipParam::Opacity, 0, 1.0);
    key(&mut state, clip, ClipParam::Opacity, 12, 1.0);
    key(&mut state, clip, ClipParam::Opacity, 24, 1.0);

    assert!(state.press_clip_line(clip, LinePress::Line(6), BAND));
    assert!(state.drag_clip_line(LineTravel::down(20.0)));
    assert!(state.release_clip_line());
    let moved = keys(&state, clip, ClipParam::Opacity);
    assert_eq!(moved.len(), 3);
    assert!(close(moved[0].1, 0.5) && close(moved[1].1, 0.5));
    assert_eq!(moved[2].1, 1.0, "the key outside the pressed segment stays");

    assert!(state.press_clip_line(clip, LinePress::Key(12), BAND));
    assert!(state.drag_clip_line(LineTravel::down(10.0)));
    assert!(state.release_clip_line());
    let moved = keys(&state, clip, ClipParam::Opacity);
    assert!(close(moved[0].1, 0.5), "the key before is left alone");
    assert!(close(moved[1].1, 0.25));
    assert_eq!(moved[2].1, 1.0);

    // Before the first key and after the last the line is level with it.
    assert!(state.press_clip_line(clip, LinePress::Line(40), BAND));
    assert!(state.drag_clip_line(LineTravel::down(40.0)));
    assert!(state.release_clip_line());
    let moved = keys(&state, clip, ClipParam::Opacity);
    assert!(close(moved[1].1, 0.25));
    assert_eq!(moved[2].1, 0.0);
}

/// The travel of a press on `clip`'s line, `down` pixels and `along` ticks
/// from the press, with no magnet.
fn moved(down: f32, along: i64) -> LineTravel {
    LineTravel {
        down,
        along,
        magnet: 0,
    }
}

#[test]
fn a_dot_drag_carries_its_keyframe_along_the_lane_and_a_line_drag_keeps_the_ticks() {
    let (mut state, clip) = with_clip();
    key(&mut state, clip, ClipParam::Opacity, 0, 1.0);
    key(&mut state, clip, ClipParam::Opacity, 12, 1.0);
    key(&mut state, clip, ClipParam::Opacity, 24, 1.0);
    let pressed = keys(&state, clip, ClipParam::Opacity);

    assert!(state.press_clip_line(clip, LinePress::Key(12), BAND));
    assert!(state.drag_clip_line(moved(10.0, 6)));
    assert_eq!(
        keys(&state, clip, ClipParam::Opacity),
        [(0, 1.0), (18, 0.75), (24, 1.0)]
    );
    // The travel is measured from the press: the same height and a shorter
    // run along the lane leaves the key short of where the first move put it.
    assert!(state.drag_clip_line(moved(10.0, 3)));
    assert_eq!(
        keys(&state, clip, ClipParam::Opacity),
        [(0, 1.0), (15, 0.75), (24, 1.0)]
    );
    let feedback = state.trim_feedback.clone().expect("a drag shows its value");
    assert_eq!(feedback.delta_ticks, 3);
    assert!(feedback.detail_label.starts_with("At "), "{feedback:?}");
    // The key's own tick holds it nowhere: a short run in reach of a magnet
    // still moves it.
    state.set_playhead(0);
    let short = LineTravel {
        magnet: 3,
        ..moved(10.0, 1)
    };
    assert!(state.drag_clip_line(short));
    assert_eq!(keys(&state, clip, ClipParam::Opacity)[1].0, 13);
    assert!(state.release_clip_line());
    assert!(state.undo(), "the drag is on the undo stack");
    assert_eq!(
        keys(&state, clip, ClipParam::Opacity),
        pressed,
        "one undo takes back both the value and the time"
    );

    // Escape puts a key moved along the lane back at its tick.
    assert!(state.press_clip_line(clip, LinePress::Key(24), BAND));
    assert!(state.drag_clip_line(moved(20.0, -4)));
    assert!(state.cancel_clip_line());
    assert_eq!(keys(&state, clip, ClipParam::Opacity), pressed);

    // A press between two keys holds their ticks wherever the pointer goes.
    assert!(state.press_clip_line(clip, LinePress::Line(6), BAND));
    assert!(state.drag_clip_line(moved(20.0, 30)));
    assert!(state.release_clip_line());
    assert_eq!(
        keys(&state, clip, ClipParam::Opacity),
        [(0, 0.5), (12, 0.5), (24, 1.0)]
    );
}

#[test]
fn a_dot_drag_stays_inside_its_clip_and_pulls_to_the_playhead_once_it_moves_along() {
    let (mut state, clip) = with_clip();
    let end = state
        .ui_clip_ref(clip)
        .map(|ui| ui.timeline_end_ticks)
        .expect("the clip is on the timeline");
    key(&mut state, clip, ClipParam::Opacity, 0, 1.0);
    key(&mut state, clip, ClipParam::Opacity, 24, 1.0);
    state.set_playhead(26);

    assert!(state.press_clip_line(clip, LinePress::Key(24), BAND));
    // Straight down leaves the key's tick, though the playhead is in reach.
    let down = LineTravel {
        magnet: 3,
        ..moved(10.0, 0)
    };
    assert!(state.drag_clip_line(down));
    assert_eq!(keys(&state, clip, ClipParam::Opacity)[1].0, 24);
    assert!(state.snap_guide.is_none());

    let near = LineTravel {
        magnet: 3,
        ..moved(10.0, 1)
    };
    assert!(state.drag_clip_line(near));
    assert_eq!(keys(&state, clip, ClipParam::Opacity)[1].0, 26);
    assert_eq!(state.snap_guide.map(|guide| guide.tick), Some(26));
    assert!(
        state.drag_clip_line(moved(10.0, 1)),
        "no magnet holds the key"
    );
    assert_eq!(keys(&state, clip, ClipParam::Opacity)[1].0, 25);
    assert!(state.snap_guide.is_none());

    // Past the clip's end the key stops on its last tick, which the model keys.
    assert!(state.drag_clip_line(moved(10.0, end * 4)));
    assert_eq!(keys(&state, clip, ClipParam::Opacity)[1].0, end - 1);
    assert!(state.drag_clip_line(near));
    assert!(state.release_clip_line());
    assert!(state.snap_guide.is_none(), "the release drops the guide");
    assert_eq!(keys(&state, clip, ClipParam::Opacity)[1], (26, 0.75));

    // Escape drops the guide with the key it puts back.
    assert!(state.press_clip_line(clip, LinePress::Key(26), BAND));
    let back = LineTravel {
        magnet: 3,
        ..moved(20.0, -1)
    };
    assert!(state.drag_clip_line(back));
    assert_eq!(state.snap_guide.map(|guide| guide.tick), Some(26));
    assert!(state.cancel_clip_line());
    assert!(state.snap_guide.is_none(), "Escape drops the guide");
    assert_eq!(keys(&state, clip, ClipParam::Opacity)[1], (26, 0.75));
}

#[test]
fn a_line_drawing_more_than_one_number_is_not_dragged() {
    let (mut state, clip) = with_clip();
    state
        .apply_command(Command::Edit(EditCommand::SetClipLineParam {
            clip,
            param: Some(ClipParam::Position),
        }))
        .expect("choose Position for the line");
    let line = state
        .ui_clip_ref(clip)
        .and_then(|ui| ui.line.clone())
        .expect("the bar draws Position");
    assert_eq!(line.param, ClipParam::Position);
    assert!(!line.draggable);
    assert!(!state.press_clip_line(clip, LinePress::Line(0), BAND));
    assert!(!state.clip_line_held());

    // The chosen property is the one a drag writes.
    state
        .apply_command(Command::Edit(EditCommand::SetClipLineParam {
            clip,
            param: Some(ClipParam::Rotation),
        }))
        .expect("choose Rotation for the line");
    assert!(state.press_clip_line(clip, LinePress::Line(0), BAND));
    assert!(state.drag_clip_line(LineTravel::down(-10.0)));
    assert!(state.release_clip_line());
    assert!(constant(&state, clip, ClipParam::Rotation) > 0.0);
    assert_eq!(constant(&state, clip, ClipParam::Opacity), 1.0);
}
