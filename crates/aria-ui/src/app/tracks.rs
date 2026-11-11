//! Track-level commands: stack order, the duck-source flag, and the lane
//! shift that opens and closes time on a lane.
//!
//! `MoveTrack`, `SetTrackDuckSource`, and `ShiftClips` had no operator path. The store holds tracks top-first (the projection reverses the
//! model's bottom-first order), so "move up" here means "raise in the composite",
//! and the index sent to the engine is converted back at the boundary — one place,
//! rather than every caller guessing which way the stack runs.

use aria_commands::{Command, EditCommand};
use aria_engine::EngineError;
use aria_models::{RationalTime, TrackId};

use super::AppState;
use super::decline::Decline;

impl AppState {
    /// Where a track sits in the projected (top-first) list.
    fn track_row(&self, track: TrackId) -> Option<usize> {
        self.store
            .sequence
            .tracks
            .iter()
            .position(|lane| lane.id == track.raw())
    }

    /// Start dragging a track header. Inert on a lane that is not in the stack.
    pub fn begin_track_drag(&mut self, track: TrackId) {
        if self.track_row(track).is_some() {
            self.track_drag = Some(track.raw());
            self.track_drop_row = None;
        }
    }

    /// Whether a header drag is in flight.
    pub fn track_dragging(&self) -> bool {
        self.track_drag.is_some()
    }

    /// Mark the row the pointer is over as the drop target. Inert with no drag, and
    /// on the dragged lane's own row, so the indicator only appears where the drop
    /// would change something.
    pub fn hover_track_drop(&mut self, row: usize) -> bool {
        let Some(dragged) = self.track_drag else {
            return false;
        };
        let own = self.track_row(TrackId::from_raw(dragged));
        let target = (own != Some(row) && row < self.store.sequence.tracks.len()).then_some(row);
        let changed = self.track_drop_row != target;
        self.track_drop_row = target;
        changed
    }

    /// Let go of a header drag, wherever the button comes up: the dragged lane
    /// drops on the row the indicator marks, one reorder and one undo entry,
    /// and with no row marked the drag ends and writes nothing. `true` when a
    /// drag was in flight.
    pub fn release_track_drag(&mut self) -> Result<bool, EngineError> {
        let Some(dragged) = self.track_drag.take() else {
            return Ok(false);
        };
        if let Some(row) = self.track_drop_row.take() {
            self.move_track_to_row(TrackId::from_raw(dragged), row)?;
        }
        Ok(true)
    }

    /// Abandon a header drag at no cost.
    pub fn cancel_track_drag(&mut self) {
        self.track_drag = None;
        self.track_drop_row = None;
    }

    /// The lane holding the selected clip. Palette entries and keys act on a lane
    /// through the selection, since there is no separate "current track".
    pub fn selected_track(&self) -> Option<TrackId> {
        let clip = self.selected_clip()?;
        self.store
            .sequence
            .tracks
            .iter()
            .find(|lane| lane.clips.iter().any(|c| c.id == clip.raw()))
            .map(|lane| TrackId::from_raw(lane.id))
    }

    /// Move a track one row up in the panel, which is one layer higher in the
    /// composite. A lane at the top of its part of the stack declines.
    pub fn move_track_up(&mut self, track: TrackId) -> Result<(), EngineError> {
        self.step_track(track, true)
    }

    /// Move a track one row down in the panel. A lane at the bottom of its part
    /// of the stack declines.
    pub fn move_track_down(&mut self, track: TrackId) -> Result<(), EngineError> {
        self.step_track(track, false)
    }

    fn step_track(&mut self, track: TrackId, up: bool) -> Result<(), EngineError> {
        let index = match self.track_step(track, up) {
            Ok(index) => index,
            Err(why) => {
                self.decline(why);
                return Ok(());
            }
        };
        self.apply_command(Command::Edit(EditCommand::MoveTrack { track, index }))?;
        Ok(())
    }

    /// The index `MoveTrack` sends to move `track` one row up (`up`) or down,
    /// or why it stays: the row past it holds a lane of another part of the
    /// stack, or there is none. The rows' guards, the track menu and the move
    /// read this.
    pub(crate) fn track_step(&self, track: TrackId, up: bool) -> Result<usize, Decline> {
        let edge = if up {
            Decline::TrackAtTop
        } else {
            Decline::TrackAtBottom
        };
        let row = self.track_row(track).ok_or(edge)?;
        let to = if up {
            row.checked_sub(1)
        } else {
            row.checked_add(1)
        };
        to.and_then(|to| self.track_move_index(track, to))
            .ok_or(edge)
    }

    /// Move a track to a row of the projected list. A drop that would leave
    /// the stack as it is sends nothing.
    pub fn move_track_to_row(&mut self, track: TrackId, row: usize) -> Result<(), EngineError> {
        let Some(index) = self.track_move_index(track, row) else {
            return Ok(());
        };
        self.apply_command(Command::Edit(EditCommand::MoveTrack { track, index }))?;
        Ok(())
    }

    /// The index `MoveTrack` sends to put `track` at projected `row`, or `None`
    /// where the stack would come out as it is. The timeline keeps a lane
    /// among the lanes of its zone, which sit together, so `row` is held to
    /// that run first: a row past the run lands the lane at its edge, and a
    /// lane already at that edge does not move.
    ///
    /// Visual lanes above the A/V divider are projected top-first (`V3, V2,
    /// V1`), while audio lanes below the divider are projected downward (`A1,
    /// A2, A3`), and the index is converted back to the timeline's bottom-first
    /// order here.
    fn track_move_index(&self, track: TrackId, row: usize) -> Option<usize> {
        let tracks = &self.store.sequence.tracks;
        let from_row = self.track_row(track)?;
        let visual_count = tracks
            .iter()
            .take_while(|lane| {
                !matches!(
                    lane.kind,
                    crate::types::UiTrackKind::Audio | crate::types::UiTrackKind::Submix
                )
            })
            .count();
        let audio = from_row >= visual_count;
        if row >= tracks.len() || (row >= visual_count) != audio {
            return None;
        }
        let zone = tracks[from_row].zone;
        let same = |r: &usize| tracks[*r].zone == zone;
        let first = (0..from_row)
            .rev()
            .take_while(same)
            .last()
            .unwrap_or(from_row);
        let last = (from_row + 1..tracks.len())
            .take_while(same)
            .last()
            .unwrap_or(from_row);
        let row = row.clamp(first, last);
        if row == from_row {
            None
        } else if audio {
            Some(row - visual_count)
        } else {
            Some(tracks.len() - 1 - row)
        }
    }

    /// Whether a lane is flagged as the voice that auto-duck listens to.
    pub fn track_is_duck_source(&self, track: TrackId) -> bool {
        let engine = self.engine.lock();
        engine
            .project()
            .timeline()
            .track(track)
            .map(|lane| lane.duck_source)
            .unwrap_or(false)
    }

    /// Toggle the duck-source flag. Only an audio lane can be the voice, so the
    /// toggle is inert elsewhere rather than raising a refusal.
    pub fn toggle_track_duck_source(&mut self, track: TrackId) -> Result<(), EngineError> {
        let is_audio =
            self.store.sequence.tracks.iter().any(|lane| {
                lane.id == track.raw() && lane.kind == crate::types::UiTrackKind::Audio
            });
        if !is_audio {
            return Ok(());
        }
        let duck_source = !self.track_is_duck_source(track);
        self.apply_command(Command::Edit(EditCommand::SetTrackDuckSource {
            track,
            duck_source,
        }))?;
        Ok(())
    }

    /// Toggle whether a lane is the destination for source-monitor edits and
    /// media drops of its kind.
    ///
    /// Several lanes of a kind may be targeted at once; the topmost of them
    /// wins a drop. Untargeting every lane of a kind is allowed — the drop then
    /// falls back to the kind's first lane, which is what happened before
    /// targeting existed.
    pub fn toggle_track_targeted(&mut self, track: TrackId) -> Result<(), EngineError> {
        let Some(targeted) = self
            .store
            .sequence
            .tracks
            .iter()
            .find(|lane| lane.id == track.raw())
            .map(|lane| lane.targeted)
        else {
            return Ok(());
        };
        self.apply_command(Command::Edit(EditCommand::SetTrackTargeted {
            track,
            targeted: !targeted,
        }))?;
        Ok(())
    }

    /// Toggle whether a lane rides along with an insert on another lane.
    ///
    /// Clearing it pins the lane's clips: an insert elsewhere then slides
    /// picture past sound, which is the point of being able to clear it.
    pub fn toggle_track_sync_lock(&mut self, track: TrackId) -> Result<(), EngineError> {
        let Some(sync_lock) = self
            .store
            .sequence
            .tracks
            .iter()
            .find(|lane| lane.id == track.raw())
            .map(|lane| lane.sync_lock)
        else {
            return Ok(());
        };
        self.apply_command(Command::Edit(EditCommand::SetTrackSyncLock {
            track,
            sync_lock: !sync_lock,
        }))?;
        Ok(())
    }

    /// Toggle solo on a lane. While any lane of a kind is soloed the others of
    /// that kind stop contributing, so this is the only control that can silence
    /// a lane without touching its own mute flag.
    pub fn toggle_track_solo(&mut self, track: TrackId) -> Result<(), EngineError> {
        let Some(solo) = self
            .store
            .sequence
            .tracks
            .iter()
            .find(|lane| lane.id == track.raw())
            .map(|lane| lane.solo)
        else {
            return Ok(());
        };
        self.apply_command(Command::Edit(EditCommand::SetTrackSolo {
            track,
            solo: !solo,
        }))?;
        Ok(())
    }

    /// Shift every clip on `track` that starts at or after the playhead by
    /// `delta` ticks. Positive opens a hole, negative closes a gap. Declines,
    /// for the reason [`Self::playhead_shift_decline`] gives, when there is no
    /// clip to move or the gap is shorter than the close. A shift the engine
    /// still rejects is reported.
    pub fn shift_clips_from_playhead(
        &mut self,
        track: TrackId,
        delta: i64,
    ) -> Result<(), EngineError> {
        if delta == 0 {
            return Ok(());
        }
        if let Some(why) = self.playhead_shift_decline(track, delta) {
            self.decline(why);
            return Ok(());
        }
        let rate = self.playhead.rate;
        self.apply_command(Command::Edit(EditCommand::ShiftClips {
            track,
            from: self.playhead,
            delta: RationalTime::new(delta, rate),
        }))?;
        Ok(())
    }

    /// Why shifting the clips on `track` that start at or after the playhead
    /// by `delta` ticks moves nothing, or `None` when it moves them: no clip
    /// starts there, or a close is longer than the free span before the first
    /// of them, which ends at the clip before it or at tick 0. The shift, the
    /// palette's gap rows and the track menu's gap rows read this.
    pub(crate) fn playhead_shift_decline(&self, track: TrackId, delta: i64) -> Option<Decline> {
        let at = self.playhead.value;
        let clips = self
            .store
            .sequence
            .tracks
            .iter()
            .find(|lane| lane.id == track.raw())
            .map_or(&[][..], |lane| &*lane.clips);
        let Some(first) = clips
            .iter()
            .map(|clip| clip.timeline_start_ticks)
            .filter(|&start| start >= at)
            .min()
        else {
            return Some(Decline::NothingAfterPlayhead);
        };
        let free_from = clips
            .iter()
            .filter(|clip| clip.timeline_start_ticks < at)
            .map(|clip| clip.timeline_end_ticks)
            .max()
            .unwrap_or(0);
        (first - free_from < -delta).then_some(Decline::GapTooShort)
    }

    /// The lanes the selection spans, each with the earliest selected clip start on
    /// it, in projected (top-first) order.
    ///
    /// The start is where a shift has to begin on that lane: shifting from the
    /// playhead instead would move unselected clips.
    pub fn selection_shift_origins(&self) -> Vec<(TrackId, i64)> {
        let mut out: Vec<(TrackId, i64)> = Vec::new();
        for lane in &self.store.sequence.tracks {
            let start = lane
                .clips
                .iter()
                .filter(|clip| self.selection.iter().any(|sel| sel.raw() == clip.id))
                .map(|clip| clip.timeline_start_ticks)
                .min();
            if let Some(start) = start {
                out.push((TrackId::from_raw(lane.id), start));
            }
        }
        out
    }

    /// Whether a shift of `frames` would take any spanned lane below tick 0.
    ///
    /// Checked across every lane before the first one is written, because a
    /// half-applied shift is worse than a refused one. `frames` is typed text,
    /// so the sum saturates: a shift past the last tick is not one before the
    /// first, and the engine refuses it as an overflow.
    fn shift_would_leave_the_sequence(&self, origins: &[(TrackId, i64)], frames: i64) -> bool {
        origins
            .iter()
            .any(|(_, start)| start.saturating_add(frames) < 0)
    }

    /// Shift the selection by `frames`, on every lane it spans, from the earliest
    /// selected clip on each.
    ///
    /// Everything at or after that point on the lane moves with it, which is what
    /// keeps two lanes of one take together. A shift that would move any lane
    /// before the start of the sequence is refused whole, and a multi-lane shift
    /// commits as one undo step.
    pub fn shift_selection(&mut self, frames: i64) -> Result<(), EngineError> {
        if frames == 0 {
            return Ok(());
        }
        let origins = self.selection_shift_origins();
        if origins.is_empty() {
            self.decline(Decline::NoSelection);
            return Ok(());
        }
        if self.shift_would_leave_the_sequence(&origins, frames) {
            self.decline(Decline::ShiftBeforeStart);
            return Ok(());
        }
        let rate = self.playhead.rate;
        self.one_step(|this| {
            for (track, start) in origins {
                this.apply_command(Command::Edit(EditCommand::ShiftClips {
                    track,
                    from: RationalTime::new(start, rate),
                    delta: RationalTime::new(frames, rate),
                }))?;
            }
            Ok(())
        })
    }

    /// The selected clip's lane, which Shift Lane moves by `delta` ticks from
    /// its first clip on. Declined with nothing selected, or when the shift
    /// would take that first clip before the start of the sequence.
    pub(crate) fn lane_shift(&self, delta: i64) -> Result<TrackId, Decline> {
        let track = self.selected_track().ok_or(Decline::NoSelection)?;
        if self.lane_start(track) + delta < 0 {
            return Err(Decline::ShiftBeforeStart);
        }
        Ok(track)
    }

    /// Where the lane's first clip starts, in ticks; 0 for an empty lane.
    fn lane_start(&self, track: TrackId) -> i64 {
        self.store
            .sequence
            .tracks
            .iter()
            .find(|lane| lane.id == track.raw())
            .and_then(|lane| {
                lane.clips
                    .iter()
                    .map(|clip| clip.timeline_start_ticks)
                    .min()
            })
            .unwrap_or(0)
    }

    /// Shift the whole lane, from its first clip on, by `delta` ticks.
    pub fn shift_track(&mut self, track: TrackId, delta: i64) -> Result<(), EngineError> {
        if delta == 0 {
            return Ok(());
        }
        let rate = self.playhead.rate;
        let from = self.lane_start(track);
        self.apply_command(Command::Edit(EditCommand::ShiftClips {
            track,
            from: RationalTime::new(from, rate),
            delta: RationalTime::new(delta, rate),
        }))?;
        Ok(())
    }
}
