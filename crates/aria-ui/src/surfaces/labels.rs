//! The label palette: what a swatch is, and what pressing one writes.
//!
//! A swatch is not a text row, so the panel owns the drawing. What it must not
//! own is the action: a color click and a clear click are the only controls
//! that set a label, and while both lived inside a gpui closure nothing
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

/// Every swatch the palette draws, in order: one per color, then the clear.
///
/// The clear is `None` rather than a seventeenth color: clearing returns the
/// clip to the label its kind is drawn in.
pub fn swatches() -> impl Iterator<Item = Option<LabelColor>> {
    LabelColor::ALL
        .iter()
        .copied()
        .map(Some)
        .chain(std::iter::once(None))
}

/// The tip a group chip shows: the selection its click makes.
pub fn group_tip(color: LabelColor, count: usize) -> String {
    match count {
        1 => format!("Select the {} clip", color.name()),
        n => format!("Select the {n} {} clips", color.name()),
    }
}

/// Select every clip drawn in `color`, which is what a group chip asks for.
pub fn select_group(state: &mut AppState, color: LabelColor) {
    state.select_by_label(color);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WHY: a group chip is a bare count, so its tip is the only place it
    /// reads as words. Closes "a chip that counts one clip in the plural" and
    /// "a chip tip naming another color". Does not check the hover delay or
    /// that the panel attaches the tip.
    #[test]
    fn a_chip_tip_states_the_selection_its_click_makes() {
        assert_eq!(group_tip(LabelColor::Teal, 1), "Select the Teal clip");
        assert_eq!(group_tip(LabelColor::Tan, 11), "Select the 11 Tan clips");
    }
}
