//! Contracts for the generated property rows.
//!
//! The class closed here is "an inspector knob writes the wrong thing": it
//! flattens a curve, it resets its siblings, or it asks the model for a value
//! outside the range the model enforces. The row list is derived from
//! [`aria_models::clip_params_for`] at run time, so a property the model gains
//! is covered without editing this file, and a property that stops reaching a
//! control fails here.
//!
//! What it does not catch: whether a panel actually draws the rows it is
//! offered. That is the panel's own test.

use aria_models::{ClipParam, LookParam, MaskKind, ParamKind, ParamValue};

use super::premiere_tests::with_clip;

/// Every group the catalog declares, measured rather than listed.
fn declared_groups() -> Vec<String> {
    let mut groups: Vec<String> = aria_models::clip_param_catalog()
        .iter()
        .map(|spec| spec.group.to_string())
        .collect();
    groups.sort();
    groups.dedup();
    groups
}

#[test]
fn a_group_offers_a_row_for_every_scalar_and_vector_property_it_owns() {
    let (mut state, _) = with_clip();
    state
        .set_selected_mask_kind(MaskKind::Circle)
        .expect("mask");

    // The rows the model says are reachable on this clip, by group.
    let clip_id = state.selected_clip().expect("selection");
    let expected: Vec<(String, String, usize)> = {
        let engine = state.engine.lock();
        let clip = engine
            .project()
            .timeline()
            .clip(clip_id)
            .expect("clip")
            .clone();
        aria_models::clip_params_for(&clip)
            .into_iter()
            .filter(|spec| !matches!(spec.kind, ParamKind::Color | ParamKind::Rect))
            .map(|spec| {
                let rows = match spec.kind {
                    ParamKind::Scalar => 1,
                    _ => 2,
                };
                (spec.group.to_string(), spec.id.to_string(), rows)
            })
            .collect()
    };
    assert!(
        !expected.is_empty(),
        "the fixture reaches no animatable property at all"
    );

    for (group, id, rows) in expected {
        let offered = state.param_rows(&group);
        let matching = offered
            .iter()
            .filter(|row| row.id.as_str() == id || row.id.starts_with(&format!("{id}-")))
            .count();
        assert_eq!(
            matching,
            rows,
            "group {group} offered {matching} rows for {id}, expected {rows}: {:?}",
            offered.iter().map(|row| &row.id).collect::<Vec<_>>()
        );
    }
}

#[test]
fn every_declared_group_is_reachable_by_name() {
    // A group name nobody can ask for is a block that cannot be drawn. This does
    // not require the fixture to own the group: it requires the accessor to
    // answer without panicking and to refuse rows it does not own.
    let (state, _) = with_clip();
    for group in declared_groups() {
        let rows = state.param_rows(&group);
        for row in &rows {
            assert!(
                !row.id.is_empty() && !row.label.is_empty(),
                "group {group} offered a nameless row"
            );
        }
    }
    assert!(
        state.param_rows("no-such-group").is_empty(),
        "an unknown group produced rows"
    );
}

#[test]
fn a_row_edit_on_a_constant_property_stays_a_constant() {
    let (mut state, _) = with_clip();
    state
        .set_param_scalar(ClipParam::Opacity, None, 0.4)
        .expect("edit");

    assert!(
        (state.param_scalar(ClipParam::Opacity, None).expect("read") - 0.4).abs() < 1e-4,
        "the edit did not take"
    );
    assert!(
        state.keyframes_of(ClipParam::Opacity).is_empty(),
        "a constant edit invented a keyframe"
    );
}

#[test]
fn a_row_edit_on_an_animated_property_moves_the_keyframe_instead_of_flattening_it() {
    let (mut state, _) = with_clip();
    state
        .set_selected_mask_kind(MaskKind::Circle)
        .expect("mask");
    let feather = ClipParam::Look {
        param: LookParam::MaskFeather,
    };
    state.add_keyframe_at_playhead(feather).expect("keyframe");
    assert_eq!(
        state.keyframes_of(feather).len(),
        1,
        "the fixture is not animated"
    );

    state
        .set_param_scalar(feather, None, 0.75)
        .expect("slider edit");

    assert_eq!(
        state.keyframes_of(feather).len(),
        1,
        "the edit dropped the curve it was supposed to move"
    );
    assert!(
        (state.param_scalar(feather, None).expect("read") - 0.75).abs() < 1e-4,
        "the keyframe did not take the edited value"
    );
}

#[test]
fn a_row_edit_keeps_the_rest_of_its_block() {
    // The knob tables this replaces rebuilt the whole mask from the projection,
    // so one edit reset every sibling. Rotation must survive a feather edit.
    let (mut state, _) = with_clip();
    state
        .set_selected_mask_kind(MaskKind::Circle)
        .expect("mask");
    let rotation = ClipParam::Look {
        param: LookParam::MaskRotation,
    };
    let feather = ClipParam::Look {
        param: LookParam::MaskFeather,
    };
    state
        .set_param_scalar(rotation, None, 30.0)
        .expect("rotation");
    state.set_param_scalar(feather, None, 0.6).expect("feather");

    assert!(
        (state.param_scalar(rotation, None).expect("read") - 30.0).abs() < 1e-3,
        "the feather edit reset the rotation"
    );
}

#[test]
fn a_row_edit_clamps_to_the_range_the_model_declares() {
    let (mut state, _) = with_clip();
    state
        .set_param_scalar(ClipParam::Opacity, None, 4.0)
        .expect("over");
    assert!(
        (state.param_scalar(ClipParam::Opacity, None).expect("read") - 1.0).abs() < 1e-4,
        "opacity accepted a value above its declared maximum"
    );
    state
        .set_param_scalar(ClipParam::Opacity, None, -2.0)
        .expect("under");
    assert!(
        state.param_scalar(ClipParam::Opacity, None).expect("read") <= 1e-4,
        "opacity accepted a value below its declared minimum"
    );
}

#[test]
fn a_vector_row_edits_one_component_and_keeps_the_other() {
    let (mut state, _) = with_clip();
    state
        .set_param_scalar(ClipParam::Position, Some(0), 0.25)
        .expect("x");
    state
        .set_param_scalar(ClipParam::Position, Some(1), -0.5)
        .expect("y");
    assert!(
        (state.param_scalar(ClipParam::Position, Some(0)).expect("x") - 0.25).abs() < 1e-4,
        "the Y edit overwrote X"
    );
    assert!(
        (state.param_scalar(ClipParam::Position, Some(1)).expect("y") + 0.5).abs() < 1e-4,
        "the Y edit did not take"
    );
}

#[test]
fn a_component_that_does_not_fit_the_property_is_inert() {
    let (mut state, _) = with_clip();
    let before = state.history_depth();
    // A scalar has no component; a vector has no scalar row.
    state
        .set_param_scalar(ClipParam::Opacity, Some(0), 0.5)
        .expect("inert");
    state
        .set_param_scalar(ClipParam::Position, None, 0.5)
        .expect("inert");
    assert_eq!(
        state.history_depth(),
        before,
        "a mismatched component reached the model"
    );
}

#[test]
fn a_nudge_moves_by_its_step_and_stops_at_the_bound() {
    let (mut state, _) = with_clip();
    state
        .set_param_scalar(ClipParam::Opacity, None, 0.5)
        .expect("start");
    state
        .nudge_param(ClipParam::Opacity, None, 0.1)
        .expect("up");
    assert!(
        (state.param_scalar(ClipParam::Opacity, None).expect("read") - 0.6).abs() < 1e-4,
        "the nudge did not move by its step"
    );
    for _ in 0..10 {
        state
            .nudge_param(ClipParam::Opacity, None, 0.1)
            .expect("up");
    }
    assert!(
        (state.param_scalar(ClipParam::Opacity, None).expect("read") - 1.0).abs() < 1e-4,
        "the nudge walked past the declared maximum"
    );
}

/// **An audio-lane clip is offered no visual property.**
///
/// The lane decides this, not the clip: a media clip carries a transform, and on
/// a sound lane there is nothing for it to move, so the engine refuses the write
/// and a row offering it would be dead. Both sides are taken from a fixture that
/// is what it claims to be. This test used to branch on whatever lane the default
/// fixture happened to land on, which is a video lane, so the audio side was
/// never reached and dropping the refusal altogether kept it green.
#[test]
fn an_audio_lane_clip_is_offered_no_visual_group() {
    let (visual, _) = with_clip();
    assert_eq!(
        visual.selected_ui_clip().expect("clip").kind,
        crate::types::UiClipKind::Media,
        "the default fixture no longer lands on a picture lane"
    );
    assert!(
        !visual.param_rows("transform").is_empty(),
        "a visual clip was offered no transform rows"
    );

    let (audio, _) = super::fixtures::with_audio();
    assert_eq!(
        audio.selected_ui_clip().expect("clip").kind,
        crate::types::UiClipKind::Audio,
        "the audio fixture no longer lands on a sound lane"
    );
    assert!(
        audio.param_rows("transform").is_empty(),
        "an audio-lane clip was offered transform rows"
    );
    assert!(
        !audio.param_rows("audio").is_empty(),
        "an audio-lane clip was offered no audio rows"
    );
}

#[test]
fn a_row_reports_the_value_the_model_holds() {
    let (mut state, _) = with_clip();
    state
        .set_param_scalar(ClipParam::Opacity, None, 0.3)
        .expect("edit");
    let row = state
        .param_rows("transform")
        .into_iter()
        .find(|row| row.id == "opacity")
        .expect("opacity row");
    assert!((row.value - 0.3).abs() < 1e-4, "the row lagged the model");
    assert_eq!(
        row.range,
        aria_models::ParamRange::between(0.0, 1.0),
        "the row lost its bounds"
    );
    assert!(!row.animated, "a constant reported as animated");

    let clip_id = state.selected_clip().expect("selection");
    let sampled = {
        let engine = state.engine.lock();
        let clip = engine.project().timeline().clip(clip_id).expect("clip");
        aria_models::clip_params_for(clip)
            .into_iter()
            .find(|spec| spec.param == ClipParam::Opacity)
            .expect("spec")
            .sample(clip, 0)
    };
    assert_eq!(
        sampled,
        Some(ParamValue::Scalar(0.3)),
        "the row and the model disagree"
    );
}

/// One row of one group, as the panel draws it.
fn row_of(state: &super::AppState, group: &str, param: ClipParam) -> super::clip_params::ParamRow {
    state
        .param_rows(group)
        .into_iter()
        .find(|row| row.param == param)
        .unwrap_or_else(|| panic!("the {group} group offers no row for {param:?}"))
}

/// **A row says whether a key sits under the playhead.**
///
/// The stopwatch draws three states — constant, animated, pinned — and the third
/// is what tells the operator that pressing it removes a key rather than writing
/// one. It used to be answered by asking the state again per row while the group
/// was being built; it travels on the row now, so this holds the three apart: an
/// unkeyed property is neither, a keyed one with the playhead on the key is both,
/// and one tick off the key it is animated and not pinned.
#[test]
fn a_row_reports_whether_a_keyframe_sits_under_the_playhead() {
    let (mut state, _) = with_clip();
    state
        .set_selected_mask_kind(MaskKind::Circle)
        .expect("mask");
    let feather = ClipParam::Look {
        param: LookParam::MaskFeather,
    };
    let at = state.playhead.value;

    let constant = row_of(&state, "mask", feather);
    assert!(
        !constant.animated && !constant.at_playhead,
        "an unkeyed row reports itself animated or pinned"
    );

    state.add_keyframe_at_playhead(feather).expect("keyframe");
    let pinned = row_of(&state, "mask", feather);
    assert!(
        pinned.animated && pinned.at_playhead,
        "the row does not report the key under the playhead"
    );

    state.set_playhead(at + 1);
    let off = row_of(&state, "mask", feather);
    assert!(
        off.animated && !off.at_playhead,
        "the row reports a key one tick off the playhead as pinned"
    );
}

/// **A component row's text is built where the property list is projected.**
///
/// A `Vec2` property is drawn as two steppers, so its rows need an id and a
/// label each. Those were two `format!` calls per row per frame — the group is
/// rebuilt every frame the block is open — and the row then held both as owned
/// strings for the draw to copy again. They are built once per projection now
/// and the row carries shared text. The axis set is read out of the catalog at
/// run time: a property the model gains is covered here without editing this
/// file.
#[test]
fn a_vector_property_carries_its_component_row_text() {
    let (mut state, _) = with_clip();
    state
        .set_selected_mask_kind(MaskKind::Circle)
        .expect("mask");
    let mut vectors = 0usize;
    let mut scalars = 0usize;
    for row in state.key_params().iter() {
        match row.spec.kind {
            ParamKind::Vec2 => {
                vectors += 1;
                let axes: Vec<_> = row.axes().iter().collect();
                assert_eq!(
                    axes.len(),
                    2,
                    "{} is a vector property and is drawn as two rows",
                    row.id
                );
                assert_eq!(axes[0].id.as_str(), format!("{}-x", row.id));
                assert_eq!(axes[1].id.as_str(), format!("{}-y", row.id));
                assert_eq!(axes[0].label.as_str(), format!("{} X", row.label));
                assert_eq!(axes[1].label.as_str(), format!("{} Y", row.label));
            }
            // A colour needs a picker and a rectangle needs handles, so neither
            // is drawn as a stepper either.
            ParamKind::Scalar | ParamKind::Color | ParamKind::Rect => {
                scalars += 1;
                assert!(
                    row.axes().is_empty(),
                    "{} is a single value and carries component text anyway",
                    row.id
                );
            }
        }
    }
    assert!(
        vectors > 0 && scalars > 0,
        "the fixture reaches {vectors} vector and {scalars} single-value properties, \
         so one side of this is untested"
    );

    // What the panel draws is that same text, not a copy of it.
    let position = ClipParam::Position;
    let projected = state
        .key_param(position)
        .expect("the clip can animate its position");
    let drawn: Vec<_> = state
        .param_rows("transform")
        .into_iter()
        .filter(|row| row.param == position)
        .map(|row| (row.id, row.label, row.component))
        .collect();
    assert_eq!(
        drawn,
        vec![
            (
                projected.axes()[0].id.clone(),
                projected.axes()[0].label.clone(),
                Some(0u8)
            ),
            (
                projected.axes()[1].id.clone(),
                projected.axes()[1].label.clone(),
                Some(1u8)
            ),
        ],
        "the drawn rows do not carry the projected component text"
    );
}
