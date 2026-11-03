//! Timeline time values and the clip and marker summaries that project
//! queries return.

use serde::{Deserialize, Serialize};

use crate::MarkerColor;

/// Rational frame rate or timebase (`num / den` ticks per second).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Rational {
    /// Numerator.
    pub num: i64,
    /// Denominator.
    pub den: i64,
}

impl Rational {
    /// 24 per second.
    pub const FPS_24: Self = Self { num: 24, den: 1 };
    /// 25 per second.
    pub const FPS_25: Self = Self { num: 25, den: 1 };
    /// 30 per second.
    pub const FPS_30: Self = Self { num: 30, den: 1 };
    /// 60 per second.
    pub const FPS_60: Self = Self { num: 60, den: 1 };

    /// The rate `num / den` per second.
    pub const fn new(num: i64, den: i64) -> Self {
        Self { num, den }
    }
}

/// Exact timestamp `value / rate` seconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RationalTime {
    /// Tick count.
    pub value: i64,
    /// Ticks per second.
    pub rate: Rational,
}

impl RationalTime {
    /// The time `value / rate` seconds.
    pub const fn new(value: i64, rate: Rational) -> Self {
        Self { value, rate }
    }
}

/// Half-open time interval `[start, start + duration)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TimeRange {
    /// The first tick.
    pub start: RationalTime,
    /// The length.
    pub duration: RationalTime,
}

impl TimeRange {
    /// The range from `start` lasting `duration`.
    pub const fn new(start: RationalTime, duration: RationalTime) -> Self {
        Self { start, duration }
    }
}

/// Summary of a timeline clip returned by [`Host`](crate::Host) project queries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClipRef {
    /// The clip id.
    pub id: u64,
    /// Id of the track holding the clip.
    pub track: u64,
    /// The clip's first tick on the timeline.
    pub start: RationalTime,
    /// The tick after the clip's last frame.
    pub end: RationalTime,
    /// `media`, `sequence`, `multicam`, the generator id of a generated clip,
    /// or the clip type id of a typed clip.
    pub kind: String,
    /// The clip's title. A typed clip with no name of its own reads the type's
    /// name.
    pub name: String,
}

/// Sequence In and Out points returned by [`Host::in_out`](crate::Host::in_out).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InOut {
    /// The In point, or `None` when unset.
    pub mark_in: Option<RationalTime>,
    /// The Out point, or `None` when unset.
    pub mark_out: Option<RationalTime>,
}

/// Marker category (`Comment` by default).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MarkerKind {
    /// A note.
    #[default]
    Comment,
    /// A chapter point.
    Chapter,
    /// A segment, such as a range an analyzer writes.
    Segmentation,
}

impl MarkerKind {
    /// Whether this is [`Self::Comment`], the kind of a marker whose JSON omits
    /// `kind`.
    pub fn is_default(&self) -> bool {
        *self == Self::Comment
    }
}

fn zero_marker_duration() -> RationalTime {
    RationalTime::new(0, Rational::FPS_24)
}

fn is_zero_marker_duration(duration: &RationalTime) -> bool {
    duration.value == 0
}

/// A ruler marker returned by [`Host::markers_in_range`](crate::Host::markers_in_range).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Marker {
    /// The marker id.
    pub id: u64,
    /// Position on the sequence timeline.
    pub tick: RationalTime,
    /// Span of a range marker, or 0 ticks for a point marker.
    #[serde(
        default = "zero_marker_duration",
        skip_serializing_if = "is_zero_marker_duration"
    )]
    pub duration: RationalTime,
    /// The marker name.
    pub name: String,
    /// The marker colour.
    pub color: MarkerColor,
    /// Comment text, empty when none.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub comment: String,
    /// The marker category.
    #[serde(default, skip_serializing_if = "MarkerKind::is_default")]
    pub kind: MarkerKind,
}

/// Which sequence or clip [`Host::render_frame`](crate::Host::render_frame) composites.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FrameSource {
    /// The sequence open for editing.
    Open,
    /// The root sequence (`None`) or the nested sequence with the given id.
    Sequence(Option<u64>),
    /// A single clip by id, rendered through its own effects on its sequence canvas.
    Clip(u64),
}

/// Output dimensions for [`Host::render_frame`](crate::Host::render_frame).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FrameSize {
    /// Sequence canvas resolution (at most 4096 on each side).
    Canvas,
    /// Fit inside `max_width` by `max_height` preserving aspect ratio and rounding to even.
    Fit {
        /// Largest output width in pixels (1 through 4096).
        max_width: u32,
        /// Largest output height in pixels (1 through 4096).
        max_height: u32,
    },
    /// Exact `width` by `height` in pixels (each 1 through 4096).
    Exact {
        /// Output width in pixels.
        width: u32,
        /// Output height in pixels.
        height: u32,
    },
}

/// Straight-alpha RGBA8 frame returned by [`Host::render_frame`](crate::Host::render_frame).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderedFrame {
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
    /// Straight-alpha RGBA8 pixel bytes (`width * height * 4`, top row first).
    pub data: Vec<u8>,
}
