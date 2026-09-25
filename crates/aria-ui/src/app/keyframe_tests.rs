//! Contracts for keyframes on the animatable clip properties.
//!
//! The class closed here is "a keyframe is written at the wrong tick, with the
//! wrong value, or on a property the lane refuses". Properties are enumerated
//! from `aria_models::clip_param_catalog()` through `state.key_params()` at run
//! time, so a new animatable property is exercised by these tests the moment the
//! model declares it — there is no table here to fall out of step.

use aria_models::{ClipParam, PiecewiseEasingPreset};

use super::premiere_tests::with_clip;
use crate::app::AppState;
use crate::app::clip_keys::{PIECEWISE_PRESETS, easing_presets};

/// The playhead placed inside the selected clip, plus that clip's extent.
fn inside(state: &mut AppState) -> (i64, i64) {
    let ui = state.selected_ui_clip().expect("a selected clip");
    let (start, end) = (ui.timeline_start_ticks, ui.timeline_end_ticks);
    state.set_playhead(start + (end - start) / 2);
    (start, end)
}

/// **The property rows are one list per change, not one per read.**
///
/// Two reads inside one frame must be the same allocation; a scrub must keep it,
/// because the rows are the clip's own properties and no instant moves them; and
/// an edit must replace it, because a new keyframe is a new row. Written against
/// pointer identity rather than contents: an equal list rebuilt per read is
/// exactly the cost this removes, and comparing contents cannot see the
/// difference.
#[test]
fn the_property_rows_are_one_list_per_change() {
    let (mut state, _) = with_clip();
    inside(&mut state);
    state.sync_from_engine();
    let first = state.key_params();
    assert!(!first.is_empty(), "a media clip must animate something");
    assert!(
        std::sync::Arc::ptr_eq(&first, &state.key_params()),
        "two reads of the property rows built two lists"
    );

    // The poll a scrub frame makes, not just the tick: a playhead-only poll
    // re-samples what is sampled at the instant and must keep the rest.
    state.set_playhead(state.playhead.value + 1);
    state.sync_from_engine();
    assert!(
        std::sync::Arc::ptr_eq(&first, &state.key_params()),
        "a scrub rebuilt the property rows, which no instant can change"
    );

    state
        .add_keyframe_at_playhead(ClipParam::Opacity)
        .expect("keyframe");
    let after = state.key_params();
    assert!(
        !std::sync::Arc::ptr_eq(&first, &after),
        "an edit left the old property rows in place"
    );
    assert!(
        after
            .iter()
            .any(|row| row.param() == ClipParam::Opacity && row.marks.len() == 1),
        "the re-projected rows do not carry the keyframe that was just written"
    );
}

#[test]
fn every_offered_property_writes_and_removes_a_keyframe_at_the_playhead() {
    let (mut state, _) = with_clip();
    let (start, _) = inside(&mut state);
    let at = state.playhead.value;
    let params = state.key_params();
    assert!(!params.is_empty(), "a media clip must animate something");

    for row in params.iter() {
        let (id, label, param) = (&row.id, &row.label, row.param());
        assert!(
            !state.has_keyframe_at_playhead(param),
            "{id} started with a keyframe"
        );
        state
            .add_keyframe_at_playhead(param)
            .unwrap_or_else(|err| panic!("{label}: {err:?}"));
        assert!(
            state.has_keyframe_at_playhead(param),
            "{label} did not write a keyframe"
        );

        // Absolute in, absolute out: the stored tick is clip-local, so a leaked
        // convention would show up as a keyframe at `at - start`.
        let marks = state.keyframes_of(param);
        assert_eq!(marks.len(), 1, "{label} wrote more than one keyframe");
        assert_eq!(marks[0].tick, at, "{label} reported a clip-local tick");
        assert_ne!(start, at, "the fixture must not start under the playhead");

        // The projected union must see it too, in clip-local ticks.
        let ui = state.selected_ui_clip().expect("clip");
        assert!(
            ui.keyframe_ticks.contains(&(at - start)),
            "{label} is missing from the projected strip"
        );

        state
            .remove_keyframe_at_playhead(param)
            .unwrap_or_else(|err| panic!("remove {label}: {err:?}"));
        assert!(
            !state.has_keyframe_at_playhead(param),
            "{label} kept its keyframe"
        );
    }
}

#[test]
fn a_keyframe_pins_the_value_that_is_already_on_screen() {
    let (mut state, _) = with_clip();
    inside(&mut state);

    let before = state.selected_ui_clip().expect("clip").transform;
    state
        .add_keyframe_at_playhead(ClipParam::Opacity)
        .expect("key opacity");
    let after = state.selected_ui_clip().expect("clip").transform;
    assert!(
        (before.opacity - after.opacity).abs() < 1e-4,
        "writing a keyframe changed the value it pinned"
    );

    // A second property must not disturb the first.
    state
        .add_keyframe_at_playhead(ClipParam::Rotation)
        .expect("key rotation");
    assert_eq!(state.keyframes_of(ClipParam::Opacity).len(), 1);
    assert_eq!(state.keyframes_of(ClipParam::Rotation).len(), 1);
}

#[test]
fn a_keyframe_off_the_clip_is_inert_rather_than_an_error() {
    let (mut state, _) = with_clip();
    let ui = state.selected_ui_clip().expect("clip");
    let end = ui.timeline_end_ticks;

    // Past the clip's out point the model refuses the position, so every path
    // here must be a no-op instead of a reported failure.
    state.set_playhead(end + 100);
    let params = state.key_params();
    for row in params.iter() {
        let (label, param) = (&row.label, row.param());
        state
            .add_keyframe_at_playhead(param)
            .unwrap_or_else(|err| panic!("{label}: {err:?}"));
        assert!(
            state.keyframes_of(param).is_empty(),
            "{label} wrote a keyframe outside the clip"
        );
        state.remove_keyframe_at_playhead(param).expect("inert");
        state.flatten_param(param).expect("inert");
        state
            .set_keyframe_easing_preset(param, &easing_presets()[0].id)
            .expect("inert");
        state
            .apply_piecewise_preset(param, PiecewiseEasingPreset::BounceOut)
            .expect("inert");
    }

    // And with nothing selected.
    state.clear_selection();
    assert!(state.key_params().is_empty());
    state
        .add_keyframe_at_playhead(ClipParam::Opacity)
        .expect("inert");
}

#[test]
fn every_named_easing_preset_lands_on_the_keyframe_under_the_playhead() {
    let (mut state, _) = with_clip();
    inside(&mut state);
    let at = state.playhead.value;
    state
        .add_keyframe_at_playhead(ClipParam::Opacity)
        .expect("key");

    assert!(!easing_presets().is_empty());
    for preset in easing_presets() {
        state
            .set_keyframe_easing_preset(ClipParam::Opacity, &preset.id)
            .unwrap_or_else(|err| panic!("{}: {err:?}", preset.id));
        let marks = state.keyframes_of(ClipParam::Opacity);
        let mark = marks
            .iter()
            .find(|mark| mark.tick == at)
            .expect("the keyframe survived the easing change");
        assert_eq!(
            mark.preset,
            Some(preset.id.as_str()),
            "{} did not land on the keyframe",
            preset.id
        );
    }

    // An id the catalog does not know must not disturb the easing already set.
    let before = state.keyframes_of(ClipParam::Opacity);
    state
        .set_keyframe_easing_preset(ClipParam::Opacity, "no-such-easing")
        .expect("inert");
    assert_eq!(state.keyframes_of(ClipParam::Opacity), before);
}

#[test]
fn every_piecewise_preset_expands_the_segment_it_leaves() {
    for (preset, label) in PIECEWISE_PRESETS {
        let (mut state, _) = with_clip();
        let ui = state.selected_ui_clip().expect("clip");
        let (start, end) = (ui.timeline_start_ticks, ui.timeline_end_ticks);

        // A preset expands the segment *leaving* a keyframe, so it needs one
        // after it to expand into.
        state.set_playhead(start + (end - start) / 4);
        state
            .add_keyframe_at_playhead(ClipParam::Opacity)
            .expect("first key");
        let first = state.playhead.value;
        state.set_playhead(start + (end - start) * 3 / 4);
        state
            .add_keyframe_at_playhead(ClipParam::Opacity)
            .expect("second key");
        assert_eq!(state.keyframes_of(ClipParam::Opacity).len(), 2);

        state.set_playhead(first);
        state
            .apply_piecewise_preset(ClipParam::Opacity, preset)
            .unwrap_or_else(|err| panic!("{label}: {err:?}"));
        assert!(
            state.keyframes_of(ClipParam::Opacity).len() > 2,
            "{label} did not expand the segment"
        );
    }
}

#[test]
fn navigation_and_flatten_walk_and_clear_the_curve() {
    let (mut state, _) = with_clip();
    let ui = state.selected_ui_clip().expect("clip");
    let (start, end) = (ui.timeline_start_ticks, ui.timeline_end_ticks);
    let ticks = [
        start + (end - start) / 4,
        start + (end - start) / 2,
        start + (end - start) * 3 / 4,
    ];
    for tick in ticks.into_iter().rev() {
        state.set_playhead(tick);
        state
            .add_keyframe_at_playhead(ClipParam::Opacity)
            .expect("key");
    }
    assert_eq!(state.keyframes_of(ClipParam::Opacity).len(), 3);
    assert_eq!(
        state
            .keyframes_of(ClipParam::Opacity)
            .iter()
            .map(|mark| mark.tick)
            .collect::<Vec<_>>(),
        ticks,
        "projected keyframes must stay sorted for binary search"
    );

    // Walking forward stops at the last keyframe rather than running off.
    state.set_playhead(start);
    for expected in ticks {
        state.go_to_next_keyframe(ClipParam::Opacity);
        assert_eq!(state.playhead.value, expected);
    }
    state.go_to_next_keyframe(ClipParam::Opacity);
    assert_eq!(state.playhead.value, ticks[2], "walked past the last key");

    for expected in ticks.iter().rev().skip(1) {
        state.go_to_prev_keyframe(ClipParam::Opacity);
        assert_eq!(state.playhead.value, *expected);
    }
    state.go_to_prev_keyframe(ClipParam::Opacity);
    assert_eq!(state.playhead.value, ticks[0], "walked past the first key");

    // Flatten drops every keyframe, and a second flatten is inert.
    state.set_playhead(ticks[1]);
    state.flatten_param(ClipParam::Opacity).expect("flatten");
    assert!(state.keyframes_of(ClipParam::Opacity).is_empty());
    state.flatten_param(ClipParam::Opacity).expect("inert");
    assert!(state.keyframes_of(ClipParam::Opacity).is_empty());
}

#[test]
fn a_lane_only_offers_the_properties_it_can_animate() {
    let (mut state, _) = with_clip();
    let offered = state.key_params();
    // The fixture clip sits on a video lane: the transform properties are
    // offered, and the audio envelopes only because it is media-backed.
    for param in [
        ClipParam::Position,
        ClipParam::Scale,
        ClipParam::Rotation,
        ClipParam::Opacity,
    ] {
        assert!(
            offered.iter().any(|row| row.param() == param),
            "a video lane must animate {param:?}"
        );
    }

    // A generated clip has no audio envelope, so volume and pan must not be
    // offered on one.
    state
        .add_generated(aria_models::Generator::text("Title"))
        .expect("text clip");
    let text = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .find(|clip| clip.kind == crate::types::UiClipKind::Text)
        .map(|clip| aria_models::ClipId::from_raw(clip.id))
        .expect("a text clip");
    state.select_clip(text);
    let offered = state.key_params();
    for param in [ClipParam::Volume, ClipParam::Pan] {
        assert!(
            !offered.iter().any(|row| row.param() == param),
            "a generated clip must not animate {param:?}"
        );
    }
    assert!(
        offered.iter().any(|row| row.param() == ClipParam::Opacity),
        "a text clip still animates its transform"
    );
}

/// A transform slider edit on an animated clip must write the keyframe under the
/// playhead, not collapse the property to a constant. Before the fix,
/// `SetClipTransform` was always sent with `at: None`, so moving any slider
/// silently deleted every keyframe on every transform property.
#[test]
fn a_slider_edit_writes_the_keyframe_instead_of_flattening_the_curve() {
    let (mut state, _) = with_clip();
    let ui = state.selected_ui_clip().expect("clip");
    let (start, end) = (ui.timeline_start_ticks, ui.timeline_end_ticks);
    let first = start + (end - start) / 4;
    let second = start + (end - start) * 3 / 4;

    for tick in [first, second] {
        state.set_playhead(tick);
        state
            .add_keyframe_at_playhead(ClipParam::Opacity)
            .expect("key");
    }
    assert_eq!(state.keyframes_of(ClipParam::Opacity).len(), 2);

    state.set_playhead(second);
    let mut edited = state.selected_ui_clip().expect("clip").transform;
    edited.opacity = 0.25;
    state.set_selected_transform(edited).expect("slider edit");

    assert_eq!(
        state.keyframes_of(ClipParam::Opacity).len(),
        2,
        "a slider edit flattened the curve"
    );
    assert!(
        (state.selected_ui_clip().expect("clip").transform.opacity - 0.25).abs() > 1e-4
            || first == 0,
        "the projection samples tick 0, so the edit at the later keyframe must not \
         show there"
    );

    // Editing at the playhead changes that keyframe's value, sampled locally.
    let sampled = {
        let engine = state.engine.lock();
        engine
            .project()
            .timeline()
            .clip(state.selected_clip().expect("clip"))
            .expect("clip")
            .transform
            .opacity
            .sample(second - start)
    };
    assert!(
        (sampled - 0.25).abs() < 1e-4,
        "the edit did not land on the keyframe under the playhead"
    );

    // Reset is the one path that must clear the animation outright.
    state.reset_selected_transform().expect("reset");
    assert!(
        state.keyframes_of(ClipParam::Opacity).is_empty(),
        "Reset left a curve behind"
    );
}
