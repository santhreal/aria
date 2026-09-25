// --- Text effect presets --------------------------------------------------------------

use std::sync::LazyLock;

use crate::clip::{TextBackground, TextShadow, TextStroke};

/// A text effect preset (CapCut text effects): a named combination of the
/// stroke / shadow / background treatments [`crate::TextStyle`] already
/// persists. Applying a preset bakes these fields onto the style (see
/// [`crate::Generator::resolve_presets`]), so the file stays self-describing
/// and renderers never need the catalog.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct TextEffectSpec {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub stroke: Option<TextStroke>,
    #[serde(default)]
    pub shadow: Option<TextShadow>,
    #[serde(default)]
    pub background: Option<TextBackground>,
}

/// Every text effect, read once. Shipped set at `data/text_effects.toml`.
static TEXT_EFFECTS: LazyLock<Vec<TextEffectSpec>> = LazyLock::new(|| {
    crate::catalog::load(
        "text_effects",
        include_str!("../../data/text_effects.toml"),
        |entries| crate::catalog::unique_ids(entries, |s| &s.id, |s| &s.label),
    )
});

/// Every text effect preset (UI browsing order).
pub fn text_effect_catalog() -> &'static [TextEffectSpec] {
    &TEXT_EFFECTS
}

/// The catalog entry for `id`, or `None`.
pub fn text_effect_spec(id: &str) -> Option<&'static TextEffectSpec> {
    TEXT_EFFECTS.iter().find(|s| s.id == id)
}
