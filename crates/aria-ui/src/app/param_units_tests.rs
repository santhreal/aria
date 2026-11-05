//! Contracts for the units a property row prints and drags in.
//!
//! The class closed here is "a drag steps by a share of a model bound that is
//! not the property's travel". A title may be 4096 pixels tall, so its size
//! stepped 41 pixels per unit of drag; a mask rotation stepped 72 degrees; and a
//! position, stored as a fraction of the frame and printed bare, crossed forty
//! frames in an 80-pixel drag. Every numeric row the catalog declares, and every
//! mask row, is held to at most one printed unit per unit of drag, and a row of
//! each measure is dragged through the write its readout hands the model.
//!
//! What it does not catch: an effect parameter, whose plugin declares its own
//! travel, and a property given a measure that steps by at most one printed unit
//! but prints the wrong unit. `Measure::of` names every property, so a property
//! the model gains does not compile until it is given a measure.

use std::rc::Rc;

use aria_analysis::linear_from_db;
use aria_models::{
    AudioSend, ClipParam, EASY_EASE_INFLUENCE, MAX_TEMPORAL_INFLUENCE, MIN_TEMPORAL_INFLUENCE,
    MaskKind, MaskParam, ParamKind, TextParam, clip_param_catalog, mask_param_group,
};

use super::{Scale, Units};
use crate::app::AppState;
use crate::app::animation_verbs::VelocitySide;
use crate::app::clip_params::ParamRow;
use crate::app::fixtures::{a_clip, a_title};
use crate::app::mixer::gain_label;
use crate::app::property_rows::GraphHandleSide;
use crate::app::value_scrub::{ScrubTarget, ValueKind, Write, write_number};
use crate::motion::MotionKey;

/// No row skips a value it prints at whole units: one unit of drag moves the
/// printed number by one unit at most, whatever bounds the model declares.
#[test]
fn one_unit_of_drag_moves_no_row_more_than_one_printed_unit() {
    let masks = MaskParam::ALL.iter().map(|&param| {
        (
            ClipParam::Mask { mask: 0, param },
            param.kind(),
            param.range(),
        )
    });
    let rows = clip_param_catalog()
        .iter()
        .map(|spec| (spec.param, spec.kind, spec.range))
        .chain(masks)
        .filter(|(_, kind, _)| matches!(kind, ParamKind::Scalar | ParamKind::Vec2));
    let mut held = 0;
    for (param, _, range) in rows {
        let drag = Units::of(param, &range).number.drag;
        assert!(
            drag > 0.0 && drag <= 1.0,
            "{param:?} moves {drag} printed units per unit of drag"
        );
        held += 1;
    }
    assert!(held > 0, "the catalog declares no numeric row");
}

/// One row, with the project it is drawn on.
struct Case {
    dress: fn() -> AppState,
    group: String,
    param: ClipParam,
    component: Option<u8>,
    before: &'static str,
    after: &'static str,
}

fn a_masked_clip() -> AppState {
    let mut state = a_clip();
    state
        .add_selected_mask(MaskKind::Rectangle)
        .expect("the mask lands");
    state
}

/// The row of `param` in `group` on the selected clip.
fn row(state: &AppState, group: &str, param: ClipParam, component: Option<u8>) -> ParamRow {
    state
        .param_rows(group)
        .into_iter()
        .find(|row| row.param == param && row.component == component)
        .unwrap_or_else(|| panic!("the {group} block draws no {param:?} {component:?} row"))
}

/// What the Inspector prints for `row`, and the readout it draws: printed,
/// dragged and typed in the row's units, through the write a catalog row
/// hands its stepper.
fn readout(row: &ParamRow) -> (String, ScrubTarget) {
    let units = Units::of(row.param, &row.range);
    let (param, component, scale) = (row.param, row.component, units.number.scale);
    let target = ScrubTarget {
        key: MotionKey::new("param-units-test", &row.id),
        value: scale.shown(row.value),
        unit: units.number.drag,
        kind: units.number.kind(),
        write: Write::Project(Rc::new(move |state: &mut AppState, delta| {
            state.nudge_shared_param_in(param, component, scale, delta)
        })),
    };
    (units.print(row.value), target)
}

/// An 80-pixel drag is forty units of drag, written as one undo step: a place
/// on the frame or the picture moves four percent of it, an angle forty
/// degrees, a length forty pixels and a multiplier forty percent.
#[test]
fn an_eighty_pixel_drag_moves_each_measure_forty_printed_steps() {
    let mask = |param| ClipParam::Mask { mask: 0, param };
    let text = |param| ClipParam::Text { param };
    let masks = mask_param_group(0);
    let case = |dress: fn() -> AppState,
                group: &str,
                param: ClipParam,
                component: Option<u8>,
                before: &'static str,
                after: &'static str| Case {
        dress,
        group: group.to_string(),
        param,
        component,
        before,
        after,
    };
    let cases = [
        case(
            a_clip,
            "transform",
            ClipParam::Position,
            Some(0),
            "0.0%",
            "4.0%",
        ),
        case(
            a_clip,
            "transform",
            ClipParam::Position,
            Some(1),
            "0.0%",
            "4.0%",
        ),
        case(
            a_clip,
            "transform",
            ClipParam::AnchorPoint,
            Some(0),
            "50.0%",
            "54.0%",
        ),
        case(
            a_clip,
            "transform",
            ClipParam::Rotation,
            None,
            "0.0°",
            "40.0°",
        ),
        case(
            a_masked_clip,
            &masks,
            mask(MaskParam::Center),
            Some(0),
            "0.0%",
            "4.0%",
        ),
        case(
            a_masked_clip,
            &masks,
            mask(MaskParam::Size),
            Some(1),
            "100.0%",
            "104.0%",
        ),
        case(
            a_masked_clip,
            &masks,
            mask(MaskParam::Rotation),
            None,
            "0.0°",
            "40.0°",
        ),
        case(
            a_masked_clip,
            &masks,
            mask(MaskParam::Expansion),
            None,
            "0.0",
            "40.0",
        ),
        case(
            a_title,
            "text",
            text(TextParam::Size),
            None,
            "90.0",
            "130.0",
        ),
        case(
            a_title,
            "text",
            text(TextParam::LineSpacing),
            None,
            "120%",
            "160%",
        ),
    ];
    for case in cases {
        let what = format!("{:?} {:?}", case.param, case.component);
        let mut state = (case.dress)();
        let depth = state.history_depth();
        let (before, target) = readout(&row(&state, &case.group, case.param, case.component));
        assert_eq!(before, case.before, "{what} before the drag");
        state.press_value(target);
        assert!(
            state.drag_value(80.0, false, false),
            "{what}: the drag wrote nothing"
        );
        assert!(state.release_value(), "{what}: the press was lost");
        let (after, _) = readout(&row(&state, &case.group, case.param, case.component));
        assert_eq!(after, case.after, "{what} after an 80-pixel drag");
        assert_eq!(
            state.history_depth(),
            depth + 1,
            "{what}: one drag, one undo step"
        );
    }
}

/// A number that rounds to zero at the digits printed reads `0`, with no
/// sign, at every digit count, signed or not; any other number keeps its
/// sign. A drag back to rest lands a hair either side of zero, which printed
/// `-0.0%`, `-0.0°` or `+0.0 dB`.
#[test]
fn a_number_that_rounds_to_zero_prints_no_sign() {
    for decimals in 0..=3u8 {
        let half_step = 0.4 * 10f32.powi(-i32::from(decimals));
        let zero = format!("{:.*}", usize::from(decimals), 0.0);
        for signed in [false, true] {
            for value in [-half_step, -0.0, 0.0, half_step] {
                let mut out = String::from("x");
                write_number(&mut out, value, decimals, signed);
                assert_eq!(out, format!("x{zero}"), "{value} at {decimals} digits");
            }
            let step = 10f32.powi(-i32::from(decimals));
            let mut below = String::new();
            write_number(&mut below, -step, decimals, signed);
            assert!(below.starts_with('-'), "{below}");
            let mut above = String::new();
            write_number(&mut above, step, decimals, signed);
            assert_eq!(above.starts_with('+'), signed, "{above}");
        }
    }
}

/// Every printer of a property's number prints a value a hair below rest as
/// it prints rest: each catalog row, each mask row, each effect and transition
/// parameter, a field opening a number, and a fader's level. Not covered: a
/// status line that prints a level, and a readout whose caller prints its own
/// text.
#[test]
fn every_printed_number_reads_rest_unsigned() {
    let masks = MaskParam::ALL
        .iter()
        .map(|&param| Units::of(ClipParam::Mask { mask: 0, param }, &param.range()));
    let catalog = clip_param_catalog()
        .iter()
        .map(|spec| Units::of(spec.param, &spec.range));
    let video = aria_models::effect_catalog();
    let audio = aria_models::audio_effect_catalog();
    let transitions = aria_models::transition_catalog();
    let effects = video
        .iter()
        .flat_map(|spec| spec.params.iter())
        .chain(audio.iter().flat_map(|spec| spec.params.iter()))
        .chain(transitions.iter().flat_map(|spec| spec.params.iter()))
        .map(Units::of_effect);
    let mut held = 0;
    for units in catalog.chain(masks).chain(effects) {
        let scale = units.number.scale;
        let below = -0.4 * 10f32.powi(-i32::from(units.number.decimals));
        let rest = units.print(scale.stored(0.0));
        let hair = units.print(scale.stored(below));
        assert_eq!(hair, rest, "{units:?} a hair below rest");
        assert!(
            !rest.starts_with(['-', '+']),
            "{units:?} prints rest as {rest}"
        );
        held += 1;
    }
    assert!(held > 0, "no printer was enumerated");
    for decimals in 0..=3 {
        let kind = ValueKind::Number { decimals };
        let below = -0.4 * 10f32.powi(-i32::from(decimals));
        assert_eq!(
            kind.format(below),
            kind.format(0.0),
            "a field at {decimals} digits"
        );
    }
    let unity = gain_label(1.0);
    assert_eq!(unity, "0.0 dB");
    assert_eq!(gain_label(linear_from_db(-0.04)), unity);
    assert_eq!(gain_label(linear_from_db(0.04)), unity);
}

/// A fader reads a level as every gain row of the catalog prints it, silence
/// included, from its floor to the top of the send travel. Not covered: a
/// level under the floor, which a fader reads at the floor and a row prints
/// as stored.
#[test]
fn a_fader_reads_a_level_as_a_gain_row_prints_it() {
    let rows: Vec<_> = clip_param_catalog()
        .iter()
        .map(|spec| (&spec.id, Units::of(spec.param, &spec.range)))
        .filter(|(_, units)| units.number.scale == Scale::Gain)
        .collect();
    assert!(!rows.is_empty(), "the catalog declares no gain row");
    let floor = aria_analysis::GAIN_FLOOR_DB;
    let levels = std::iter::once(0.0).chain(
        (0..=160).map(|step| linear_from_db(floor + step as f32 * 0.5).min(AudioSend::MAX_LEVEL)),
    );
    for level in levels {
        for (id, units) in &rows {
            assert_eq!(gain_label(level), units.print(level), "{id} at {level}");
        }
    }
}

/// The Keyframe Velocity dialog and the Graph Editor's handle labels print one
/// influence text across the segment span, from the 0.1% floor through Easy
/// Ease to 100.0%.
#[test]
fn keyframe_velocity_and_graph_handles_print_one_influence_text() {
    let mut state = a_clip();
    let clip = state.selected_clip().expect("a selected clip");
    for (tick, value) in [(0, 0.0), (4, 0.5), (20, 1.0)] {
        state.set_playhead(tick);
        state
            .set_keyframe_scalar(ClipParam::Opacity, tick, value)
            .expect("a keyframe");
    }
    state.select_timeline_property(clip, ClipParam::Opacity, false);
    state.select_timeline_keyframe(clip, ClipParam::Opacity, 4, false);

    let scale = Units::INFLUENCE.number.scale;
    let samples = [
        MIN_TEMPORAL_INFLUENCE,
        0.1,
        EASY_EASE_INFLUENCE,
        0.5,
        0.75,
        MAX_TEMPORAL_INFLUENCE,
    ]
    .into_iter()
    .chain((1..=100).map(|pct| pct as f32 / 100.0));

    for stored in samples {
        let percent = scale.shown(stored);
        state.open_keyframe_velocity();
        for side in [VelocitySide::Incoming, VelocitySide::Outgoing] {
            state
                .set_keyframe_velocity_influence(side, percent)
                .expect("influence inside span");
        }
        state.apply_keyframe_velocity();
        state.open_keyframe_velocity();

        let graph = state.build_timeline_graph_model(24.0, 20.0, 0.0, 120.0);
        let curve = graph
            .curves
            .iter()
            .find(|c| c.clip == clip && c.param == ClipParam::Opacity)
            .expect("opacity curve");
        for (velocity_side, graph_side) in [
            (VelocitySide::Incoming, GraphHandleSide::Incoming),
            (VelocitySide::Outgoing, GraphHandleSide::Outgoing),
        ] {
            let handle = curve
                .influence_handles
                .iter()
                .find(|h| h.tick == 4 && h.side == graph_side)
                .expect("handle on middle keyframe");
            let dialog_text = state.keyframe_velocity.influence_label(velocity_side);
            let graph_text = Units::INFLUENCE.print(
                handle
                    .influence
                    .clamp(MIN_TEMPORAL_INFLUENCE, MAX_TEMPORAL_INFLUENCE),
            );
            assert_eq!(
                dialog_text,
                Units::INFLUENCE.print(stored),
                "{velocity_side:?} dialog readout at {stored}"
            );
            assert_eq!(
                dialog_text, graph_text,
                "{velocity_side:?} dialog vs graph handle at {stored}"
            );
        }
    }
}
