//! Resolve persisted look-animation presets into transform/opacity deltas.

use aria_core::Rational;
use aria_models::{
    AnimationRef, AnimationSlot, ClipTransform, Easing, LookAnimations, animation_spec,
    look_animation_combo_period_ticks, look_animation_window_ticks,
};

/// Normalized slide distance as a fraction of canvas height (+y down).
const SLIDE_OFFSET: f32 = 0.18;

/// Multiplicative transform delta sampled from one animation preset.
#[derive(Debug, Clone, Copy, PartialEq)]
struct AnimationDelta {
    position: [f32; 2],
    scale: f32,
    rotation: f32,
    opacity: f32,
}

impl AnimationDelta {
    const IDENTITY: Self = Self {
        position: [0.0, 0.0],
        scale: 1.0,
        rotation: 0.0,
        opacity: 1.0,
    };

    /// Scale motion / opacity swing by `intensity` (`0` = identity, `1` = catalog).
    fn with_intensity(self, intensity: f32) -> Self {
        let i = intensity.clamp(0.0, 2.0);
        Self {
            position: [self.position[0] * i, self.position[1] * i],
            scale: 1.0 + (self.scale - 1.0) * i,
            rotation: self.rotation * i,
            opacity: 1.0 + (self.opacity - 1.0) * i,
        }
    }
}

/// Shrink (or stretch) a tick window by animation speed (`1` = catalog).
pub(crate) fn scaled_ticks(base: i64, speed: f32) -> i64 {
    let speed = speed.max(0.25);
    ((base as f32) / speed).round().max(1.0) as i64
}

/// Stable looping phase shared by whole-layer and per-character combo effects.
pub(crate) fn combo_phase(local_tick_f: f64, rate: Rational, speed: f32) -> f64 {
    let period = scaled_ticks(look_animation_combo_period_ticks(rate), speed) as f64;
    local_tick_f.rem_euclid(period) / period
}

/// Fold look-animation presets onto a clip's sampled transform at resolve time.
///
/// The slots arrive as [`LookAnimations`] rather than being read off the clip, so a
/// hover preview can substitute one without cloning the clip or committing an edit.
pub(crate) fn apply_look_animations(
    anims: LookAnimations<'_>,
    duration_ticks: i64,
    base: ClipTransform,
    local_tick: i64,
    local_tick_f: f64,
    rate: Rational,
) -> ClipTransform {
    let duration = duration_ticks.max(1);
    let base_window = look_animation_window_ticks(duration, rate);
    let mut deltas = Vec::with_capacity(2);

    if let Some(combo) = anims.combo {
        // Per-character (text_only) presets are sampled into TextAnimation at
        // resolve time and applied per glyph — skip the whole-layer path.
        if !is_per_character(&combo.id) {
            let phase = combo_phase(local_tick_f, rate, combo.speed);
            if let Some(delta) = sample_combo(&combo.id, phase) {
                deltas.push(delta.with_intensity(combo.intensity));
            }
        }
    } else {
        if let Some(anim) = anims.entrance
            && !is_per_character(&anim.id)
        {
            let window = scaled_ticks(base_window, anim.speed).min(duration);
            if local_tick < window {
                let raw = (local_tick_f / window as f64).clamp(0.0, 1.0);
                let eased = f64::from(Easing::EaseOut.apply(raw as f32));
                if let Some(delta) = sample_entrance(&anim.id, eased) {
                    deltas.push(delta.with_intensity(anim.intensity));
                }
            }
        }
        if let Some(anim) = anims.exit
            && !is_per_character(&anim.id)
        {
            let window = scaled_ticks(base_window, anim.speed).min(duration);
            let out_start = duration - window;
            if local_tick >= out_start {
                let raw = ((local_tick_f - out_start as f64) / (window - 1).max(1) as f64)
                    .clamp(0.0, 1.0);
                let eased = f64::from(Easing::EaseIn.apply(raw as f32));
                if let Some(delta) = sample_exit(&anim.id, eased) {
                    deltas.push(delta.with_intensity(anim.intensity));
                }
            }
        }
    }

    if deltas.is_empty() {
        return base;
    }
    compose_transform(base, &deltas)
}

fn compose_transform(base: ClipTransform, deltas: &[AnimationDelta]) -> ClipTransform {
    let mut xf = base;
    for delta in deltas {
        xf.position[0] += delta.position[0];
        xf.position[1] += delta.position[1];
        xf.scale *= delta.scale; // uniform look-animation delta on both axes
        xf.rotation += delta.rotation;
        xf.opacity = (xf.opacity * delta.opacity).clamp(0.0, 1.0);
    }
    xf
}

/// Text-only catalog presets animate per character on the glyph path.
pub(crate) fn is_per_character(id: &str) -> bool {
    animation_spec(id).is_some_and(|s| s.text_only)
}

/// Knobs from an [`AnimationRef`] for the text animation path.
pub(crate) fn text_knobs(anim: &AnimationRef) -> (f32, f32) {
    (anim.intensity, anim.stagger)
}

/// True when this build can animate `id` in `slot`.
///
/// The answer is taken from the sampler that would run, not from a second list:
/// a text-only preset is asked of the per-character path in
/// `render::text_anim`, everything else of the whole-layer samplers
/// below. So a catalog id no sampler implements reports false here, and the UI
/// can say so instead of applying a preset that moves nothing.
pub fn renders_animation(id: &str, slot: AnimationSlot) -> bool {
    if is_per_character(id) {
        return crate::render::text_anim::renders_cluster(id, slot);
    }
    // Mid-window progress: every sampler either matches the id at any t or
    // matches it at none, so one probe answers for the whole window.
    match slot {
        AnimationSlot::In => sample_entrance(id, 0.5).is_some(),
        AnimationSlot::Out => sample_exit(id, 0.5).is_some(),
        AnimationSlot::Combo => sample_combo(id, 0.5).is_some(),
    }
}

/// The whole-layer entrance delta for `id`, or `None` when this build has no
/// implementation for it.
fn sample_entrance(id: &str, t: f64) -> Option<AnimationDelta> {
    let t = t.clamp(0.0, 1.0);
    let inv = 1.0 - t;
    Some(match id {
        "fade_in" => AnimationDelta {
            opacity: t as f32,
            ..AnimationDelta::IDENTITY
        },
        "slide_up" => AnimationDelta {
            position: [0.0, inv as f32 * SLIDE_OFFSET],
            opacity: t as f32,
            ..AnimationDelta::IDENTITY
        },
        "zoom_in" => AnimationDelta {
            scale: (0.25 + 0.75 * t) as f32,
            opacity: t as f32,
            ..AnimationDelta::IDENTITY
        },
        "spin_in" => AnimationDelta {
            rotation: (inv * -360.0) as f32,
            opacity: t as f32,
            scale: (0.5 + 0.5 * t) as f32,
            ..AnimationDelta::IDENTITY
        },
        "bounce" => AnimationDelta {
            scale: bounce_scale(t) as f32,
            opacity: t as f32,
            position: [0.0, inv as f32 * SLIDE_OFFSET * 0.35],
            ..AnimationDelta::IDENTITY
        },
        _ => return None,
    })
}

/// The whole-layer exit delta for `id`, or `None` when there is no implementation.
fn sample_exit(id: &str, t: f64) -> Option<AnimationDelta> {
    let t = t.clamp(0.0, 1.0);
    let inv = 1.0 - t;
    Some(match id {
        "fade_out" => AnimationDelta {
            opacity: inv as f32,
            ..AnimationDelta::IDENTITY
        },
        "slide_down" => AnimationDelta {
            position: [0.0, t as f32 * SLIDE_OFFSET],
            opacity: inv as f32,
            ..AnimationDelta::IDENTITY
        },
        "zoom_out" => AnimationDelta {
            scale: (1.0 - 0.75 * t) as f32,
            opacity: inv as f32,
            ..AnimationDelta::IDENTITY
        },
        "spin_out" => AnimationDelta {
            rotation: (t * 360.0) as f32,
            opacity: inv as f32,
            scale: (1.0 - 0.5 * t) as f32,
            ..AnimationDelta::IDENTITY
        },
        "drop" => AnimationDelta {
            position: [0.0, t as f32 * SLIDE_OFFSET * 1.4],
            opacity: inv as f32,
            scale: (1.0 - 0.35 * t) as f32,
            ..AnimationDelta::IDENTITY
        },
        _ => return None,
    })
}

/// The whole-layer looping delta for `id`, or `None` when there is no
/// implementation. Text-only presets are not implemented here by design.
fn sample_combo(id: &str, phase: f64) -> Option<AnimationDelta> {
    let phase = phase.fract();
    let wave = (phase * std::f64::consts::TAU).sin();
    Some(match id {
        "pulse" => AnimationDelta {
            scale: (1.0 + 0.08 * wave) as f32,
            ..AnimationDelta::IDENTITY
        },
        "rock" => AnimationDelta {
            rotation: (6.0 * wave) as f32,
            ..AnimationDelta::IDENTITY
        },
        "swing" => AnimationDelta {
            rotation: (12.0 * (phase * std::f64::consts::PI).sin()) as f32,
            ..AnimationDelta::IDENTITY
        },
        "flicker" => AnimationDelta {
            opacity: if (phase * 8.0).fract() < 0.5 {
                1.0
            } else {
                0.35
            },
            ..AnimationDelta::IDENTITY
        },
        "breathe" => AnimationDelta {
            scale: (1.0 + 0.05 * wave) as f32,
            opacity: (0.85 + 0.15 * ((phase * std::f64::consts::PI).sin() + 1.0) * 0.5) as f32,
            ..AnimationDelta::IDENTITY
        },
        // text_only presets (typewriter, text_fade, …) are handled by
        // `render::text_anim` — they must not reach this whole-layer sampler.
        _ => return None,
    })
}

/// Penner-style ease-out bounce for the entrance preset.
fn bounce_scale(t: f64) -> f64 {
    if t < 1.0 / 2.75 {
        7.5625 * t * t
    } else if t < 2.0 / 2.75 {
        let t = t - 1.5 / 2.75;
        7.5625 * t * t + 0.75
    } else if t < 2.5 / 2.75 {
        let t = t - 2.25 / 2.75;
        7.5625 * t * t + 0.9375
    } else {
        let t = t - 2.625 / 2.75;
        7.5625 * t * t + 0.984375
    }
}

#[cfg(test)]
#[path = "animation_tests.rs"]
mod tests;
