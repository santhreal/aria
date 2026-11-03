//! Every keyframed curve a preset holds is held to `MAX_PRESET_KEYFRAMES`.
//!
//! Closes the class where `Preset::validate` checks a curve on only some paths,
//! so a preset whose over-long curve sits in an unchecked slot loads: an
//! effect's compositing opacity was checked once per scalar parameter, and an
//! effect without one loaded any number of opacity keyframes. Each case holds
//! one over-long curve and nothing else, so a slot whose check depends on a
//! sibling slot fails here. Covers every curve slot of an effect instance, an
//! audio insert's parameters, and a property. Does not cover a curve field
//! added to `EffectInstance` later: that field needs a case here.

use super::*;
use crate::param::{Easing, Keyframe};

/// A curve of `len` keyframes on consecutive ticks, every value `value`.
fn curve<T: Clone>(len: usize, value: T) -> Param<T> {
    Param::Keyframed {
        keyframes: (0_i64..)
            .take(len)
            .map(|tick| Keyframe::new(tick, value.clone(), Easing::Linear))
            .collect(),
    }
}

fn effect_preset(effect: EffectInstance) -> Preset {
    Preset::new("Long", vec![effect], Vec::new(), Vec::new())
}

/// One preset per curve slot, each with a single `len`-keyframe curve, and the
/// text the refusal names that slot by.
fn one_long_curve_per_slot(len: usize) -> Vec<(&'static str, Preset)> {
    let effect = || EffectInstance::new("vignette");
    let mut opacity = effect();
    opacity.opacity = curve(len, 1.0);
    let mut scalar = effect();
    scalar.params.insert("amount".into(), curve(len, 0.5));
    let mut color = effect();
    color
        .color_params
        .insert("tint".into(), curve(len, [255_u8; 4]));
    let mut linear = effect();
    linear
        .linear_color_params
        .insert("glow".into(), curve(len, [1.0_f32; 4]));
    let mut vec2 = effect();
    vec2.vec2_params
        .insert("center".into(), curve(len, [0.5_f32; 2]));
    let mut vec3 = effect();
    vec3.vec3_params
        .insert("point".into(), curve(len, [0.0_f32; 3]));
    let mut gain = AudioEffectInstance::new("gain");
    gain.params.insert("level".into(), curve(len, 0.0));
    vec![
        (
            "effect `vignette` (index 0) opacity",
            effect_preset(opacity),
        ),
        ("parameter `amount`", effect_preset(scalar)),
        ("parameter `tint`", effect_preset(color)),
        ("parameter `glow`", effect_preset(linear)),
        ("parameter `center`", effect_preset(vec2)),
        ("parameter `point`", effect_preset(vec3)),
        (
            "audio effect `gain` (index 0) parameter `level`",
            Preset::new("Long", Vec::new(), vec![gain], Vec::new()),
        ),
        (
            "property `opacity`",
            Preset::new(
                "Long",
                Vec::new(),
                Vec::new(),
                vec![PresetProperty {
                    id: "opacity".into(),
                    curve: curve(len, ParamValue::Scalar(1.0)),
                }],
            ),
        ),
    ]
}

#[test]
fn a_curve_past_the_limit_is_refused_in_every_slot_by_name() {
    let over = MAX_PRESET_KEYFRAMES + 1;
    let count = format!("curve has {over} keyframes");
    for (slot, preset) in one_long_curve_per_slot(over) {
        let err = match preset.validate() {
            Ok(()) => panic!("{slot}: a {over}-keyframe curve loaded"),
            Err(err) => err.to_string(),
        };
        assert!(
            err.contains(slot) && err.contains(&count),
            "{slot}: got {err}"
        );
    }
}

#[test]
fn a_curve_at_the_limit_loads() {
    let mut opacity = EffectInstance::new("vignette");
    opacity.opacity = curve(MAX_PRESET_KEYFRAMES, 1.0);
    let property = PresetProperty {
        id: "opacity".into(),
        curve: curve(MAX_PRESET_KEYFRAMES, ParamValue::Scalar(1.0)),
    };
    let preset = Preset::new("Limit", vec![opacity], Vec::new(), vec![property]);
    if let Err(err) = preset.validate() {
        panic!("a {MAX_PRESET_KEYFRAMES}-keyframe curve was refused: {err}");
    }
}
