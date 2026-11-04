//! Per-character text animation: sample a look preset into one transform /
//! opacity delta per shaping cluster.
//!
//! Clusters come from [`aria_text::TextRenderer::shape`] in logical order
//! (line, then byte position) — the same order a typewriter should reveal,
//! including for RTL. Stagger timing is driven by that index.

use aria_compositor::GlyphInstance;
use aria_models::AnimationSlot;
use aria_text::{ClusterBox, ShapedText, TextStyle};

use crate::animation::LookDelta;
use crate::scene::TextAnimation;

/// Compute per-cluster deltas for `anim` over `shaped`.
pub fn cluster_deltas(shaped: &ShapedText, anim: &TextAnimation) -> Vec<LookDelta> {
    let count = shaped.clusters.len();
    if count == 0 {
        return Vec::new();
    }
    let Some(kind) = ClusterAnimKind::from_slot(&anim.id, anim.slot) else {
        return vec![LookDelta::IDENTITY; count];
    };
    let ctx = SampleContext::new(anim);
    if count == 1 {
        return vec![ctx.sample(kind, 0.0, 0)];
    }
    let n = (count - 1) as f32;
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let stagger = i as f32 / n;
        out.push(ctx.sample(kind, stagger, i));
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClusterAnimKind {
    InTypewriter,
    InFade,
    InBounce,
    InSlide,
    InPop,
    OutFade,
    OutFallAway,
    OutTypewriter,
    ComboTypewriter,
    ComboFade,
    ComboBounce,
    ComboSlide,
    ComboPop,
    ComboWave,
    ComboJitter,
    ComboPulse,
}

impl ClusterAnimKind {
    fn from_slot(id: &str, slot: AnimationSlot) -> Option<Self> {
        Some(match slot {
            AnimationSlot::In => match id {
                "typewriter" | "char_typewriter" => Self::InTypewriter,
                "char_fade_in" => Self::InFade,
                "char_bounce_in" => Self::InBounce,
                "char_slide_in" => Self::InSlide,
                "char_pop_in" => Self::InPop,
                _ => return None,
            },
            AnimationSlot::Out => match id {
                "char_fade_out" => Self::OutFade,
                "char_fall_away" => Self::OutFallAway,
                "char_typewriter_out" => Self::OutTypewriter,
                _ => return None,
            },
            AnimationSlot::Combo => match id {
                "typewriter" => Self::ComboTypewriter,
                "text_fade" => Self::ComboFade,
                "text_bounce" => Self::ComboBounce,
                "text_slide" => Self::ComboSlide,
                "pop" => Self::ComboPop,
                "wave" => Self::ComboWave,
                "char_jitter" => Self::ComboJitter,
                "char_pulse" => Self::ComboPulse,
                _ => return None,
            },
        })
    }
}

/// True when this build has a per-character implementation for `id` in `slot`.
///
/// Asked of [`ClusterAnimKind::from_slot`], so it cannot drift from what the
/// sampler matches.
pub(crate) fn renders_cluster(id: &str, slot: AnimationSlot) -> bool {
    ClusterAnimKind::from_slot(id, slot).is_some()
}

/// Map global progress `t` and per-cluster `stagger` ∈ [0,1) into a local
/// 0…1 progress that rises earlier for lower-index clusters.
#[inline]
fn stagger_progress(t: f32, stagger: f32, window: f32) -> f32 {
    let start = stagger * (1.0 - window);
    ((t - start) / window).clamp(0.0, 1.0)
}

#[derive(Clone, Copy)]
struct SampleContext {
    t: f32,
    phase: f32,
    window: f32,
    combo_tw_reveal: f32,
    combo_tw_window: f32,
    jitter_bucket: f32,
    intensity: f32,
}

impl SampleContext {
    #[inline]
    fn new(anim: &TextAnimation) -> Self {
        let t = anim.t.clamp(0.0, 1.0);
        // Catalog stagger window 0.55, stretched by the clip's stagger knob.
        let window = (0.55 * anim.stagger.clamp(0.05, 2.0)).clamp(0.05, 0.95);
        let phase = t.fract();
        Self {
            t,
            phase,
            window,
            combo_tw_reveal: (phase / 0.85).clamp(0.0, 1.0),
            combo_tw_window: (window * 0.4 / 0.55).clamp(0.05, 0.95),
            jitter_bucket: (phase * 12.0).floor() * 78.233,
            intensity: anim.intensity,
        }
    }

    #[inline]
    fn sample(self, kind: ClusterAnimKind, stagger: f32, index: usize) -> LookDelta {
        let delta = match kind {
            ClusterAnimKind::InTypewriter => {
                let local = stagger_progress(self.t, stagger, self.window);
                LookDelta {
                    opacity: if local > 0.0 { 1.0 } else { 0.0 },
                    ..LookDelta::IDENTITY
                }
            }
            ClusterAnimKind::InFade => {
                let local = stagger_progress(self.t, stagger, self.window);
                LookDelta {
                    opacity: local,
                    ..LookDelta::IDENTITY
                }
            }
            ClusterAnimKind::InBounce => {
                let local = stagger_progress(self.t, stagger, self.window);
                let inv = 1.0 - local;
                LookDelta {
                    position: [0.0, inv * 18.0],
                    scale: 0.4 + 0.6 * bounce_out(local),
                    opacity: local,
                    ..LookDelta::IDENTITY
                }
            }
            ClusterAnimKind::InSlide => {
                let local = stagger_progress(self.t, stagger, self.window);
                let inv = 1.0 - local;
                LookDelta {
                    position: [inv * 24.0, 0.0],
                    opacity: local,
                    ..LookDelta::IDENTITY
                }
            }
            ClusterAnimKind::InPop => {
                let local = stagger_progress(self.t, stagger, self.window);
                LookDelta {
                    scale: 0.2 + 0.8 * local,
                    opacity: local,
                    ..LookDelta::IDENTITY
                }
            }
            ClusterAnimKind::OutFade => {
                // Exit staggers in reverse so the last character leaves first.
                let local = stagger_progress(self.t, 1.0 - stagger, self.window);
                LookDelta {
                    opacity: 1.0 - local,
                    ..LookDelta::IDENTITY
                }
            }
            ClusterAnimKind::OutFallAway => {
                let local = stagger_progress(self.t, 1.0 - stagger, self.window);
                LookDelta {
                    position: [0.0, local * 28.0],
                    scale: 1.0 - 0.35 * local,
                    opacity: 1.0 - local,
                    ..LookDelta::IDENTITY
                }
            }
            ClusterAnimKind::OutTypewriter => {
                let local = stagger_progress(self.t, 1.0 - stagger, self.window);
                LookDelta {
                    opacity: if local < 1.0 { 1.0 } else { 0.0 },
                    ..LookDelta::IDENTITY
                }
            }
            ClusterAnimKind::ComboTypewriter => {
                // Looping reveal: phase 0..0.85 types in, then holds / resets.
                let local = stagger_progress(self.combo_tw_reveal, stagger, self.combo_tw_window);
                LookDelta {
                    opacity: if local > 0.0 { 1.0 } else { 0.0 },
                    ..LookDelta::IDENTITY
                }
            }
            ClusterAnimKind::ComboFade => LookDelta {
                opacity: 0.55
                    + 0.45
                        * ((self.phase * std::f32::consts::PI + stagger * 2.0).sin() * 0.5 + 0.5),
                ..LookDelta::IDENTITY
            },
            ClusterAnimKind::ComboBounce => {
                let wave = ((self.phase + stagger) * std::f32::consts::TAU).sin().abs();
                LookDelta {
                    position: [0.0, wave * 6.0],
                    scale: 1.0 + 0.08 * wave,
                    ..LookDelta::IDENTITY
                }
            }
            ClusterAnimKind::ComboSlide => {
                let wave = ((self.phase + stagger) * std::f32::consts::TAU).sin();
                LookDelta {
                    position: [wave * 5.0, 0.0],
                    ..LookDelta::IDENTITY
                }
            }
            ClusterAnimKind::ComboPop => {
                let pulse = (self.phase * std::f32::consts::PI + stagger * std::f32::consts::PI)
                    .sin()
                    .max(0.0);
                LookDelta {
                    scale: 1.0 + 0.18 * pulse,
                    ..LookDelta::IDENTITY
                }
            }
            ClusterAnimKind::ComboWave => {
                let wave = ((self.phase + stagger) * std::f32::consts::TAU).sin();
                LookDelta {
                    position: [0.0, wave * 8.0],
                    rotation: wave * 0.12,
                    ..LookDelta::IDENTITY
                }
            }
            ClusterAnimKind::ComboJitter => {
                // Deterministic tiny noise from index + phase buckets.
                let seed = (index as f32 * 12.9898 + self.jitter_bucket).sin() * 43_758.547;
                let nx = seed.fract() * 2.0 - 1.0;
                let ny = ((seed * 1.37).fract()) * 2.0 - 1.0;
                LookDelta {
                    position: [nx * 2.5, ny * 2.5],
                    rotation: nx * 0.05,
                    ..LookDelta::IDENTITY
                }
            }
            ClusterAnimKind::ComboPulse => LookDelta {
                scale: 1.0 + 0.12 * ((self.phase * std::f32::consts::TAU + stagger * 3.0).sin()),
                ..LookDelta::IDENTITY
            },
        };
        delta.with_intensity(self.intensity)
    }
}

fn bounce_out(t: f32) -> f32 {
    let t = t as f64;
    if t < 1.0 / 2.75 {
        (7.5625 * t * t) as f32
    } else if t < 2.0 / 2.75 {
        let t = t - 1.5 / 2.75;
        (7.5625 * t * t + 0.75) as f32
    } else if t < 2.5 / 2.75 {
        let t = t - 2.25 / 2.75;
        (7.5625 * t * t + 0.9375) as f32
    } else {
        let t = t - 2.625 / 2.75;
        (7.5625 * t * t + 0.984375) as f32
    }
}

/// Place shaped clusters on the canvas with per-cluster deltas applied.
///
/// `origin` is the top-left of the shaped extent on the canvas (after text
/// alignment). `scale` is the per-axis bitmap scale. `layer_rotation` /
/// `layer_opacity` are the clip's whole-layer transform (already excluding
/// per-character look presets).
pub fn place_clusters(
    shaped: &ShapedText,
    deltas: &[LookDelta],
    origin: [f32; 2],
    scale: [f32; 2],
    layer_rotation: f32,
    layer_opacity: f32,
) -> Vec<GlyphInstance> {
    let mut out = Vec::with_capacity(shaped.clusters.len().min(deltas.len()));
    for (i, (cluster, delta)) in shaped.clusters.iter().zip(deltas.iter()).enumerate() {
        if cluster.image.width > 0 && cluster.image.height > 0 {
            out.push(instance_for_cluster(
                i as u32,
                cluster,
                delta,
                origin,
                scale,
                layer_rotation,
                layer_opacity,
            ));
        }
    }
    out
}

fn instance_for_cluster(
    glyph: u32,
    cluster: &ClusterBox,
    delta: &LookDelta,
    origin: [f32; 2],
    scale: [f32; 2],
    layer_rotation: f32,
    layer_opacity: f32,
) -> GlyphInstance {
    let rest_size = [
        cluster.image.width as f32 * scale[0],
        cluster.image.height as f32 * scale[1],
    ];
    let size = [rest_size[0] * delta.scale, rest_size[1] * delta.scale];
    // Rest center of the glyph quad.
    let rest_center = [
        origin[0] + cluster.offset[0] * scale[0] + rest_size[0] * 0.5,
        origin[1] + cluster.offset[1] * scale[1] + rest_size[1] * 0.5,
    ];
    // Anchor rise/drop about the baseline for natural motion.
    let baseline_y = origin[1] + cluster.baseline * scale[1];
    let center = [
        rest_center[0] + delta.position[0] * scale[0],
        baseline_y + (rest_center[1] - baseline_y) * delta.scale + delta.position[1] * scale[1],
    ];
    GlyphInstance {
        glyph,
        center,
        size,
        rotation: layer_rotation + delta.rotation,
        opacity: (layer_opacity * delta.opacity).clamp(0.0, 1.0),
    }
}

/// Top-left canvas origin for an ink-tight run given the layer's aligned
/// quad center (from [`crate::scene::SceneLayer::text_quad_center`]).
pub(super) fn extent_origin(
    aligned_center: [f32; 2],
    extent: (u32, u32),
    scale: [f32; 2],
) -> [f32; 2] {
    [
        aligned_center[0] - extent.0 as f32 * scale[0] * 0.5,
        aligned_center[1] - extent.1 as f32 * scale[1] * 0.5,
    ]
}

/// Stable atlas cache key for a text run + style (shape-affecting fields).
pub(super) fn atlas_key(content: &str, style: &TextStyle) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    content.hash(&mut h);
    style.font_size.to_bits().hash(&mut h);
    style.line_height.to_bits().hash(&mut h);
    style.color.hash(&mut h);
    style.family.hash(&mut h);
    style.bold.hash(&mut h);
    style.italic.hash(&mut h);
    style.letter_spacing.to_bits().hash(&mut h);
    style.kerning.hash(&mut h);
    style.align.hash(&mut h);
    style.max_width.map(f32::to_bits).unwrap_or(0).hash(&mut h);
    // Effects folded into cluster images change atlas contents.
    style.underline.hash(&mut h);
    style
        .stroke
        .map(|s| (s.rgba, s.width.to_bits()))
        .hash(&mut h);
    style
        .shadow
        .map(|s| (s.rgba, s.blur.to_bits(), s.distance.to_bits()))
        .hash(&mut h);
    style.runs.len().hash(&mut h);
    for r in &style.runs {
        r.range.hash(&mut h);
        r.family.hash(&mut h);
        r.font_size.map(f32::to_bits).hash(&mut h);
        r.bold.hash(&mut h);
        r.italic.hash(&mut h);
        r.underline.hash(&mut h);
        r.color.hash(&mut h);
        r.letter_spacing.map(f32::to_bits).hash(&mut h);
    }
    h.finish()
}

/// Stable raster cache key for a static text bitmap (all [`TextStyle`] fields
/// that affect [`aria_text::TextRenderer::rasterize`]).
pub(super) fn static_raster_key(content: &str, style: &TextStyle) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    atlas_key(content, style).hash(&mut h);
    style.padding.hash(&mut h);
    style
        .background
        .map(|b| (b.rgba, b.radius.to_bits()))
        .hash(&mut h);
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use aria_models::AnimationSlot;

    fn anim(id: &str, slot: AnimationSlot, t: f32) -> TextAnimation {
        TextAnimation {
            id: id.into(),
            slot,
            t,
            intensity: 1.0,
            stagger: 1.0,
        }
    }

    #[test]
    fn typewriter_reveals_in_order() {
        let shaped = ShapedText {
            extent: (40, 20),
            clusters: (0..4)
                .map(|i| ClusterBox {
                    text_range: i..i + 1,
                    line: 0,
                    offset: [i as f32 * 10.0, 0.0],
                    baseline: 16.0,
                    image: aria_core::RgbaImage::new(8, 12, vec![255; 8 * 12 * 4]),
                })
                .collect(),
        };
        // Mid-reveal: earlier clusters visible, later ones not.
        let deltas = cluster_deltas(&shaped, &anim("typewriter", AnimationSlot::Combo, 0.3));
        assert!(deltas[0].opacity > 0.5);
        assert!(deltas[3].opacity < 0.5);
    }

    #[test]
    fn wave_offsets_vary_by_index() {
        let shaped = ShapedText {
            extent: (30, 10),
            clusters: (0..3)
                .map(|i| ClusterBox {
                    text_range: i..i + 1,
                    line: 0,
                    offset: [i as f32 * 10.0, 0.0],
                    baseline: 8.0,
                    image: aria_core::RgbaImage::new(6, 8, vec![255; 6 * 8 * 4]),
                })
                .collect(),
        };
        let deltas = cluster_deltas(&shaped, &anim("wave", AnimationSlot::Combo, 0.1));
        assert!(
            (deltas[0].position[1] - deltas[1].position[1]).abs() > 0.01
                || (deltas[0].rotation - deltas[1].rotation).abs() > 0.001,
            "wave should differentiate neighboring clusters"
        );
    }

    /// WHY: with one cluster the stagger denominator `len - 1` is zero, so
    /// `i / n` produced NaN, every sampled delta became NaN, and a
    /// single-character title never animated at all. Asserts the fast path
    /// treats the lone cluster as stagger 0.0, so entrances run start to finish
    /// and looping combos stay finite at every phase.
    ///
    /// Does not cover multi-line single-cluster layouts (a lone cluster has one
    /// line by construction) or the shaping that produced it.
    #[test]
    fn single_cluster_animation_progresses_to_the_end() {
        let shaped = ShapedText {
            extent: (10, 12),
            clusters: vec![ClusterBox {
                text_range: 0..1,
                line: 0,
                offset: [0.0, 0.0],
                baseline: 10.0,
                image: aria_core::RgbaImage::new(6, 8, vec![255; 6 * 8 * 4]),
            }],
        };
        let finite = |d: &LookDelta| {
            d.opacity.is_finite()
                && d.scale.is_finite()
                && d.rotation.is_finite()
                && d.position.iter().all(|v| v.is_finite())
        };

        // Entrances: invisible at t = 0, fully settled at t = 1.
        for id in [
            "typewriter",
            "char_fade_in",
            "char_bounce_in",
            "char_slide_in",
            "char_pop_in",
        ] {
            let start = cluster_deltas(&shaped, &anim(id, AnimationSlot::In, 0.0));
            let end = cluster_deltas(&shaped, &anim(id, AnimationSlot::In, 1.0));
            assert_eq!((start.len(), end.len()), (1, 1), "{id}");
            assert!(finite(&start[0]) && finite(&end[0]), "{id}: non-finite");
            assert!(start[0].opacity < 0.01, "{id}: visible before it starts");
            assert!((end[0].opacity - 1.0).abs() < 1e-3, "{id} opacity");
            assert!((end[0].scale - 1.0).abs() < 1e-3, "{id} scale");
            assert!(end[0].rotation.abs() < 1e-3, "{id} rotation");
            assert!(
                end[0].position.iter().all(|v| v.abs() < 1e-3),
                "{id} position"
            );
        }

        // Looping combos: no phase may produce a NaN placement.
        for id in [
            "typewriter",
            "text_fade",
            "text_bounce",
            "text_slide",
            "pop",
            "wave",
            "char_jitter",
            "char_pulse",
        ] {
            for step in 0..=10 {
                let t = step as f32 / 10.0;
                let deltas = cluster_deltas(&shaped, &anim(id, AnimationSlot::Combo, t));
                assert_eq!(deltas.len(), 1, "{id} @ {t}");
                assert!(finite(&deltas[0]), "{id} @ {t}: non-finite");
            }
        }

        // The reveal must actually move for a lone cluster, not sit at one value.
        let hidden = cluster_deltas(&shaped, &anim("typewriter", AnimationSlot::In, 0.0));
        let shown = cluster_deltas(&shaped, &anim("typewriter", AnimationSlot::In, 1.0));
        assert!(
            shown[0].opacity - hidden[0].opacity > 0.5,
            "typewriter never progressed for a single cluster"
        );
    }

    #[test]
    fn atlas_identity_is_stable_and_pixel_sensitive() {
        let style = TextStyle::new(32.0);
        assert_eq!(atlas_key("Aria", &style), atlas_key("Aria", &style));
        assert_ne!(atlas_key("Aria", &style), atlas_key("aria", &style));

        let mut underlined = style.clone();
        underlined.underline = true;
        assert_ne!(atlas_key("Aria", &style), atlas_key("Aria", &underlined));

        let run = aria_text::TextRun {
            range: 0..2,
            color: Some([255, 0, 0, 255]),
            ..Default::default()
        };
        let with_run = style.clone().with_runs(vec![run]);
        assert_ne!(atlas_key("Aria", &style), atlas_key("Aria", &with_run));
    }
}
