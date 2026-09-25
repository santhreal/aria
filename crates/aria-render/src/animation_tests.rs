//! Easing-curve and animation-window behaviour for `animation.rs`.

use super::*;
use aria_core::Rational;
use aria_models::{AnimationRef, Clip, Generator, TimeRange, animation_catalog, animation_spec};

const R24: Rational = Rational::new(24, 1);

fn solid_clip(duration: i64) -> Clip {
    Clip::generated(
        Generator::SolidColor {
            rgba: [255, 0, 0, 255],
        },
        TimeRange::at_rate(0, duration, R24),
    )
}

/// Every shipped catalog id has an implementation, and it is the right one.
///
/// The class this closes: a data-file id no sampler matches used to fall
/// through to identity, so applying it looked like a working edit and moved
/// nothing. Enumerating the catalog at run time means a new entry is red
/// here until it is implemented, and `renders_animation` is the same answer
/// the UI shows.
#[test]
fn every_catalog_id_is_implemented_by_the_sampler_that_owns_it() {
    assert!(!animation_catalog().is_empty());
    for spec in animation_catalog() {
        let delta = match spec.slot {
            AnimationSlot::In => sample_entrance(&spec.id, 0.5),
            AnimationSlot::Out => sample_exit(&spec.id, 0.92),
            AnimationSlot::Combo => {
                let phase = if spec.id == "typewriter" { 0.9 } else { 0.07 };
                sample_combo(&spec.id, phase)
            }
        };
        if spec.text_only {
            // Whole-layer sampler must not claim a per-character preset.
            assert_eq!(delta, None, "'{}' reached the layer sampler", spec.id);
        } else {
            let delta = delta.unwrap_or_else(|| panic!("'{}' has no sampler", spec.id));
            assert_ne!(
                delta,
                AnimationDelta::IDENTITY,
                "animation '{}' produced identity at sample",
                spec.id
            );
        }
        assert!(
            renders_animation(&spec.id, spec.slot),
            "'{}' is offered but nothing renders it",
            spec.id
        );
        assert!(animation_spec(&spec.id).is_some());
    }
}

#[test]
fn an_id_no_sampler_matches_does_not_render() {
    for slot in [AnimationSlot::In, AnimationSlot::Out, AnimationSlot::Combo] {
        assert!(!renders_animation("not_a_preset", slot));
    }
    // A real id in the wrong slot is just as unrendered.
    assert!(!renders_animation("fade_in", AnimationSlot::Out));
    assert!(!renders_animation("char_fade_in", AnimationSlot::Combo));
}

#[test]
fn fade_in_ramps_opacity_from_zero_at_start() {
    let mut clip = solid_clip(48);
    clip.animation_in = Some(AnimationRef::new("fade_in"));
    let start = apply_look_animations(
        LookAnimations::of(&clip),
        clip.timeline.duration.value,
        ClipTransform::IDENTITY,
        0,
        0.0,
        R24,
    );
    assert!(start.opacity < 0.05);
    let mid = apply_look_animations(
        LookAnimations::of(&clip),
        clip.timeline.duration.value,
        ClipTransform::IDENTITY,
        24,
        24.0,
        R24,
    );
    assert!(mid.opacity > 0.5);
}

#[test]
fn intensity_zero_is_identity() {
    let mut clip = solid_clip(48);
    let mut anim = AnimationRef::new("slide_up");
    anim.intensity = 0.0;
    clip.animation_in = Some(anim);
    let start = apply_look_animations(
        LookAnimations::of(&clip),
        clip.timeline.duration.value,
        ClipTransform::IDENTITY,
        0,
        0.0,
        R24,
    );
    assert_eq!(start.position, [0.0, 0.0]);
    assert!((start.opacity - 1.0).abs() < 1e-5);
}

#[test]
fn speed_shortens_entrance_window() {
    let mut slow = solid_clip(48);
    slow.animation_in = Some(AnimationRef::new("fade_in"));
    let mut fast = solid_clip(48);
    let mut anim = AnimationRef::new("fade_in");
    anim.speed = 2.0;
    fast.animation_in = Some(anim);
    // At tick 6 a 2×-speed entrance (window ≈ 6) has finished; default
    // (window ≈ 12) is still mid-fade.
    let slow_mid = apply_look_animations(
        LookAnimations::of(&slow),
        slow.timeline.duration.value,
        ClipTransform::IDENTITY,
        6,
        6.0,
        R24,
    );
    let fast_mid = apply_look_animations(
        LookAnimations::of(&fast),
        fast.timeline.duration.value,
        ClipTransform::IDENTITY,
        6,
        6.0,
        R24,
    );
    assert!(fast_mid.opacity > slow_mid.opacity);
    assert!((fast_mid.opacity - 1.0).abs() < 1e-5);
}

#[test]
fn slide_up_starts_below() {
    let mut clip = solid_clip(48);
    clip.animation_in = Some(AnimationRef::new("slide_up"));
    let start = apply_look_animations(
        LookAnimations::of(&clip),
        clip.timeline.duration.value,
        ClipTransform::IDENTITY,
        0,
        0.0,
        R24,
    );
    assert!(start.position[1] > 0.0);
    let mid = apply_look_animations(
        LookAnimations::of(&clip),
        clip.timeline.duration.value,
        ClipTransform::IDENTITY,
        24,
        24.0,
        R24,
    );
    assert!(mid.position[1] < start.position[1]);
}

#[test]
fn zoom_in_starts_small() {
    let mut clip = solid_clip(48);
    clip.animation_in = Some(AnimationRef::new("zoom_in"));
    let start = apply_look_animations(
        LookAnimations::of(&clip),
        clip.timeline.duration.value,
        ClipTransform::IDENTITY,
        0,
        0.0,
        R24,
    );
    assert!(start.scale.x < 0.5);
}

#[test]
fn fade_out_dims_at_tail() {
    let mut clip = solid_clip(48);
    clip.animation_out = Some(AnimationRef::new("fade_out"));
    let tail = apply_look_animations(
        LookAnimations::of(&clip),
        clip.timeline.duration.value,
        ClipTransform::IDENTITY,
        47,
        47.0,
        R24,
    );
    assert!(tail.opacity < 0.5);
    let mid = apply_look_animations(
        LookAnimations::of(&clip),
        clip.timeline.duration.value,
        ClipTransform::IDENTITY,
        20,
        20.0,
        R24,
    );
    assert!((mid.opacity - 1.0).abs() < 1e-5);
}

#[test]
fn combo_supersedes_in_out() {
    let mut clip = solid_clip(48);
    clip.animation_in = Some(AnimationRef::new("fade_in"));
    clip.animation_out = Some(AnimationRef::new("fade_out"));
    clip.animation_combo = Some(AnimationRef::new("pulse"));
    let a = apply_look_animations(
        LookAnimations::of(&clip),
        clip.timeline.duration.value,
        ClipTransform::IDENTITY,
        0,
        0.0,
        R24,
    );
    // Pulse at phase 0: scale = 1 (sin 0); not a fade.
    assert!((a.opacity - 1.0).abs() < 1e-5);
}

#[test]
fn pulse_varies_scale_over_period() {
    let mut clip = solid_clip(48);
    clip.animation_combo = Some(AnimationRef::new("pulse"));
    let a = apply_look_animations(
        LookAnimations::of(&clip),
        clip.timeline.duration.value,
        ClipTransform::IDENTITY,
        0,
        0.0,
        R24,
    );
    let b = apply_look_animations(
        LookAnimations::of(&clip),
        clip.timeline.duration.value,
        ClipTransform::IDENTITY,
        6,
        6.0,
        R24,
    );
    assert!((a.scale.x - b.scale.x).abs() > 0.01);
}
#[test]
fn combo_phase_wraps_identically_before_and_after_zero() {
    let period = scaled_ticks(look_animation_combo_period_ticks(R24), 1.0) as f64;
    assert_eq!(combo_phase(0.25 * period, R24, 1.0), 0.25);
    assert_eq!(combo_phase(-0.75 * period, R24, 1.0), 0.25);
    assert!((0.0..1.0).contains(&combo_phase(-1.0, R24, 1.0)));
}
