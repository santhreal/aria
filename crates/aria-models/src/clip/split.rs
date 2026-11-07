//! What a split does to a clip's stored time: whether the representation
//! survives being cut, and how a speed ramp is divided when it does.
//!
//! Both questions belong to the retiming math next door in
//! [`super::retiming`], which owns the normalized speed-curve domain and the
//! duration derivation. They live in their own module because a split has one
//! extra obligation the rest of retiming does not: it must refuse rather than
//! approximate. A curve that cannot be divided exactly, or an edge-anchored
//! animation that would be retimed by the cut, fails closed.

use crate::error::ModelError;
use crate::param::{Easing, Keyframe, Param};
use crate::time::Rational;

use super::retiming::{SPEED_CURVE_SCALE, validate_speed_curve};
use super::{Clip, ClipSource, Generator};

/// Why a cut at a point would change what a clip renders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitRefusal {
    /// The cut lands inside the clip's fade-in.
    InsideFadeIn,
    /// The cut lands inside the clip's fade-out.
    InsideFadeOut,
    /// The cut is off a whole cycle of the clip's combo animation.
    OffComboCycle,
    /// The head would be too short for the clip's entrance animation.
    ShortensEntrance,
    /// The tail would be too short for the clip's exit animation.
    ShortensExit,
    /// The clip plays a Lottie file, which holds no animation-time offset.
    Lottie,
    /// The clip shows an animated sticker, which holds no animation-time
    /// offset.
    AnimatedSticker,
    /// A speed-ramp keyframe sits less than one curve tick from the cut.
    RampKeyframeAtCut,
}

impl SplitRefusal {
    /// Every refusal, in declaration order.
    pub const ALL: [SplitRefusal; 8] = [
        Self::InsideFadeIn,
        Self::InsideFadeOut,
        Self::OffComboCycle,
        Self::ShortensEntrance,
        Self::ShortensExit,
        Self::Lottie,
        Self::AnimatedSticker,
        Self::RampKeyframeAtCut,
    ];
}

impl std::fmt::Display for SplitRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InsideFadeIn => "the cut lands inside the clip's fade-in",
            Self::InsideFadeOut => "the cut lands inside the clip's fade-out",
            Self::OffComboCycle => "the cut is off a whole cycle of the clip's combo animation",
            Self::ShortensEntrance => {
                "the cut leaves the head too short for its entrance animation"
            }
            Self::ShortensExit => "the cut leaves the tail too short for its exit animation",
            Self::Lottie => "a Lottie clip holds no animation-time offset, so it cannot be cut",
            Self::AnimatedSticker => {
                "an animated sticker holds no animation-time offset, so it cannot be cut"
            }
            Self::RampKeyframeAtCut => "a speed-ramp keyframe sits too close to the cut",
        })
    }
}

/// Reject splits that the current clip representation cannot express without
/// changing the rendered result. Ordinary keyframes can be rebased, while
/// edge-anchored fades/look animations are partitioned after these checks.
pub(crate) fn validate_split_render_continuity(
    clip: &Clip,
    left_duration: i64,
    right_duration: i64,
    rate: Rational,
) -> Result<(), ModelError> {
    let refusal = if clip.fade_in > left_duration {
        Some(SplitRefusal::InsideFadeIn)
    } else if clip.fade_out > right_duration {
        Some(SplitRefusal::InsideFadeOut)
    } else if clip.animation_combo.is_some() {
        let period = super::look_animation_combo_period_ticks(rate);
        (left_duration % period != 0).then_some(SplitRefusal::OffComboCycle)
    } else {
        let window = |duration| super::look_animation_window_ticks(duration, rate);
        let original = window(clip.timeline.duration.value);
        if clip.animation_in.is_some() && window(left_duration) != original {
            Some(SplitRefusal::ShortensEntrance)
        } else if clip.animation_out.is_some() && window(right_duration) != original {
            Some(SplitRefusal::ShortensExit)
        } else {
            None
        }
    };
    let refusal = refusal.or(match &clip.content {
        ClipSource::Generated(Generator::Lottie { .. }) => Some(SplitRefusal::Lottie),
        ClipSource::Generated(Generator::Sticker { asset })
            if crate::sticker::sticker_spec(asset).is_some_and(|spec| spec.animated) =>
        {
            Some(SplitRefusal::AnimatedSticker)
        }
        ClipSource::Media { .. }
        | ClipSource::Multicam { .. }
        | ClipSource::Sequence { .. }
        | ClipSource::Generated(_) => None,
    });
    refusal.map_or(Ok(()), |refusal| Err(ModelError::Split(refusal)))
}

/// Divide a normalized speed curve at timeline fraction `split`, stretching
/// each restricted domain back onto `0..=`[`SPEED_CURVE_SCALE`].
///
/// The values are instantaneous rates, so preserving the original function on
/// each subdomain preserves its cumulative integral (and therefore
/// [`Clip::source_time_at`]) after the source window is divided at the same
/// point. Boundary easings are subdivided exactly; the fixed integer domain
/// can still make an original keyframe unrepresentable when it lands less than
/// one normalized tick from the cut, in which case splitting fails closed.
pub(crate) fn split_speed_curve(
    curve: &Param<f32>,
    split: f64,
) -> Result<(Param<f32>, Param<f32>), ModelError> {
    if !split.is_finite() || !(0.0..1.0).contains(&split) {
        return Err(ModelError::InvalidParam(
            "speed-ramp split must lie strictly inside the clip".into(),
        ));
    }
    match curve {
        Param::Constant(value) => Ok((Param::Constant(*value), Param::Constant(*value))),
        Param::Keyframed { .. } => Ok((
            speed_curve_subsegment(curve, 0.0, split)?,
            speed_curve_subsegment(curve, split, 1.0)?,
        )),
    }
}

fn speed_curve_subsegment(
    curve: &Param<f32>,
    start: f64,
    end: f64,
) -> Result<Param<f32>, ModelError> {
    let scale = SPEED_CURVE_SCALE as f64;
    let start_tick = start * scale;
    let end_tick = end * scale;
    let mut positions = Vec::with_capacity(curve.keyframes().len() + 2);
    positions.push(start_tick);
    positions.extend(
        curve
            .keyframes()
            .iter()
            .map(|kf| kf.tick as f64)
            .filter(|tick| *tick > start_tick && *tick < end_tick),
    );
    positions.push(end_tick);

    let mut keyframes = Vec::with_capacity(positions.len());
    for (index, &position) in positions.iter().enumerate() {
        let tick = if index == 0 {
            0
        } else if index + 1 == positions.len() {
            SPEED_CURVE_SCALE
        } else {
            (((position - start_tick) / (end_tick - start_tick)) * scale).round() as i64
        };
        if keyframes
            .last()
            .is_some_and(|previous: &Keyframe<f32>| previous.tick >= tick)
        {
            return Err(ModelError::Split(SplitRefusal::RampKeyframeAtCut));
        }
        let easing = positions
            .get(index + 1)
            .map_or(Ok(Easing::Linear), |&next| {
                speed_curve_interval_easing(curve, position, next)
            })?;
        keyframes.push(Keyframe {
            tick,
            value: curve.sample_at(position),
            easing,
            tangents: None,
        });
    }

    let result = Param::Keyframed { keyframes };
    validate_speed_curve(&result)?;
    Ok(result)
}

/// Easing for one interval that is wholly inside an original curve segment
/// (all original keyframe positions were inserted into the interval list).
fn speed_curve_interval_easing(
    curve: &Param<f32>,
    from: f64,
    to: f64,
) -> Result<Easing, ModelError> {
    let keyframes = curve.keyframes();
    let first = &keyframes[0];
    let last = &keyframes[keyframes.len() - 1];
    if from < first.tick as f64 || from >= last.tick as f64 {
        return Ok(Easing::Linear);
    }

    let upper = keyframes.partition_point(|kf| kf.tick as f64 <= from);
    let lower = upper.saturating_sub(1);
    let Some(next) = keyframes.get(lower + 1) else {
        return Ok(Easing::Linear);
    };
    let current = &keyframes[lower];
    if current.value == next.value {
        return Ok(Easing::Linear);
    }
    if to > next.tick as f64 + f64::EPSILON {
        return Err(ModelError::InvalidParam(
            "speed-ramp split crossed an untracked keyframe".into(),
        ));
    }

    let span = (next.tick - current.tick) as f64;
    let local_from = ((from - current.tick as f64) / span).clamp(0.0, 1.0) as f32;
    let local_to = ((to - current.tick as f64) / span).clamp(0.0, 1.0) as f32;
    current.easing.subsegment(local_from, local_to)
}
