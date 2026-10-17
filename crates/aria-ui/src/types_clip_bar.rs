//! What a clip bar on the timeline draws besides its fill and its content:
//! the fx badge, and the line one property of the clip is drawn as.

use std::sync::Arc;

use aria_models::ClipParam;
use gpui::SharedString;

/// What the fx badge on a clip bar reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct UiFxBadge {
    /// An effect chain, a grade, a mask, or a key is applied to the clip.
    pub effects: bool,
    /// A property every clip has (Motion, Opacity, Crop, Time Remapping,
    /// Volume, Pan) differs from a new clip's.
    pub intrinsic: bool,
}

/// How a clip line maps a value to a height on the bar.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LineScale {
    /// Evenly from `min` at the bottom to `max` at the top.
    Linear { min: f32, max: f32 },
    /// A gain, in decibels from [`LineScale::FLOOR_DB`] at the bottom to the
    /// gain `max` at the top. Unity sits where 0 dB falls between the two.
    Gain { max: f32 },
}

impl LineScale {
    /// The level a gain line draws at the bottom of the bar.
    pub const FLOOR_DB: f32 = -60.0;

    /// The height of `value` on the bar: 0 at the bottom, 1 at the top.
    pub fn fraction(self, value: f32) -> f32 {
        match self {
            Self::Linear { min, max } => {
                let span = max - min;
                if span.abs() <= f32::EPSILON {
                    0.5
                } else {
                    ((value - min) / span).clamp(0.0, 1.0)
                }
            }
            Self::Gain { max } => {
                if value <= 0.0 {
                    return 0.0;
                }
                let top = gain_db(max);
                ((gain_db(value) - Self::FLOOR_DB) / (top - Self::FLOOR_DB)).clamp(0.0, 1.0)
            }
        }
    }

    /// The value the bar draws at `fraction` of its height.
    pub fn value(self, fraction: f32) -> f32 {
        let fraction = fraction.clamp(0.0, 1.0);
        match self {
            Self::Linear { min, max } => min + (max - min) * fraction,
            Self::Gain { max } => {
                if fraction <= 0.0 {
                    return 0.0;
                }
                let db = Self::FLOOR_DB + (gain_db(max) - Self::FLOOR_DB) * fraction;
                10f32.powf(db / 20.0)
            }
        }
    }
}

fn gain_db(gain: f32) -> f32 {
    20.0 * gain.max(f32::MIN_POSITIVE).log10()
}

/// The property a clip bar draws as its line, sampled for drawing.
#[derive(Debug, Clone, PartialEq)]
pub struct UiClipLine {
    pub param: ClipParam,
    pub label: SharedString,
    pub scale: LineScale,
    /// Whether a drag on the line writes the value. True for a property that
    /// is one number; a vector property draws its first axis and a colour or
    /// a rectangle draws a level line, and neither is dragged.
    pub draggable: bool,
    /// The line left to right as `(timeline tick, value)`, from the clip's
    /// first frame to its end. Two points for a constant property.
    pub points: Arc<[(f64, f32)]>,
    /// Every keyframe on the property as `(timeline tick, value)`.
    pub keys: Arc<[(i64, f32)]>,
}

impl UiClipLine {
    /// The value the line draws at timeline `tick`, between the two points
    /// around it.
    pub fn value_at(&self, tick: f64) -> f32 {
        let points = &self.points;
        let Some(&(first_tick, first)) = points.first() else {
            return 0.0;
        };
        if tick <= first_tick {
            return first;
        }
        let after = points.partition_point(|&(at, _)| at <= tick);
        let Some(&(next_tick, next)) = points.get(after) else {
            return points.last().map_or(first, |&(_, value)| value);
        };
        let (prev_tick, prev) = points[after - 1];
        let span = next_tick - prev_tick;
        if span <= 0.0 {
            return next;
        }
        let t = ((tick - prev_tick) / span) as f32;
        prev + (next - prev) * t
    }

    /// The height the line draws at timeline `tick`, 0 at the bottom of the
    /// bar and 1 at the top.
    pub fn fraction_at(&self, tick: f64) -> f32 {
        self.scale.fraction(self.value_at(tick))
    }
}
