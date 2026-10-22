//! What a clip bar draws from the model besides its fill and its content: the
//! speed in the name strip, the fx badge, the offset of a drifted link, the
//! offline state, and the line one property is drawn as.

use std::path::Path;
use std::sync::Arc;

use aria_models::{Clip, ClipAttribute, ClipParam, ParamKind, ParamRange};
use gpui::SharedString;

use crate::app::curve_geometry::sample_param_scalar;
use crate::types::{LineScale, UiClipLine, UiFxBadge};

/// Points sampled evenly along an animated line, besides each keyframe and
/// the frame before it.
const LINE_SAMPLES: usize = 48;

/// The level a property with no number on a line (a colour, a rectangle)
/// draws at: halfway up the bar.
const LEVEL: f32 = 0.5;

/// The speed the name strip prints: `None` at 100 % forwards, and for a speed
/// ramp, whose speed is not one number.
pub(super) fn speed_label(clip: &Clip) -> Option<SharedString> {
    if clip.has_speed_curve() || (clip.speed.num == clip.speed.den && !clip.reversed) {
        return None;
    }
    let percent = f64::from(clip.speed.num) / f64::from(clip.speed.den.max(1)) * 100.0;
    let sign = if clip.reversed { "-" } else { "" };
    Some(format!("{sign}{percent:.0}%").into())
}

/// Which colour the fx badge draws: whether anything is applied to the clip,
/// and whether a property every clip has was changed.
pub(super) fn fx_badge(clip: &Clip) -> UiFxBadge {
    let mut badge = UiFxBadge::default();
    for group in ClipAttribute::ALL {
        if group.is_set(clip) {
            if group.is_intrinsic() {
                badge.intrinsic = true;
            } else {
                badge.effects = true;
            }
        }
    }
    badge
}

/// The name an offline clip shows for its media file at `path`: the file name,
/// or the whole path when it ends in no file name.
pub(super) fn offline(path: &str) -> SharedString {
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path)
        .to_owned()
        .into()
}

/// The line `clip`'s bar draws: the property chosen for it, else the lane's
/// default. `None` when the clip has neither.
pub(super) fn clip_line(clip: &Clip, audio_lane: bool) -> Option<UiClipLine> {
    let spec = clip.line_spec(audio_lane)?;
    let start = clip.timeline.start.value;
    let end = clip.timeline.end_tick().max(start + 1);
    let numeric = matches!(
        spec.kind,
        ParamKind::Scalar | ParamKind::Vec2 | ParamKind::Vec3
    );
    let component = matches!(spec.kind, ParamKind::Vec2 | ParamKind::Vec3).then_some(0u8);
    let sample_local = |local: f64| -> f32 {
        if numeric {
            sample_param_scalar(&spec, clip, component, local).unwrap_or(0.0)
        } else {
            LEVEL
        }
    };
    let keys: Arc<[(i64, f32)]> = spec
        .keyframes(clip)
        .into_iter()
        .map(|(local, _)| (clip.param_timeline_tick(local), sample_local(local as f64)))
        .collect();

    let (low, high) = (start as f64, end as f64);
    let mut ticks = vec![low, high];
    if !keys.is_empty() {
        let span = high - low;
        ticks.extend((1..LINE_SAMPLES).map(|step| low + span * step as f64 / LINE_SAMPLES as f64));
        // The frame before each keyframe keeps a held segment square rather
        // than ramping it across the gap to the next even sample.
        for &(tick, _) in keys.iter() {
            ticks.push(tick as f64);
            ticks.push((tick - 1) as f64);
        }
        ticks.retain(|tick| (low..=high).contains(tick));
        ticks.sort_by(f64::total_cmp);
        ticks.dedup();
    }
    let points: Arc<[(f64, f32)]> = ticks
        .into_iter()
        .map(|tick| (tick, sample_local(clip.param_tick_f(tick))))
        .collect();

    let scale = if !numeric {
        LineScale::Linear { min: 0.0, max: 1.0 }
    } else if spec.param == ClipParam::Volume {
        LineScale::Gain {
            max: spec.range.max.unwrap_or(aria_models::MAX_CLIP_VOLUME),
        }
    } else {
        fitted(spec.range, points.iter().map(|&(_, value)| value))
    };
    Some(UiClipLine {
        param: spec.param,
        label: match spec.label {
            std::borrow::Cow::Borrowed(label) => SharedString::new_static(label),
            std::borrow::Cow::Owned(label) => label.into(),
        },
        scale,
        draggable: spec.kind == ParamKind::Scalar,
        points,
        keys,
    })
}

/// The range a line spans: the property's own bounds when it declares both,
/// else the values it takes with room around them, inside whichever bound it
/// does declare.
fn fitted(range: ParamRange, values: impl Iterator<Item = f32>) -> LineScale {
    if let (Some(min), Some(max)) = (range.min, range.max) {
        return LineScale::Linear { min, max };
    }
    let (low, high) = values.fold((f32::INFINITY, f32::NEG_INFINITY), |(low, high), value| {
        (low.min(value), high.max(value))
    });
    if !low.is_finite() || !high.is_finite() {
        return LineScale::Linear { min: 0.0, max: 1.0 };
    }
    let pad = if high - low < 1e-3 {
        low.abs().max(1.0)
    } else {
        (high - low) * 0.25
    };
    let min = range.min.map_or(low - pad, |floor| (low - pad).max(floor));
    let max = range
        .max
        .map_or(high + pad, |ceiling| (high + pad).min(ceiling));
    LineScale::Linear { min, max }
}

#[cfg(test)]
#[path = "projection_bar_tests.rs"]
mod tests;
