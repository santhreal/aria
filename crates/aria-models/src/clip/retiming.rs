use std::sync::LazyLock;

use crate::error::ModelError;
use crate::ids::{MediaId, MulticamGroupId, SequenceId};
use crate::param::{Easing, Keyframe, Param};
use crate::time::{RationalTime, TimeRange, resample, time_sub};

use super::Clip;
use super::generator::ClipSource;
use super::{is_unit_speed, is_unit_speed_curve};

/// Normalized tick span of a [`Clip::speed_curve`]: keyframe tick `0` is the
/// clip's start, [`SPEED_CURVE_SCALE`] its end. The ramp is stored over this
/// fixed domain (not absolute clip ticks) so its shape survives trims and
/// base-speed changes that re-derive the clip's timeline duration.
pub const SPEED_CURVE_SCALE: i64 = 1000;

/// Slowest instantaneous speed multiplier a ramp keyframe may hold (matches
/// the agent's `set_clip_speed` floor). A positive floor keeps the curve's
/// average — and thus the derived duration — finite.
pub const MIN_SPEED: f32 = 0.05;

/// Fastest instantaneous speed multiplier a ramp keyframe may hold.
pub const MAX_SPEED: f32 = 100.0;

/// Which time a clip's parameter keyframes are addressed in.
///
/// A picture clip's transform is clip-local: the animation belongs to the
/// content, so dragging the clip drags its animation with it. A lane bar has no
/// content — it reprocesses whatever is composited beneath its track — so its
/// look belongs to the stretch of programme it covers, not to the bar. The two
/// conventions differ by the clip's start tick, and [`Clip::param_tick`]
/// converts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamTime {
    /// Ticks from the clip's own start (the default for anything that paints).
    ClipLocal,
    /// Absolute sequence ticks. Moving the clip does not move its animation.
    Sequence,
}

/// Timeline ticks a retimed clip occupies: `source ÷ (base_speed × average
/// ramp)`. A flat ramp keeps the exact integer division M1 used (no f64 drift
/// on the common constant-speed path); an active ramp folds in its average
/// multiplier. Always at least one tick.
///
/// Retiming math, so it lives with the rest of it. A shell that draws a
/// candidate ramp asks [`Clip::duration_under`] rather than reimplementing this,
/// and the edit that commits the ramp derives its new extent from the same call
/// — a preview cannot drift from the edit it previews.
pub fn retimed_duration(
    src_dur_tl: i64,
    speed: crate::time::Rational,
    average: f64,
    has_curve: bool,
) -> i64 {
    if !has_curve {
        return (src_dur_tl * i64::from(speed.den) / i64::from(speed.num)).max(1);
    }
    let base = f64::from(speed.num) / f64::from(speed.den);
    let effective = base * average;
    if effective <= 0.0 {
        return src_dur_tl.max(1);
    }
    (src_dur_tl as f64 / effective).round().max(1.0) as i64
}

impl Clip {
    /// Timeline ticks this clip would occupy under `curve` at `rate`, without
    /// applying it.
    ///
    /// `None` for a generated clip: a generator has no source window to retime.
    /// A shell that draws a ramp shows the length it implies before committing,
    /// and the edit derives its new extent from this same call.
    pub fn duration_under(&self, curve: &Param<f32>, rate: crate::time::Rational) -> Option<i64> {
        let source = self.source_range()?;
        let has_curve = !matches!(curve, Param::Constant(v) if *v == 1.0);
        let average = match curve {
            Param::Constant(v) => f64::from(*v),
            Param::Keyframed { .. } => speed_curve_integral(curve, 1.0),
        };
        let src_dur_tl = resample(source.duration, rate).value;
        Some(retimed_duration(src_dur_tl, self.speed, average, has_curve))
    }

    /// True iff the clip plays at anything but forward 1× — the audio mixers
    /// time-stretch retimed clips (M8 Phase 3) and the UI badges them. A
    /// non-flat speed ramp counts (M2 speed curves).
    pub fn is_retimed(&self) -> bool {
        !is_unit_speed(&self.speed) || self.reversed || self.has_speed_curve()
    }

    /// True iff the clip carries a non-flat playback-rate ramp (M2 speed
    /// curves) — the constant `1.0` default does not.
    pub fn has_speed_curve(&self) -> bool {
        !is_unit_speed_curve(&self.speed_curve)
    }

    /// Frequency multiplier the varispeed renderer (M8 Phase 3) applies to a
    /// retimed clip's audio: `1.0` when pitch is locked (the CapCut default,
    /// time-stretch preserves pitch), else the clip's overall playback-speed
    /// ratio (`base speed × ramp average`) so pitch rides the speed — the
    /// optional "chipmunk" mode. Reverse does not change pitch.
    pub fn audio_pitch_factor(&self) -> f32 {
        if self.preserve_pitch {
            1.0
        } else {
            let base = f64::from(self.speed.num) / f64::from(self.speed.den);
            (base * self.speed_curve_average()) as f32
        }
    }

    /// `∫₀ᵖ speed_curve(q) dq` over the normalized clip span, `p` in `0..=1`
    /// (`0` = clip start, `1` = clip end). The speed curve is a *rate*, so
    /// this cumulative integral — not the sampled value — is what maps a
    /// timeline position to a fraction of the source window. Pure and
    /// allocation-free; `O(keyframes)` (a handful of ramp points).
    pub fn speed_curve_integral(&self, p: f64) -> f64 {
        speed_curve_integral(&self.speed_curve, p)
    }

    /// Average instantaneous multiplier of the speed ramp over the whole clip
    /// (`speed_curve_integral(1.0)`). The clip's timeline duration derives
    /// from `source_duration ÷ (base_speed × this)`.
    pub fn speed_curve_average(&self) -> f64 {
        self.speed_curve_integral(1.0)
    }

    /// Source ticks consumed by `tl_ticks` timeline ticks at this clip's
    /// speed (both in the same rate; exact rational scale, truncating).
    pub fn scale_by_speed(&self, tl_ticks: i64) -> i64 {
        tl_ticks * i64::from(self.speed.num) / i64::from(self.speed.den)
    }

    /// Timeline ticks covered by `src_ticks` source ticks at this clip's
    /// speed (the inverse of [`Self::scale_by_speed`], truncating).
    pub fn unscale_by_speed(&self, src_ticks: i64) -> i64 {
        src_ticks * i64::from(self.speed.den) / i64::from(self.speed.num)
    }

    /// Clip-relative animation tick for an absolute timeline position: the
    /// offset from the clip's start. Positions outside the clip clamp into
    /// `[0, duration)` so callers sampling at a stale playhead still get the
    /// nearest in-range value.
    pub fn animation_tick(&self, timeline_tick: i64) -> i64 {
        let offset = timeline_tick - self.timeline.start.value;
        offset.clamp(0, (self.timeline.duration.value - 1).max(0))
    }

    /// [`animation_tick`](Self::animation_tick) for a fractional timeline
    /// position — sub-frame export sampling. Clamps into the same
    /// `[0, duration - 1]` range, so the last frame's value holds through
    /// any trailing output frames.
    pub fn animation_tick_f(&self, timeline_tick: f64) -> f64 {
        let offset = timeline_tick - self.timeline.start.value as f64;
        offset.clamp(0.0, (self.timeline.duration.value - 1).max(0) as f64)
    }

    /// Which convention this clip's parameter keyframes are addressed in.
    ///
    /// A generated clip's catalog row declares it; anything with pixels of its
    /// own is clip-local, which is what a transform on a picture clip means.
    /// Exhaustive rather than defaulted: a new kind of source that wanted
    /// timeline-addressed keyframes would otherwise get clip-local ones and
    /// half its look would move with the bar.
    pub fn param_time(&self) -> ParamTime {
        match &self.content {
            crate::clip::ClipSource::Generated(generator) => generator.spec().param_time,
            crate::clip::ClipSource::Media { .. }
            | crate::clip::ClipSource::Multicam { .. }
            | crate::clip::ClipSource::Sequence { .. } => ParamTime::ClipLocal,
        }
    }

    /// The tick a parameter of this clip is sampled and stored at, for the
    /// absolute timeline position `timeline_tick`.
    ///
    /// One call converts in whichever direction the clip's row declares, so a
    /// caller neither knows nor asks: every read and every write goes through
    /// here, which is why a lane bar's look cannot half-move with the bar.
    /// Positions outside the clip clamp to its last frame, matching
    /// [`Self::animation_tick`].
    pub fn param_tick(&self, timeline_tick: i64) -> i64 {
        match self.param_time() {
            ParamTime::ClipLocal => self.animation_tick(timeline_tick),
            ParamTime::Sequence => {
                let start = self.timeline.start.value;
                let last = start + (self.timeline.duration.value - 1).max(0);
                timeline_tick.clamp(start, last)
            }
        }
    }

    /// [`param_tick`](Self::param_tick) for a fractional timeline position
    /// (sub-frame export sampling).
    pub fn param_tick_f(&self, timeline_tick: f64) -> f64 {
        match self.param_time() {
            ParamTime::ClipLocal => self.animation_tick_f(timeline_tick),
            ParamTime::Sequence => {
                let start = self.timeline.start.value as f64;
                let last = start + (self.timeline.duration.value - 1).max(0) as f64;
                timeline_tick.clamp(start, last)
            }
        }
    }

    /// The absolute timeline position a stored keyframe tick sits at — the
    /// inverse of [`Self::param_tick`], for a surface that draws keyframes on a
    /// timeline it addresses absolutely.
    pub fn param_timeline_tick(&self, stored_tick: i64) -> i64 {
        match self.param_time() {
            ParamTime::ClipLocal => stored_tick + self.timeline.start.value,
            ParamTime::Sequence => stored_tick,
        }
    }

    /// Timeline start position.
    pub fn start(&self) -> RationalTime {
        self.timeline.start
    }

    /// Exclusive timeline end.
    pub fn end(&self) -> Result<RationalTime, ModelError> {
        self.timeline.end().map_err(Into::into)
    }

    /// The media this clip references, or `None` for generated, multicam, or sequence content.
    pub fn media(&self) -> Option<MediaId> {
        match &self.content {
            ClipSource::Media { media, .. } => Some(*media),
            ClipSource::Generated(_)
            | ClipSource::Multicam { .. }
            | ClipSource::Sequence { .. } => None,
        }
    }

    /// The multicam group this clip references, or `None`.
    pub fn multicam_group(&self) -> Option<MulticamGroupId> {
        match &self.content {
            ClipSource::Multicam { group, .. } => Some(*group),
            ClipSource::Media { .. } | ClipSource::Generated(_) | ClipSource::Sequence { .. } => {
                None
            }
        }
    }

    /// The sequence this clip references, or `None`.
    pub fn sequence(&self) -> Option<SequenceId> {
        match &self.content {
            ClipSource::Sequence { sequence, .. } => Some(*sequence),
            ClipSource::Media { .. } | ClipSource::Generated(_) | ClipSource::Multicam { .. } => {
                None
            }
        }
    }

    /// The source in/out range, or `None` for generated content.
    pub fn source_range(&self) -> Option<TimeRange> {
        match &self.content {
            ClipSource::Media { source, .. }
            | ClipSource::Multicam { source, .. }
            | ClipSource::Sequence { source, .. } => Some(*source),
            ClipSource::Generated(_) => None,
        }
    }

    pub fn is_generated(&self) -> bool {
        matches!(self.content, ClipSource::Generated(_))
    }

    pub fn is_multicam(&self) -> bool {
        matches!(self.content, ClipSource::Multicam { .. })
    }

    pub fn is_sequence(&self) -> bool {
        matches!(self.content, ClipSource::Sequence { .. })
    }

    /// Map a timeline position to the corresponding source time, for media
    /// clips. Honors the clip's retiming: without a ramp the timeline offset
    /// scales by `speed` (exact rational math); with a [`Self::speed_curve`]
    /// the source offset is the curve's cumulative integral (speed is a
    /// rate). `reversed` walks the source window backward from its end. The
    /// result clamps into the source window so duration rounding can never
    /// read past an edge.
    pub fn source_time_at(
        &self,
        timeline_pos: RationalTime,
    ) -> Result<Option<RationalTime>, ModelError> {
        if !self.timeline.contains(timeline_pos)? {
            return Ok(None);
        }
        match &self.content {
            ClipSource::Media { source, .. }
            | ClipSource::Multicam { source, .. }
            | ClipSource::Sequence { source, .. } => {
                if self.freeze_frame {
                    return Ok(Some(source.start));
                }
                let offset_tl = time_sub(&timeline_pos, &self.timeline.start)?;
                let first = source.start.value;
                let last = first + (source.duration.value - 1).max(0);
                let offset_src = if self.has_speed_curve() {
                    // Speed is a rate: the fraction of the source window swept
                    // by clip-relative position `p` is `∫₀ᵖ curve ÷ ∫₀¹ curve`.
                    // base_speed and the derived duration cancel in the ratio
                    // (the duration was derived to consume the window exactly),
                    // so the curve *shape* alone places the source frame.
                    let dur = self.timeline.duration.value.max(1) as f64;
                    let p = offset_tl.value as f64 / dur;
                    let total = self.speed_curve_average();
                    let ratio = if total > 0.0 {
                        self.speed_curve_integral(p) / total
                    } else {
                        p
                    };
                    (source.duration.value as f64 * ratio).round() as i64
                } else {
                    // Flat ramp: the exact rational fast path (zero f64 drift),
                    // identical to M1 constant speed.
                    let scaled =
                        RationalTime::new(self.scale_by_speed(offset_tl.value), offset_tl.rate);
                    resample(scaled, source.start.rate).value
                };
                let tick = if self.reversed {
                    last - offset_src
                } else {
                    first + offset_src
                };
                Ok(Some(RationalTime::new(
                    tick.clamp(first, last),
                    source.start.rate,
                )))
            }
            ClipSource::Generated(_) => Ok(None),
        }
    }
}

/// `∫₀ᵖ curve(q) dq` over the normalized clip span, `p` in `0..=1` (`0` = clip
/// start, `1` = clip end). Free-function core of [`Clip::speed_curve_integral`]
/// so the audio mixers can evaluate a clip's ramp without a whole [`Clip`].
/// Pure and allocation-free; `O(keyframes)`.
pub fn speed_curve_integral(curve: &Param<f32>, p: f64) -> f64 {
    let p = p.clamp(0.0, 1.0);
    match curve {
        Param::Constant(v) => f64::from(*v) * p,
        Param::Keyframed { keyframes } => {
            let scale = SPEED_CURVE_SCALE as f64;
            let pos = |kf: &Keyframe<f32>| kf.tick as f64 / scale;
            let first = &keyframes[0];
            let q0 = pos(first);
            // Leading flat region holds the first value (CapCut clamp).
            let mut acc = f64::from(first.value) * p.min(q0);
            if p <= q0 {
                return acc;
            }
            for pair in keyframes.windows(2) {
                let (k0, k1) = (&pair[0], &pair[1]);
                let (qa, qb) = (pos(k0), pos(k1));
                if p <= qa {
                    return acc;
                }
                let seg = qb - qa;
                if seg > 0.0 {
                    let upper = p.min(qb);
                    let t_hi = ((upper - qa) / seg) as f32;
                    let (va, vb) = (f64::from(k0.value), f64::from(k1.value));
                    // ∫ over [qa, upper] of lerp(va, vb, e(t)) dq, dq = seg·dt
                    //   = seg·[va·t_hi + (vb − va)·∫₀^{t_hi} e].
                    let e_int = f64::from(k0.easing.integral_to(t_hi));
                    acc += seg * (va * f64::from(t_hi) + (vb - va) * e_int);
                }
                if p <= qb {
                    return acc;
                }
            }
            // Trailing flat region holds the last value.
            let last = &keyframes[keyframes.len() - 1];
            acc + f64::from(last.value) * (p - pos(last))
        }
    }
}

/// Fraction of a clip's source window swept by clip-relative output position
/// `p` in `0..=1`: the normalized cumulative integral `∫₀ᵖ curve ÷ ∫₀¹ curve`.
/// Monotonic with `0 ↦ 0` and `1 ↦ 1`, this is exactly the source placement
/// [`Clip::source_time_at`] uses for video, so the varispeed audio renderer
/// (M8 Phase 3) warps the sound in lockstep with the picture. A degenerate
/// (zero-area) curve falls back to a linear map.
pub fn speed_curve_source_fraction(curve: &Param<f32>, p: f64) -> f64 {
    let total = speed_curve_integral(curve, 1.0);
    if total > 0.0 {
        (speed_curve_integral(curve, p) / total).clamp(0.0, 1.0)
    } else {
        p.clamp(0.0, 1.0)
    }
}

/// Validate a speed ramp (M2 speed curves) before it is stored: a structurally
/// sound `Param` (sorted, non-empty, valid easings) whose every keyframe value
/// is finite and within `[`[`MIN_SPEED`]`, `[`MAX_SPEED`]`]`, with normalized
/// ticks inside `0..=`[`SPEED_CURVE_SCALE`].
pub fn validate_speed_curve(curve: &Param<f32>) -> Result<(), ModelError> {
    curve.validate_shape()?;
    for kf in curve.keyframes() {
        if kf.tick < 0 || kf.tick > SPEED_CURVE_SCALE {
            return Err(ModelError::InvalidParam(format!(
                "speed ramp keyframe tick {} is outside 0..={SPEED_CURVE_SCALE}",
                kf.tick
            )));
        }
    }
    curve.for_each_value(|v| {
        if !v.is_finite() || !(MIN_SPEED..=MAX_SPEED).contains(v) {
            return Err(ModelError::InvalidParam(format!(
                "speed ramp value {v} must be within {MIN_SPEED}..={MAX_SPEED}"
            )));
        }
        Ok(())
    })
}

/// One point on a preset ramp: where along the clip, how fast, and the curve
/// leaving it.
#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize)]
pub struct SpeedPresetPoint {
    /// Position along the clip, `0.0` … `1.0`.
    pub at: f64,
    /// Multiplier on the clip's base speed.
    pub value: f32,
    /// Curve leaving this point. Linear when the file omits it.
    #[serde(default)]
    pub easing: Easing,
}

/// One speed-ramp preset: id, display label, and the ramp itself.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct SpeedPresetSpec {
    pub id: String,
    pub label: String,
    pub points: Vec<SpeedPresetPoint>,
}

impl SpeedPresetSpec {
    /// The preset as a normalized [`Param`] over `0..=`[`SPEED_CURVE_SCALE`].
    pub fn curve(&self) -> Param<f32> {
        let scale = SPEED_CURVE_SCALE as f64;
        Param::Keyframed {
            keyframes: self
                .points
                .iter()
                .map(|point| Keyframe {
                    tick: (point.at * scale).round() as i64,
                    value: point.value,
                    easing: point.easing,
                    tangents: None,
                })
                .collect(),
        }
    }
}

/// Every speed-ramp preset, read once. Shipped set at `data/speed_presets.toml`.
///
/// A preset is validated the way a clip's own ramp is: the same tick range and
/// speed limits, so a catalog file cannot introduce a curve the model would
/// refuse if a user drew it.
static SPEED_PRESETS: LazyLock<Vec<SpeedPresetSpec>> = LazyLock::new(|| {
    crate::catalog::load(
        "speed_presets",
        include_str!("../../data/speed_presets.toml"),
        |entries: &[SpeedPresetSpec]| {
            crate::catalog::unique_ids(entries, |s| &s.id, |s| &s.label)?;
            for spec in entries {
                if spec.points.len() < 2 {
                    return Err(format!("{} needs at least two points", spec.id));
                }
                if spec.points.iter().any(|p| !(0.0..=1.0).contains(&p.at)) {
                    return Err(format!("{} has a point outside 0…1", spec.id));
                }
                validate_speed_curve(&spec.curve()).map_err(|err| format!("{}: {err}", spec.id))?;
            }
            Ok(())
        },
    )
});

/// Every speed-ramp preset (UI browsing order).
pub fn speed_preset_catalog() -> &'static [SpeedPresetSpec] {
    &SPEED_PRESETS
}

/// The catalog id whose curve equals `curve`, or `None` for a flat ramp or a
/// hand-edited curve. How the shells highlight the active preset tile: curves
/// are normalized over `0..=`[`SPEED_CURVE_SCALE`], so a preset's shape (and
/// this match) survives trims and base-speed changes.
pub fn speed_preset_id(curve: &Param<f32>) -> Option<&'static str> {
    SPEED_PRESETS
        .iter()
        .find(|spec| &spec.curve() == curve)
        .map(|spec| spec.id.as_str())
}

/// A built-in speed ramp by id, or `None` for an unknown one.
pub fn speed_preset(name: &str) -> Option<Param<f32>> {
    SPEED_PRESETS
        .iter()
        .find(|spec| spec.id == name)
        .map(SpeedPresetSpec::curve)
}
