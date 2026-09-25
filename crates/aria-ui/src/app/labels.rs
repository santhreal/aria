//! Label colours: assigning one, clearing it, and selecting by it.
//!
//! A label is organizational only — nothing renders it into a frame — so this is
//! the whole feature: one command per clip, and a way to get back every clip
//! that carries a colour. The palette is [`aria_models::MarkerColor`], shared
//! with ruler markers, so "the teal group" is one hue everywhere.

use aria_commands::{Command, EditCommand, ProjectCommand};
use aria_engine::EngineError;
use aria_models::{ClipId, MarkerColor, MediaId};

use super::AppState;

impl AppState {
    /// Set (`Some`) or clear (`None`) the label on every selected clip.
    ///
    /// The engine has no batch command, so a multi-clip selection is one undo
    /// entry per clip; the first failure stops the walk and is reported rather
    /// than leaving the rest silently unlabelled.
    pub fn set_selection_label(&mut self, label: Option<MarkerColor>) -> Result<(), EngineError> {
        let clips: Vec<ClipId> = self.selection.clone();
        for clip in clips {
            self.apply_command(Command::Edit(EditCommand::SetClipLabel { clip, label }))?;
        }
        Ok(())
    }

    /// The label on the selected clip, if it has one.
    pub fn selected_label(&self) -> Option<MarkerColor> {
        self.selected_ui_clip()?.label
    }

    /// Replace the selection with every clip carrying `label`, in timeline
    /// order. This is what makes a label useful rather than decorative: colour a
    /// group once, then reach all of it later.
    pub fn select_by_label(&mut self, label: MarkerColor) {
        let mut hits: Vec<(i64, u64)> = self
            .store
            .sequence
            .tracks
            .iter()
            .flat_map(|track| track.clips.iter())
            .filter(|clip| clip.label == Some(label))
            .map(|clip| (clip.timeline_start_ticks, clip.id))
            .collect();
        hits.sort_unstable();
        if hits.is_empty() {
            self.status_message = Some("No clips with that label".into());
            return;
        }
        let count = hits.len();
        self.selection = hits
            .into_iter()
            .map(|(_, id)| ClipId::from_raw(id))
            .collect();
        self.status_message = Some(format!("Selected {count}"));
        self.rebuild_store();
    }

    /// How many clips carry each label, for a surface that offers only the
    /// colours actually in use.
    pub fn label_counts(&self) -> Vec<(MarkerColor, usize)> {
        MarkerColor::ALL
            .iter()
            .filter_map(|color| {
                let count = self
                    .store
                    .sequence
                    .tracks
                    .iter()
                    .flat_map(|track| track.clips.iter())
                    .filter(|clip| clip.label == Some(*color))
                    .count();
                (count > 0).then_some((*color, count))
            })
            .collect()
    }

    /// Set (`Some`) or clear (`None`) the label on one media-pool entry.
    ///
    /// `media` is the projected row id, which is the raw [`aria_models::MediaId`]
    /// the bin already draws with.
    pub fn set_media_label(
        &mut self,
        media: u64,
        label: Option<MarkerColor>,
    ) -> Result<(), EngineError> {
        self.apply_command(Command::Project(ProjectCommand::SetMediaLabel {
            media: MediaId::from_raw(media),
            label,
        }))?;
        Ok(())
    }

    /// The label on one pool row, if it has one.
    pub fn media_label(&self, media: u64) -> Option<MarkerColor> {
        self.store
            .media
            .iter()
            .find(|item| item.id == media)
            .and_then(|item| item.label)
    }
}
