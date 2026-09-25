//! Copy and the four pastes.
//!
//! The clipboard holds a clip *reference*, not a copy of its content: a paste is
//! a `DuplicateClip` of the still-live source, so nothing has to be serialized
//! and a paste of a deleted clip is simply inert.
//!
//! Four shapes, matching an NLE's Paste menu:
//!
//! * **Paste** drops the copy at the playhead on the source's own lane, over
//!   whatever is there (the engine refuses an overlap).
//! * **Paste Insert** first opens a hole the length of the copy by rippling that
//!   lane right from the playhead, so nothing is overwritten.
//! * **Paste Attributes** writes the source's look — transform, filter, effect
//!   chain with every parameter — onto the selected clip and leaves timing alone.
//! * **Paste To Fit** drops the copy into the sequence's marked range, retimed to
//!   fill it, starting at the In point.
//!
//! Paste Insert remains two commands (shift, then duplicate), and a failed
//! duplicate rolls the shift back rather than leaving a hole. Paste Attributes
//! is one engine transaction with one rollback snapshot and one undo entry.

use aria_commands::{Command, EditCommand};
use aria_engine::EngineError;
use aria_models::{ClipId, RationalTime, TrackId};

use super::AppState;

impl AppState {
    pub fn copy_selected(&mut self) {
        self.clipboard = self.selected_clip();
        if self.clipboard.is_some() {
            self.status_message = Some("Copied".into());
        }
    }

    /// The projected clip the clipboard points at, if it still exists.
    fn clipboard_ui_clip(&self) -> Option<&crate::types::UiClip> {
        let clip = self.clipboard?;
        self.store
            .sequence
            .tracks
            .iter()
            .flat_map(|t| t.clips.iter())
            .find(|c| c.id == clip.raw())
    }

    pub fn paste_clipboard(&mut self) -> Result<(), EngineError> {
        let Some((clip, to_track)) = self
            .clipboard_ui_clip()
            .map(|ui| (ClipId::from_raw(ui.id), TrackId::from_raw(ui.track_id)))
        else {
            return Ok(());
        };
        self.apply_command(Command::Edit(EditCommand::DuplicateClip {
            clip,
            to_track,
            start: self.playhead,
        }))?;
        Ok(())
    }

    /// Paste the copy into the marked range, retimed to fill it exactly.
    ///
    /// The length comes from the sequence's in and out marks, because that is
    /// the only number in the session that says how long the copy should become,
    /// and the copy lands at the in point rather than at the playhead: the range
    /// is the destination. Inert with nothing copied, and it says so with no
    /// range marked, because a retimed paste at the playhead would be a
    /// different edit than the one the row names.
    pub fn paste_to_fit(&mut self) -> Result<(), EngineError> {
        let Some((clip, to_track)) = self
            .clipboard_ui_clip()
            .map(|ui| (ClipId::from_raw(ui.id), TrackId::from_raw(ui.track_id)))
        else {
            return Ok(());
        };
        let Some((from, to)) = self.marked_range() else {
            self.status_message = Some("Mark In and Out to paste into a range".into());
            return Ok(());
        };
        let rate = self.playhead.rate;
        self.apply_command(Command::Edit(EditCommand::PasteRetimed {
            clip,
            to_track,
            start: RationalTime::new(from, rate),
            target_duration: Some(RationalTime::new(to - from, rate)),
            target_speed: None,
        }))?;
        Ok(())
    }

    /// Paste at the playhead without overwriting: ripple the lane right by the
    /// copy's length first, then drop the copy into the hole.
    pub fn paste_insert(&mut self) -> Result<(), EngineError> {
        let Some((clip, track, duration)) = self.clipboard_ui_clip().map(|ui| {
            (
                ClipId::from_raw(ui.id),
                TrackId::from_raw(ui.track_id),
                ui.timeline_end_ticks - ui.timeline_start_ticks,
            )
        }) else {
            return Ok(());
        };
        if duration <= 0 {
            return Ok(());
        }
        let rate = self.playhead.rate;
        self.apply_command(Command::Edit(EditCommand::ShiftClips {
            track,
            from: self.playhead,
            delta: RationalTime::new(duration, rate),
        }))?;
        let placed = self.apply_command(Command::Edit(EditCommand::DuplicateClip {
            clip,
            to_track: track,
            start: self.playhead,
        }));
        if let Err(err) = placed {
            // The hole is only justified by the clip that fills it.
            self.apply_command(Command::Edit(EditCommand::ShiftClips {
                track,
                from: self.playhead,
                delta: RationalTime::new(-duration, rate),
            }))?;
            return Err(err);
        }
        Ok(())
    }

    /// Write the clipboard clip's look onto the selected clip: transform, filter,
    /// and the whole effect chain with its parameters. Timing, media, and audio
    /// levels are not attributes and are left alone.
    ///
    /// Inert when the clipboard is empty, when nothing is selected, or when the
    /// selection *is* the source.
    pub fn paste_attributes(&mut self) -> Result<(), EngineError> {
        let Some(source) = self.clipboard else {
            return Ok(());
        };
        let Some(target) = self.selected_clip() else {
            return Ok(());
        };
        if target == source {
            return Ok(());
        }
        self.apply_command(Command::Edit(EditCommand::PasteAttributes {
            source,
            clip: target,
        }))?;
        self.status_message = Some("Pasted attributes".into());
        Ok(())
    }
}
