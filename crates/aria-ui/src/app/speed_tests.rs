//! Contracts for speed ramps.
//!
//! The class closed here is "a ramp is applied blind, and editing one point of it
//! resets the others". Presets are enumerated from `speed_preset_catalog()` at run
//! time, so a new preset is exercised without a second list, and the active preset
//! is read back by shape, which is what the panel highlights.

use aria_models::{MAX_SPEED, MIN_SPEED, SPEED_CURVE_SCALE, speed_preset_catalog};

use super::premiere_tests::with_clip;
use crate::app::clip_speed::RAMP_POINTS;

#[test]
fn every_catalog_preset_applies_and_reads_back_by_shape() {
    let (mut state, _) = with_clip();
    assert!(state.selected_can_ramp(), "a media clip can be retimed");
    assert!(!state.selected_has_ramp(), "a fresh clip has flat speed");
    assert_eq!(state.selected_speed_preset(), None);
    assert!(state.selected_ramp_values().is_empty());
    assert!(!speed_preset_catalog().is_empty());

    for spec in speed_preset_catalog() {
        state
            .set_selected_speed_preset(&spec.id)
            .unwrap_or_else(|err| panic!("{}: {err:?}", spec.id));
        assert!(state.selected_has_ramp(), "{} left the clip flat", spec.id);
        assert_eq!(
            state.selected_speed_preset(),
            Some(spec.id.as_str()),
            "{} did not read back",
            spec.id
        );
        // Every point of the readout must be inside the model's speed bounds.
        let values = state.selected_ramp_values();
        assert_eq!(values.len(), RAMP_POINTS.len());
        for (id, _, value) in values {
            assert!(
                (MIN_SPEED..=MAX_SPEED).contains(&value),
                "{} put {id} at {value}",
                spec.id
            );
        }
    }

    state.clear_selected_speed_curve().expect("clear");
    assert!(!state.selected_has_ramp());
    assert_eq!(state.selected_speed_preset(), None);
    state.clear_selected_speed_curve().expect("inert");
}

#[test]
fn editing_one_ramp_point_leaves_the_others_alone() {
    let (mut state, _) = with_clip();
    // A flat clip seeds a unit ramp, so the untouched points stay at 1x.
    state
        .set_selected_ramp_point(SPEED_CURVE_SCALE / 2, 2.0)
        .expect("middle");
    let values = state.selected_ramp_values();
    let at = |id: &str| {
        values
            .iter()
            .find(|(point, ..)| *point == id)
            .map(|(.., value)| *value)
            .unwrap_or_else(|| panic!("{id} missing"))
    };
    assert!((at("ramp-mid") - 2.0).abs() < 1e-3, "the edit did not land");
    assert!((at("ramp-in") - 1.0).abs() < 1e-3, "the start moved");
    assert!((at("ramp-out") - 1.0).abs() < 1e-3, "the end moved");

    // A hand-edited ramp matches no preset.
    assert_eq!(state.selected_speed_preset(), None);

    // Each remaining point sets independently.
    state.set_selected_ramp_point(0, 0.5).expect("start");
    state
        .set_selected_ramp_point(SPEED_CURVE_SCALE, 3.0)
        .expect("end");
    let values = state.selected_ramp_values();
    let at = |id: &str| {
        values
            .iter()
            .find(|(point, ..)| *point == id)
            .map(|(.., value)| *value)
            .unwrap()
    };
    assert!((at("ramp-in") - 0.5).abs() < 1e-3);
    assert!((at("ramp-mid") - 2.0).abs() < 1e-3, "the middle was reset");
    assert!((at("ramp-out") - 3.0).abs() < 1e-3);
}

#[test]
fn a_ramp_point_clamps_to_the_models_speed_bounds() {
    let (mut state, _) = with_clip();
    for (_, _, tick) in RAMP_POINTS {
        state
            .set_selected_ramp_point(tick, MAX_SPEED * 10.0)
            .expect("high");
        state.set_selected_ramp_point(tick, -50.0).expect("low");
        state.set_selected_ramp_point(tick, f32::NAN).expect("nan");
    }
    for (id, _, value) in state.selected_ramp_values() {
        assert!(
            (MIN_SPEED..=MAX_SPEED).contains(&value),
            "{id} escaped the bounds at {value}"
        );
        assert!(value.is_finite(), "{id} took a NaN");
    }

    // A tick inside the normalized domain adds a point; one outside it is inert.
    let before = state.selected_ramp_curve().len();
    state.set_selected_ramp_point(7, 2.0).expect("added");
    assert_eq!(state.selected_ramp_curve().len(), before + 1);
    state
        .set_selected_ramp_point(SPEED_CURVE_SCALE + 1, 2.0)
        .expect("inert");
    state.set_selected_ramp_point(-1, 2.0).expect("inert");
    assert_eq!(state.selected_ramp_curve().len(), before + 1);
}

#[test]
fn a_ramp_retimes_the_clip_and_clearing_it_restores_the_length() {
    let (mut state, _) = with_clip();
    let flat = {
        let ui = state.selected_ui_clip().expect("clip");
        ui.timeline_end_ticks - ui.timeline_start_ticks
    };

    // Doubling the speed across the whole clip must shorten it.
    for (_, _, tick) in RAMP_POINTS {
        state.set_selected_ramp_point(tick, 2.0).expect("fast");
    }
    let fast = {
        let ui = state.selected_ui_clip().expect("clip");
        ui.timeline_end_ticks - ui.timeline_start_ticks
    };
    assert!(
        fast < flat,
        "a 2x ramp did not shorten the clip ({fast} vs {flat})"
    );

    state.clear_selected_speed_curve().expect("clear");
    let cleared = {
        let ui = state.selected_ui_clip().expect("clip");
        ui.timeline_end_ticks - ui.timeline_start_ticks
    };
    assert_eq!(
        cleared, flat,
        "clearing the ramp did not restore the length"
    );
}

#[test]
fn a_generated_clip_refuses_a_ramp_and_the_paths_are_inert() {
    let (mut state, _) = with_clip();
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

    assert!(!state.selected_can_ramp());
    state
        .set_selected_speed_preset(&speed_preset_catalog()[0].id)
        .expect("inert");
    assert!(!state.selected_has_ramp());
    state.set_selected_ramp_point(0, 2.0).expect("inert");
    assert!(!state.selected_has_ramp());

    // An unknown preset id, and no selection at all, are both inert.
    state.select_clip(aria_models::ClipId::from_raw(u64::MAX));
    state.clear_selection();
    assert!(!state.selected_can_ramp());
    state
        .set_selected_speed_preset("no-such-ramp")
        .expect("inert");
    state.nudge_selected_ramp_point(0, 1.0).expect("inert");
}

/// The curve editor's own class: "the graph draws sampled values, points cannot be
/// added or removed, and every pointer move is an undo entry".
#[test]
fn a_graph_drag_is_one_edit_and_lands_where_the_pointer_left_it() {
    let (mut state, _) = with_clip();
    // Seed the three-point ramp the graph starts from.
    state
        .set_selected_ramp_point(SPEED_CURVE_SCALE / 2, 2.0)
        .expect("seed");
    let before = state.engine().lock().revision();

    let mid = SPEED_CURVE_SCALE / 2;
    assert!(
        state.begin_ramp_drag(mid + 3),
        "a nearby press grabs the point"
    );
    assert_eq!(state.ramp_dragging().map(|drag| drag.origin), Some(mid));
    // Pointer moves are drafts: the graph follows, the engine does not.
    assert!(state.update_ramp_drag(mid + 40, 4.0));
    assert!(state.update_ramp_drag(mid + 80, 6.0));
    assert_eq!(
        state.engine().lock().revision(),
        before,
        "a pointer move reached the engine"
    );
    // The draft is what the graph draws.
    assert!(
        state
            .selected_ramp_curve()
            .iter()
            .any(|(tick, value)| *tick == mid + 80 && (*value - 6.0).abs() < 1e-3),
        "the draft is not on the curve"
    );

    state.commit_ramp_drag().expect("commit");
    assert_eq!(
        state.engine().lock().revision(),
        before + 2,
        "moving a point should remove the old one and set the new one"
    );
    let curve = state.selected_ramp_curve();
    assert!(state.ramp_dragging().is_none());
    assert!(
        !curve.iter().any(|(tick, _)| *tick == mid),
        "the point stayed behind at its old tick"
    );
    assert!(
        curve
            .iter()
            .any(|(tick, value)| *tick == mid + 80 && (*value - 6.0).abs() < 1e-3)
    );
}

#[test]
fn an_abandoned_or_still_graph_drag_costs_nothing() {
    let (mut state, _) = with_clip();
    state
        .set_selected_ramp_point(SPEED_CURVE_SCALE / 2, 2.0)
        .expect("seed");
    let before = state.engine().lock().revision();

    // A press on empty graph grabs nothing, so the panel adds a point instead.
    assert!(!state.begin_ramp_drag(SPEED_CURVE_SCALE / 4));
    // Esc abandons.
    assert!(state.begin_ramp_drag(0));
    assert!(state.update_ramp_drag(0, 3.0));
    state.cancel_ramp_drag();
    assert!(state.ramp_dragging().is_none());
    assert_eq!(state.engine().lock().revision(), before);

    // A release that moved nothing writes nothing.
    assert!(state.begin_ramp_drag(0));
    state.commit_ramp_drag().expect("still release");
    assert_eq!(state.engine().lock().revision(), before);

    // Updating with no drag, and a non-finite value, are both inert.
    assert!(!state.update_ramp_drag(10, 2.0));
    assert!(state.begin_ramp_drag(SPEED_CURVE_SCALE));
    assert!(!state.update_ramp_drag(10, f32::NAN));
    state.cancel_ramp_drag();

    // Committing with no drag is inert.
    state.commit_ramp_drag().expect("no drag");
    assert_eq!(state.engine().lock().revision(), before);
}

#[test]
fn the_end_points_keep_their_tick_and_the_rest_stay_inside_the_ramp() {
    let (mut state, _) = with_clip();
    state
        .set_selected_ramp_point(SPEED_CURVE_SCALE / 2, 2.0)
        .expect("seed");

    for origin in [0, SPEED_CURVE_SCALE] {
        assert!(state.begin_ramp_drag(origin));
        state.update_ramp_drag(SPEED_CURVE_SCALE / 3, 3.0);
        assert_eq!(
            state.ramp_dragging().map(|drag| drag.tick),
            Some(origin),
            "an end point walked off its tick"
        );
        state.commit_ramp_drag().expect("commit");
        let curve = state.selected_ramp_curve();
        assert!(
            curve.iter().any(|(tick, _)| *tick == origin),
            "the ramp lost an end point"
        );
    }

    // An interior point cannot be dragged onto either end.
    assert!(state.begin_ramp_drag(SPEED_CURVE_SCALE / 2));
    state.update_ramp_drag(-500, 2.0);
    assert_eq!(state.ramp_dragging().map(|drag| drag.tick), Some(1));
    state.update_ramp_drag(SPEED_CURVE_SCALE * 2, 2.0);
    assert_eq!(
        state.ramp_dragging().map(|drag| drag.tick),
        Some(SPEED_CURVE_SCALE - 1)
    );
    state.cancel_ramp_drag();
}

#[test]
fn a_point_is_added_and_removed_and_the_last_two_are_kept() {
    let (mut state, _) = with_clip();
    state
        .set_selected_ramp_point(SPEED_CURVE_SCALE / 2, 2.0)
        .expect("seed");
    assert_eq!(state.selected_ramp_curve().len(), RAMP_POINTS.len());

    // Adding is setting a point at a tick that has none.
    state
        .set_selected_ramp_point(SPEED_CURVE_SCALE / 4, 0.5)
        .expect("add");
    let curve = state.selected_ramp_curve();
    assert_eq!(curve.len(), RAMP_POINTS.len() + 1);
    assert!(
        curve
            .iter()
            .any(|(tick, value)| *tick == SPEED_CURVE_SCALE / 4 && (*value - 0.5).abs() < 1e-3)
    );
    // Points stay in tick order, which is what the graph draws along.
    assert!(curve.windows(2).all(|pair| pair[0].0 < pair[1].0));

    // Removing takes exactly the addressed point.
    state
        .remove_selected_ramp_point(SPEED_CURVE_SCALE / 4)
        .expect("remove");
    assert_eq!(state.selected_ramp_curve().len(), RAMP_POINTS.len());
    // A tick with no point there is inert.
    let revision = state.engine().lock().revision();
    state.remove_selected_ramp_point(11).expect("inert");
    assert_eq!(state.engine().lock().revision(), revision);

    // Down to two points, removal is refused rather than collapsing the ramp.
    state
        .remove_selected_ramp_point(SPEED_CURVE_SCALE / 2)
        .expect("third");
    assert_eq!(state.selected_ramp_curve().len(), 2);
    let revision = state.engine().lock().revision();
    state.remove_selected_ramp_point(0).expect("refused");
    assert_eq!(state.selected_ramp_curve().len(), 2);
    assert_eq!(state.engine().lock().revision(), revision);
    assert!(
        state
            .status_message
            .as_deref()
            .unwrap_or_default()
            .contains("two points"),
        "the refusal was silent: {:?}",
        state.status_message
    );
    assert!(state.selected_has_ramp(), "the ramp survived the refusal");
}

#[test]
fn the_readout_shows_the_length_the_drafted_ramp_implies() {
    let (mut state, _) = with_clip();
    let flat = state.selected_ramp_length_ticks().expect("flat length");
    let actual = {
        let ui = state.selected_ui_clip().expect("clip");
        ui.timeline_end_ticks - ui.timeline_start_ticks
    };
    assert_eq!(flat, actual, "the readout disagrees with the clip");

    for (_, _, tick) in RAMP_POINTS {
        state.set_selected_ramp_point(tick, 2.0).expect("fast");
    }
    let fast = state.selected_ramp_length_ticks().expect("ramped length");
    assert!(fast < flat, "a 2x ramp did not shorten the readout");
    let committed = {
        let ui = state.selected_ui_clip().expect("clip");
        ui.timeline_end_ticks - ui.timeline_start_ticks
    };
    assert_eq!(fast, committed, "the readout drifted from the commit");

    // A draft moves the readout before anything is committed.
    assert!(state.begin_ramp_drag(0));
    assert!(state.update_ramp_drag(0, MIN_SPEED));
    let drafted = state.selected_ramp_length_ticks().expect("drafted length");
    assert!(
        drafted > fast,
        "slowing the start did not lengthen the readout ({drafted} vs {fast})"
    );
    assert_eq!(
        {
            let ui = state.selected_ui_clip().expect("clip");
            ui.timeline_end_ticks - ui.timeline_start_ticks
        },
        fast,
        "the draft reached the clip"
    );
    state.cancel_ramp_drag();
    assert_eq!(state.selected_ramp_length_ticks(), Some(fast));
}

/// Graph arithmetic: the axes round-trip and a press only grabs what is near it.
#[test]
fn the_graph_axes_round_trip_and_the_grab_radius_is_nearest_wins() {
    use crate::app::clip_speed::{
        RAMP_GRAB_TICKS, ramp_frac_to_tick, ramp_frac_to_value, ramp_point_at, ramp_tick_to_frac,
        ramp_value_to_frac,
    };

    assert_eq!(ramp_frac_to_tick(0.0), 0);
    assert_eq!(ramp_frac_to_tick(1.0), SPEED_CURVE_SCALE);
    // Out of range clamps rather than escaping the domain.
    assert_eq!(ramp_frac_to_tick(-2.0), 0);
    assert_eq!(ramp_frac_to_tick(9.0), SPEED_CURVE_SCALE);
    for tick in [0, 1, SPEED_CURVE_SCALE / 3, SPEED_CURVE_SCALE] {
        assert_eq!(ramp_frac_to_tick(ramp_tick_to_frac(tick)), tick);
    }

    assert!((ramp_value_to_frac(MIN_SPEED) - 0.0).abs() < 1e-6);
    assert!((ramp_value_to_frac(MAX_SPEED) - 1.0).abs() < 1e-6);
    for value in [MIN_SPEED, 0.5, 1.0, 2.0, 25.0, MAX_SPEED] {
        let round = ramp_frac_to_value(ramp_value_to_frac(value));
        assert!((round - value).abs() < 1e-2, "{value} became {round}");
    }
    // The axis is logarithmic, so 1x sits well clear of the floor a linear axis
    // would pin it to.
    assert!(
        ramp_value_to_frac(1.0) > 0.3,
        "1x landed at {} of the graph",
        ramp_value_to_frac(1.0)
    );
    // Bounds hold in both directions.
    assert_eq!(ramp_frac_to_value(-1.0), MIN_SPEED);
    assert_eq!(ramp_frac_to_value(2.0), MAX_SPEED);

    let points = [(0_i64, 1.0_f32), (500, 2.0), (1000, 1.0)];
    assert_eq!(ramp_point_at(&points, 0), Some(0));
    assert_eq!(ramp_point_at(&points, 500 + RAMP_GRAB_TICKS), Some(500));
    assert_eq!(ramp_point_at(&points, 500 + RAMP_GRAB_TICKS + 1), None);
    // Nearest wins when two are in reach.
    let close = [(100_i64, 1.0_f32), (110, 2.0)];
    assert_eq!(ramp_point_at(&close, 104), Some(100));
    assert_eq!(ramp_point_at(&close, 107), Some(110));
    assert_eq!(ramp_point_at(&[], 0), None);
}
