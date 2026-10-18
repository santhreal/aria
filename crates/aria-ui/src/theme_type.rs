//! The UI typeface and the OpenType features UI text is shaped with.

use super::*;

/// The family UI text is set in: the embedded Inter that
/// [`crate::register_fonts`] adds to the text system.
pub const UI_FAMILY: &str = "Inter";

/// The OpenType features a readout is shaped with: tabular figures (`tnum`),
/// so a timecode, a value, or a frame count keeps each digit's width while it
/// changes, and numbers in a column align.
///
/// Only readouts take it. Every other run keeps Inter's proportional figures:
/// its tabular `one` and hyphen are as wide as a zero, which sets a name such
/// as `clip-1080p60` like a monospace face.
pub fn tabular_figures() -> gpui::FontFeatures {
    static FEATURES: std::sync::LazyLock<gpui::FontFeatures> = std::sync::LazyLock::new(|| {
        gpui::FontFeatures(std::sync::Arc::new(vec![("tnum".to_owned(), 1)]))
    });
    FEATURES.clone()
}

/// No OpenType features beyond the shaper's defaults: Inter's proportional
/// figures. One shared list, so a run built per frame does not allocate one.
fn proportional_figures() -> gpui::FontFeatures {
    static FEATURES: std::sync::LazyLock<gpui::FontFeatures> =
        std::sync::LazyLock::new(gpui::FontFeatures::default);
    FEATURES.clone()
}

impl Theme {
    /// The font of a text run built by hand: the UI family at `weight`, with
    /// proportional figures, the way an element inherits it from the window
    /// root.
    pub fn ui_font(&self, weight: gpui::FontWeight) -> gpui::Font {
        self.font_with(weight, proportional_figures())
    }

    /// The font of a readout built by hand: [`Self::ui_font`] with
    /// [`tabular_figures`].
    pub fn figures_font(&self, weight: gpui::FontWeight) -> gpui::Font {
        self.font_with(weight, tabular_figures())
    }

    fn font_with(&self, weight: gpui::FontWeight, features: gpui::FontFeatures) -> gpui::Font {
        gpui::Font {
            family: self.font_sans.clone(),
            features,
            fallbacks: None,
            weight,
            style: gpui::FontStyle::Normal,
        }
    }
}
