//! Marker, link, and clip-derivation edits.
//!
//! Every edit here goes through [`AppState::apply_command`], so each one is a
//! single undoable engine command and no surface mutates the store directly.
//!
//! Moving, renaming, recolouring, and annotating a marker all ride the one
//! `SetMarker` command, which carries the whole row. Each of those four edits
//! therefore has to resend the three fields it is not changing, or it would
//! erase them; [`AppState::set_marker`] is the single place that does so.
//!
//! An edit naming a marker that no longer exists is inert and `Ok`, not an
//! error: a menu built one frame ago may reference a marker deleted since, and
//! that is a stale surface rather than a failure the operator should see.

use aria_commands::{Command, EditCommand};
use aria_engine::EngineError;
use aria_models::{MarkerColor, MarkerId, RationalTime};

use super::AppState;
use crate::types::UiMarker;

impl AppState {
    /// The name for the next marker: one past the highest number used.
    ///
    /// Derived from the highest existing number rather than the count, so
    /// deleting a marker never hands its name to a later one.
    pub(super) fn next_marker_name(&self) -> String {
        let highest = self
            .store
            .sequence
            .markers
            .iter()
            .filter_map(|marker| {
                marker
                    .name
                    .strip_prefix("Marker ")
                    .and_then(|rest| rest.parse::<u32>().ok())
            })
            .max()
            .unwrap_or(0);
        format!("Marker {}", highest.saturating_add(1))
    }

    /// The colour for the next marker, stepped so neighbours differ.
    pub(super) fn next_marker_color(&self) -> MarkerColor {
        MarkerColor::cycle(self.store.sequence.markers.len())
    }

    /// The number of ticks in one second at the sequence rate.
    ///
    /// The window every "near the playhead" marker action uses, so zoom never
    /// changes which marker a keypress reaches.
    fn marker_window(&self) -> i64 {
        i64::from(self.store.sequence.frame_rate.num.max(1))
    }

    /// The marker row for `marker`, or `None` when it is gone.
    fn marker_row(&self, marker: u64) -> Option<&UiMarker> {
        self.store
            .sequence
            .markers
            .iter()
            .find(|row| row.id == marker)
    }

    /// The marker nearest `at`, within `within` ticks on either side.
    pub fn marker_near(&self, at: i64, within: i64) -> Option<&UiMarker> {
        self.store
            .sequence
            .markers
            .iter()
            .filter(|marker| (marker.tick - at).abs() <= within)
            .min_by_key(|marker| (marker.tick - at).abs())
    }

    /// The marker under the playhead, within the standard one-second window.
    pub fn marker_at_playhead(&self) -> Option<&UiMarker> {
        self.marker_near(self.playhead.value, self.marker_window())
    }

    /// Rewrite one marker in full.
    ///
    /// `SetMarker` replaces the whole row, so every caller passes all four
    /// fields and changes only the one it owns. Inert when the marker is gone.
    fn set_marker(
        &mut self,
        marker: u64,
        at: i64,
        name: String,
        color: MarkerColor,
        comment: String,
    ) -> Result<(), EngineError> {
        let rate = self.playhead.rate;
        self.apply_command(Command::Edit(EditCommand::SetMarker {
            marker: MarkerId::from_raw(marker),
            at: RationalTime::new(at.max(0), rate),
            name,
            color,
            comment,
        }))?;
        Ok(())
    }

    /// Rename a marker, keeping its position, colour, and note.
    pub fn rename_marker(&mut self, marker: u64, name: String) -> Result<(), EngineError> {
        let Some(row) = self.marker_row(marker) else {
            return Ok(());
        };
        let (at, color, comment) = (row.tick, row.color, row.comment.to_string());
        self.set_marker(marker, at, name, color, comment)
    }

    /// Write or clear a marker's note, keeping its position, name, and colour.
    ///
    /// An empty `comment` is a real edit that clears the note, not a no-op.
    pub fn set_marker_comment(&mut self, marker: u64, comment: String) -> Result<(), EngineError> {
        let Some(row) = self.marker_row(marker) else {
            return Ok(());
        };
        let (at, name, color) = (row.tick, row.name.to_string(), row.color);
        self.set_marker(marker, at, name, color, comment)
    }

    /// Move a marker to `to`, keeping its name, colour, and note.
    pub fn move_marker(&mut self, marker: u64, to: i64) -> Result<(), EngineError> {
        let Some(row) = self.marker_row(marker) else {
            return Ok(());
        };
        let (name, color, comment) = (row.name.to_string(), row.color, row.comment.to_string());
        self.set_marker(marker, to, name, color, comment)
    }

    /// Step a marker to the next colour, keeping everything else.
    pub fn cycle_marker_color(&mut self, marker: u64) -> Result<(), EngineError> {
        let Some(row) = self.marker_row(marker) else {
            return Ok(());
        };
        let next = MarkerColor::ALL
            .iter()
            .position(|candidate| *candidate == row.color)
            .map_or(0, |index| index.saturating_add(1));
        let (at, name, comment) = (row.tick, row.name.to_string(), row.comment.to_string());
        self.set_marker(marker, at, name, MarkerColor::cycle(next), comment)
    }

    /// Delete one marker by id. Inert when it is already gone.
    pub fn remove_marker(&mut self, marker: u64) -> Result<(), EngineError> {
        if self.marker_row(marker).is_none() {
            return Ok(());
        }
        self.apply_command(Command::Edit(EditCommand::RemoveMarker {
            marker: MarkerId::from_raw(marker),
        }))?;
        Ok(())
    }

    /// Delete the marker under the playhead. Inert when none is near.
    pub fn remove_marker_at_playhead(&mut self) -> Result<(), EngineError> {
        let Some(marker) = self.marker_at_playhead().map(|row| row.id) else {
            self.status_message = Some("No marker at the playhead".into());
            return Ok(());
        };
        self.remove_marker(marker)
    }

    /// Move the playhead to the next marker after it. Stops at the last.
    pub fn go_to_next_marker(&mut self) {
        let at = self.playhead.value;
        let next = self
            .store
            .sequence
            .markers
            .iter()
            .map(|marker| marker.tick)
            .filter(|tick| *tick > at)
            .min();
        match next {
            Some(tick) => self.set_playhead(tick),
            None => self.status_message = Some("No later marker".into()),
        }
    }

    /// Move the playhead to the previous marker. Stops at the first.
    pub fn go_to_prev_marker(&mut self) {
        let at = self.playhead.value;
        let previous = self
            .store
            .sequence
            .markers
            .iter()
            .map(|marker| marker.tick)
            .filter(|tick| *tick < at)
            .max();
        match previous {
            Some(tick) => self.set_playhead(tick),
            None => self.status_message = Some("No earlier marker".into()),
        }
    }

    /// Whether the selection is one link group of two or more clips.
    ///
    /// `linked_to` carries a group id shared by every member, so a genuine
    /// group is a selection whose clips all report the same one.
    pub fn selection_is_linked(&self) -> bool {
        if self.selection.len() < 2 {
            return false;
        }
        let mut group = None;
        for id in &self.selection {
            let Some(link) = self.ui_clip(*id).and_then(|clip| clip.linked_to) else {
                return false;
            };
            match group {
                None => group = Some(link),
                Some(seen) if seen == link => {}
                Some(_) => return false,
            }
        }
        true
    }

    /// Link every selected clip, so selecting one gathers them all.
    ///
    /// A single clip is not a group: inert rather than an error, because the
    /// menu offering it cannot know the selection size at build time.
    pub fn link_selected(&mut self) -> Result<(), EngineError> {
        if self.selection.len() < 2 {
            self.status_message = Some("Select two or more clips to link".into());
            return Ok(());
        }
        let clips = self.selection.to_vec();
        self.apply_command(Command::Edit(EditCommand::LinkClips { clips }))?;
        self.status_message = Some("Linked".into());
        Ok(())
    }

    /// Dissolve the link group covering the selection.
    pub fn unlink_selected(&mut self) -> Result<(), EngineError> {
        if self.selection.is_empty() {
            self.status_message = Some("Select a clip to unlink".into());
            return Ok(());
        }
        let clips = self.selection.to_vec();
        self.apply_command(Command::Edit(EditCommand::UnlinkClips { clips }))?;
        self.status_message = Some("Unlinked".into());
        Ok(())
    }

    /// Whether the selected clip carries audio that can be split off.
    ///
    /// Read from the projected clip, which is the engine's own answer, so this
    /// gate and the command it guards can never disagree.
    pub fn selected_can_extract_audio(&self) -> bool {
        self.selected_ui_clip()
            .is_some_and(|clip| clip.can_extract_audio)
    }

    /// Whether the selected clip can yield a freeze frame.
    pub fn selected_can_freeze_frame(&self) -> bool {
        self.selected_ui_clip()
            .is_some_and(|clip| clip.can_freeze_frame)
    }

    /// Split the selected clip's audio onto its own track.
    ///
    /// Refusal is a status message, not an error: the capability is the
    /// engine's to decide and the operator only needs to be told.
    pub fn extract_selected_audio(&mut self) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip() else {
            self.status_message = Some("Select a clip first".into());
            return Ok(());
        };
        if !self.selected_can_extract_audio() {
            self.status_message = Some("This clip has no audio to extract".into());
            return Ok(());
        }
        self.apply_command(Command::Edit(EditCommand::ExtractAudio {
            clip,
            to_track: None,
        }))?;
        self.status_message = Some("Extracted audio".into());
        Ok(())
    }

    /// Hold the frame under the playhead for one second.
    pub fn freeze_selected_frame(&mut self) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip() else {
            self.status_message = Some("Select a clip first".into());
            return Ok(());
        };
        if !self.selected_can_freeze_frame() {
            self.status_message = Some("This clip cannot be frozen".into());
            return Ok(());
        }
        let rate = self.playhead.rate;
        let at = self.playhead;
        let duration = RationalTime::new(self.marker_window(), rate);
        self.apply_command(Command::Edit(EditCommand::FreezeFrame {
            clip,
            at,
            duration,
        }))?;
        self.status_message = Some("Froze frame".into());
        Ok(())
    }
}
