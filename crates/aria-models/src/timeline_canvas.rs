//! The ruler and the canvas: what a marker is, and what shape the frame is.
//!
//! Moved out of [`crate::timeline`] unchanged. None of it reads a [`Timeline`]:
//! a marker is a labelled tick and a canvas is a size, so the sequence file is
//! left holding only the sequence.
//!
//! [`Timeline`]: crate::timeline::Timeline

use serde::{Deserialize, Serialize};

use crate::ids::MarkerId;
use crate::time::{Rational, RationalTime};

/// Fixed marker flag palette (M1 markers). Serialized by name so project
/// files stay readable; [`rgba`](Self::rgba) gives the render color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MarkerColor {
    Teal,
    Blue,
    Purple,
    Pink,
    Red,
    Orange,
    Yellow,
    Green,
}

impl MarkerColor {
    pub const ALL: [MarkerColor; 8] = [
        MarkerColor::Teal,
        MarkerColor::Blue,
        MarkerColor::Purple,
        MarkerColor::Pink,
        MarkerColor::Red,
        MarkerColor::Orange,
        MarkerColor::Yellow,
        MarkerColor::Green,
    ];

    /// The color for the `index`-th marker when none was chosen explicitly:
    /// cycle through the palette so neighboring markers stay distinguishable.
    pub fn cycle(index: usize) -> Self {
        Self::ALL[index % Self::ALL.len()]
    }

    /// Render color as `[r, g, b, a]`.
    pub fn rgba(self) -> [u8; 4] {
        match self {
            MarkerColor::Teal => [0x00, 0xE5, 0xC7, 0xFF],
            MarkerColor::Blue => [0x4A, 0x9E, 0xF5, 0xFF],
            MarkerColor::Purple => [0xA7, 0x7B, 0xF5, 0xFF],
            MarkerColor::Pink => [0xF5, 0x6F, 0xC0, 0xFF],
            MarkerColor::Red => [0xF5, 0x5A, 0x5A, 0xFF],
            MarkerColor::Orange => [0xF5, 0x9A, 0x3C, 0xFF],
            MarkerColor::Yellow => [0xF0, 0xD0, 0x4A, 0xFF],
            MarkerColor::Green => [0x6F, 0xD8, 0x5E, 0xFF],
        }
    }

    /// The serialized lowercase name ("teal", "blue", …).
    pub fn name(self) -> &'static str {
        match self {
            MarkerColor::Teal => "teal",
            MarkerColor::Blue => "blue",
            MarkerColor::Purple => "purple",
            MarkerColor::Pink => "pink",
            MarkerColor::Red => "red",
            MarkerColor::Orange => "orange",
            MarkerColor::Yellow => "yellow",
            MarkerColor::Green => "green",
        }
    }
}

/// Canvas aspect-ratio presets (M1 canvas settings, the CapCut ratio list).
/// Serialized by ratio name (`"16:9"`) so project files stay readable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum CanvasAspect {
    /// Follow the footage: canvas shape and size derive from the largest
    /// video media on the timeline (the pre-canvas-settings behavior).
    #[default]
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "16:9")]
    Wide16x9,
    #[serde(rename = "9:16")]
    Tall9x16,
    #[serde(rename = "1:1")]
    Square1x1,
    #[serde(rename = "4:5")]
    Portrait4x5,
    #[serde(rename = "21:9")]
    Cinema21x9,
}

impl CanvasAspect {
    pub const ALL: [CanvasAspect; 6] = [
        CanvasAspect::Auto,
        CanvasAspect::Wide16x9,
        CanvasAspect::Tall9x16,
        CanvasAspect::Square1x1,
        CanvasAspect::Portrait4x5,
        CanvasAspect::Cinema21x9,
    ];

    /// `(w, h)` ratio for fixed presets; `None` follows the footage.
    pub fn ratio(self) -> Option<(u32, u32)> {
        match self {
            CanvasAspect::Auto => None,
            CanvasAspect::Wide16x9 => Some((16, 9)),
            CanvasAspect::Tall9x16 => Some((9, 16)),
            CanvasAspect::Square1x1 => Some((1, 1)),
            CanvasAspect::Portrait4x5 => Some((4, 5)),
            CanvasAspect::Cinema21x9 => Some((21, 9)),
        }
    }

    /// The serialized name (`"auto"`, `"16:9"`, …) — also the UI label and
    /// the agent-facing identifier.
    pub fn name(self) -> &'static str {
        match self {
            CanvasAspect::Auto => "auto",
            CanvasAspect::Wide16x9 => "16:9",
            CanvasAspect::Tall9x16 => "9:16",
            CanvasAspect::Square1x1 => "1:1",
            CanvasAspect::Portrait4x5 => "4:5",
            CanvasAspect::Cinema21x9 => "21:9",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.name() == name)
    }

    /// Pixel size for this preset at `baseline` on the shorter edge, or `None`
    /// for `Auto` (which has no shape of its own). The shorter edge is what
    /// "1080p" names in both framings: 1920×1080 wide, 1080×1920 vertical.
    /// Both edges are rounded down to even values, because H.264 needs them.
    pub fn pixels_for_baseline(self, baseline: u32) -> Option<[u32; 2]> {
        let (rw, rh) = self.ratio()?;
        let base = baseline as f32;
        let (w, h) = if rw >= rh {
            ((base * rw as f32 / rh as f32).round(), base)
        } else {
            (base, (base * rh as f32 / rw as f32).round())
        };
        Some([even(w as u32), even(h as u32)])
    }
}

/// Round `v` down to the nearest even value (≥ 2): H.264 needs even dimensions.
fn even(v: u32) -> u32 {
    (v & !1).max(2)
}

/// Audio channel layout for the timeline sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AudioLayout {
    Mono,
    #[default]
    Stereo,
    #[serde(rename = "5.1")]
    Surround51,
}

impl AudioLayout {
    pub const ALL: [AudioLayout; 3] = [
        AudioLayout::Mono,
        AudioLayout::Stereo,
        AudioLayout::Surround51,
    ];

    /// The serialized name (`"mono"`, `"stereo"`, `"5.1"`).
    pub fn name(self) -> &'static str {
        match self {
            AudioLayout::Mono => "mono",
            AudioLayout::Stereo => "stereo",
            AudioLayout::Surround51 => "5.1",
        }
    }

    /// The human-readable label for UI pickers.
    pub fn label(self) -> &'static str {
        match self {
            AudioLayout::Mono => "Mono (1.0)",
            AudioLayout::Stereo => "Stereo (2.0)",
            AudioLayout::Surround51 => "5.1 Surround",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|l| l.name() == name)
    }

    pub fn channels(self) -> u16 {
        match self {
            AudioLayout::Mono => 1,
            AudioLayout::Stereo => 2,
            AudioLayout::Surround51 => 6,
        }
    }

    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// How a sequence counts time on its ruler: SMPTE timecode, a frame count, or
/// feet and frames of 35 mm 4-perf film, sixteen frames to the foot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TimeDisplay {
    #[default]
    Timecode,
    Frames,
    FeetFrames,
}

impl TimeDisplay {
    pub const ALL: [TimeDisplay; 3] = [
        TimeDisplay::Timecode,
        TimeDisplay::Frames,
        TimeDisplay::FeetFrames,
    ];

    /// Frames in one foot of 35 mm 4-perf film.
    pub const FRAMES_PER_FOOT: i64 = 16;

    /// The serialized name (`"timecode"`, `"frames"`, `"feet_frames"`).
    pub fn name(self) -> &'static str {
        match self {
            TimeDisplay::Timecode => "timecode",
            TimeDisplay::Frames => "frames",
            TimeDisplay::FeetFrames => "feet_frames",
        }
    }

    /// The human-readable label for UI pickers.
    pub fn label(self) -> &'static str {
        match self {
            TimeDisplay::Timecode => "Timecode",
            TimeDisplay::Frames => "Frames",
            TimeDisplay::FeetFrames => "Feet + Frames 35mm",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|display| display.name() == name)
    }

    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// Per-project canvas settings (M1): aspect preset, background color, and an
/// optional explicit pixel size. The default (`Auto` + black + derived size)
/// reproduces the pre-canvas-settings render exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CanvasSettings {
    #[serde(default, skip_serializing_if = "CanvasSettings::aspect_is_auto")]
    pub aspect: CanvasAspect,
    /// Opaque canvas background, `[r, g, b]`. Layers composite over it;
    /// uncovered canvas shows it in preview and export.
    #[serde(default, skip_serializing_if = "CanvasSettings::background_is_black")]
    pub background: [u8; 3],
    /// Explicit canvas size in pixels, `[w, h]`. `None` derives it: a fixed
    /// aspect resolves to a 1080 baseline, `Auto` follows the largest video
    /// media. An explicit size wins over both, so a delivery spec is a project
    /// setting rather than an export-time guess.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<[u32; 2]>,
}

impl CanvasSettings {
    /// Smallest explicit canvas edge. Below this a canvas is not a frame
    /// anyone edits, and the even-rounding floor is 2.
    pub const MIN_DIM: u32 = 16;
    /// Largest explicit canvas edge: the compositor's texture bound.
    pub const MAX_DIM: u32 = 8192;

    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// Whether `[w, h]` is an encodable canvas: both edges within bounds and
    /// even. Callers validate before storing, so no later stage has to guess
    /// what a 0-pixel or 12000-pixel canvas meant.
    /// Whether `[w, h]` is an encodable canvas: both edges within bounds, >= 2, and even.
    pub fn resolution_is_valid([w, h]: [u32; 2]) -> bool {
        [w, h]
            .into_iter()
            .all(|v| v >= 2 && (Self::MIN_DIM..=Self::MAX_DIM).contains(&v) && v % 2 == 0)
    }

    /// Create custom canvas settings with validated explicit resolution.
    pub fn custom(
        aspect: CanvasAspect,
        background: [u8; 3],
        resolution: Option<[u32; 2]>,
    ) -> Result<Self, crate::error::ModelError> {
        if let Some(res) = resolution {
            if !Self::resolution_is_valid(res) {
                return Err(crate::error::ModelError::InvalidParam(format!(
                    "invalid canvas resolution {}x{}: dimensions must be even and between {} and {}",
                    res[0],
                    res[1],
                    Self::MIN_DIM,
                    Self::MAX_DIM
                )));
            }
        }
        Ok(Self {
            aspect,
            background,
            resolution,
        })
    }
    fn aspect_is_auto(aspect: &CanvasAspect) -> bool {
        *aspect == CanvasAspect::Auto
    }

    fn background_is_black(background: &[u8; 3]) -> bool {
        *background == [0, 0, 0]
    }
}

/// Marker category (`Comment` by default).
///
/// Omitted from saves while `Comment` so older project files load unchanged and
/// ordinary comment markers stay byte-identical.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MarkerKind {
    #[default]
    Comment,
    Chapter,
    Segmentation,
}

impl MarkerKind {
    pub const ALL: [MarkerKind; 3] = [
        MarkerKind::Comment,
        MarkerKind::Chapter,
        MarkerKind::Segmentation,
    ];

    pub fn name(self) -> &'static str {
        match self {
            MarkerKind::Comment => "comment",
            MarkerKind::Chapter => "chapter",
            MarkerKind::Segmentation => "segmentation",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            MarkerKind::Comment => "Comment",
            MarkerKind::Chapter => "Chapter",
            MarkerKind::Segmentation => "Segmentation",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "comment" => Some(MarkerKind::Comment),
            "chapter" => Some(MarkerKind::Chapter),
            "segmentation" => Some(MarkerKind::Segmentation),
            _ => None,
        }
    }

    pub fn cycle_next(self) -> Self {
        match self {
            MarkerKind::Comment => MarkerKind::Chapter,
            MarkerKind::Chapter => MarkerKind::Segmentation,
            MarkerKind::Segmentation => MarkerKind::Comment,
        }
    }

    pub fn is_default(&self) -> bool {
        *self == MarkerKind::Comment
    }
}

fn zero_marker_duration() -> RationalTime {
    RationalTime::new(0, Rational::FPS_24)
}

fn is_zero_marker_duration(duration: &RationalTime) -> bool {
    duration.value == 0
}

/// A named, colored marker on the timeline ruler or on a clip (in clip time).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Marker {
    pub id: MarkerId,
    /// Position on the timeline (for sequence markers) or in clip time (for
    /// clip markers).
    pub tick: RationalTime,
    /// Span of a range marker (`value == 0` for a point marker). Omitted from
    /// saves while zero so older project files load unchanged.
    #[serde(
        default = "zero_marker_duration",
        skip_serializing_if = "is_zero_marker_duration"
    )]
    pub duration: RationalTime,
    /// Short label shown beside the flag. May be empty (unnamed marker).
    pub name: String,
    pub color: MarkerColor,
    /// Longer note kept with the marker: the reason it is there. Empty on a
    /// marker that carries only a label, and absent from saves while empty so
    /// older files load unchanged.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub comment: String,
    /// Marker category (`Comment` by default). Absent from saves while
    /// `Comment` so older files load unchanged.
    #[serde(default, skip_serializing_if = "MarkerKind::is_default")]
    pub kind: MarkerKind,
}

impl Marker {
    /// A fresh point marker with a newly allocated id, no comment, zero
    /// duration, and [`MarkerKind::Comment`].
    pub fn new(tick: RationalTime, name: impl Into<String>, color: MarkerColor) -> Self {
        Self {
            id: MarkerId::next(),
            tick,
            duration: RationalTime::new(0, tick.rate),
            name: name.into(),
            color,
            comment: String::new(),
            kind: MarkerKind::Comment,
        }
    }

    /// The same marker carrying `comment`.
    #[must_use]
    pub fn with_comment(mut self, comment: impl Into<String>) -> Self {
        self.comment = comment.into();
        self
    }

    /// The same marker carrying `duration`.
    #[must_use]
    pub fn with_duration(mut self, duration: RationalTime) -> Self {
        self.duration = duration;
        self
    }

    /// The same marker carrying `kind`.
    #[must_use]
    pub fn with_kind(mut self, kind: MarkerKind) -> Self {
        self.kind = kind;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canvas_resolution_validation_enforces_even_and_bounds() {
        assert!(CanvasSettings::resolution_is_valid([1920, 1080]));
        assert!(CanvasSettings::resolution_is_valid([16, 16]));

        // Odd dimensions rejected
        assert!(!CanvasSettings::resolution_is_valid([1921, 1080]));
        assert!(!CanvasSettings::resolution_is_valid([1920, 1081]));

        // Below MIN_DIM (16) or < 2 rejected
        assert!(!CanvasSettings::resolution_is_valid([0, 1080]));
        assert!(!CanvasSettings::resolution_is_valid([14, 14]));

        // Above MAX_DIM (8192) rejected
        assert!(!CanvasSettings::resolution_is_valid([8194, 8192]));

        // Constructor validation
        assert!(CanvasSettings::custom(CanvasAspect::Auto, [0, 0, 0], Some([1920, 1080])).is_ok());
        assert!(CanvasSettings::custom(CanvasAspect::Auto, [0, 0, 0], Some([1921, 1080])).is_err());
        assert!(CanvasSettings::custom(CanvasAspect::Auto, [0, 0, 0], None).is_ok());
    }

    #[test]
    fn old_marker_json_loads_with_zero_duration_and_comment_kind() {
        let old_json = r#"{"id":42,"tick":{"value":24,"rate":{"num":24,"den":1}},"name":"Beat","color":"teal"}"#;
        let marker: Marker = serde_json::from_str(old_json).expect("old marker shape loads");
        assert_eq!(marker.id, MarkerId::from_raw(42));
        assert_eq!(marker.tick, RationalTime::new(24, Rational::FPS_24));
        assert_eq!(marker.duration.value, 0);
        assert_eq!(marker.kind, MarkerKind::Comment);
        assert_eq!(marker.comment, "");

        let resaved = serde_json::to_string(&marker).expect("serialize default marker");
        assert!(!resaved.contains("duration"));
        assert!(!resaved.contains("kind"));
    }

    #[test]
    fn marker_with_duration_and_kind_round_trips() {
        let marker = Marker::new(
            RationalTime::new(48, Rational::FPS_24),
            "Act I",
            MarkerColor::Orange,
        )
        .with_comment("Opening sequence")
        .with_duration(RationalTime::new(72, Rational::FPS_24))
        .with_kind(MarkerKind::Chapter);

        let json = serde_json::to_string(&marker).expect("serialize range chapter marker");
        assert!(json.contains(r#""kind":"chapter""#));
        assert!(json.contains(r#""duration""#));
        let decoded: Marker = serde_json::from_str(&json).expect("deserialize marker");
        assert_eq!(decoded, marker);
    }
}
