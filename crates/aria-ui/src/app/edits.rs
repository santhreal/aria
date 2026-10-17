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
use aria_models::{MarkerColor, MarkerId, MarkerKind, RationalTime};
use gpui::SharedString;

use super::{AppState, DialogKind};
use crate::types::UiMarker;

/// Which text field inside the Marker Edit dialog holds keyboard focus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkerEditField {
    Name,
    Comment,
    Duration,
}

/// Draft state for [`DialogKind::MarkerEdit`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarkerEditForm {
    pub marker: u64,
    pub name: String,
    pub comment: String,
    pub duration: String,
    pub color: MarkerColor,
    pub kind: MarkerKind,
    pub focused: Option<MarkerEditField>,
    pub error: Option<SharedString>,
}

impl Default for MarkerEditForm {
    fn default() -> Self {
        Self {
            marker: 0,
            name: String::new(),
            comment: String::new(),
            duration: String::new(),
            color: MarkerColor::Teal,
            kind: MarkerKind::Comment,
            focused: None,
            error: None,
        }
    }
}

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
    /// Resolve the model-space tick (`RationalTime`) for `marker` when moving to
    /// `timeline_tick` (`Some`) or keeping its current position (`None`).
    fn marker_model_tick(&self, marker: u64, timeline_tick: Option<i64>) -> Option<RationalTime> {
        let row = self.marker_row(marker)?;
        let rate = self.playhead.rate;
        if let Some(clip_raw) = row.clip_id {
            let clip_rate = row.clip_rate.unwrap_or(rate);
            let tick = match timeline_tick {
                Some(target) => {
                    let clip = self.ui_clip(aria_models::ClipId::from_raw(clip_raw))?;
                    if target < clip.timeline_start_ticks || target >= clip.timeline_end_ticks {
                        return None;
                    }
                    let tl_dur = clip.timeline_end_ticks - clip.timeline_start_ticks;
                    if tl_dur <= 0 {
                        return None;
                    }
                    let offset = target - clip.timeline_start_ticks;
                    let src_dur = clip.source_end_ticks - clip.source_start_ticks;
                    if src_dur > 0 {
                        if tl_dur == src_dur {
                            if clip.reversed {
                                clip.source_start_ticks + src_dur - offset
                            } else {
                                clip.source_start_ticks + offset
                            }
                        } else {
                            let mut frac = offset as f64 / tl_dur as f64;
                            if clip.reversed {
                                frac = 1.0 - frac;
                            }
                            clip.source_start_ticks + (frac * src_dur as f64).round() as i64
                        }
                    } else {
                        offset
                    }
                }
                None => row.clip_tick.unwrap_or(row.tick),
            };
            Some(RationalTime::new(tick.max(0), clip_rate))
        } else {
            let tick = timeline_tick.unwrap_or(row.tick);
            Some(RationalTime::new(tick.max(0), rate))
        }
    }

    /// Rewrite one marker's basic fields (`at`, `name`, `color`, `comment`).
    ///
    /// `SetMarker` replaces the row while preserving `duration` and `kind` in
    /// the model. Inert when the marker is gone.
    fn set_marker(
        &mut self,
        marker: u64,
        timeline_tick: Option<i64>,
        name: String,
        color: MarkerColor,
        comment: String,
    ) -> Result<(), EngineError> {
        let Some(at) = self.marker_model_tick(marker, timeline_tick) else {
            return Ok(());
        };
        self.apply_command(Command::Edit(EditCommand::SetMarker {
            marker: MarkerId::from_raw(marker),
            at,
            name,
            color,
            comment,
        }))?;
        Ok(())
    }

    /// Edit a marker's `name`, `comment`, `color`, `duration`, and `kind` as one
    /// undo step.
    pub fn edit_marker_full(
        &mut self,
        marker: u64,
        name: String,
        comment: String,
        color: MarkerColor,
        duration_ticks: i64,
        kind: MarkerKind,
    ) -> Result<(), EngineError> {
        let Some(at) = self.marker_model_tick(marker, None) else {
            return Ok(());
        };
        let rate = self.playhead.rate;
        let duration = RationalTime::new(duration_ticks, rate);
        let id = MarkerId::from_raw(marker);
        // Validate via `Engine::edit_marker` inside `one_step` alongside
        // `EditCommand::SetMarker` so `CommandProposed` is emitted for the
        // command census and the whole edit is a single undo step. If validation
        // fails, run `Engine::edit_marker` first so nothing is committed on error.
        self.one_step(|this| {
            {
                let mut engine = this.engine.lock();
                engine.edit_marker(id, at, duration, name.clone(), color, comment.clone(), kind)?;
            }
            this.apply_command(Command::Edit(EditCommand::SetMarker {
                marker: id,
                at,
                name,
                color,
                comment,
            }))?;
            Ok(())
        })
    }

    /// Open [`DialogKind::MarkerEdit`] populated from `marker`.
    pub fn open_marker_edit(&mut self, marker: u64) -> bool {
        let Some(row) = self.marker_row(marker).cloned() else {
            self.status_message = Some("No marker at the playhead".into());
            return false;
        };
        let rate = self.store.sequence.frame_rate;
        let duration = crate::timecode::format_ticks(row.duration_ticks.max(0), rate.num, rate.den);
        self.marker_edit = MarkerEditForm {
            marker: row.id,
            name: row.name.to_string(),
            comment: row.comment.to_string(),
            duration,
            color: row.color,
            kind: row.kind,
            focused: Some(MarkerEditField::Name),
            error: None,
        };
        self.open_dialog(DialogKind::MarkerEdit);
        true
    }

    /// Open [`DialogKind::MarkerEdit`] for the marker at the playhead (or the
    /// nearest marker when the menu is invoked with markers on the sequence).
    pub fn edit_marker_at_playhead(&mut self) {
        let at = self.playhead.value;
        let target = self
            .marker_at_playhead()
            .or_else(|| self.marker_near(at, i64::MAX / 4))
            .map(|row| row.id);
        let Some(id) = target else {
            self.status_message = Some("No marker at the playhead".into());
            return;
        };
        self.open_marker_edit(id);
    }

    /// Commit [`DialogKind::MarkerEdit`] as a single undo step.
    pub fn apply_marker_edit(&mut self) {
        let form = self.marker_edit.clone();
        let rate = self.store.sequence.frame_rate;
        let duration_ticks = match parse_marker_duration_ticks(&form.duration, rate.num, rate.den) {
            Ok(ticks) => ticks,
            Err(msg) => {
                self.marker_edit.error = Some(SharedString::from(msg.clone()));
                self.status_message = Some(msg.clone());
                self.report_error(msg);
                return;
            }
        };
        match self.edit_marker_full(
            form.marker,
            form.name.trim().to_string(),
            form.comment.trim().to_string(),
            form.color,
            duration_ticks,
            form.kind,
        ) {
            Ok(()) => {
                self.marker_edit.error = None;
                self.open_dialog(DialogKind::None);
                self.status_message = Some("Updated marker".into());
            }
            Err(err) => {
                let msg = match &err {
                    EngineError::Model(aria_models::ModelError::InvalidParam(reason)) => {
                        reason.clone()
                    }
                    other => super::describe_engine_error(other),
                };
                self.marker_edit.error = Some(SharedString::from(msg.clone()));
                self.status_message = Some(msg.clone());
                self.report_error(msg);
            }
        }
    }

    pub fn marker_edit_focused(&self) -> bool {
        self.dialog == DialogKind::MarkerEdit && self.marker_edit.focused.is_some()
    }

    pub fn marker_edit_text(&self) -> String {
        match self.marker_edit.focused {
            Some(MarkerEditField::Name) => self.marker_edit.name.clone(),
            Some(MarkerEditField::Comment) => self.marker_edit.comment.clone(),
            Some(MarkerEditField::Duration) => self.marker_edit.duration.clone(),
            None => String::new(),
        }
    }

    pub fn escape_marker_edit(&mut self) {
        self.open_dialog(DialogKind::None);
    }

    pub fn marker_edit_pop(&mut self) {
        match self.marker_edit.focused {
            Some(MarkerEditField::Name) => {
                self.marker_edit.name.pop();
            }
            Some(MarkerEditField::Comment) => {
                self.marker_edit.comment.pop();
            }
            Some(MarkerEditField::Duration) => {
                self.marker_edit.duration.pop();
            }
            None => {}
        }
    }

    pub fn marker_edit_push(&mut self, ch: char) {
        match self.marker_edit.focused {
            Some(MarkerEditField::Name) => {
                if self.marker_edit.name.chars().count() < 120 {
                    self.marker_edit.name.push(ch);
                }
            }
            Some(MarkerEditField::Comment) => {
                if self.marker_edit.comment.chars().count() < 500 {
                    self.marker_edit.comment.push(ch);
                }
            }
            Some(MarkerEditField::Duration) => {
                if (ch.is_ascii_digit() || matches!(ch, ':' | ';' | '.' | '-'))
                    && self.marker_edit.duration.len() < 24
                {
                    self.marker_edit.duration.push(ch);
                }
            }
            None => {}
        }
    }

    pub fn focus_marker_edit_field(&mut self, field: Option<MarkerEditField>) {
        self.marker_edit.focused = field;
    }

    pub fn set_marker_edit_color(&mut self, color: MarkerColor) {
        self.marker_edit.color = color;
    }

    pub fn set_marker_edit_kind(&mut self, kind: MarkerKind) {
        self.marker_edit.kind = kind;
    }

    /// Active colour filter in the Markers panel (`None` shows all markers).
    pub fn marker_color_filter(&self) -> Option<MarkerColor> {
        self.marker_color_filter
    }

    /// Set or clear the Markers panel colour filter.
    pub fn set_marker_color_filter(&mut self, color: Option<MarkerColor>) {
        self.marker_color_filter = color;
    }

    /// Markers visible in the Markers panel after applying `marker_color_filter`.
    pub fn visible_markers(&self) -> Vec<UiMarker> {
        self.store
            .sequence
            .markers
            .iter()
            .filter(|m| self.marker_color_filter.is_none_or(|c| m.color == c))
            .cloned()
            .collect()
    }

    /// Rename a marker, keeping its position, colour, and note.
    pub fn rename_marker(&mut self, marker: u64, name: String) -> Result<(), EngineError> {
        let Some(row) = self.marker_row(marker) else {
            return Ok(());
        };
        let (color, comment) = (row.color, row.comment.to_string());
        self.set_marker(marker, None, name, color, comment)
    }

    /// Write or clear a marker's note, keeping its position, name, and colour.
    ///
    /// An empty `comment` is a real edit that clears the note, not a no-op.
    pub fn set_marker_comment(&mut self, marker: u64, comment: String) -> Result<(), EngineError> {
        let Some(row) = self.marker_row(marker) else {
            return Ok(());
        };
        let (name, color) = (row.name.to_string(), row.color);
        self.set_marker(marker, None, name, color, comment)
    }

    /// Move a marker to `to`, keeping its name, colour, and note.
    pub fn move_marker(&mut self, marker: u64, to: i64) -> Result<(), EngineError> {
        let Some(row) = self.marker_row(marker) else {
            return Ok(());
        };
        let (name, color, comment) = (row.name.to_string(), row.color, row.comment.to_string());
        self.set_marker(marker, Some(to), name, color, comment)
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
        let (name, comment) = (row.name.to_string(), row.comment.to_string());
        self.set_marker(marker, None, name, MarkerColor::cycle(next), comment)
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

fn parse_marker_duration_ticks(text: &str, rate_num: i32, rate_den: i32) -> Result<i64, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(0);
    }
    if let Some(rest) = trimmed.strip_prefix('-') {
        let mag = crate::timecode::parse_ticks(rest, rate_num, rate_den)
            .or_else(|| rest.trim().parse::<i64>().ok())
            .unwrap_or(1)
            .max(1);
        return Ok(-mag);
    }
    crate::timecode::parse_ticks(trimmed, rate_num, rate_den)
        .ok_or_else(|| format!("Invalid duration timecode '{trimmed}'"))
}
