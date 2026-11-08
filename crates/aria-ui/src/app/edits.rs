//! Marker, link, and clip-derivation edits.
//!
//! Every edit here goes through [`AppState::apply_command`], so each one is a
//! single undoable engine command and no surface mutates the store directly.
//!
//! Moving, renaming, recoloring, and annotating a marker all ride the one
//! `SetMarker` command, which carries the whole row. Each of those four edits
//! therefore has to resend the three fields it is not changing, or it would
//! erase them; [`AppState::set_marker`] is the single place that does so.
//!
//! An edit naming a marker that no longer exists is inert and `Ok`, not an
//! error: a menu built one frame ago may reference a marker deleted since, and
//! that is a stale surface rather than a failure to report.

use aria_commands::{Command, EditCommand};
use aria_engine::EngineError;
use aria_models::{MarkerColor, MarkerId, MarkerKind, RationalTime};
use gpui::SharedString;

use super::decline::Decline;
use super::line_text::{LineRule, LineText};
use super::menu_keys::step;
use super::{AppState, DialogKind};
use crate::keys::fields::CaretKey;
use crate::types::UiMarker;

/// What the Marker Edit dialog's Name field takes.
const MARKER_NAME_RULE: LineRule = LineRule::chars(120);

/// What its Comment field takes.
const MARKER_COMMENT_RULE: LineRule = LineRule::chars(500);

/// What its Duration field takes: frames or a timecode.
const MARKER_DURATION_RULE: LineRule = LineRule::chars(24)
    .admitting(|_, ch| ch.is_ascii_digit() || matches!(ch, ':' | ';' | '.' | '-'));

/// Which text field inside the Marker Edit dialog holds keyboard focus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkerEditField {
    Name,
    Comment,
    Duration,
}

impl MarkerEditField {
    /// The dialog's boxes in the order it draws them, top to bottom, which is
    /// the order Tab walks them.
    pub const ORDER: [Self; 3] = [Self::Name, Self::Comment, Self::Duration];

    /// The box's label and what its empty value reads as.
    pub fn caption(self) -> (&'static str, &'static str) {
        match self {
            Self::Name => ("Name", "Marker name"),
            Self::Comment => ("Comment", "Note or review comment"),
            Self::Duration => ("Duration (timecode)", "00:00:00:00"),
        }
    }
}

/// Draft state for [`DialogKind::MarkerEdit`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarkerEditForm {
    pub marker: u64,
    pub name: LineText,
    pub comment: LineText,
    pub duration: LineText,
    pub color: MarkerColor,
    pub kind: MarkerKind,
    pub focused: Option<MarkerEditField>,
    pub error: Option<SharedString>,
}

impl Default for MarkerEditForm {
    fn default() -> Self {
        Self {
            marker: 0,
            name: LineText::default(),
            comment: LineText::default(),
            duration: LineText::default(),
            color: MarkerColor::Teal,
            kind: MarkerKind::Comment,
            focused: None,
            error: None,
        }
    }
}

impl MarkerEditForm {
    /// The text of the box `field`.
    pub fn line(&self, field: MarkerEditField) -> &LineText {
        match field {
            MarkerEditField::Name => &self.name,
            MarkerEditField::Comment => &self.comment,
            MarkerEditField::Duration => &self.duration,
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

    /// The color for the next marker, stepped so neighbors differ.
    pub(super) fn next_marker_color(&self) -> MarkerColor {
        MarkerColor::cycle(self.store.sequence.markers.len())
    }

    /// The number of ticks in one timecode second of the sequence: 24 at
    /// 23.976 fps, 30 at 29.97 fps.
    ///
    /// The window every "near the playhead" marker action uses, so zoom never
    /// changes which marker a keypress reaches, and the step of every "by a
    /// second" action, so the gesture is the same size at 24 and 60 fps.
    pub(crate) fn one_second(&self) -> i64 {
        self.store.sequence.frame_rate.nominal_fps()
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
        self.marker_near(self.playhead.value, self.one_second())
    }

    /// Resolve the model-space tick (`RationalTime`) for `marker` when moving to
    /// `timeline_tick` (`Some`) or keeping its current position (`None`).
    fn marker_model_tick(&self, marker: u64, timeline_tick: Option<i64>) -> Option<RationalTime> {
        let row = self.marker_row(marker)?;
        let rate = self.playhead.rate;
        if let Some(clip_raw) = row.clip_id {
            let clip_rate = row.clip_rate.unwrap_or(rate);
            let tick = match timeline_tick {
                Some(target) => self
                    .host
                    .snapshot()
                    .timeline()
                    .clip(aria_models::ClipId::from_raw(clip_raw))?
                    .clip_tick_for_timeline(target)?,
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

    /// Open [`DialogKind::MarkerEdit`] populated from `marker`, with the name
    /// focused and selected so a typed name replaces it.
    pub fn open_marker_edit(&mut self, marker: u64) -> bool {
        let Some(row) = self.marker_row(marker).cloned() else {
            self.status_message = Some("No marker at the playhead".into());
            return false;
        };
        let rate = self.store.sequence.frame_rate;
        let duration = crate::timecode::format_ticks(row.duration_ticks.max(0), rate.num, rate.den);
        self.marker_edit = MarkerEditForm {
            marker: row.id,
            name: LineText::selecting_all(row.name.to_string()),
            comment: LineText::new(row.comment.to_string()),
            duration: LineText::new(duration),
            color: row.color,
            kind: row.kind,
            focused: Some(MarkerEditField::Name),
            error: None,
        };
        self.open_dialog(DialogKind::MarkerEdit);
        true
    }

    /// Open [`DialogKind::MarkerEdit`] for the marker at the playhead.
    pub fn edit_marker_at_playhead(&mut self) {
        let Some(id) = self.marker_at_playhead().map(|row| row.id) else {
            self.decline(Decline::NoMarkerAtPlayhead);
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
        let name = form.name.trim();
        let comment = form.comment.trim();
        if self.marker_row(form.marker).is_some_and(|row| {
            row.name.as_ref() == name
                && row.comment.as_ref() == comment
                && row.color == form.color
                && row.kind == form.kind
                && row.duration_ticks == duration_ticks
        }) {
            self.marker_edit.error = None;
            self.open_dialog(DialogKind::None);
            return;
        }
        match self.edit_marker_full(
            form.marker,
            name.to_string(),
            comment.to_string(),
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

    /// The dialog field that holds the keys, `None` with none focused.
    pub fn marker_edit_line(&self) -> Option<&LineText> {
        let form = &self.marker_edit;
        Some(form.line(form.focused?))
    }

    pub fn marker_edit_line_mut(&mut self) -> Option<&mut LineText> {
        let form = &mut self.marker_edit;
        Some(match form.focused? {
            MarkerEditField::Name => &mut form.name,
            MarkerEditField::Comment => &mut form.comment,
            MarkerEditField::Duration => &mut form.duration,
        })
    }

    /// What the focused dialog field takes.
    pub fn marker_edit_rule(&self) -> LineRule {
        match self.marker_edit.focused {
            Some(MarkerEditField::Duration) => MARKER_DURATION_RULE,
            Some(MarkerEditField::Comment) => MARKER_COMMENT_RULE,
            Some(MarkerEditField::Name) | None => MARKER_NAME_RULE,
        }
    }

    pub fn escape_marker_edit(&mut self) {
        self.open_dialog(DialogKind::None);
    }

    pub fn focus_marker_edit_field(&mut self, field: Option<MarkerEditField>) {
        self.marker_edit.focused = field;
    }

    /// Tab: focus the dialog's next box, or the one before it with Shift,
    /// wrapping, with its text selected so typing replaces it. Nothing is
    /// applied before Save, so the picture is unchanged: returns `false`.
    pub fn tab_marker_edit(&mut self, back: bool) -> bool {
        let order = MarkerEditField::ORDER;
        let at = self
            .marker_edit
            .focused
            .and_then(|field| order.iter().position(|&each| each == field));
        if let Some(next) = step(at, order.len(), if back { -1 } else { 1 }) {
            self.focus_marker_edit_field(Some(order[next]));
            if let Some(line) = self.marker_edit_line_mut() {
                line.key(CaretKey::SelectAll, false);
            }
        }
        false
    }

    pub fn set_marker_edit_color(&mut self, color: MarkerColor) {
        self.marker_edit.color = color;
    }

    pub fn set_marker_edit_kind(&mut self, kind: MarkerKind) {
        self.marker_edit.kind = kind;
    }

    /// Active color filter in the Markers panel (`None` shows all markers).
    pub fn marker_color_filter(&self) -> Option<MarkerColor> {
        self.marker_color_filter
    }

    /// Set or clear the Markers panel color filter.
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

    /// Rename a marker, keeping its position, color, and note.
    pub fn rename_marker(&mut self, marker: u64, name: String) -> Result<(), EngineError> {
        let Some(row) = self.marker_row(marker) else {
            return Ok(());
        };
        let (color, comment) = (row.color, row.comment.to_string());
        self.set_marker(marker, None, name, color, comment)
    }

    /// Write or clear a marker's note, keeping its position, name, and color.
    ///
    /// An empty `comment` is a real edit that clears the note, not a no-op.
    pub fn set_marker_comment(&mut self, marker: u64, comment: String) -> Result<(), EngineError> {
        let Some(row) = self.marker_row(marker) else {
            return Ok(());
        };
        let (name, color) = (row.name.to_string(), row.color);
        self.set_marker(marker, None, name, color, comment)
    }

    /// Move a marker to `to`, keeping its name, color, and note.
    pub fn move_marker(&mut self, marker: u64, to: i64) -> Result<(), EngineError> {
        let Some(row) = self.marker_row(marker) else {
            return Ok(());
        };
        let (name, color, comment) = (row.name.to_string(), row.color, row.comment.to_string());
        self.set_marker(marker, Some(to), name, color, comment)
    }

    /// Step a marker to the next color, keeping everything else.
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
            self.decline(Decline::NoMarkerAtPlayhead);
            return Ok(());
        };
        self.remove_marker(marker)
    }

    /// Move the playhead to the next marker after it. Stops at the last.
    pub fn go_to_next_marker(&mut self) {
        let at = self.playhead.value;
        let ticks = self.store.sequence.markers.iter().map(|marker| marker.tick);
        match ticks.filter(|tick| *tick > at).min() {
            Some(tick) => self.set_playhead(tick),
            None if self.store.sequence.markers.is_empty() => self.decline(Decline::NoMarkers),
            None => self.status_message = Some("No later marker".into()),
        }
    }

    /// Move the playhead to the previous marker. Stops at the first.
    pub fn go_to_prev_marker(&mut self) {
        let at = self.playhead.value;
        let ticks = self.store.sequence.markers.iter().map(|marker| marker.tick);
        match ticks.filter(|tick| *tick < at).max() {
            Some(tick) => self.set_playhead(tick),
            None if self.store.sequence.markers.is_empty() => self.decline(Decline::NoMarkers),
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
    /// A single clip is not a group, and a selection that is already one link
    /// group has nothing to link: both are inert rather than an error.
    pub fn link_selected(&mut self) -> Result<(), EngineError> {
        if self.selection.len() < 2 {
            self.status_message = Some("Select two or more clips to link".into());
            return Ok(());
        }
        if self.selection_is_linked() {
            self.decline(Decline::AlreadyLinked);
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

    /// Whether the selected clip can yield a freeze frame.
    pub fn selected_can_freeze_frame(&self) -> bool {
        self.selected_ui_clip()
            .is_some_and(|clip| clip.can_freeze_frame)
    }

    /// Split the selected clip's audio onto its own track.
    ///
    /// Refusal is a status message, not an error: the capability is the
    /// engine's to decide and the status line only reports it.
    pub fn extract_selected_audio(&mut self) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip_or_decline() else {
            return Ok(());
        };
        if let Some(why) = crate::chrome::menu_guards::extractable("seq-extract-audio", self) {
            self.decline(why);
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
        let Some(clip) = self.selected_clip_or_decline() else {
            return Ok(());
        };
        if let Some(why) = crate::chrome::menu_clip::freeze_ready("seq-freeze", self) {
            self.decline(why);
            return Ok(());
        }
        let rate = self.playhead.rate;
        let at = self.playhead;
        let duration = RationalTime::new(self.one_second(), rate);
        self.apply_command(Command::Edit(EditCommand::FreezeFrame {
            clip,
            at,
            duration,
        }))?;
        self.status_message = Some("Froze frame".into());
        Ok(())
    }

    /// Apply `SetClipSpeed` (optionally clearing a ramp first) inside an open
    /// edit group, pushing trailing clips first when Ripple mode is active and
    /// the new speed lengthens the clip.
    pub(crate) fn apply_clip_speed_in_step(
        &mut self,
        clip: aria_models::ClipId,
        speed: aria_models::Rational,
        reversed: bool,
        clear_ramp: bool,
    ) -> Result<(), EngineError> {
        let ripple = (self.edit_mode == crate::types::EditMode::Ripple)
            .then(|| {
                let track = self.track_of(clip)?;
                let rate = self.store.sequence.frame_rate;
                let mut held = self.engine.lock().project().clip(clip)?.clone();
                let old_end = held.timeline.end_tick();
                let old_dur = held.timeline.duration.value;
                if clear_ramp {
                    held.speed_curve = aria_models::Param::Constant(1.0);
                }
                let new_dur = held.retimed(speed, rate).ok()?.duration.value;
                let delta = new_dur - old_dur;
                (delta > 0).then(|| {
                    (
                        self.ripple_speed_lanes(clip, track, old_end),
                        old_end,
                        delta,
                        rate,
                    )
                })
            })
            .flatten()
            .filter(|(lanes, ..)| !lanes.is_empty());

        if let Some((lanes, old_end, delta, rate)) = ripple {
            for lane in lanes {
                self.apply_command(Command::Edit(EditCommand::ShiftClips {
                    track: lane,
                    from: aria_models::RationalTime::new(old_end, rate),
                    delta: aria_models::RationalTime::new(delta, rate),
                }))?;
            }
        }
        if clear_ramp {
            self.apply_command(Command::Edit(EditCommand::SetSpeedCurve {
                clip,
                curve: None,
            }))?;
        }
        self.apply_command(Command::Edit(EditCommand::SetClipSpeed {
            clip,
            speed,
            reversed,
        }))?;
        Ok(())
    }

    /// The lanes a speed change lengthening `clip` in Ripple mode moves from
    /// `from`, the clip's old end, as a ripple trim of its tail moves them:
    /// `primary`, every unlocked lane holding a clip linked to it while linked
    /// selection is on, and every other sync-locked lane. A lane with no clip
    /// starting at or after `from` is left out.
    pub(crate) fn ripple_speed_lanes(
        &self,
        clip: aria_models::ClipId,
        primary: aria_models::TrackId,
        from: i64,
    ) -> Vec<aria_models::TrackId> {
        let mut lanes = self.linked_tracks_of(clip, primary);
        let riders = self.ripple_riders(&lanes);
        lanes.extend(riders);
        lanes.retain(|lane| {
            self.store
                .sequence
                .tracks
                .iter()
                .find(|t| t.id == lane.raw())
                .is_some_and(|t| t.clips.iter().any(|c| c.timeline_start_ticks >= from))
        });
        lanes
    }

    /// Apply `SetSpeedCurve` to `clip`, pushing trailing clips in Ripple mode
    /// when the ramp lengthens the clip, as one undo step.
    pub(crate) fn apply_speed_curve_edit(
        &mut self,
        clip: aria_models::ClipId,
        curve: Option<aria_models::Param<f32>>,
    ) -> Result<(), EngineError> {
        let ripple = (self.edit_mode == crate::types::EditMode::Ripple)
            .then(|| {
                let track = self.track_of(clip)?;
                let rate = self.store.sequence.frame_rate;
                let held = self.engine.lock().project().clip(clip)?.clone();
                let old_end = held.timeline.end_tick();
                let effective = curve.clone().unwrap_or(aria_models::Param::Constant(1.0));
                let new_dur = held.duration_under(&effective, rate)?;
                let delta = new_dur - held.timeline.duration.value;
                (delta > 0).then(|| {
                    (
                        self.ripple_speed_lanes(clip, track, old_end),
                        old_end,
                        delta,
                        rate,
                    )
                })
            })
            .flatten()
            .filter(|(lanes, ..)| !lanes.is_empty());

        if let Some((lanes, old_end, delta, rate)) = ripple {
            self.one_step_as("Set speed curve", |this| {
                for lane in lanes {
                    this.apply_command(Command::Edit(EditCommand::ShiftClips {
                        track: lane,
                        from: aria_models::RationalTime::new(old_end, rate),
                        delta: aria_models::RationalTime::new(delta, rate),
                    }))?;
                }
                this.apply_command(Command::Edit(EditCommand::SetSpeedCurve { clip, curve }))?;
                Ok(())
            })
        } else {
            self.apply_command(Command::Edit(EditCommand::SetSpeedCurve { clip, curve }))?;
            Ok(())
        }
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
#[cfg(test)]
mod boxed_speed_tests {
    //! Speed changes and speed ramps on a clip followed by a neighbor.
    //!
    //! # Why
    //!
    //! Closes the class where applying a lengthening speed ramp (`Hero`) or a
    //! slower Speed/Duration percentage to a clip with a right-hand neighbor
    //! fails with `ModelError::Overlap`: in Overwrite mode the retimed clip's
    //! out point trims to the neighbor's start so the neighbor stays in place,
    //! in Ripple mode trailing clips shift by the added length so the retimed
    //! clip keeps its full length, and each operation undoes in one step. Does
    //! not cover rate-stretch pointer drags, which clamp to room before release.

    use crate::app::AppState;
    use crate::types::EditMode;
    use aria_models::{ClipId, speed_preset};

    fn boxed_pair() -> (AppState, ClipId, ClipId, i64, i64) {
        let (mut state, first) = crate::app::premiere_tests::with_clip();
        let first_ui = state.ui_clip_ref(first).expect("first clip").clone();
        let cut = first_ui.timeline_end_ticks;
        let media = aria_models::MediaId::from_raw(first_ui.media_id.expect("media"));
        state
            .drop_media_on_timeline(media, None, cut)
            .expect("drop trailing neighbor");
        let neighbor = state
            .store
            .sequence
            .tracks
            .iter()
            .flat_map(|t| t.clips.iter())
            .find(|c| c.id != first.raw() && c.timeline_start_ticks == cut)
            .map(|c| ClipId::from_raw(c.id))
            .expect("neighbor clip");
        let neighbor_end = state
            .ui_clip_ref(neighbor)
            .expect("neighbor")
            .timeline_end_ticks;
        state.select_clip(first);
        (state, first, neighbor, cut, neighbor_end)
    }

    #[test]
    fn hero_and_slowdown_on_boxed_clip_trim_in_overwrite_and_shift_in_ripple_with_single_undo() {
        let (mut state, first, neighbor, cut, neighbor_end) = boxed_pair();
        let hero = speed_preset("hero").expect("hero preset");
        let full_hero_len = state
            .engine
            .lock()
            .project()
            .speed_curve_duration(first, &hero)
            .expect("hero duration");
        let added = full_hero_len - cut;
        assert!(
            added > 0,
            "hero must lengthen the clip ({full_hero_len} vs {cut})"
        );

        // 1. Overwrite mode: Hero trims the retimed clip's out point to the
        // neighbor's start, keeps the neighbor unmoved, and undoes in one step.
        state.edit_mode = EditMode::Overwrite;
        state
            .set_selected_speed_preset("hero")
            .expect("hero on boxed clip in Overwrite mode");
        assert_eq!(state.selected_speed_preset(), Some("hero"));
        let first_ui = state.ui_clip_ref(first).expect("first");
        let neighbor_ui = state.ui_clip_ref(neighbor).expect("neighbor");
        assert_eq!(
            (first_ui.timeline_start_ticks, first_ui.timeline_end_ticks),
            (0, cut),
            "retimed clip must end at the neighbor's start in Overwrite mode"
        );
        assert_eq!(
            (
                neighbor_ui.timeline_start_ticks,
                neighbor_ui.timeline_end_ticks
            ),
            (cut, neighbor_end),
            "neighbor must stay unmoved in Overwrite mode"
        );
        assert!(state.undo(), "one undo step restores Overwrite Hero");
        assert!(!state.selected_has_ramp());
        assert_eq!(
            state.ui_clip_ref(first).expect("first").timeline_end_ticks,
            cut
        );
        assert_eq!(
            state
                .ui_clip_ref(neighbor)
                .expect("neighbor")
                .timeline_start_ticks,
            cut
        );

        // Overwrite Speed/Duration slowdown (50%) also trims to the neighbor.
        state.open_speed_duration();
        state.set_speed_duration_percent(50.0).expect("50%");
        state.apply_speed_duration();
        assert_eq!(state.speed_duration.error, None);
        assert_eq!(
            state.ui_clip_ref(first).expect("first").timeline_end_ticks,
            cut
        );
        assert_eq!(
            state
                .ui_clip_ref(neighbor)
                .expect("neighbor")
                .timeline_start_ticks,
            cut
        );
        assert!(state.undo(), "one undo step restores Overwrite 50% speed");

        // 2. Ripple mode: Hero pushes the neighbor by the added length so the
        // retimed clip keeps its full length, and undoes in one step.
        state.edit_mode = EditMode::Ripple;
        state
            .apply_speed_curve_edit(first, Some(hero))
            .expect("hero on boxed clip in Ripple mode");
        assert_eq!(state.selected_speed_preset(), Some("hero"));
        let first_ui = state.ui_clip_ref(first).expect("first");
        let neighbor_ui = state.ui_clip_ref(neighbor).expect("neighbor");
        assert_eq!(
            (first_ui.timeline_start_ticks, first_ui.timeline_end_ticks),
            (0, full_hero_len),
            "retimed clip keeps its full length in Ripple mode"
        );
        assert_eq!(
            (
                neighbor_ui.timeline_start_ticks,
                neighbor_ui.timeline_end_ticks
            ),
            (cut + added, neighbor_end + added),
            "neighbor moves by the added length in Ripple mode"
        );
        assert!(
            state.undo(),
            "one undo step restores both clip and neighbor"
        );
        assert!(!state.selected_has_ramp());
        assert_eq!(
            state.ui_clip_ref(first).expect("first").timeline_end_ticks,
            cut
        );
        assert_eq!(
            (
                state
                    .ui_clip_ref(neighbor)
                    .expect("neighbor")
                    .timeline_start_ticks,
                state
                    .ui_clip_ref(neighbor)
                    .expect("neighbor")
                    .timeline_end_ticks,
            ),
            (cut, neighbor_end)
        );

        // Ripple Speed/Duration slowdown (50%) pushes the neighbor by `cut`.
        state.open_speed_duration();
        state.set_speed_duration_percent(50.0).expect("50%");
        state.apply_speed_duration();
        assert_eq!(state.speed_duration.error, None);
        assert_eq!(
            state.ui_clip_ref(first).expect("first").timeline_end_ticks,
            cut * 2
        );
        assert_eq!(
            state
                .ui_clip_ref(neighbor)
                .expect("neighbor")
                .timeline_start_ticks,
            cut * 2
        );
        assert!(state.undo(), "one undo step restores Ripple 50% speed");
        assert_eq!(
            state.ui_clip_ref(first).expect("first").timeline_end_ticks,
            cut
        );
        assert_eq!(
            state
                .ui_clip_ref(neighbor)
                .expect("neighbor")
                .timeline_start_ticks,
            cut
        );
    }
}

#[cfg(test)]
#[path = "speed_rider_tests.rs"]
mod speed_riders;
