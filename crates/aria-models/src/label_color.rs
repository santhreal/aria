//! Clip and media label colours.
//!
//! Sixteen named colours, the set Premiere Pro labels clips with, so a label
//! survives a round trip through an editor that uses the same names. A label is
//! visual only: the timeline, the Project panel and the inspector draw it, and
//! nothing composites it.
//!
//! Saved by snake-case name. Files saved when labels shared the eight marker
//! colours load through aliases: `pink` reads as Magenta, `red` as Rose, and
//! `orange` as Mango; the other five names exist in both sets.

use serde::{Deserialize, Serialize};

use crate::MarkerColor;

/// A clip or pool entry label colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LabelColor {
    Violet,
    Iris,
    Caribbean,
    Lavender,
    Cerulean,
    Forest,
    #[serde(alias = "red")]
    Rose,
    #[serde(alias = "orange")]
    Mango,
    Purple,
    Blue,
    Teal,
    #[serde(alias = "pink")]
    Magenta,
    Tan,
    Green,
    Brown,
    Yellow,
}

impl LabelColor {
    /// Every label, in menu order.
    pub const ALL: [LabelColor; 16] = [
        LabelColor::Violet,
        LabelColor::Iris,
        LabelColor::Caribbean,
        LabelColor::Lavender,
        LabelColor::Cerulean,
        LabelColor::Forest,
        LabelColor::Rose,
        LabelColor::Mango,
        LabelColor::Purple,
        LabelColor::Blue,
        LabelColor::Teal,
        LabelColor::Magenta,
        LabelColor::Tan,
        LabelColor::Green,
        LabelColor::Brown,
        LabelColor::Yellow,
    ];

    /// The default label of a video clip.
    pub const VIDEO: LabelColor = LabelColor::Teal;
    /// The default label of an audio clip.
    pub const AUDIO: LabelColor = LabelColor::Forest;
    /// The default label of a generator, title, or shape clip.
    pub const GENERATOR: LabelColor = LabelColor::Mango;
    /// The default label of a nested sequence clip.
    pub const NESTED: LabelColor = LabelColor::Tan;
    /// The default label of a clip on an adjustment lane.
    pub const ADJUSTMENT: LabelColor = LabelColor::Yellow;

    /// The saved name: `"violet"`, `"iris"`, ...
    pub fn id(self) -> &'static str {
        match self {
            LabelColor::Violet => "violet",
            LabelColor::Iris => "iris",
            LabelColor::Caribbean => "caribbean",
            LabelColor::Lavender => "lavender",
            LabelColor::Cerulean => "cerulean",
            LabelColor::Forest => "forest",
            LabelColor::Rose => "rose",
            LabelColor::Mango => "mango",
            LabelColor::Purple => "purple",
            LabelColor::Blue => "blue",
            LabelColor::Teal => "teal",
            LabelColor::Magenta => "magenta",
            LabelColor::Tan => "tan",
            LabelColor::Green => "green",
            LabelColor::Brown => "brown",
            LabelColor::Yellow => "yellow",
        }
    }

    /// The name menus show: `"Violet"`, `"Iris"`, ...
    pub fn name(self) -> &'static str {
        match self {
            LabelColor::Violet => "Violet",
            LabelColor::Iris => "Iris",
            LabelColor::Caribbean => "Caribbean",
            LabelColor::Lavender => "Lavender",
            LabelColor::Cerulean => "Cerulean",
            LabelColor::Forest => "Forest",
            LabelColor::Rose => "Rose",
            LabelColor::Mango => "Mango",
            LabelColor::Purple => "Purple",
            LabelColor::Blue => "Blue",
            LabelColor::Teal => "Teal",
            LabelColor::Magenta => "Magenta",
            LabelColor::Tan => "Tan",
            LabelColor::Green => "Green",
            LabelColor::Brown => "Brown",
            LabelColor::Yellow => "Yellow",
        }
    }

    /// The label whose saved name or menu name is `name`, ignoring case. The
    /// three marker names the label set lacks resolve as their aliases do.
    pub fn from_name(name: &str) -> Option<Self> {
        let name = name.trim();
        if let Some(label) = Self::ALL
            .into_iter()
            .find(|label| label.id().eq_ignore_ascii_case(name))
        {
            return Some(label);
        }
        ["red", "orange", "pink"]
            .into_iter()
            .find(|alias| alias.eq_ignore_ascii_case(name))
            .and_then(|alias| MarkerColor::ALL.into_iter().find(|m| m.name() == alias))
            .map(Self::from)
    }

    /// The fill as `[r, g, b]`.
    pub fn rgb(self) -> [u8; 3] {
        match self {
            LabelColor::Violet => [0xA5, 0x8C, 0xDE],
            LabelColor::Iris => [0x71, 0x80, 0xDD],
            LabelColor::Caribbean => [0x1D, 0xC2, 0x97],
            LabelColor::Lavender => [0xD8, 0x8F, 0xDB],
            LabelColor::Cerulean => [0x1B, 0xA2, 0xE4],
            LabelColor::Forest => [0x41, 0x8F, 0x37],
            LabelColor::Rose => [0xF1, 0x4A, 0x75],
            LabelColor::Mango => [0xF1, 0x9E, 0x1F],
            LabelColor::Purple => [0x98, 0x39, 0xA6],
            LabelColor::Blue => [0x2E, 0x52, 0xD6],
            LabelColor::Teal => [0x19, 0x91, 0x91],
            LabelColor::Magenta => [0xE3, 0x3B, 0xAF],
            LabelColor::Tan => [0xC9, 0xB0, 0x86],
            LabelColor::Green => [0x3C, 0xA8, 0x3C],
            LabelColor::Brown => [0x8B, 0x52, 0x20],
            LabelColor::Yellow => [0xE3, 0xD3, 0x2A],
        }
    }

    /// The fill of a selected clip with this label: the fill a third of the way
    /// to white, so selection reads on every hue without a second palette.
    pub fn selected_rgb(self) -> [u8; 3] {
        self.rgb()
            .map(|channel| channel + ((255 - u16::from(channel)) / 3) as u8)
    }
}

/// The label a marker colour reads as: the same name where both sets have it.
impl From<MarkerColor> for LabelColor {
    fn from(color: MarkerColor) -> Self {
        match color {
            MarkerColor::Teal => LabelColor::Teal,
            MarkerColor::Blue => LabelColor::Blue,
            MarkerColor::Purple => LabelColor::Purple,
            MarkerColor::Pink => LabelColor::Magenta,
            MarkerColor::Red => LabelColor::Rose,
            MarkerColor::Orange => LabelColor::Mango,
            MarkerColor::Yellow => LabelColor::Yellow,
            MarkerColor::Green => LabelColor::Green,
        }
    }
}

#[cfg(test)]
#[path = "label_color_tests.rs"]
mod tests;
