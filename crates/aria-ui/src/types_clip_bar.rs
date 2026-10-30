//! What a clip bar on the timeline draws besides its fill and its content:
//! the fx badge, and the line one property of the clip is drawn as.

use std::sync::Arc;

use aria_models::ClipParam;
use gpui::SharedString;

use super::Taper;

/// What the fx badge on a clip bar reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct UiFxBadge {
    /// An effect chain, a grade, a mask, or a key is applied to the clip.
    pub effects: bool,
    /// A property every clip has (Motion, Opacity, Crop, Time Remapping,
    /// Volume, Pan) differs from a new clip's.
    pub intrinsic: bool,
}

/// The property a clip bar draws as its line, sampled for drawing.
#[derive(Debug, Clone, PartialEq)]
pub struct UiClipLine {
    pub param: ClipParam,
    pub label: SharedString,
    pub scale: Taper,
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
