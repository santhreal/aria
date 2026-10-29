//! The label palette: what a swatch is, and what pressing one writes.
//!
//! A swatch is not a text row, so the panel owns the drawing. What it must not
//! own is the action: a colour click and a clear click are the only way an
//! operator sets a label, and while both lived inside a gpui closure nothing
//! could check that either still wrote anything.

use aria_engine::EngineError;
use aria_models::LabelColor;

use crate::app::AppState;

/// What a swatch click labels: the timeline selection, or one media-pool row.
///
/// One row of swatches serves both surfaces because a label means the same thing
/// on either: the write differs, the palette does not.
#[derive(Debug, Clone, Copy)]
pub enum LabelTarget {
    Selection,
    Media(u64),
}

impl LabelTarget {
    /// Both targets, so a census presses the palette on each of them.
    pub const ALL: [Self; 2] = [Self::Selection, Self::Media(0)];

    /// Write `label` to whatever this target names.
    pub fn write(self, state: &mut AppState, label: Option<LabelColor>) -> Result<(), EngineError> {
        match self {
            Self::Selection => state.set_selection_label(label),
            Self::Media(media) => state.set_media_label(media, label),
        }
    }

    /// The element id prefix the swatches take, so the two palettes cannot share
    /// hover state.
    pub fn id_prefix(self) -> &'static str {
        match self {
            Self::Selection => "ctx-label-clip",
            Self::Media(_) => "ctx-label-media",
        }
    }

    /// The same target aimed at `media`, for a palette drawn on a pool row.
    pub fn on_media(self, media: u64) -> Self {
        match self {
            Self::Selection => Self::Selection,
            Self::Media(_) => Self::Media(media),
        }
    }
}

/// Every swatch the palette draws, in order: one per colour, then the clear.
///
/// The clear is `None` rather than a seventeenth colour: clearing returns the
/// clip to the label its kind is drawn in.
pub fn swatches() -> impl Iterator<Item = Option<LabelColor>> {
    LabelColor::ALL
        .iter()
        .copied()
        .map(Some)
        .chain(std::iter::once(None))
}

/// Select every clip drawn in `color`, which is what a group chip asks for.
pub fn select_group(state: &mut AppState, color: LabelColor) {
    state.select_by_label(color);
}
