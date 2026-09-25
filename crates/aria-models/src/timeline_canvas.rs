//! The ruler and the canvas: what a marker is, and what shape the frame is.
//!
//! Moved out of [`crate::timeline`] unchanged. None of it reads a [`Timeline`]:
//! a marker is a labelled tick and a canvas is a size, so the sequence file is
//! left holding only the sequence.
//!
//! [`Timeline`]: crate::timeline::Timeline

use serde::{Deserialize, Serialize};

use crate::ids::MarkerId;
use crate::time::RationalTime;

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

/// A named, colored anchor point on the timeline ruler (M1 markers): the
/// agent aligns edits to them, beat-sync (M8) will emit them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Marker {
    pub id: MarkerId,
    /// Position on the timeline, stored at the timeline frame rate
    /// ([`crate::Timeline::add_marker`] resamples on insert).
    pub tick: RationalTime,
    /// Short label shown beside the flag. May be empty (unnamed marker).
    pub name: String,
    pub color: MarkerColor,
    /// Longer note kept with the marker: the reason it is there. Empty on a
    /// marker that carries only a label, and absent from saves while empty so
    /// older files load unchanged.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub comment: String,
}

impl Marker {
    /// A fresh marker with a newly allocated id and no comment.
    pub fn new(tick: RationalTime, name: impl Into<String>, color: MarkerColor) -> Self {
        Self {
            id: MarkerId::next(),
            tick,
            name: name.into(),
            color,
            comment: String::new(),
        }
    }

    /// The same marker carrying `comment`.
    #[must_use]
    pub fn with_comment(mut self, comment: impl Into<String>) -> Self {
        self.comment = comment.into();
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
}
