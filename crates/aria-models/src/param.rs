//! Animatable parameters: the M2 keystone.
//!
//! A [`Param<T>`] is either a constant value or a keyframed curve. One type
//! serves every animatable property — clip transforms today; effect
//! parameters, volume envelopes, and speed ramps as later milestones land.
//!
//! Design notes:
//! - **Ticks are clip-relative.** A keyframe's `tick` is the offset from the
//!   owning clip's timeline start, at the timeline rate. Moving a clip moves
//!   its animation for free; no fix-ups on `MoveClip`/`ShiftClips`.
//! - **Compact, forward-tolerant serialization.** A constant param
//!   serializes as the bare value (`1.0`, `[0.0, 0.5]`) — byte-identical to
//!   the pre-M2 format — and a keyframed param as `{"kf":[...]}`. Old
//!   projects load unchanged; constant-only projects stay readable by old
//!   builds.
//! - **Sampling is hot-path.** `sample` is pure and allocation-free: a
//!   binary search over the keyframe slice plus an eased lerp. It runs
//!   per-layer-per-frame in `resolve_layers`.
//! - **Spatial tangents** (`Keyframe::tangents`) are a serde slot on every
//!   keyframe type for schema simplicity, but only `[f32; 2]` sampling reads
//!   them (cubic bezier motion paths). The project routing layer rejects
//!   tangents on non-position params.

use serde::{Deserialize, Serialize};

use crate::error::ModelError;

mod deserialize;
mod easing;
mod interpolate;
mod presets;

pub use easing::{
    EASING_PRESETS, EASY_EASE_INFLUENCE, Easing, EasingPreset, MAX_TEMPORAL_INFLUENCE,
    MIN_TEMPORAL_INFLUENCE, TemporalEase, easing_preset,
};
pub use interpolate::{Extrapolate, Lerp, SegmentSample, SpatialTangents};
pub use presets::{
    MIN_PRESET_SEGMENT_TICKS, PiecewiseEasingPreset, PiecewisePresetSpec, Waypoint, expand_preset,
    piecewise_preset_by_id, piecewise_presets,
};

/// A zero constant parameter `Param::Constant(0.0)`. Used as serde default.
pub fn default_zero_param() -> Param<f32> {
    Param::Constant(0.0)
}

/// Returns true iff `p` is a constant parameter with value 0.0. Used for serde `skip_serializing_if`.
pub fn is_zero_param(p: &Param<f32>) -> bool {
    p.constant() == Some(0.0)
}

/// One point on a keyframed curve. `tick` is clip-relative (offset from the
/// clip's timeline start) at the timeline rate.
///
/// `tangents` is present on every keyframe type for serde simplicity, but
/// only `[f32; 2]` [`SegmentSample`] reads it. Routing rejects tangents on
/// non-position params.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Keyframe<T> {
    /// Offset from the clip's timeline start, in timeline-rate ticks.
    #[serde(rename = "t")]
    pub tick: i64,
    /// Property value at this keyframe.
    #[serde(rename = "v")]
    pub value: T,
    /// Curve of the segment leaving this keyframe.
    #[serde(rename = "e", default, skip_serializing_if = "is_linear")]
    pub easing: Easing,
    /// Spatial bezier handles (motion paths). Only meaningful for
    /// `[f32; 2]` position params; ignored by other samplers.
    #[serde(rename = "s", default, skip_serializing_if = "Option::is_none")]
    pub tangents: Option<SpatialTangents>,
}

impl<T> Keyframe<T> {
    /// Build a keyframe with no spatial tangents (straight-line segments).
    pub fn new(tick: i64, value: T, easing: Easing) -> Self {
        Self {
            tick,
            value,
            easing,
            tangents: None,
        }
    }
}

fn is_linear(easing: &Easing) -> bool {
    *easing == Easing::Linear
}

/// An animatable property: a constant, or a keyframed curve.
///
/// Invariants when keyframed: at least one keyframe, sorted by strictly
/// increasing `tick`. Mutators preserve this; deserialization re-validates
/// through [`Param::validate_shape`].
///
/// Deserializes in place: a map whose first key is `kf` is a curve, and any
/// other value is the constant.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Param<T> {
    /// Keyframed curve. Serializes as `{"kf":[{"t":..,"v":..},..]}`.
    Keyframed {
        #[serde(rename = "kf")]
        keyframes: Vec<Keyframe<T>>,
    },
    /// Fixed value. Serializes as the bare value, matching the pre-M2 format.
    Constant(T),
}

impl<T: SegmentSample> Param<T> {
    /// Value at a clip-relative `tick`. Before the first keyframe the first
    /// value holds; after the last, the last (CapCut behavior). Between two
    /// keyframes the segment's easing shapes the lerp.
    ///
    /// Hot path: pure, allocation-free, O(log k).
    pub fn sample(&self, tick: i64) -> T {
        self.sample_at(tick as f64)
    }

    /// [`sample`](Self::sample) at a fractional clip-relative tick. Curves
    /// are continuous in time between keyframes, so they can be evaluated
    /// between timeline frames — what export uses when the output rate
    /// exceeds the timeline rate (a 60 fps export of a 24 fps timeline
    /// samples animation at the exact output frame times instead of
    /// repeating the 24 fps values in an uneven 3-2 cadence).
    pub fn sample_at(&self, tick: f64) -> T {
        match self {
            Param::Constant(value) => *value,
            Param::Keyframed { keyframes } => {
                // Invariant: non-empty (mutators preserve it; deserialization
                // is checked through `validate_shape`).
                let first = &keyframes[0];
                if tick <= first.tick as f64 {
                    return first.value;
                }
                let last = &keyframes[keyframes.len() - 1];
                if tick >= last.tick as f64 {
                    return last.value;
                }
                // Index of the first keyframe with kf.tick > tick; the
                // segment is [idx-1, idx]. Bounds hold: first.tick < tick <
                // last.tick.
                let idx = keyframes.partition_point(|kf| (kf.tick as f64) <= tick);
                let k0 = &keyframes[idx - 1];
                let k1 = &keyframes[idx];
                let span = (k1.tick - k0.tick) as f64;
                let t = ((tick - k0.tick as f64) / span) as f32;
                let eased_t = k0.easing.apply(t);
                T::segment_sample(k0, k1, eased_t)
            }
        }
    }

    /// Instantaneous rate of change (in value units per tick) at a fractional
    /// clip-relative `tick`. Used by the timeline speed graph.
    pub fn segment_speed_at(&self, tick: f64) -> f32 {
        let Param::Keyframed { keyframes } = self else {
            return 0.0;
        };
        if keyframes.len() < 2 {
            return 0.0;
        }
        let first = &keyframes[0];
        let last = &keyframes[keyframes.len() - 1];
        if tick < first.tick as f64 || tick > last.tick as f64 {
            return 0.0;
        }
        let idx = if tick >= last.tick as f64 {
            keyframes.len() - 1
        } else {
            keyframes.partition_point(|kf| (kf.tick as f64) <= tick)
        };
        let k0 = &keyframes[idx - 1];
        let k1 = &keyframes[idx];
        let dt = (k1.tick - k0.tick).max(1) as f32;
        let dv = T::segment_delta(k0, k1);
        let t = (((tick - k0.tick as f64) / f64::from(dt)) as f32).clamp(0.0, 1.0);
        (dv / dt) * k0.easing.derivative(t)
    }

    /// Insert or replace the keyframe at `tick`. A constant param becomes a
    /// single-keyframe curve. Replacing an existing keyframe preserves its
    /// spatial tangents (use [`Self::set_keyframe_tangents`] to change them).
    pub fn set_keyframe(&mut self, tick: i64, value: T, easing: Easing) {
        match self {
            Param::Constant(_) => {
                *self = Param::Keyframed {
                    keyframes: vec![Keyframe::new(tick, value, easing)],
                };
            }
            Param::Keyframed { keyframes } => {
                match keyframes.binary_search_by_key(&tick, |kf| kf.tick) {
                    Ok(i) => {
                        let tangents = keyframes[i].tangents;
                        keyframes[i] = Keyframe {
                            tick,
                            value,
                            easing,
                            tangents,
                        };
                    }
                    Err(i) => keyframes.insert(i, Keyframe::new(tick, value, easing)),
                }
            }
        }
        self.sync_temporal_and_roving();
    }

    /// Insert or replace the keyframe at `tick` together with its spatial
    /// `tangents` before solving roving ticks and temporal handles.
    pub fn set_keyframe_with_tangents(
        &mut self,
        tick: i64,
        value: T,
        easing: Easing,
        tangents: Option<SpatialTangents>,
    ) {
        match self {
            Param::Constant(_) => {
                *self = Param::Keyframed {
                    keyframes: vec![Keyframe {
                        tick,
                        value,
                        easing,
                        tangents,
                    }],
                };
            }
            Param::Keyframed { keyframes } => {
                match keyframes.binary_search_by_key(&tick, |kf| kf.tick) {
                    Ok(i) => {
                        keyframes[i] = Keyframe {
                            tick,
                            value,
                            easing,
                            tangents,
                        };
                    }
                    Err(i) => keyframes.insert(
                        i,
                        Keyframe {
                            tick,
                            value,
                            easing,
                            tangents,
                        },
                    ),
                }
            }
        }
        self.sync_temporal_and_roving();
    }

    /// Set or clear spatial tangents on the keyframe at `tick`. Errors when
    /// no keyframe sits there (including on a constant param).
    pub fn set_keyframe_tangents(
        &mut self,
        tick: i64,
        tangents: Option<SpatialTangents>,
    ) -> Result<(), ModelError> {
        let Param::Keyframed { keyframes } = self else {
            return Err(ModelError::InvalidParam(format!(
                "no keyframe at {tick} to set tangents"
            )));
        };
        let Ok(i) = keyframes.binary_search_by_key(&tick, |kf| kf.tick) else {
            return Err(ModelError::InvalidParam(format!(
                "no keyframe at {tick} to set tangents"
            )));
        };
        if let Some(t) = tangents {
            t.validate()?;
        }
        keyframes[i].tangents = tangents;
        self.sync_temporal_and_roving();
        Ok(())
    }

    /// Effective `(incoming, outgoing)` temporal ease handles for the keyframe
    /// at `tick`, derived from explicit [`TemporalEase`] handles when present
    /// or from the adjacent segment easings.
    pub fn keyframe_temporal_ease(&self, tick: i64) -> Option<(TemporalEase, TemporalEase)> {
        let Param::Keyframed { keyframes } = self else {
            return None;
        };
        let i = keyframes.binary_search_by_key(&tick, |kf| kf.tick).ok()?;
        let kf = &keyframes[i];
        let in_ease = kf.easing.in_ease().unwrap_or_else(|| {
            if i > 0 {
                let prev = &keyframes[i - 1];
                let dt = (kf.tick - prev.tick).max(1) as f32;
                let dv = T::segment_delta(prev, kf);
                TemporalEase::from_segment_easing(prev.easing, dt, dv).1
            } else {
                TemporalEase::EASY_EASE
            }
        });
        let out_ease = kf.easing.out_ease().unwrap_or_else(|| {
            if i + 1 < keyframes.len() {
                let next = &keyframes[i + 1];
                let dt = (next.tick - kf.tick).max(1) as f32;
                let dv = T::segment_delta(kf, next);
                TemporalEase::from_segment_easing(kf.easing, dt, dv).0
            } else {
                TemporalEase::EASY_EASE
            }
        });
        Some((in_ease, out_ease))
    }

    /// Set incoming/outgoing temporal ease and/or hold state on the keyframe
    /// at `tick`.
    pub fn set_keyframe_temporal_ease(
        &mut self,
        tick: i64,
        in_ease: Option<TemporalEase>,
        out_ease: Option<TemporalEase>,
        hold: bool,
    ) -> Result<(), ModelError> {
        if let Some(inp) = in_ease {
            inp.validate()?;
        }
        if let Some(out) = out_ease {
            out.validate()?;
        }
        let Param::Keyframed { keyframes } = self else {
            return Err(ModelError::InvalidParam(format!(
                "no keyframe at {tick} to set temporal ease"
            )));
        };
        let Ok(i) = keyframes.binary_search_by_key(&tick, |kf| kf.tick) else {
            return Err(ModelError::InvalidParam(format!(
                "no keyframe at {tick} to set temporal ease"
            )));
        };
        keyframes[i].easing = keyframes[i]
            .easing
            .with_temporal_ease(in_ease, out_ease, hold);
        self.sync_temporal_and_roving();
        Ok(())
    }

    /// Apply Easy Ease (`33.33%` influence, `0.0` speed on both sides) to the
    /// keyframe at `tick`.
    pub fn easy_ease_keyframe(&mut self, tick: i64) -> Result<(), ModelError> {
        self.set_keyframe_temporal_ease(
            tick,
            Some(TemporalEase::EASY_EASE),
            Some(TemporalEase::EASY_EASE),
            false,
        )
    }

    /// Apply Easy Ease In (`33.33%` incoming influence, `0.0` incoming speed)
    /// to the keyframe at `tick`, preserving its outgoing temporal ease.
    pub fn easy_ease_in_keyframe(&mut self, tick: i64) -> Result<(), ModelError> {
        let out_ease = self
            .keyframes()
            .binary_search_by_key(&tick, |kf| kf.tick)
            .ok()
            .and_then(|i| self.keyframes()[i].easing.out_ease());
        self.set_keyframe_temporal_ease(tick, Some(TemporalEase::EASY_EASE), out_ease, false)
    }

    /// Apply Easy Ease Out (`33.33%` outgoing influence, `0.0` outgoing speed)
    /// to the keyframe at `tick`, preserving its incoming temporal ease.
    pub fn easy_ease_out_keyframe(&mut self, tick: i64) -> Result<(), ModelError> {
        let in_ease = self
            .keyframes()
            .binary_search_by_key(&tick, |kf| kf.tick)
            .ok()
            .and_then(|i| self.keyframes()[i].easing.in_ease());
        self.set_keyframe_temporal_ease(tick, in_ease, Some(TemporalEase::EASY_EASE), false)
    }

    /// Toggle Hold interpolation on the keyframe at `tick`.
    pub fn toggle_hold_keyframe(&mut self, tick: i64) -> Result<Easing, ModelError> {
        let Param::Keyframed { keyframes } = self else {
            return Err(ModelError::InvalidParam(format!(
                "no keyframe at {tick} to toggle hold"
            )));
        };
        let Ok(i) = keyframes.binary_search_by_key(&tick, |kf| kf.tick) else {
            return Err(ModelError::InvalidParam(format!(
                "no keyframe at {tick} to toggle hold"
            )));
        };
        let current = keyframes[i].easing;
        let next_hold = !current.is_hold();
        keyframes[i].easing =
            current.with_temporal_ease(current.in_ease(), current.out_ease(), next_hold);
        self.sync_temporal_and_roving();
        Ok(self.keyframes()[i].easing)
    }

    /// Set or clear roving on the keyframe at `tick` and return its solved
    /// tick. Roving is ignored on the first and last keyframe of a curve.
    pub fn set_keyframe_roving(&mut self, tick: i64, roving: bool) -> Result<i64, ModelError> {
        let Param::Keyframed { keyframes } = self else {
            return Err(ModelError::InvalidParam(format!(
                "no keyframe at {tick} to set roving"
            )));
        };
        let Ok(i) = keyframes.binary_search_by_key(&tick, |kf| kf.tick) else {
            return Err(ModelError::InvalidParam(format!(
                "no keyframe at {tick} to set roving"
            )));
        };
        let effective_roving = roving && i > 0 && i + 1 < keyframes.len();
        keyframes[i].easing = keyframes[i].easing.with_roving(effective_roving);
        self.sync_temporal_and_roving();
        Ok(self.keyframes()[i].tick)
    }

    /// Remove the keyframe at exactly `tick`. Removing the last keyframe
    /// collapses the param to a constant of that keyframe's value (the
    /// property keeps its on-screen value, CapCut-style). Returns `false` if
    /// no keyframe sits at `tick`.
    pub fn remove_keyframe(&mut self, tick: i64) -> bool {
        let Param::Keyframed { keyframes } = self else {
            return false;
        };
        let Ok(i) = keyframes.binary_search_by_key(&tick, |kf| kf.tick) else {
            return false;
        };
        let removed = keyframes.remove(i);
        if keyframes.is_empty() {
            *self = Param::Constant(removed.value);
        } else {
            self.sync_temporal_and_roving();
        }
        true
    }

    /// Re-solve interior roving keyframe ticks from spatial/value arc lengths
    /// and recompute outgoing cubic-bezier control points from per-keyframe
    /// temporal ease handles.
    pub fn sync_temporal_and_roving(&mut self) {
        let Param::Keyframed { keyframes } = self else {
            return;
        };
        let len = keyframes.len();
        if len < 2 {
            return;
        }

        // 1. Solve contiguous runs of interior roving keyframes (1..len-1).
        let mut idx = 1;
        while idx + 1 < len {
            if !keyframes[idx].easing.is_roving() {
                idx += 1;
                continue;
            }
            let run_start = idx;
            while idx + 1 < len && keyframes[idx].easing.is_roving() {
                idx += 1;
            }
            let run_end = idx - 1;
            let anchor_a = run_start - 1;
            let anchor_b = run_end + 1;
            let t_a = keyframes[anchor_a].tick;
            let t_b = keyframes[anchor_b].tick;
            let seg_count = anchor_b - anchor_a;
            let span = t_b - t_a;
            if span >= seg_count as i64 {
                let mut dists = Vec::with_capacity(seg_count);
                for s in anchor_a..anchor_b {
                    dists.push(T::segment_distance(&keyframes[s], &keyframes[s + 1]).max(0.0));
                }
                let total_dist: f32 = dists.iter().sum();
                let mut cum = 0.0f32;
                for k in run_start..=run_end {
                    cum += dists[k - anchor_a - 1];
                    let frac = if total_dist > 1e-6 {
                        f64::from(cum / total_dist)
                    } else {
                        (k - anchor_a) as f64 / seg_count as f64
                    };
                    let raw_tick = t_a + (span as f64 * frac).round() as i64;
                    let min_tick = keyframes[k - 1].tick + 1;
                    let max_tick = t_b - (anchor_b - k) as i64;
                    keyframes[k].tick = raw_tick.clamp(min_tick, max_tick);
                }
            }
        }

        // 2. Recompute cubic-bezier control points on segments driven by
        //    explicit `in_ease` / `out_ease` handles.
        for i in 0..(len - 1) {
            let k0 = keyframes[i];
            let k1 = keyframes[i + 1];
            let out_ease = k0.easing.out_ease();
            let next_in_ease = k1.easing.in_ease();
            if out_ease.is_some() || next_in_ease.is_some() {
                let dt = (k1.tick - k0.tick).max(1) as f32;
                let dv = T::segment_delta(&k0, &k1);
                let mut points =
                    TemporalEase::control_points_for_segment(out_ease, next_in_ease, dt, dv);
                if out_ease.is_none()
                    && !matches!(k0.easing, Easing::Linear | Easing::Temporal { .. })
                    && let Some([ox1, oy1, _, _]) = k0.easing.control_points()
                {
                    points[0] = ox1;
                    points[1] = oy1;
                }
                keyframes[i].easing = Easing::Temporal {
                    in_ease: k0.easing.in_ease(),
                    out_ease,
                    roving: k0.easing.is_roving(),
                    hold: k0.easing.is_hold(),
                    points,
                };
            } else if let Easing::Temporal {
                in_ease,
                out_ease: None,
                roving,
                hold,
                ..
            } = k0.easing
            {
                if !roving && in_ease.is_none() {
                    keyframes[i].easing = if hold { Easing::Hold } else { Easing::Linear };
                } else {
                    keyframes[i].easing = Easing::Temporal {
                        in_ease,
                        out_ease: None,
                        roving,
                        hold,
                        points: TemporalEase::control_points_for_segment(None, None, 1.0, 1.0),
                    };
                }
            }
        }
    }
}

impl<T: Copy> Param<T> {
    /// The constant value, or `None` when keyframed.
    pub fn constant(&self) -> Option<T> {
        match self {
            Param::Constant(value) => Some(*value),
            Param::Keyframed { .. } => None,
        }
    }

    /// Replace the param (and any keyframes) with a constant.
    pub fn set_constant(&mut self, value: T) {
        *self = Param::Constant(value);
    }
}

impl<T: SegmentSample + Extrapolate> Param<T> {
    /// Expand the outgoing segment at `from_tick` with a piecewise easing
    /// preset. Requires a successor keyframe. Removes any intermediates
    /// strictly between `from` and `to`, then writes the expanded sequence.
    /// Short segments (< [`MIN_PRESET_SEGMENT_TICKS`]) leave the pair unchanged
    /// (still `Ok`).
    pub fn apply_easing_preset(
        &mut self,
        from_tick: i64,
        preset: PiecewiseEasingPreset,
    ) -> Result<(), ModelError> {
        let Param::Keyframed { keyframes } = self else {
            return Err(ModelError::InvalidParam(
                "easing preset requires a keyframed parameter".into(),
            ));
        };
        let idx = keyframes
            .iter()
            .position(|kf| kf.tick == from_tick)
            .ok_or_else(|| {
                ModelError::InvalidParam(format!(
                    "no keyframe at tick {from_tick} for easing preset"
                ))
            })?;
        let Some(to) = keyframes.get(idx + 1).copied() else {
            return Err(ModelError::InvalidParam(
                "easing preset requires a following keyframe on the segment".into(),
            ));
        };
        let from = keyframes[idx];
        let expanded = expand_preset(preset, &from, &to);
        let remove: Vec<i64> = keyframes
            .iter()
            .filter(|kf| kf.tick > from.tick && kf.tick < to.tick)
            .map(|kf| kf.tick)
            .collect();
        for tick in remove {
            self.remove_keyframe(tick);
        }
        for kf in expanded {
            // `set_keyframe` preserves existing spatial tangents on replace;
            // new intermediate ticks start with `None`.
            self.set_keyframe(kf.tick, kf.value, kf.easing);
        }
        Ok(())
    }
}

impl<T: Clone> Param<T> {
    /// A copy with every keyframe tick remapped by `f` (constants pass through
    /// unchanged). Used to rebase a clip-relative envelope from timeline ticks
    /// into the audio mixer's sample-frame domain once per span, so the
    /// per-sample gain lookup stays a plain tick compare. `f` must be
    /// monotonic so the remapped keyframes stay sorted.
    pub fn map_ticks(&self, f: impl Fn(i64) -> i64) -> Param<T> {
        match self {
            Param::Constant(value) => Param::Constant(value.clone()),
            Param::Keyframed { keyframes } => Param::Keyframed {
                keyframes: keyframes
                    .iter()
                    .map(|kf| Keyframe {
                        tick: f(kf.tick),
                        value: kf.value.clone(),
                        easing: kf.easing,
                        tangents: kf.tangents,
                    })
                    .collect(),
            },
        }
    }

    /// Shift every keyframe by `delta` ticks (constants pass through
    /// unchanged). The operation is atomic: an overflowing tick leaves the
    /// parameter untouched.
    ///
    /// Clip splitting uses this to preserve an absolute animation curve on
    /// the tail while rebasing its clip-relative origin to the split point.
    /// Spatial tangents travel with their keyframes unchanged — the full
    /// segment (including bezier handles) stays intact on both halves, so
    /// mid-segment splits need no de Casteljau subdivision.
    pub fn shift_ticks(&mut self, delta: i64) -> Result<(), ModelError> {
        let Param::Keyframed { keyframes } = self else {
            return Ok(());
        };
        let shifted = keyframes
            .iter()
            .map(|kf| {
                Ok(Keyframe {
                    tick: kf.tick.checked_add(delta).ok_or(ModelError::TimeOverflow)?,
                    value: kf.value.clone(),
                    easing: kf.easing,
                    tangents: kf.tangents,
                })
            })
            .collect::<Result<Vec<_>, ModelError>>()?;
        *keyframes = shifted;
        Ok(())
    }

    /// Resample every keyframe tick from `old_rate` to `new_rate` (constants
    /// pass through unchanged). Monotonicity and strict ordering are preserved.
    pub fn resample_ticks(
        &mut self,
        old_rate: crate::time::Rational,
        new_rate: crate::time::Rational,
    ) {
        let Param::Keyframed { keyframes } = self else {
            return;
        };
        let mut prev: Option<i64> = None;
        for kf in keyframes.iter_mut() {
            let resampled =
                crate::time::resample(crate::time::RationalTime::new(kf.tick, old_rate), new_rate)
                    .value;
            let tick = match prev {
                Some(p) if resampled <= p => p + 1,
                _ => resampled,
            };
            kf.tick = tick;
            prev = Some(tick);
        }
    }
}

impl<T> Param<T> {
    pub fn is_animated(&self) -> bool {
        matches!(self, Param::Keyframed { .. })
    }

    /// Keyframes in tick order; empty for a constant.
    pub fn keyframes(&self) -> &[Keyframe<T>] {
        match self {
            Param::Constant(_) => &[],
            Param::Keyframed { keyframes } => keyframes,
        }
    }

    /// The keyframe at exactly `tick`, if one sits there.
    pub fn keyframe_at(&self, tick: i64) -> Option<&Keyframe<T>> {
        let keyframes = self.keyframes();
        keyframes
            .binary_search_by_key(&tick, |kf| kf.tick)
            .ok()
            .map(|index| &keyframes[index])
    }

    /// Easing of the keyframe at exactly `tick`.
    pub fn easing_at(&self, tick: i64) -> Option<Easing> {
        self.keyframe_at(tick).map(|kf| kf.easing)
    }

    /// Spatial tangents stored on the keyframe at exactly `tick`. `None` both
    /// when no keyframe sits there and when the keyframe has straight segments.
    pub fn tangents_at(&self, tick: i64) -> Option<SpatialTangents> {
        self.keyframe_at(tick).and_then(|kf| kf.tangents)
    }

    /// Structural invariants: keyframed params are non-empty, strictly
    /// sorted by tick, with valid easings. Call after deserializing.
    pub fn validate_shape(&self) -> Result<(), ModelError> {
        let Param::Keyframed { keyframes } = self else {
            return Ok(());
        };
        if keyframes.is_empty() {
            return Err(ModelError::InvalidParam(
                "keyframed param with no keyframes".into(),
            ));
        }
        for pair in keyframes.windows(2) {
            if pair[1].tick <= pair[0].tick {
                return Err(ModelError::InvalidParam(
                    "keyframes must be strictly sorted by tick".into(),
                ));
            }
        }
        for kf in keyframes {
            kf.easing.validate()?;
            if let Some(t) = kf.tangents {
                t.validate()?;
            }
        }
        Ok(())
    }

    /// Visit every stored value (constant or per-keyframe) — the hook for
    /// per-property range validation.
    pub fn for_each_value<E>(&self, mut f: impl FnMut(&T) -> Result<(), E>) -> Result<(), E> {
        match self {
            Param::Constant(value) => f(value),
            Param::Keyframed { keyframes } => {
                for kf in keyframes {
                    f(&kf.value)?;
                }
                Ok(())
            }
        }
    }

    /// Replace every stored value (constant or per-keyframe) with `f` of it.
    /// Ticks, easings, and tangents are kept.
    pub fn map_values(&mut self, mut f: impl FnMut(&T) -> T) {
        match self {
            Param::Constant(value) => *value = f(value),
            Param::Keyframed { keyframes } => {
                for kf in keyframes {
                    kf.value = f(&kf.value);
                }
            }
        }
    }
}

impl<T> From<T> for Param<T> {
    fn from(value: T) -> Self {
        Param::Constant(value)
    }
}

#[cfg(test)]
mod tests;
