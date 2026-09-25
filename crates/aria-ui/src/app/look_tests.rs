//! Contracts for the look and grade surface: the eleven colour adjustments,
//! blend mode, stabilization, denoise, and pitch lock.
//!
//! These close the class "a one-shot command that carries every field resets
//! the fields the caller did not touch", plus "a picker offers an action the
//! command layer refuses".

use super::premiere_tests::with_clip;

// --- look and grade ------------------------------------------------------
//
// Closes "a one-shot command that carries every slider resets the ones the
// caller did not touch", plus the toggles that had no UI call site.

#[test]
fn every_grade_slider_moves_independently() {
    let (mut state, _) = with_clip();

    // Derived from the property catalog at run time, so a new adjustment fails
    // here until it is covered rather than silently going unwired.
    let rows = state.param_rows("grade");
    assert!(!rows.is_empty(), "the grade block offered no rows");
    for row in &rows {
        state
            .set_param_scalar(row.param, row.component, 0.5)
            .unwrap_or_else(|err| panic!("set {}: {err:?}", row.id));
        let now = state.param_scalar(row.param, row.component).expect("read");
        assert!(
            (now - 0.5).abs() < 1e-4,
            "{} did not take the value it was set to",
            row.id
        );
    }

    // Every slider must still hold its value: `SetClipAdjustments` carries all
    // of them, so a per-slider edit that rebuilt the block would show up as
    // earlier sliders back at neutral.
    for row in &rows {
        let now = state.param_scalar(row.param, row.component).expect("read");
        assert!(
            (now - 0.5).abs() < 1e-4,
            "{} was reset by a later slider edit",
            row.id
        );
    }

    state.clear_selected_adjustments().expect("clear");
    for row in &rows {
        let now = state.param_scalar(row.param, row.component).expect("read");
        let neutral = if row.id.contains("gamma") || row.id.contains("gain") {
            1.0
        } else {
            0.0
        };
        assert!(
            (now - neutral).abs() < 1e-6,
            "{} survived the reset (expected {neutral}, got {now})",
            row.id
        );
    }
}

#[test]
fn a_grade_slider_edit_keeps_its_keyframes() {
    // The old per-slider setter rebuilt `ColorAdjustments` from the projection,
    // which turned every animated adjustment into a constant on the next drag.
    let (mut state, _) = with_clip();
    let contrast = state
        .param_rows("grade")
        .into_iter()
        .find(|row| row.id == "adjust-contrast")
        .expect("a contrast row");
    state
        .add_keyframe_at_playhead(contrast.param)
        .expect("keyframe");
    assert_eq!(state.keyframes_of(contrast.param).len(), 1);

    state
        .set_param_scalar(contrast.param, contrast.component, 0.4)
        .expect("slider edit");

    assert_eq!(
        state.keyframes_of(contrast.param).len(),
        1,
        "the slider flattened the curve it was supposed to move"
    );
    assert!(
        (state
            .param_scalar(contrast.param, contrast.component)
            .expect("read")
            - 0.4)
            .abs()
            < 1e-4
    );
}

#[test]
fn grade_sliders_clamp_to_their_validated_range() {
    let (mut state, _) = with_clip();
    let rows = state.param_rows("grade");
    assert!(!rows.is_empty());
    for row in rows {
        state
            .set_param_scalar(row.param, row.component, 10.0)
            .expect("high");
        let high = state.param_scalar(row.param, row.component).expect("read");
        let max = row.range.max.expect("an adjustment declares a maximum");
        assert!(
            high <= max + 1e-6,
            "{} exceeded its maximum of {max}",
            row.id
        );
        state
            .set_param_scalar(row.param, row.component, -10.0)
            .expect("low");
        let low = state.param_scalar(row.param, row.component).expect("read");
        let min = row.range.min.expect("an adjustment declares a minimum");
        assert!(
            low >= min - 1e-6,
            "{} went below its minimum of {min}",
            row.id
        );
    }
}

#[test]
fn nudging_a_slider_accumulates_from_the_current_value() {
    use aria_models::{ClipParam, LookParam};
    let contrast = ClipParam::Look {
        param: LookParam::AdjustContrast,
    };
    let (mut state, _) = with_clip();
    state.set_param_scalar(contrast, None, 0.0).expect("seed");
    for _ in 0..4 {
        state.nudge_param(contrast, None, 0.1).expect("nudge");
    }
    assert!(
        (state.param_scalar(contrast, None).expect("read") - 0.4).abs() < 1e-4,
        "nudges must accumulate, not overwrite"
    );
}

#[test]
fn a_property_no_row_offers_is_inert() {
    use aria_models::{ClipParam, TextParam};
    let (mut state, _) = with_clip();
    let before = state.history_depth();
    // A text property on a media clip: the model has the variant, this clip has
    // no route for it, so nothing is written and nothing errors.
    state
        .set_param_scalar(
            ClipParam::Text {
                param: TextParam::Size,
            },
            None,
            48.0,
        )
        .expect("inert");
    assert_eq!(state.history_depth(), before);
    // Nothing moved on the grade either — read through the axis the inspector
    // reads, which is where a stray write would show up.
    for slider in aria_models::ColorAdjustments::SLIDERS {
        let axis = ClipParam::Look { param: slider.axis };
        assert_eq!(
            state.param_scalar(axis, None).expect("read"),
            slider.neutral,
            "an unreachable property moved {}",
            slider.name
        );
    }
}

#[test]
fn every_blend_mode_round_trips_through_the_projection() {
    use aria_models::BlendMode;
    let (mut state, _) = with_clip();
    assert_eq!(
        state.selected_ui_clip().expect("clip").blend_mode,
        BlendMode::Normal,
        "Normal is the default"
    );
    // Enumerated from the model, so a new blend mode is covered automatically.
    for mode in BlendMode::ALL {
        state
            .set_selected_blend_mode(*mode)
            .unwrap_or_else(|err| panic!("set {:?}: {err:?}", mode));
        assert_eq!(
            state.selected_ui_clip().expect("clip").blend_mode,
            *mode,
            "{mode:?} did not reach the projection"
        );
    }
}

/// The fixture is a still, so the engine refuses stabilization on it. The
/// contract under test is that the UI honours the projected capability instead
/// of surfacing a picker the command layer would reject.
#[test]
fn stabilize_follows_the_projected_capability() {
    use aria_models::StabilizeLevel;
    let (mut state, _) = with_clip();
    let allowed = state.selected_can_stabilize();
    assert_eq!(state.selected_ui_clip().expect("clip").stabilize, None);

    for level in StabilizeLevel::ALL {
        state
            .set_selected_stabilize(Some(level))
            .unwrap_or_else(|err| panic!("set {level:?}: {err:?}"));
        let seen = state.selected_ui_clip().expect("clip").stabilize;
        if allowed {
            assert_eq!(seen, Some(level), "{level:?} did not reach the projection");
        } else {
            assert_eq!(
                seen, None,
                "a clip that cannot be stabilized must stay unstabilized"
            );
        }
    }

    state.set_selected_stabilize(None).expect("clear");
    assert_eq!(
        state.selected_ui_clip().expect("clip").stabilize,
        None,
        "clearing must return to Off, not the last level"
    );
}

#[test]
fn denoise_and_pitch_lock_toggle_and_are_independent() {
    let (mut state, _) = with_clip();
    let ui = state.selected_ui_clip().expect("clip");
    assert!(ui.preserve_pitch, "pitch is locked by default");

    state.toggle_selected_denoise().expect("denoise");
    let ui = state.selected_ui_clip().expect("clip");
    let denoise_on = ui.denoise;
    assert!(ui.preserve_pitch, "denoise must not touch pitch lock");

    state.toggle_selected_pitch_lock().expect("unlock");
    let ui = state.selected_ui_clip().expect("clip");
    assert!(!ui.preserve_pitch, "pitch lock must toggle off");
    assert_eq!(ui.denoise, denoise_on, "pitch lock must not touch denoise");

    state.toggle_selected_pitch_lock().expect("relock");
    assert!(state.selected_ui_clip().expect("clip").preserve_pitch);
    state.toggle_selected_denoise().expect("denoise off");
    assert_ne!(
        state.selected_ui_clip().expect("clip").denoise,
        denoise_on,
        "denoise must toggle back"
    );
}

#[test]
fn look_commands_are_inert_with_no_selection() {
    use aria_models::{BlendMode, StabilizeLevel};
    let (mut state, _) = with_clip();
    state.clear_selection();
    state
        .set_selected_blend_mode(BlendMode::Multiply)
        .expect("inert");
    state
        .set_selected_stabilize(Some(StabilizeLevel::Smooth))
        .expect("inert");
    state.toggle_selected_denoise().expect("inert");
    state.toggle_selected_pitch_lock().expect("inert");
    state
        .set_param_scalar(
            aria_models::ClipParam::Look {
                param: aria_models::LookParam::AdjustContrast,
            },
            None,
            0.5,
        )
        .expect("inert");
    state.clear_selected_adjustments().expect("inert");
    // Nothing selected means nothing changed.
    let clip = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .next()
        .expect("clip still there");
    assert_eq!(clip.blend_mode, BlendMode::Normal);
    assert_eq!(clip.stabilize, None);
}

#[test]
fn lift_gamma_gain_channels_animate_independently() {
    use aria_models::{ClipParam, LookParam};
    let (mut state, _) = with_clip();

    let lift_r = ClipParam::Look {
        param: LookParam::AdjustLiftR,
    };
    let gamma_g = ClipParam::Look {
        param: LookParam::AdjustGammaG,
    };
    let gain_master = ClipParam::Look {
        param: LookParam::AdjustGainMaster,
    };

    state
        .set_param_scalar(lift_r, None, 0.2)
        .expect("set lift_r");
    state
        .set_param_scalar(gamma_g, None, 1.4)
        .expect("set gamma_g");
    state
        .set_param_scalar(gain_master, None, 2.1)
        .expect("set gain_master");

    assert!(
        (state.param_scalar(lift_r, None).expect("read") - 0.2).abs() < 1e-4,
        "lift_r mismatch"
    );
    assert!(
        (state.param_scalar(gamma_g, None).expect("read") - 1.4).abs() < 1e-4,
        "gamma_g mismatch"
    );
    assert!(
        (state.param_scalar(gain_master, None).expect("read") - 2.1).abs() < 1e-4,
        "gain_master mismatch"
    );
}
