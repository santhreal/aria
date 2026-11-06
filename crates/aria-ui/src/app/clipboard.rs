//! Copy, Cut, and the four pastes.
//!
//! The clipboard holds the selected clips *as they were* when they were
//! copied, not references to them: Cut removes the clips and a paste puts them
//! back, and a change to a clip after the copy does not reach the clipboard.
//! Replacing the project empties it, because a copy plays media by id and ids
//! repeat across projects.
//!
//! A paste keeps the copies' spacing: the earliest copy lands at the playhead
//! and the rest follow at their copied offsets from it. Each copy lands on its
//! own lane while the open sequence has it, and on the open sequence's first
//! lane of the same kind otherwise, so a copy taken in one sequence pastes into
//! another. Every paste is one undo step, and a copy that cannot land takes
//! the others back with it.
//!
//! Four shapes, matching an NLE's Paste menu:
//!
//! * **Paste** drops the copies at the playhead, over whatever is there (the
//!   engine refuses an overlap).
//! * **Paste Insert** first opens a hole the length of the copies by rippling
//!   each target lane right from the playhead, so nothing is overwritten.
//! * **Paste Attributes** writes the first copy's look — transform, filter,
//!   effect chain with every parameter — onto the selected clip and leaves
//!   timing alone.
//! * **Paste To Fit** drops a single copy into the sequence's marked range,
//!   retimed to fill it, starting at the In point.

use aria_commands::{Command, EditCommand};
use aria_engine::EngineError;
use aria_models::{Clip, Rational, RationalTime, Timeline, TrackId, TrackKind, resample};

use super::AppState;
use super::decline::Decline;

/// What Copy and Cut put on the clipboard: the selected clips as they were
/// when they were taken, in selection order. Never empty.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Copied {
    pub(crate) clips: Vec<CopiedClip>,
}

/// One clip of a [`Copied`], with the lane it was on.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CopiedClip {
    pub(crate) clip: Box<Clip>,
    pub(crate) track: TrackId,
    pub(crate) kind: TrackKind,
}

impl Copied {
    /// The rate the copies were taken at: the copied sequence's.
    fn rate(&self) -> Rational {
        self.clips[0].clip.timeline.start.rate
    }

    /// The earliest copied start, which a paste puts at the playhead.
    fn anchor(&self) -> i64 {
        self.clips
            .iter()
            .map(|copy| copy.clip.timeline.start.value)
            .min()
            .unwrap_or(0)
    }

    /// From the earliest copied start to the latest copied end.
    fn span(&self) -> i64 {
        let end = self
            .clips
            .iter()
            .map(|copy| copy.clip.timeline.end_tick())
            .max()
            .unwrap_or(0);
        end - self.anchor()
    }
}

/// One copy placed by a paste: the clip, its lane, and its offset from the
/// playhead in the open sequence's ticks.
struct Placement {
    copy: Box<Clip>,
    track: TrackId,
    offset: i64,
}

/// The lane a paste of `copy` lands on: the copy's own lane while the open
/// sequence has it, else the open sequence's first lane of the same kind.
fn paste_lane(timeline: &Timeline, copy: &CopiedClip) -> Option<TrackId> {
    if timeline.track(copy.track).is_some() {
        return Some(copy.track);
    }
    timeline
        .tracks_ordered()
        .find(|track| track.kind == copy.kind)
        .map(|track| track.id)
}

impl AppState {
    /// Ctrl+C: put copies of the selected clips on the clipboard. Inert with
    /// nothing selected, so the clipboard keeps what it held.
    pub fn copy_selected(&mut self) {
        let clips: Vec<CopiedClip> = {
            let engine = self.engine.lock();
            let timeline = engine.project().timeline();
            self.selection
                .iter()
                .filter_map(|&id| {
                    let clip = timeline.clip(id)?;
                    let track = timeline.track(timeline.track_of(id)?)?;
                    Some(CopiedClip {
                        clip: Box::new(clip.clone()),
                        track: track.id,
                        kind: track.kind,
                    })
                })
                .collect()
        };
        if clips.is_empty() {
            return;
        }
        self.clipboard = Some(Copied { clips });
        self.status_message = Some("Copied".into());
    }

    /// Where each clipboard copy lands, and the length of the whole copy in
    /// the open sequence's ticks, or nothing plus the status line that says
    /// why.
    fn paste_plan(&mut self) -> Option<(Vec<Placement>, i64)> {
        let Some(copied) = &self.clipboard else {
            self.decline(Decline::NothingCopied);
            return None;
        };
        let (from, to) = (copied.rate(), self.playhead.rate);
        let ticks = |value: i64| resample(RationalTime::new(value, from), to).value;
        let anchor = copied.anchor();
        let span = ticks(copied.span());
        let placements: Option<Vec<Placement>> = {
            let engine = self.engine.lock();
            let timeline = engine.project().timeline();
            copied
                .clips
                .iter()
                .map(|copy| {
                    Some(Placement {
                        copy: copy.clip.clone(),
                        track: paste_lane(timeline, copy)?,
                        offset: ticks(copy.clip.timeline.start.value - anchor),
                    })
                })
                .collect()
        };
        match placements {
            Some(placements) => Some((placements, span)),
            None => {
                self.decline(Decline::NoLaneForCopy);
                None
            }
        }
    }

    /// Send one placement per copy, at the playhead plus its offset.
    fn place_copies(
        &mut self,
        placements: Vec<Placement>,
        at: RationalTime,
    ) -> Result<(), EngineError> {
        for Placement {
            copy,
            track,
            offset,
        } in placements
        {
            self.apply_command(Command::Edit(EditCommand::PasteClip {
                copy,
                track,
                start: RationalTime::new(at.value + offset, at.rate),
            }))?;
        }
        Ok(())
    }

    /// Ctrl+V: drop the copies at the playhead, as one undo step.
    pub fn paste_clipboard(&mut self) -> Result<(), EngineError> {
        let Some((placements, _)) = self.paste_plan() else {
            return Ok(());
        };
        let at = self.playhead;
        self.one_step(|state| state.place_copies(placements, at))
    }

    /// Paste one copy into the marked range, retimed to fill it exactly.
    ///
    /// The length comes from the sequence's in and out marks, because that is
    /// the only number in the session that says how long the copy should become,
    /// and the copy lands at the in point rather than at the playhead: the range
    /// is the destination. Says so with nothing copied, several clips copied,
    /// or no range marked, because a retimed paste at the playhead would be a
    /// different edit than the one the row names.
    pub fn paste_to_fit(&mut self) -> Result<(), EngineError> {
        let Some((mut placements, _)) = self.paste_plan() else {
            return Ok(());
        };
        if placements.len() > 1 {
            self.decline(Decline::SeveralCopied);
            return Ok(());
        }
        let Some((from, to)) = self.marked_range() else {
            self.decline(Decline::NoMarkedRange);
            return Ok(());
        };
        let Placement { copy, track, .. } = placements.remove(0);
        let rate = self.playhead.rate;
        self.apply_command(Command::Edit(EditCommand::PasteRetimed {
            copy,
            track,
            start: RationalTime::new(from, rate),
            target_duration: Some(RationalTime::new(to - from, rate)),
            target_speed: None,
        }))?;
        Ok(())
    }

    /// Paste at the playhead without overwriting: ripple each target lane right
    /// by the copy's length first, then drop the copies into the hole, as one
    /// undo step.
    pub fn paste_insert(&mut self) -> Result<(), EngineError> {
        let Some((placements, span)) = self.paste_plan() else {
            return Ok(());
        };
        if span <= 0 {
            return Ok(());
        }
        let mut lanes: Vec<TrackId> = placements.iter().map(|place| place.track).collect();
        lanes.sort_unstable();
        lanes.dedup();
        let at = self.playhead;
        self.one_step_as("Paste insert", |state| {
            for track in lanes {
                state.apply_command(Command::Edit(EditCommand::ShiftClips {
                    track,
                    from: at,
                    delta: RationalTime::new(span, at.rate),
                }))?;
            }
            state.place_copies(placements, at)
        })
    }

    /// Write the first copy's look onto the selected clip: transform, filter,
    /// and the whole effect chain with its parameters. Timing, media, and audio
    /// levels are not attributes and are left alone.
    ///
    /// Inert when the selection *is* the clip the copy was taken from.
    pub fn paste_attributes(&mut self) -> Result<(), EngineError> {
        let Some(copied) = &self.clipboard else {
            self.decline(Decline::NothingCopied);
            return Ok(());
        };
        let copy = copied.clips[0].clip.clone();
        let Some(target) = self.selected_clip_or_decline() else {
            return Ok(());
        };
        if target == copy.id {
            return Ok(());
        }
        self.apply_command(Command::Edit(EditCommand::PasteAttributes {
            copy,
            clip: target,
        }))?;
        self.status_message = Some("Pasted attributes".into());
        Ok(())
    }
}
