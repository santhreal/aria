//! Every readout the shell draws, read back the way its field reads typed
//! text, written through by one unit, and read on both surfaces where a
//! property is drawn in the Inspector and on its timeline row.
//!
//! # Why
//!
//! Closes the class "a readout printed in one unit and typed or dragged in
//! another". An effect slot displayed as a percentage printed `25%` and opened
//! its field on `0.25`, so a typed `30` set the fraction, not the percent. Opacity
//! printed `1.00` in the Inspector and `100%` on its timeline row, and a gain
//! printed decibels in the Inspector and the multiplier on its timeline row.
//!
//! Every readout is drawn through `value_readout::readout`, which records the
//! text it printed and the target a press opens, so a readout added to any
//! surface is held here without being listed. Three things hold for each:
//!
//! - The number it prints, read as its field reads typed text, is the value
//!   the field opens with, to the digits it prints.
//! - A write of one unit through it moves that value by one unit, so a drag
//!   and a typed value land in the units the readout prints. A write that
//!   records no edit may leave the value where a bound holds it.
//! - A property drawn on both surfaces prints the same number on its timeline
//!   row as on its Inspector row, opens its field on the same value with the
//!   same digits, and drags by the same unit.
//!
//! The shell is drawn over a still with its Transform rows open, over the
//! still with an effect holding a slot of each way a slot is printed and its
//! Effects rows open, over the still with a transition holding the same
//! slots, and over a sound with its Audio rows open.
//!
//! Not covered: a value printed outside a readout, such as the clip line's
//! drag label; a readout drawn only after a press; a timeline row scrolled out
//! of view; a write that moves a value its readout does not print; and a
//! silent gain, which prints a word rather than a number.

use std::collections::HashSet;
use std::time::Duration;

use aria_models::{
    ClipId, ClipParam, EffectParamDisplay, EffectParamKind, EffectParamSpec, EffectSpec,
    TransitionSpec, clip_param_catalog,
};
use gpui::{Entity, TestAppContext, VisualTestContext};

use super::blocks::widgets::MIXED;
use super::strip_tests::drawn_with;
use super::tabs::effects::effects_params::vec2_axes;
use crate::app::AppState;
use crate::app::property_rows::{PropertyGroup, TimelineOutlineRow};
use crate::app::value_scrub::ValueKind;
use crate::motion::MotionKey;
use crate::panels::value_readout::{DrawnReadout, drawn_readouts, record_readouts};

/// The id of the effect holding a slot of each way a slot is printed.
const DISPLAYED: &str = "test.ui.every_display";

/// A slot of each way an effect slot is printed: as a percentage, on an angle
/// dial, as an angle, with a unit, bare, as a whole number, and as a vector.
fn displayed_slots() -> Vec<EffectParamSpec> {
    vec![
        EffectParamSpec::scalar("share", "Share", 0.25, 0.0, 1.0)
            .with_display(EffectParamDisplay::Percentage),
        EffectParamSpec::scalar("turn", "Turn", 30.0, -180.0, 180.0)
            .with_display(EffectParamDisplay::AngleDial),
        EffectParamSpec::angle("angle", "Angle", 45.0, -360.0, 360.0),
        EffectParamSpec::scalar("spread", "Spread", 12.5, 0.0, 100.0).with_unit("px"),
        EffectParamSpec::scalar("amount", "Amount", 0.5, 0.0, 1.0),
        EffectParamSpec::int("count", "Count", 3, -5, 10),
        EffectParamSpec::vec2("offset", "Offset", [0.25, -0.25], -1.0, 1.0).with_unit("px"),
    ]
}

/// The id of the lane holding `clip`.
fn lane_of(state: &AppState, clip: ClipId) -> u64 {
    state
        .store
        .sequence
        .tracks
        .iter()
        .find(|lane| lane.clips.iter().any(|placed| placed.id == clip.raw()))
        .expect("the clip's lane")
        .id
}

/// Whether `clip`'s `group` rows are open on the timeline.
fn group_open(state: &AppState, clip: ClipId, group: PropertyGroup) -> bool {
    state
        .track_outline_rows(lane_of(state, clip))
        .iter()
        .any(|row| {
            matches!(
                row,
                TimelineOutlineRow::GroupHeader { clip: on, group: header, expanded: true, .. }
                    if *on == clip && *header == group
            )
        })
}

/// Opens the selected clip's `group` rows on the timeline and closes the
/// others, so no other group's rows push them out of view.
fn open_only(state: &mut AppState, group: PropertyGroup) {
    let clip = state.selected_clip().expect("the fixture is selected");
    let lane = lane_of(state, clip);
    let outlined = state
        .track_outline_rows(lane)
        .iter()
        .any(|row| matches!(row, TimelineOutlineRow::GroupHeader { clip: on, .. } if *on == clip));
    if !outlined {
        state.toggle_clip_outline(clip.raw());
    }
    for other in PropertyGroup::ALL {
        if group_open(state, clip, other) != (other == group) {
            state.toggle_property_group(clip, other);
        }
    }
    assert!(
        group_open(state, clip, group),
        "the clip's {group:?} rows did not open on the timeline"
    );
}

/// Every panel body built anew once every glide has run to its end, such as a
/// twirl opening a clip's rows, and the readouts it drew, the last one drawn
/// under each key.
fn redraw(visual: &mut VisualTestContext) -> Vec<DrawnReadout> {
    // One frame starts a glide and one past its end draws every row at rest.
    for _ in 0..2 {
        visual.update(|window, _| window.refresh());
        visual.run_until_parked();
        visual.executor().advance_clock(Duration::from_secs(1));
    }
    record_readouts();
    // A refresh builds every panel body, so no readout is replayed unrecorded.
    visual.update(|window, _| window.refresh());
    visual.run_until_parked();
    let mut drawn = drawn_readouts();
    let mut seen = HashSet::new();
    drawn.reverse();
    drawn.retain(|readout| seen.insert(readout.target.key));
    drawn.reverse();
    drawn
}

/// The readout drawn under `key`.
fn drawn_under(drawn: &[DrawnReadout], key: MotionKey) -> Option<&DrawnReadout> {
    drawn.iter().find(|readout| readout.target.key == key)
}

/// The number `text` starts with, read as a field of `kind` reads typed text.
fn printed_number(kind: ValueKind, text: &str) -> Option<f32> {
    let end = text
        .find(|ch: char| !kind.accepts(ch))
        .unwrap_or(text.len());
    kind.parse(&text[..end])
}

/// Every readout in `drawn` prints a number its field reads back as the value
/// it opens with, to the digits it prints.
fn assert_each_reads_back(surface: &str, drawn: &[DrawnReadout]) {
    for readout in drawn {
        let (text, target) = (readout.text.as_ref(), &readout.target);
        if text == MIXED {
            continue;
        }
        let typed = printed_number(target.kind, text).unwrap_or_else(|| {
            panic!("{surface}: a readout prints `{text}`, which its field cannot read as a number")
        });
        let half_digit = match target.kind {
            ValueKind::Number { decimals } => 0.5 * 10f32.powi(-i32::from(decimals)),
            ValueKind::Frames { .. } => 0.5,
        };
        let slack = half_digit + 1e-4 * target.value.abs().max(1.0);
        assert!(
            (typed - target.value).abs() <= slack,
            "{surface}: a readout prints `{text}` and opens its field on {} ({:?}), so typing \
             back what it prints moves the value by {}",
            target.value,
            target.kind,
            typed - target.value
        );
    }
}

/// A write of one unit through each readout in `drawn` moves the value its
/// field opens with by that unit: down from a value above zero and up from any
/// other, so a value resting on a floor of zero has room to move. A write that
/// records no edit may leave the value where a bound holds it. Each recorded
/// write is undone before the next.
fn assert_each_moves_by_its_unit(
    surface: &str,
    state: &Entity<AppState>,
    visual: &mut VisualTestContext,
    drawn: &[DrawnReadout],
) {
    for readout in drawn {
        let target = &readout.target;
        let delta = if target.value > 0.0 {
            -target.unit
        } else {
            target.unit
        };
        let recorded = visual.update(|_, app| {
            state.update(app, |state, cx| {
                let depth = state.history_depth();
                (target.write)(state, delta).unwrap_or_else(|err| {
                    panic!(
                        "{surface}: writing {delta} through `{}`: {err}",
                        readout.text
                    )
                });
                cx.notify();
                state.history_depth() > depth
            })
        });
        let after = redraw(visual);
        let landed = drawn_under(&after, target.key)
            .unwrap_or_else(|| {
                panic!(
                    "{surface}: the readout printing `{}` was not drawn after its write",
                    readout.text
                )
            })
            .target
            .value;
        if recorded {
            visual.update(|_, app| {
                state.update(app, |state, cx| {
                    assert!(
                        state.undo(),
                        "{surface}: the write through `{}` did not undo",
                        readout.text
                    );
                    cx.notify();
                })
            });
            visual.run_until_parked();
        }
        let moved = landed - target.value;
        let slack = 1e-3 * target.unit.abs() + 1e-5 * target.value.abs().max(1.0);
        assert!(
            (moved - delta).abs() <= slack || (!recorded && moved.abs() <= slack),
            "{surface}: writing {delta} through the readout printing `{}` moved the value its \
             field opens with from {} to {landed}",
            readout.text,
            target.value
        );
    }
}

/// The key the Inspector draws the readout of `param`'s first component
/// under: an effect slot's in Effect Controls, or a catalog row's.
fn inspector_key(state: &AppState, param: ClipParam) -> Option<MotionKey> {
    if let ClipParam::Effect {
        effect,
        param: slot,
    } = param
    {
        let instance = &state
            .selected_ui_clip_ref()?
            .effects
            .get(effect as usize)?
            .id;
        let spec = aria_models::effect_spec(instance)?;
        let slot = spec.param_at(slot as usize)?;
        let key = MotionKey::new(&slot.name, effect as usize);
        return Some(match slot.kind {
            EffectParamKind::Vec2 | EffectParamKind::Size => key.with(vec2_axes(slot.kind)[0].1),
            EffectParamKind::Point3 => key.with("x"),
            _ => key,
        });
    }
    let group = clip_param_catalog()
        .iter()
        .find(|spec| spec.param == param)?
        .group
        .to_string();
    let row = state
        .param_rows(&group)
        .into_iter()
        .find(|row| row.param == param && row.component.unwrap_or(0) == 0)?;
    Some(MotionKey::new("insp.param", &row.id))
}

/// Every property with a readout on the selected clip's timeline rows and on
/// the Inspector's open tab prints, opens and drags there as its Inspector row
/// does. A property the open tab does not draw is skipped. Returns how many
/// were compared.
fn assert_surfaces_agree(surface: &str, state: &AppState, drawn: &[DrawnReadout]) -> usize {
    let clip = state.selected_clip().expect("the fixture is selected");
    let mut compared = 0;
    for outline in state.track_outline_rows(lane_of(state, clip)) {
        let TimelineOutlineRow::Property(row) = outline else {
            continue;
        };
        // A color or a rectangle prints without a readout.
        let timeline_key = MotionKey::new("prop-val", clip.raw()).with(&row.id);
        let Some(on_timeline) = drawn_under(drawn, timeline_key) else {
            continue;
        };
        let key = inspector_key(state, row.param)
            .unwrap_or_else(|| panic!("{surface}: `{}` has no Inspector row", row.id));
        // The open Inspector tab does not draw this property.
        let Some(in_inspector) = drawn_under(drawn, key) else {
            continue;
        };
        // A vector's timeline row prints every component; its Inspector rows
        // print one each.
        let first = on_timeline.text.split(", ").next().unwrap_or_default();
        assert_eq!(
            first,
            in_inspector.text.as_ref(),
            "{surface}: `{}` prints `{}` on its timeline row and `{}` in the Inspector",
            row.id,
            on_timeline.text,
            in_inspector.text
        );
        let (timeline, inspector) = (&on_timeline.target, &in_inspector.target);
        assert!(
            (timeline.value - inspector.value).abs() <= 1e-4 * inspector.value.abs().max(1.0)
                && (timeline.unit - inspector.unit).abs() <= 1e-6 * inspector.unit.abs().max(1.0)
                && timeline.kind == inspector.kind,
            "{surface}: `{}` opens its field on {} by {} ({:?}) on its timeline row and on {} \
             by {} ({:?}) in the Inspector",
            row.id,
            timeline.value,
            timeline.unit,
            timeline.kind,
            inspector.value,
            inspector.unit,
            inspector.kind
        );
        compared += 1;
    }
    compared
}

/// The keys the Inspector draws the readouts of `params` under, each with the
/// property it prints.
fn inspector_keys(state: &AppState, params: &[ClipParam]) -> Vec<(String, MotionKey)> {
    params
        .iter()
        .map(|&param| {
            let key = inspector_key(state, param)
                .unwrap_or_else(|| panic!("{param:?} has no Inspector row"));
            (format!("{param:?}"), key)
        })
        .collect()
}

/// Draws the shell over `media` with the Inspector on `tab` and only the
/// selected clip's `group` rows open on the timeline, after `dress`, and holds
/// every readout it draws to the three rules. Each of `required` must be drawn,
/// and at least `both` properties must be drawn on both surfaces.
fn assert_readouts_hold(
    cx: &mut TestAppContext,
    (media, tab): (&str, &str),
    group: PropertyGroup,
    dress: impl FnOnce(&mut AppState),
    required: impl FnOnce(&AppState) -> Vec<(String, MotionKey)>,
    both: usize,
) {
    let (state, visual) = drawn_with(cx, &format!("units-{tab}"), media, tab, |state| {
        dress(state);
        open_only(state, group);
    });
    let drawn = redraw(visual);
    for (name, key) in visual.read(|app| required(state.read(app))) {
        assert!(
            drawn_under(&drawn, key).is_some(),
            "{tab}: the Inspector drew no readout for {name}"
        );
    }
    assert_each_reads_back(tab, &drawn);
    let compared = visual.read(|app| assert_surfaces_agree(tab, state.read(app), &drawn));
    assert!(
        compared >= both,
        "{tab}: {compared} properties were drawn on both surfaces, under the {both} this \
         scenario draws there; the timeline projects {:?} and the shell drew {:?}",
        visual.read(|app| {
            let state = state.read(app);
            let clip = state.selected_clip().expect("the fixture is selected");
            state
                .track_outline_rows(lane_of(state, clip))
                .into_iter()
                .filter_map(|row| match row {
                    TimelineOutlineRow::Property(row) => Some(row.id),
                    _ => None,
                })
                .collect::<Vec<_>>()
        }),
        drawn
            .iter()
            .map(|readout| &readout.text)
            .collect::<Vec<_>>()
    );
    assert_each_moves_by_its_unit(tab, &state, visual, &drawn);
}

#[gpui::test]
async fn every_transform_readout_reads_in_the_units_it_prints(cx: &mut TestAppContext) {
    assert_readouts_hold(
        cx,
        ("texture.png", "transform"),
        PropertyGroup::Transform,
        |_| {},
        |state| {
            inspector_keys(
                state,
                &[ClipParam::Opacity, ClipParam::Scale, ClipParam::Rotation],
            )
        },
        2,
    );
}

#[gpui::test]
async fn every_audio_readout_reads_in_the_units_it_prints(cx: &mut TestAppContext) {
    assert_readouts_hold(
        cx,
        ("fixture-sine-48k-2s.wav", "audio"),
        PropertyGroup::Audio,
        |_| {},
        |state| inspector_keys(state, &[ClipParam::Volume]),
        1,
    );
}

#[gpui::test]
async fn every_effect_readout_reads_in_the_units_it_prints(cx: &mut TestAppContext) {
    // Registers and drops an effect every other library test can list.
    crate::test_support::catalogs::own_process(|| {
        let _registration = aria_models::register_effect(EffectSpec {
            id: DISPLAYED.into(),
            label: "Every Display".into(),
            category: "Stylize".into(),
            params: displayed_slots(),
            ..Default::default()
        })
        .expect("register the effect");
        assert_readouts_hold(
            cx,
            ("texture.png", "effects"),
            PropertyGroup::Effects,
            |state| {
                state
                    .add_effect_to_selected(DISPLAYED)
                    .expect("add the effect");
            },
            |state| {
                let slots: Vec<ClipParam> = (0..displayed_slots().len())
                    .map(|at| ClipParam::Effect {
                        effect: 0,
                        param: at as u32,
                    })
                    .collect();
                inspector_keys(state, &slots)
            },
            2,
        );
    });
}

#[gpui::test]
async fn every_transition_readout_reads_in_the_units_it_prints(cx: &mut TestAppContext) {
    // Registers and drops a transition every other library test can list.
    crate::test_support::catalogs::own_process(|| {
        let _registration = aria_models::register_transition(
            TransitionSpec::new(DISPLAYED, "Every Display").with_params(displayed_slots()),
        )
        .expect("register the transition");
        assert_readouts_hold(
            cx,
            ("texture.png", "look"),
            PropertyGroup::Transform,
            |state| {
                let first = state.selected_clip().expect("the texture is selected");
                crate::app::fixtures::blend_right_junction(state, first);
                state.select_clip(first);
                state
                    .set_clip_transition(first, DISPLAYED)
                    .expect("apply the transition");
            },
            |_| {
                displayed_slots()
                    .iter()
                    .enumerate()
                    .map(|(slot, spec)| {
                        let key = match spec.kind {
                            EffectParamKind::Int => {
                                MotionKey::new("tr-param-int", slot).with(spec.name.as_str())
                            }
                            EffectParamKind::Vec2 => {
                                MotionKey::new("tr-vec2", slot).with(vec2_axes(spec.kind)[0].1)
                            }
                            _ => MotionKey::new("tr-param", slot).with(spec.name.as_str()),
                        };
                        (spec.name.clone(), key)
                    })
                    .collect()
            },
            0,
        );
    });
}
