//! Animation menu verbs: Add Keyframe, Time-Reverse Keyframes, Add
//! Expression, and Keyframe Velocity.
//!
//! # Why
//!
//! Each verb writes keyframes the operator did not place one by one, so a
//! wrong tick, a value carried to the wrong time, a handle left on the wrong
//! side, or an edit split across undo steps is invisible until playback. These
//! tests read the stored curve back after each verb and after its undo.
//!
//! Not caught here: the menu rows reaching these verbs (the menu census drives
//! every row) and the dialog's drawing.

use aria_models::{ClipId, ClipParam, Easing, Param, ParamValue, TemporalEase};

use crate::app::animation_verbs::VelocitySide;
use crate::app::premiere_tests::with_clip;
use crate::app::{AppState, DialogKind};

const OPACITY: ClipParam = ClipParam::Opacity;

/// The stored opacity keyframes of the selected clip: absolute tick, value,
/// easing.
fn keys(state: &AppState) -> Vec<(i64, f32, Easing)> {
    let model = state.selected_model().expect("a selected clip");
    let row = state.key_param(OPACITY).expect("an opacity row");
    match row.spec.curve(model) {
        Some(Param::Keyframed { keyframes }) => keyframes
            .iter()
            .map(|key| {
                let ParamValue::Scalar(value) = key.value else {
                    panic!("opacity is scalar");
                };
                (model.param_timeline_tick(key.tick), value, key.easing)
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn ticks_and_values(state: &AppState) -> Vec<(i64, f32)> {
    keys(state).into_iter().map(|(t, v, _)| (t, v)).collect()
}

/// Opacity keyframes 0 at 0, 0.5 at 4, 1 at 20.
fn keyed() -> (AppState, ClipId) {
    let (mut state, clip) = with_clip();
    for (tick, value) in [(0, 0.0), (4, 0.5), (20, 1.0)] {
        state.set_playhead(tick);
        state
            .set_keyframe_scalar(OPACITY, tick, value)
            .expect("a keyframe");
    }
    (state, clip)
}

#[test]
fn time_reverse_mirrors_values_handles_and_segments_in_one_step() {
    let (mut state, clip) = keyed();
    // An asymmetric middle keyframe (an incoming handle only) and a Hold on
    // the first segment, so a swap that went the wrong way shows.
    state.select_timeline_keyframe(clip, OPACITY, 4, false);
    state.easy_ease_in_selected_or_playhead().expect("ease in");
    state.select_timeline_keyframe(clip, OPACITY, 0, false);
    state.hold_selected_or_playhead().expect("hold");
    let before = keys(&state);

    state.marquee_select_keyframes(clip, OPACITY, 0, 20, false);
    assert_eq!(state.time_reverse_selected_keyframes().expect("reverse"), 3);
    assert_eq!(
        ticks_and_values(&state),
        vec![(0, 1.0), (16, 0.5), (20, 0.0)]
    );
    let after = keys(&state);
    let middle = after[1].2;
    assert_eq!(middle.in_ease(), None, "the incoming handle went out");
    assert_eq!(
        middle.out_ease().map(|ease| ease.influence),
        Some(TemporalEase::EASY_EASE.influence)
    );
    assert!(
        middle.is_hold(),
        "the held first segment now arrives at the last keyframe, leaving 16"
    );
    assert!(
        !after[0].2.is_hold(),
        "the first keyframe leaves a linear run"
    );
    let selected: Vec<i64> = state
        .selected_timeline_keyframes()
        .iter()
        .map(|key| key.tick)
        .collect();
    assert_eq!(selected, vec![0, 16, 20], "the selection follows the keys");

    assert!(state.undo(), "one step");
    assert_eq!(keys(&state), before);

    // Reversing twice returns the original curve.
    state.marquee_select_keyframes(clip, OPACITY, 0, 20, false);
    state.time_reverse_selected_keyframes().expect("reverse");
    state
        .time_reverse_selected_keyframes()
        .expect("reverse back");
    assert_eq!(
        ticks_and_values(&state),
        vec![(0, 0.0), (4, 0.5), (20, 1.0)]
    );
}

#[test]
fn time_reverse_needs_two_keyframes_on_one_property() {
    let (mut state, clip) = keyed();
    state.select_timeline_keyframe(clip, OPACITY, 4, false);
    assert_eq!(state.time_reverse_selected_keyframes().expect("inert"), 0);
    assert_eq!(
        ticks_and_values(&state),
        vec![(0, 0.0), (4, 0.5), (20, 1.0)]
    );
    assert!(
        state
            .status_message
            .as_deref()
            .is_some_and(|text| text.contains("two or more")),
        "the status line states what is missing"
    );
}

#[test]
fn time_reverse_moves_only_the_selected_span() {
    let (mut state, clip) = keyed();
    state.marquee_select_keyframes(clip, OPACITY, 4, 20, false);
    state.time_reverse_selected_keyframes().expect("reverse");
    assert_eq!(
        ticks_and_values(&state),
        vec![(0, 0.0), (4, 1.0), (20, 0.5)],
        "the keyframe at 0 is outside the selection and stays"
    );
}

#[test]
fn add_keyframe_pins_each_selected_row_at_the_playhead() {
    let (mut state, clip) = keyed();
    state.set_playhead(12);
    assert_eq!(
        state.add_keyframes_on_selected_properties().expect("inert"),
        0,
        "no row selected"
    );

    state.select_timeline_property(clip, OPACITY, false);
    let expected = 0.5 + 0.5 * (12.0 - 4.0) / 16.0;
    assert_eq!(
        state.add_keyframes_on_selected_properties().expect("add"),
        1
    );
    let added = keys(&state)
        .into_iter()
        .find(|(tick, ..)| *tick == 12)
        .expect("a keyframe at the playhead");
    assert!(
        (added.1 - expected).abs() < 1e-4,
        "it holds the value there"
    );

    assert!(state.undo());
    assert!(keys(&state).iter().all(|(tick, ..)| *tick != 12));

    // On a keyframe already there, its shape survives.
    state.set_playhead(4);
    state.select_timeline_keyframe(clip, OPACITY, 4, false);
    state.hold_selected_or_playhead().expect("hold");
    state.select_timeline_property(clip, OPACITY, false);
    state
        .add_keyframes_on_selected_properties()
        .expect("re-add");
    assert!(keys(&state)[1].2.is_hold());
}

#[test]
fn keyframe_velocity_writes_both_handles_in_one_step() {
    let (mut state, clip) = keyed();
    state.select_timeline_keyframe(clip, OPACITY, 4, false);
    state.open_keyframe_velocity();
    assert_eq!(state.dialog, DialogKind::KeyframeVelocity);
    let fps = state.keyframe_velocity.ticks_per_second;

    assert!(
        state
            .set_keyframe_velocity_influence(VelocitySide::Incoming, 0.0)
            .is_err(),
        "zero influence is refused"
    );
    state.apply_keyframe_velocity();
    assert_eq!(
        state.dialog,
        DialogKind::KeyframeVelocity,
        "an error keeps the dialog up and writes nothing"
    );

    state
        .set_keyframe_velocity_influence(VelocitySide::Incoming, 50.0)
        .expect("in influence");
    state
        .set_keyframe_velocity_speed(VelocitySide::Outgoing, 12.0)
        .expect("out speed");
    state.apply_keyframe_velocity();
    assert_eq!(state.dialog, DialogKind::None);
    let easing = keys(&state)[1].2;
    let incoming = easing.in_ease().expect("an incoming handle");
    let outgoing = easing.out_ease().expect("an outgoing handle");
    assert!((incoming.influence - 0.5).abs() < 1e-5);
    assert!((outgoing.speed - 12.0 / fps).abs() < 1e-5);

    assert!(state.undo(), "one step");
    assert_eq!(keys(&state)[1].2, Easing::Linear);
}

#[test]
fn continuous_keeps_one_speed_on_both_sides() {
    let (mut state, clip) = keyed();
    state.select_timeline_keyframe(clip, OPACITY, 4, false);
    state.open_keyframe_velocity();
    state
        .set_keyframe_velocity_speed(VelocitySide::Incoming, 6.0)
        .expect("in speed");
    if state.keyframe_velocity.continuous {
        state.toggle_keyframe_velocity_continuous();
    }
    state
        .set_keyframe_velocity_speed(VelocitySide::Outgoing, 2.0)
        .expect("out speed");
    state.toggle_keyframe_velocity_continuous();
    let form = &state.keyframe_velocity;
    assert_eq!(form.incoming.speed, form.outgoing.speed, "on: one speed");
    state
        .set_keyframe_velocity_speed(VelocitySide::Outgoing, 3.0)
        .expect("out speed");
    let form = &state.keyframe_velocity;
    assert!((form.speed_per_second(VelocitySide::Incoming) - 3.0).abs() < 1e-4);
}

#[test]
fn keyframe_velocity_without_a_keyframe_opens_nothing() {
    let (mut state, _) = with_clip();
    state.set_playhead(3);
    state.open_keyframe_velocity();
    assert_eq!(state.dialog, DialogKind::None);
}

#[test]
fn add_expression_adds_a_driver_on_the_selected_row_and_reopens_it() {
    let (mut state, clip) = with_clip();
    state
        .ctx
        .mount(&aria_plugin::ExpressionsPlugin)
        .expect("mount Expressions");
    state.add_expression_to_selected_property().expect("inert");
    assert!(state.driver_draft().is_none(), "no row selected");

    state.select_timeline_property(clip, ClipParam::Rotation, false);
    state
        .add_expression_to_selected_property()
        .expect("add a driver");
    assert!(state.selected_driver(ClipParam::Rotation).is_some());
    assert!(state.driver_draft().is_some(), "its source opens");

    state.escape_driver_draft();
    state
        .add_expression_to_selected_property()
        .expect("open the driver");
    assert!(
        state.selected_driver(ClipParam::Rotation).is_some(),
        "a second press opens the driver rather than removing it"
    );
    assert!(state.driver_draft().is_some());
}
