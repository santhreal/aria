//! Every number an effect, a transition or an audio insert declares bounds for
//! draws a value track, filled to where the value stands between the bounds.
//!
//! Closes the class "a bounded number row with no value track". The rows the
//! property catalog generates drew a track along the range the catalog
//! declares, while the number slots of an effect and of a transition, and the
//! parameters of an audio insert, drew none, though each declares its bounds
//! and a plugin manifest's default display for a number is a slider. An effect
//! and a transition with a slot of every [`EffectParamKind`], and a shipped
//! insert, are drawn. Every slot of a number kind draws a track whose fill
//! covers the value's share of the slot's bounds, an integer's being its
//! integer bounds and not its float ones, no slot of another kind draws one,
//! and every insert parameter's track is filled to its value's share of its
//! range. A kind added to `EffectParamKind` does not build until
//! [`draws_a_track`] places it.
//!
//! Not covered: a number row drawn with `unkeyed_row`, which takes no range;
//! a row the property catalog generates, which draws its track through the
//! same `Travel::along`; of the keyed axes of an effect's vector slot, which
//! share one debug selector, only the last drawn is measured; and an insert
//! other than the shipped gain.

use aria_models::{
    AudioInsertTarget, ClipParam, EffectParamKind, EffectParamSpec, EffectSpec, ParamValue,
    TransitionSpec,
};
use gpui::{TestAppContext, VisualTestContext};

use super::columns_tests::{EVERY_KIND, every_kind};
use super::strip_tests::drawn_with;
use super::tabs::effects::effects_params::vec2_axes;
use crate::interned::intern;

/// Whether a slot of `kind` draws a value track. No wildcard arm: a new kind
/// does not build until it is placed.
fn draws_a_track(kind: EffectParamKind) -> bool {
    match kind {
        EffectParamKind::Scalar
        | EffectParamKind::Int
        | EffectParamKind::Vec2
        | EffectParamKind::Point3
        | EffectParamKind::Size
        | EffectParamKind::Angle => true,
        EffectParamKind::Bool
        | EffectParamKind::Choice
        | EffectParamKind::Color
        | EffectParamKind::Curve
        | EffectParamKind::Gradient
        | EffectParamKind::Text
        | EffectParamKind::Font
        | EffectParamKind::File
        | EffectParamKind::Layer => false,
    }
}

/// The bounds the manifest declares for a number slot: the integer bounds of
/// an integer, the float bounds of any other.
fn bounds(pspec: &EffectParamSpec) -> (f32, f32) {
    match pspec.kind {
        EffectParamKind::Int => (pspec.int_min as f32, pspec.int_max as f32),
        _ => (pspec.min, pspec.max),
    }
}

/// The share of `bounds` that `value` stands at.
fn share(value: f32, (min, max): (f32, f32)) -> f32 {
    assert!(max > min, "the fixture declares bounds {min}..{max}");
    (value - min) / (max - min)
}

/// A slot of every kind, labelled for its `holder`, with the integer's float
/// bounds left as a manifest that states only `int_min` and `int_max` reads
/// them, so an integer row drawn against its float bounds draws a fill of its
/// own.
fn slots(holder: &str) -> Vec<EffectParamSpec> {
    let unset = EffectParamSpec::default();
    let mut params = every_kind(holder);
    for pspec in params.iter_mut().filter(|p| p.kind == EffectParamKind::Int) {
        (pspec.min, pspec.max) = (unset.min, unset.max);
    }
    params
}

/// The default of each axis of `pspec`, with the axis's name; one unnamed
/// axis for a single number, none for a kind no row draws as numbers.
fn axes(pspec: &EffectParamSpec) -> Vec<(Option<&'static str>, f32)> {
    match pspec.default_param_value() {
        Some(ParamValue::Scalar(value)) => vec![(None, value)],
        Some(ParamValue::Vec2(values)) => vec2_axes(pspec.kind)
            .into_iter()
            .map(|(axis, name)| (Some(name), values[axis]))
            .collect(),
        Some(ParamValue::Vec3(values)) => ["x", "y", "z"]
            .into_iter()
            .zip(values)
            .map(|(name, value)| (Some(name), value))
            .collect(),
        _ => Vec::new(),
    }
}

/// The row whose cells' debug selectors end in `cell` draws a track filled to
/// `expected` of its width, or, at `None`, draws none.
fn reads(visual: &mut VisualTestContext, holder: &str, cell: &str, expected: Option<f32>) {
    let track = visual.debug_bounds(intern(&format!("insp-track-{cell}")));
    let fill = visual.debug_bounds(intern(&format!("insp-fill-{cell}")));
    match (track.zip(fill), expected) {
        (None, None) => {}
        (Some((track, fill)), Some(share)) => {
            let (width, filled) = (f32::from(track.size.width), f32::from(fill.size.width));
            assert!(
                (filled - share * width).abs() < 1.0,
                "the {holder}'s {cell} track is filled {filled} px of {width} px, and its value stands at {share} of its bounds"
            );
        }
        (Some(_), None) => {
            panic!("the {holder}'s {cell} draws a value track, and its kind draws none")
        }
        (None, Some(_)) => panic!("the {holder}'s {cell} draws no value track"),
    }
}

#[gpui::test]
async fn every_number_slot_of_an_effect_draws_its_value_on_a_track(cx: &mut TestAppContext) {
    // Registers and drops an effect every other library test can list.
    crate::test_support::catalogs::own_process(|| {
        let params = slots("effect");
        let _registration = aria_models::register_effect(EffectSpec {
            id: EVERY_KIND.into(),
            label: "Every Kind".into(),
            category: "Stylize".into(),
            params: params.clone(),
            ..Default::default()
        })
        .expect("register the effect");
        let (_, visual) = drawn_with(cx, "slot-track-effect", "texture.png", "effects", |state| {
            state
                .add_effect_to_selected(EVERY_KIND)
                .expect("add the effect");
        });
        for (slot, pspec) in params.iter().enumerate() {
            // A slot that can be keyed stands in its property's columns.
            let keyed = format!(
                "{:?}",
                ClipParam::Effect {
                    effect: 0,
                    param: slot as u32,
                }
            );
            if !draws_a_track(pspec.kind) {
                reads(visual, "effect", &keyed, None);
                reads(visual, "effect", &pspec.label, None);
                continue;
            }
            // The keyed axes of one slot share its selector; the last drawn
            // is the one read.
            let &(_, value) = axes(pspec).last().expect("a number slot has an axis");
            reads(visual, "effect", &keyed, Some(share(value, bounds(pspec))));
        }
    });
}

#[gpui::test]
async fn every_number_slot_of_a_transition_draws_its_value_on_a_track(cx: &mut TestAppContext) {
    // Registers and drops a transition every other library test can list.
    crate::test_support::catalogs::own_process(|| {
        let params = slots("transition");
        let _registration = aria_models::register_transition(
            TransitionSpec::new(EVERY_KIND, "Every Kind").with_params(params.clone()),
        )
        .expect("register the transition");
        let (_, visual) = drawn_with(
            cx,
            "slot-track-transition",
            "texture.png",
            "look",
            |state| {
                let first = state.selected_clip().expect("the texture is selected");
                crate::app::fixtures::blend_right_junction(state, first);
                state.select_clip(first);
                state
                    .set_clip_transition(first, EVERY_KIND)
                    .expect("apply the transition");
            },
        );
        for pspec in &params {
            if !draws_a_track(pspec.kind) {
                reads(visual, "transition", &pspec.label, None);
                continue;
            }
            let bounds = bounds(pspec);
            for (axis, value) in axes(pspec) {
                let cell = axis.map_or_else(
                    || pspec.label.clone(),
                    |axis| format!("{} {axis}", pspec.label),
                );
                reads(visual, "transition", &cell, Some(share(value, bounds)));
            }
        }
    });
}

#[gpui::test]
async fn every_insert_parameter_draws_its_value_on_a_track(cx: &mut TestAppContext) {
    let (state, visual) = drawn_with(
        cx,
        "slot-track-insert",
        "fixture-sine-48k-2s.wav",
        "audio",
        |state| {
            let clip = state.selected_clip().expect("the sound is selected");
            state
                .add_audio_insert(AudioInsertTarget::Clip(clip), "gain".into())
                .expect("add a gain insert");
        },
    );
    let params = visual.read(|app| {
        state
            .read(app)
            .selected_ui_clip_ref()
            .map(|clip| {
                clip.audio_inserts
                    .iter()
                    .flat_map(|insert| insert.params.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    });
    assert!(!params.is_empty(), "the gain insert projects no parameter");
    for param in params {
        let at = share(param.value, (param.min, param.max));
        reads(visual, "insert", &param.label, Some(at));
    }
}
