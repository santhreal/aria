//! Subclip actions in the media pool.
//!
//! A subclip is created from an in/out range of parent media, appears in the pool
//! as its own bounded item, and edits like any clip.

use aria_commands::{Command, EditCommand};
use aria_engine::EngineError;
use aria_models::{BinId, MediaId, TimeRange};

use super::AppState;

impl AppState {
    /// Create a subclip in the pool from the current source monitor in/out marks.
    pub fn create_subclip_from_source(
        &mut self,
        name: Option<String>,
    ) -> Result<Option<MediaId>, EngineError> {
        let Some(view) = self.sources.focused() else {
            return Ok(None);
        };
        let media_id = MediaId::from_raw(view.media);
        let Some((start, end)) = self.source_range() else {
            return Ok(None);
        };
        let (fps, bin, parent_name) = {
            let engine = self.engine.lock();
            let parent = engine
                .project()
                .media(media_id)
                .ok_or(aria_models::ModelError::UnknownMedia(media_id))?;
            (
                parent.frame_rate,
                parent.bin,
                parent.display_name().to_string(),
            )
        };
        let source_range = TimeRange::at_rate(start, end - start, fps);
        let subclip_name = name.unwrap_or_else(|| format!("{parent_name} Subclip"));
        self.apply_command(Command::Edit(EditCommand::CreateSubclip {
            parent: media_id,
            name: subclip_name,
            source: source_range,
            bin,
        }))?;
        self.status_message = Some("Created Subclip".to_string());
        Ok(Some(media_id))
    }

    /// Create a subclip explicitly from parent media, name, and in/out source range.
    pub fn create_subclip(
        &mut self,
        parent: MediaId,
        name: String,
        source: TimeRange,
        bin: Option<BinId>,
    ) -> Result<(), EngineError> {
        self.apply_command(Command::Edit(EditCommand::CreateSubclip {
            parent,
            name,
            source,
            bin,
        }))?;
        self.status_message = Some("Created Subclip".to_string());
        Ok(())
    }
}
