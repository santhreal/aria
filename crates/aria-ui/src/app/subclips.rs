//! Subclip actions in the media pool.
//!
//! A subclip is created from an in/out range of parent media, appears in the pool
//! as its own bounded item, and edits like any clip.

use std::collections::HashSet;

use aria_commands::{Command, EditCommand};
use aria_engine::EngineError;
use aria_models::{MediaId, MediaSource, Project, TimeRange};

use super::AppState;

impl AppState {
    /// Create a subclip in the pool from the current source monitor in/out
    /// marks, named after its parent with the first number no pool entry is
    /// drawn as. Returns the new pool entry, or `None` with no source open or
    /// no marked range.
    pub fn create_subclip_from_source(&mut self) -> Result<Option<MediaId>, EngineError> {
        let Some(view) = self.sources.focused() else {
            return Ok(None);
        };
        let media_id = MediaId::from_raw(view.media);
        let Some((start, end)) = self.source_range() else {
            return Ok(None);
        };
        let (fps, bin, name) = {
            let engine = self.engine.lock();
            let project = engine.project();
            let parent = project
                .media(media_id)
                .ok_or(aria_models::ModelError::UnknownMedia(media_id))?;
            let stem = format!("{} Subclip", parent.display_name());
            (
                parent.frame_rate,
                parent.bin,
                free_pool_name(project, &stem),
            )
        };
        let source_range = TimeRange::at_rate(start, end - start, fps);
        self.apply_command(Command::Edit(EditCommand::CreateSubclip {
            parent: media_id,
            name: name.clone(),
            source: source_range,
            bin,
        }))?;
        let created = self
            .engine
            .lock()
            .project()
            .media_iter()
            .find(|media| {
                media.parent == Some(media_id) && media.name.as_deref() == Some(name.as_str())
            })
            .map(|media| media.id);
        self.status_message = Some(format!("Created {name}"));
        Ok(created)
    }
}

/// `stem`, or `stem 2`, `stem 3` and so on: the first name no pool entry is
/// drawn as. Ends within one more try than the pool has entries.
fn free_pool_name(project: &Project, stem: &str) -> String {
    let taken: HashSet<&str> = project
        .media_iter()
        .map(MediaSource::display_name)
        .collect();
    let mut name = stem.to_owned();
    let mut n = 1;
    while taken.contains(name.as_str()) {
        n += 1;
        name = format!("{stem} {n}");
    }
    name
}

#[cfg(test)]
#[path = "subclips_tests.rs"]
mod tests;
