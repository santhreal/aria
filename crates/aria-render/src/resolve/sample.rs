//! Sampling a clip's authored blocks at the tick on screen: layer styles,
//! per-character look animations, effect parameters, the crop/mirror UV rect,
//! and the contain-fit scale every placement is built from.

use super::*;
use aria_models::StoredParams as _;

/// Whether style resolution reused authored constants or evaluated curves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum StyleEvaluation {
    Constant,
    Animated,
}

/// Sample [`LayerStyles`] at a clip-local tick into canvas-pixel scene values.
///
/// Lengths (offset, blur, radius, width, padding) scale by `px` (reference →
/// canvas); colors and glow intensity pass through. `None` overall when the
/// clip has no style blocks. The common all-constant path reads each authored
/// value directly and never enters keyframe evaluation.
pub(super) fn resolve_styles(
    styles: &LayerStyles,
    local_tick: i64,
    px: f32,
) -> Option<SceneStyles> {
    resolve_styles_with_evaluation(styles, local_tick, px).map(|(styles, _)| styles)
}

pub(super) fn resolve_styles_with_evaluation(
    styles: &LayerStyles,
    local_tick: i64,
    px: f32,
) -> Option<(SceneStyles, StyleEvaluation)> {
    if styles.is_empty() {
        return None;
    }
    if let Some(styles) = resolve_constant_styles(styles, px) {
        return Some((styles, StyleEvaluation::Constant));
    }
    let styles = SceneStyles {
        shadow: styles.shadow.as_ref().map(|shadow| {
            let offset = shadow.offset.sample(local_tick);
            SceneShadow {
                rgba: shadow.rgba.sample(local_tick),
                offset: [offset[0] * px, offset[1] * px],
                blur: shadow.blur.sample(local_tick) * px,
            }
        }),
        glow: styles.glow.as_ref().map(|glow| SceneGlow {
            rgba: glow.rgba.sample(local_tick),
            radius: glow.radius.sample(local_tick) * px,
            intensity: glow.intensity.sample(local_tick),
        }),
        outline: styles.outline.as_ref().map(|outline| SceneOutline {
            rgba: outline.rgba.sample(local_tick),
            width: outline.width.sample(local_tick) * px,
        }),
        background: styles
            .background
            .as_ref()
            .map(|background| SceneBackground {
                rgba: background.rgba.sample(local_tick),
                padding: background.padding.sample(local_tick) * px,
                radius: background.radius.sample(local_tick) * px,
            }),
    };
    Some((styles, StyleEvaluation::Animated))
}

fn resolve_constant_styles(styles: &LayerStyles, px: f32) -> Option<SceneStyles> {
    let shadow = match &styles.shadow {
        Some(shadow) => {
            let offset = shadow.offset.constant()?;
            Some(SceneShadow {
                rgba: shadow.rgba.constant()?,
                offset: [offset[0] * px, offset[1] * px],
                blur: shadow.blur.constant()? * px,
            })
        }
        None => None,
    };
    let glow = match &styles.glow {
        Some(glow) => Some(SceneGlow {
            rgba: glow.rgba.constant()?,
            radius: glow.radius.constant()? * px,
            intensity: glow.intensity.constant()?,
        }),
        None => None,
    };
    let outline = match &styles.outline {
        Some(outline) => Some(SceneOutline {
            rgba: outline.rgba.constant()?,
            width: outline.width.constant()? * px,
        }),
        None => None,
    };
    let background = match &styles.background {
        Some(background) => Some(SceneBackground {
            rgba: background.rgba.constant()?,
            padding: background.padding.constant()? * px,
            radius: background.radius.constant()? * px,
        }),
        None => None,
    };
    Some(SceneStyles {
        shadow,
        glow,
        outline,
        background,
    })
}

/// Sample an active per-character look preset into resolve-time data.
pub(super) fn sample_text_animation(
    anims: LookAnimations<'_>,
    duration_ticks: i64,
    local_tick: i64,
    local_tick_f: f64,
    rate: aria_core::Rational,
) -> Option<TextAnimation> {
    let duration = duration_ticks.max(1);
    let base_window = look_animation_window_ticks(duration, rate);
    if let Some(combo) = anims.combo {
        if is_per_character(&combo.id) {
            let phase = crate::animation::combo_phase(local_tick_f, rate, combo.speed) as f32;
            let (intensity, stagger) = text_knobs(combo);
            return Some(TextAnimation {
                id: combo.id.clone(),
                slot: AnimationSlot::Combo,
                t: phase,
                intensity,
                stagger,
            });
        }
        return None;
    }

    if let Some(anim) = anims.entrance
        && is_per_character(&anim.id)
    {
        let window = scaled_ticks(base_window, anim.speed).min(duration);
        if local_tick < window {
            let raw = (local_tick_f / window as f64).clamp(0.0, 1.0) as f32;
            let eased = Easing::EaseOut.apply(raw);
            let (intensity, stagger) = text_knobs(anim);
            return Some(TextAnimation {
                id: anim.id.clone(),
                slot: AnimationSlot::In,
                t: eased,
                intensity,
                stagger,
            });
        }
    }

    if let Some(anim) = anims.exit
        && is_per_character(&anim.id)
    {
        let window = scaled_ticks(base_window, anim.speed).min(duration);
        let out_start = duration - window;
        if local_tick >= out_start {
            let raw = ((local_tick_f - out_start as f64) / (window - 1).max(1) as f64)
                .clamp(0.0, 1.0) as f32;
            let eased = Easing::EaseIn.apply(raw);
            let (intensity, stagger) = text_knobs(anim);
            return Some(TextAnimation {
                id: anim.id.clone(),
                slot: AnimationSlot::Out,
                t: eased,
                intensity,
                stagger,
            });
        }
    }

    None
}

/// Sample `clip.effects` at clip-local `tick` into compositor-ready passes.
/// `seconds` is the time since the clip started, which a registered pass
/// reads as `aria.time`. A bypassed effect and an effect no pass renders are
/// dropped. `point` is a live vec2 value for one parameter (see
/// [`ResolveOverrides::effect_vec2`]).
/// Reusable heap buffers harvested from a resolved [`Scene`] before it is
/// refilled for the next frame.
#[derive(Default)]
pub(crate) struct ResolvePool {
    effects: Vec<Vec<ResolvedPass>>,
    params: Vec<Vec<f32>>,
    strings: Vec<String>,
}

impl ResolvePool {
    pub(crate) fn recycle_scene(&mut self, scene: &mut Scene) {
        for layer in scene.layers.drain(..).chain(scene.references.drain(..)) {
            self.recycle_layer(layer);
        }
    }

    fn recycle_layer(&mut self, mut layer: SceneLayer) {
        for mut pass in layer.effects.drain(..) {
            if pass.params.capacity() > 0 && self.params.len() < 32 {
                pass.params.clear();
                self.params.push(pass.params);
            }
        }
        if layer.effects.capacity() > 0 && self.effects.len() < 32 {
            self.effects.push(layer.effects);
        }
        match layer.source {
            LayerSource::Text {
                mut content, style, ..
            } => {
                if content.capacity() > 0 && self.strings.len() < 16 {
                    content.clear();
                    self.strings.push(content);
                }
                if let aria_text::FontFamily::Named(mut font) = style.family
                    && font.capacity() > 0
                    && self.strings.len() < 16
                {
                    font.clear();
                    self.strings.push(font);
                }
            }
            LayerSource::Transition {
                outgoing, incoming, ..
            } => {
                self.recycle_layer(*outgoing);
                self.recycle_layer(*incoming);
            }
            LayerSource::Matte { layers, .. } => {
                let [a, b] = *layers;
                self.recycle_layer(a);
                self.recycle_layer(b);
            }
            _ => {}
        }
    }

    pub(crate) fn take_string(&mut self) -> String {
        self.strings.pop().unwrap_or_default()
    }
}

pub(super) fn resolve_effects_pooled(
    clip: &aria_models::Clip,
    tick: i64,
    seconds: f32,
    point: Option<(aria_models::ClipId, usize, usize, [f32; 2])>,
    pool: &mut ResolvePool,
) -> Vec<ResolvedPass> {
    if clip.effects.iter().all(|fx| fx.bypass) {
        return Vec::new();
    }
    let tick_f = tick as f64;
    let mut out = pool.effects.pop().unwrap_or_default();
    out.clear();
    for (index, fx) in clip.effects.iter().enumerate() {
        if fx.bypass {
            continue;
        }
        let Some(id) = aria_compositor::pass_id(&fx.effect_id) else {
            continue;
        };
        let point = point
            .filter(|(owner, effect, ..)| *owner == clip.id && *effect == index)
            .map(|(_, _, slot, value)| (slot, value));
        if let Ok(pass) = pack_effect(fx, tick_f, id, seconds, point, pool) {
            out.push(pass);
        }
    }
    out
}

fn pack_effect(
    fx: &EffectInstance,
    tick: f64,
    id: aria_compositor::PassId,
    seconds: f32,
    point: Option<(usize, [f32; 2])>,
    pool: &mut ResolvePool,
) -> Result<ResolvedPass, aria_models::ModelError> {
    let mut params = pool.params.pop().unwrap_or_default();
    fx.flatten_floats_into(&fx.spec()?.params, tick, point, &mut params);
    // Shipped passes do not read the time or CPU instance, and a registered
    // effect that does not declare `time_dependent` reads time 0; leaving
    // them empty keeps equal frames equal so the scene cache holds across them.
    let reads_time = match &id {
        aria_compositor::PassId::Registered(registered) => Some(registered.is_time_dependent()),
        aria_compositor::PassId::Shipped(_) => None,
    };
    let Some(reads_time) = reads_time else {
        return Ok(ResolvedPass::new(id, params));
    };
    let seconds = if reads_time { seconds } else { 0.0 };
    Ok(ResolvedPass::new(id, params)
        .with_time(seconds)
        .with_instance(fx.clone()))
}

/// UV rect from a clip's crop sampled at a clip-relative tick, with axes
/// reversed for mirror flags.
pub(super) fn crop_flip_uv(
    clip: &aria_models::Clip,
    tick: i64,
    overrides: ResolveOverrides<'_>,
) -> [f32; 4] {
    let (c, flip_h, flip_v) = match overrides.crop {
        Some((id, crop, flip_h, flip_v)) if id == clip.id => (crop, flip_h, flip_v),
        _ => (clip.crop.sample(tick), clip.flip_h, clip.flip_v),
    };
    let (mut u0, mut u1) = (c.x, c.x + c.w);
    let (mut v0, mut v1) = (c.y, c.y + c.h);
    if flip_h {
        core::mem::swap(&mut u0, &mut u1);
    }
    if flip_v {
        core::mem::swap(&mut v0, &mut v1);
    }
    [u0, v0, u1, v1]
}

/// Uniform "contain" scale fitting `nw`×`nh` content inside a `cw`×`ch` canvas.
pub(super) fn fit_scale(nw: f32, nh: f32, cw: f32, ch: f32) -> f32 {
    if nw <= 0.0 || nh <= 0.0 {
        return 1.0;
    }
    (cw / nw).min(ch / nh)
}

#[cfg(test)]
mod style_tests {
    use super::*;
    use aria_models::LayerShadow;

    #[test]
    fn constant_styles_bypass_curve_evaluation() {
        let styles = LayerStyles {
            shadow: Some(LayerShadow::default()),
            ..LayerStyles::default()
        };
        let (at_start, start_evaluation) =
            resolve_styles_with_evaluation(&styles, 0, 0.5).expect("style");
        let (at_late_tick, late_evaluation) =
            resolve_styles_with_evaluation(&styles, 10_000, 0.5).expect("style");
        assert_eq!(start_evaluation, StyleEvaluation::Constant);
        assert_eq!(late_evaluation, StyleEvaluation::Constant);
        assert_eq!(at_start, at_late_tick);
    }

    #[test]
    fn one_keyframed_style_selects_curve_evaluation() {
        let mut shadow = LayerShadow::default();
        shadow.blur.set_keyframe(0, 8.0, Easing::Linear);
        let styles = LayerStyles {
            shadow: Some(shadow),
            ..LayerStyles::default()
        };
        let (_, evaluation) = resolve_styles_with_evaluation(&styles, 5, 1.0).expect("style");
        assert_eq!(evaluation, StyleEvaluation::Animated);
    }

    #[test]
    fn text_combo_uses_the_shared_wrapped_phase() {
        let combo = aria_models::AnimationRef::new("typewriter");
        let period = scaled_ticks(
            aria_models::look_animation_combo_period_ticks(aria_core::Rational::FPS_24),
            combo.speed,
        ) as f64;
        let sampled = sample_text_animation(
            LookAnimations {
                entrance: None,
                exit: None,
                combo: Some(&combo),
            },
            100,
            0,
            -0.75 * period,
            aria_core::Rational::FPS_24,
        )
        .expect("text combo");
        assert!((sampled.t - 0.25).abs() < f32::EPSILON);
    }
}
