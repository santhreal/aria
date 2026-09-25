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
            (crop_inset(&ui.crop, id) - 0.1).abs() < 1e-3,
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
    assert!((crop_inset(&ui.crop, "crop-left") - 0.2).abs() < 1e-3);

    state.toggle_selected_flip_h().expect("unflip h");
    let ui = state.selected_ui_clip().expect("clip");
    assert!(!ui.flip_h, "flip must toggle back");
    assert!(ui.flip_v, "one mirror must not clear the other");
    assert!(
        (crop_inset(&ui.crop, "crop-left") - 0.2).abs() < 1e-3,
        "a mirror toggle must keep the crop"
    );

    state.clear_selected_crop().expect("reset");
    let ui = state.selected_ui_clip().expect("clip");
    assert!(!ui.flip_h && !ui.flip_v, "reset clears both mirrors");
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
