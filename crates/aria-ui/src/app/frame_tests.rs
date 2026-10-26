//! Contracts for the framing surface: the crop insets, mirroring, and motion
//! blur.
//!
//! `SetClipCrop` and `SetClipMotionBlur` each carry every field they own, so
//! the class closed here is "editing one edge or one knob silently resets its
//! siblings".

use super::premiere_tests::with_clip;

// --- framing: crop, mirror, motion blur -----------------------------------
//
// `SetClipCrop` carries the whole rect and both mirror flags, so the class
// closed here is "editing one edge silently resets the other three".

#[test]
fn every_crop_edge_moves_alone() {
    use crate::app::clip_frame::{CROP_EDGES, crop_inset};
    let (mut state, _) = with_clip();

    assert!(!CROP_EDGES.is_empty());
    for (id, ..) in CROP_EDGES {
        state
            .set_selected_crop_inset(id, 0.1)
            .unwrap_or_else(|err| panic!("set {id}: {err:?}"));
    }

    // Every edge must still hold its inset after the last edit.
    let ui = state.selected_ui_clip().expect("clip");
    for (id, ..) in CROP_EDGES {
        assert!(
            (crop_inset(&ui.crop, id) - 0.1).abs() < 1e-6,
            "{id} was reset by a later edge edit"
        );
    }

    state.clear_selected_crop().expect("reset");
    let ui = state.selected_ui_clip().expect("clip");
    assert!(ui.crop.is_full(), "reset must restore the whole frame");
}

#[test]
fn a_crop_edge_stops_at_the_frame_instead_of_being_refused() {
    use crate::app::clip_frame::{CROP_EDGES, crop_inset};
    let (mut state, _) = with_clip();
    for (id, ..) in CROP_EDGES {
        // Well past the far edge: the drag must clamp, not error and not
        // produce a degenerate rect the engine would reject.
        state
            .set_selected_crop_inset(id, 5.0)
            .unwrap_or_else(|err| panic!("clamp {id}: {err:?}"));
        let ui = state.selected_ui_clip().expect("clip");
        ui.crop.validate().unwrap_or_else(|err| {
            panic!("{id} produced an invalid rect: {err:?}");
        });
        assert!(crop_inset(&ui.crop, id) < 1.0, "{id} kept nothing");
        state.clear_selected_crop().expect("reset");
    }
}

/// An edge set from the whole frame lands where it is set, and no other edge
/// moves. The class closed: a clamp that keeps the origin a fixed margin inside
/// the frame. With the far edge on the frame's, the margin landed a left or top
/// inset short of its value and moved the far edge in by the margin, which a
/// tolerance of 1e-3 did not see. The values run from the smallest step the
/// Inspector writes (1%) to the widest crop the model keeps, on every edge in
/// `CROP_EDGES`. Not caught: an error under 1e-6 of the frame, or one that
/// shows only with the other edges already moved.
#[test]
fn a_crop_edge_lands_where_it_is_set_and_moves_no_other() {
    use crate::app::clip_frame::{CROP_EDGES, crop_inset};
    let (mut state, _) = with_clip();
    for value in [
        0.01,
        0.1,
        0.25,
        0.5,
        0.75,
        1.0 - aria_models::MIN_CROP_FRACTION,
    ] {
        for (id, ..) in CROP_EDGES {
            state
                .set_selected_crop_inset(id, value)
                .unwrap_or_else(|err| panic!("set {id} to {value}: {err:?}"));
            let crop = state.selected_ui_clip().expect("clip").crop;
            crop.validate()
                .unwrap_or_else(|err| panic!("{id} at {value} is an invalid rect: {err:?}"));
            for (other, ..) in CROP_EDGES {
                let want = if other == id { value } else { 0.0 };
                let got = crop_inset(&crop, other);
                assert!(
                    (got - want).abs() <= 1e-6,
                    "setting {id} to {value} left {other} at {got} instead of {want}"
                );
            }
            state.clear_selected_crop().expect("reset");
        }
    }
}

#[test]
fn an_unknown_crop_edge_is_inert() {
    let (mut state, _) = with_clip();
    state
        .set_selected_crop_inset("crop-nope", 0.5)
        .expect("inert");
    assert!(state.selected_ui_clip().expect("clip").crop.is_full());
}

#[test]
fn mirroring_survives_a_crop_edit_and_the_reverse() {
    use crate::app::clip_frame::crop_inset;
    let (mut state, _) = with_clip();
    state.toggle_selected_flip_h().expect("flip h");
    state.toggle_selected_flip_v().expect("flip v");
    state
        .set_selected_crop_inset("crop-left", 0.2)
        .expect("crop");

    let ui = state.selected_ui_clip().expect("clip");
    assert!(ui.flip_h && ui.flip_v, "a crop edit must keep the mirrors");
    assert!((crop_inset(&ui.crop, "crop-left") - 0.2).abs() < 1e-6);

    state.toggle_selected_flip_h().expect("unflip h");
    let ui = state.selected_ui_clip().expect("clip");
    assert!(!ui.flip_h, "flip must toggle back");
    assert!(ui.flip_v, "one mirror must not clear the other");
    assert!(
        (crop_inset(&ui.crop, "crop-left") - 0.2).abs() < 1e-6,
        "a mirror toggle must keep the crop"
    );

    state.clear_selected_crop().expect("reset");
    let ui = state.selected_ui_clip().expect("clip");
    assert!(!ui.flip_h && !ui.flip_v, "reset clears both mirrors");
}

/// An animated crop holds a rect per frame, so a framing edit reads and lands
/// under the playhead. Every edge in `CROP_EDGES`, both mirrors, the canvas
/// drag, and its commit keep each key already on the crop; a moved edge keys
/// the playhead and a mirror or a still release keys nothing. Each of them
/// used to rebuild the rect from the projection, which holds the clip's first
/// frame, and send it as a constant, so one nudge erased the animation. The
/// clear is the one framing edit that drops the keys. What this does not see:
/// the easing of a key a second edit overwrites, which the model resets to
/// linear.
#[test]
fn every_framing_edit_on_an_animated_crop_keeps_its_keys() {
    use aria_commands::{Command, EditCommand};
    use aria_models::{ClipParam, Easing, ParamValue, RationalTime};

    use crate::app::AppState;
    use crate::app::canvas_overlay::CropHandle;
    use crate::app::clip_frame::{CROP_EDGES, crop_inset};

    let (mut state, _) = with_clip();
    let clip = state.selected_clip().expect("a selected clip");
    let model = state.selected_model().expect("the selected model");
    let start = model.timeline.start.value;
    let mid = start + (model.timeline.end_tick() - start) / 2;
    let rate = state.store.sequence.frame_rate;
    for (tick, inset) in [(start, 0.0_f32), (mid, 0.2)] {
        let side = 1.0 - 2.0 * inset;
        state
            .apply_command(Command::Edit(EditCommand::SetParamKeyframe {
                clip,
                param: ClipParam::Crop,
                at: RationalTime::new(tick, rate),
                value: ParamValue::Rect([inset, inset, side, side]),
                easing: Easing::Linear,
                tangents: None,
            }))
            .expect("a crop key");
    }
    let ticks = |state: &AppState| -> Vec<i64> {
        state
            .keyframes_of(ClipParam::Crop)
            .iter()
            .map(|mark| mark.tick)
            .collect()
    };
    assert_eq!(ticks(&state), [start, mid], "the fixture keyed the crop");
    let here = start + (mid - start) / 2;
    assert!(start < here && here < mid, "the clip is too short to key");
    state.set_playhead(here);
    let (between, ..) = state.selected_framing_here().expect("a framing");
    assert!(
        between.x > 0.0 && between.x < 0.2,
        "the playhead reads {between:?}, not a rect between the keys"
    );

    let curve = state.selected_model().expect("model").crop.clone();
    state.toggle_selected_flip_h().expect("flip h");
    state.toggle_selected_flip_v().expect("flip v");
    let model = state.selected_model().expect("model");
    assert!(model.flip_h && model.flip_v, "a mirror did not land");
    assert_eq!(model.crop, curve, "a mirror rewrote the crop");

    for (id, ..) in CROP_EDGES {
        state.nudge_selected_crop_inset(id, 0.05).expect("an edge");
        assert_eq!(
            ticks(&state),
            [start, here, mid],
            "{id} did not key the playhead and keep the keys around it"
        );
        let (moved, ..) = state.selected_framing_here().expect("a framing");
        assert!(
            (crop_inset(&moved, id) - crop_inset(&between, id) - 0.05).abs() < 1e-4,
            "{id} moved from somewhere other than the rect under the playhead"
        );
        assert!(state.undo(), "{id} left nothing to undo");
        assert_eq!(ticks(&state), [start, mid], "undoing {id} left a key");
    }

    // The canvas draws the rect under the playhead, and a drag starts from it:
    // a release that moved nothing writes nothing.
    assert_eq!(
        state.overlay_crop(),
        Some(between),
        "the canvas draws a crop other than the one under the playhead"
    );
    state.begin_crop_drag(CropHandle::Left);
    assert_eq!(
        state.overlay_crop(),
        Some(between),
        "the drag starts from a crop other than the one under the playhead"
    );
    state.end_overlay_drag().expect("a still release");
    assert_eq!(
        ticks(&state),
        [start, mid],
        "a still release keyed the crop"
    );

    let mut dragged = between;
    dragged.x += 0.05;
    dragged.w -= 0.05;
    state.set_selected_crop(dragged).expect("a canvas commit");
    assert_eq!(
        ticks(&state),
        [start, here, mid],
        "the canvas commit did not key the playhead and keep the keys around it"
    );

    state.clear_selected_crop().expect("a clear");
    assert!(ticks(&state).is_empty(), "the clear kept the crop's keys");
}

#[test]
fn motion_blur_toggles_and_its_knobs_clamp_to_the_model_range() {
    let (mut state, _) = with_clip();
    let ui = state.selected_ui_clip().expect("clip");
    assert!(!ui.motion_blur.enabled, "motion blur is off by default");

    state.toggle_selected_motion_blur().expect("on");
    assert!(state.selected_ui_clip().expect("clip").motion_blur.enabled);

    for _ in 0..80 {
        state.nudge_selected_shutter(15.0).expect("shutter up");
    }
    let blur = state.selected_ui_clip().expect("clip").motion_blur;
    assert_eq!(blur.shutter_deg, 720.0, "shutter must stop at 720 degrees");
    for _ in 0..80 {
        state.nudge_selected_shutter(-15.0).expect("shutter down");
    }
    assert_eq!(
        state
            .selected_ui_clip()
            .expect("clip")
            .motion_blur
            .shutter_deg,
        0.0,
        "shutter must stop at 0 degrees"
    );

    for _ in 0..60 {
        state.nudge_selected_blur_samples(1).expect("samples up");
    }
    assert_eq!(
        state.selected_ui_clip().expect("clip").motion_blur.samples,
        32,
        "samples must stop at 32"
    );
    for _ in 0..60 {
        state.nudge_selected_blur_samples(-1).expect("samples down");
    }
    assert_eq!(
        state.selected_ui_clip().expect("clip").motion_blur.samples,
        2,
        "samples must stop at 2"
    );

    // Enabled state must survive a knob edit: SetClipMotionBlur carries all
    // three fields.
    assert!(state.selected_ui_clip().expect("clip").motion_blur.enabled);
    state.toggle_selected_motion_blur().expect("off");
    assert!(!state.selected_ui_clip().expect("clip").motion_blur.enabled);
}

#[test]
fn framing_commands_are_inert_with_no_selection() {
    let (mut state, _) = with_clip();
    state.clear_selection();
    state
        .set_selected_crop_inset("crop-left", 0.3)
        .expect("inert");
    state.toggle_selected_flip_h().expect("inert");
    state.toggle_selected_flip_v().expect("inert");
    state.clear_selected_crop().expect("inert");
    state.toggle_selected_motion_blur().expect("inert");
    state.nudge_selected_shutter(30.0).expect("inert");
    state.nudge_selected_blur_samples(4).expect("inert");

    let clip = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .next()
        .expect("clip still there");
    assert!(clip.crop.is_full());
    assert!(!clip.flip_h && !clip.flip_v);
    assert!(!clip.motion_blur.enabled);
}
