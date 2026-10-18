//! The UI typeface and the OpenType features UI text is shaped with.

use super::*;

/// The family UI text is set in: the embedded Inter that
/// [`crate::register_fonts`] adds to the text system.
pub const UI_FAMILY: &str = "Inter";

/// The OpenType features every run of UI text is shaped with: tabular
/// figures (`tnum`), so a timecode, a value, or a frame count keeps each
/// digit's width while it changes, and numbers in a column align.
///
/// Set once on each window's root element and inherited from there; a text
/// run built by hand takes it through [`Theme::ui_font`].
pub fn text_features() -> gpui::FontFeatures {
    static FEATURES: std::sync::LazyLock<gpui::FontFeatures> = std::sync::LazyLock::new(|| {
        gpui::FontFeatures(std::sync::Arc::new(vec![("tnum".to_owned(), 1)]))
    });
    FEATURES.clone()
}

impl Theme {
    /// The font of a text run built by hand: the UI family at `weight`, with
    /// [`text_features`].
    pub fn ui_font(&self, weight: gpui::FontWeight) -> gpui::Font {
        gpui::Font {
            family: self.font_sans.clone(),
            features: text_features(),
            fallbacks: None,
            weight,
            style: gpui::FontStyle::Normal,
        }
    }
}
