//! Source patching: which lane a Source tab's picture and sound land on, the
//! lane header button that moves the patch, and the one placement every
//! source edit and media drop goes through.

use aria_commands::{Command, EditCommand, Patch, SoundPlacement, SourceChannel};
use aria_engine::EngineError;
use aria_models::{MediaId, RationalTime, TimeRange, TrackId, TrackKind};

use super::AppState;
use crate::surfaces::lane_header::LaneFlags;

impl AppState {
    /// Press a lane's source patch button. A lit lane turns its channel off;
    /// a dark lane takes the channel.
    pub fn toggle_source_patch(&mut self, lane: LaneFlags) -> Result<(), EngineError> {
        let Some(channel) = lane.channel else {
            return Ok(());
        };
        let patch = if lane.patched {
            Patch::Off
        } else {
            Patch::Track(TrackId::from_raw(lane.id))
        };
        self.set_source_patch(channel, patch)
    }

    /// Patch one source channel of the open sequence.
    pub fn set_source_patch(
        &mut self,
        channel: SourceChannel,
        patch: Patch,
    ) -> Result<(), EngineError> {
        self.apply_command(Command::Edit(EditCommand::SetSourcePatch {
            channel,
            patch,
        }))?;
        Ok(())
    }

    /// Place `placement` through source patching: one `Source edit`, one
    /// undo step. `Ok(false)` when every channel the source has is patched
    /// off, with the status line naming the fix.
    pub(crate) fn place_source(&mut self, placement: SourcePlacement) -> Result<bool, EngineError> {
        let SourcePlacement {
            media,
            source,
            at,
            fill,
            ripple,
            aimed,
        } = placement;
        let Some(has) = self.source_channels(media) else {
            return Ok(false);
        };
        // The lane under the pointer takes the channel of its kind, whatever
        // that channel's patch says.
        let aimed = aimed.and_then(|id| {
            self.store
                .sequence
                .tracks
                .iter()
                .find(|lane| lane.id == id.raw())
                .and_then(|lane| lane.channel)
                .map(|channel| (channel, id))
        });
        let aimed_at = |channel: SourceChannel| {
            aimed
                .filter(|(aimed, _)| *aimed == channel)
                .map(|(_, id)| id)
        };
        let track = match (has.0, aimed_at(SourceChannel::Video)) {
            (false, _) => None,
            (true, Some(lane)) => Some(lane),
            (true, None) => self.patched_picture_lane()?,
        };
        let sound = match (has.1, aimed_at(SourceChannel::Audio)) {
            (false, _) => SoundPlacement::Off,
            (true, Some(lane)) => SoundPlacement::Lane(lane),
            (true, None) => self.patched_sound(track.is_some())?,
        };
        if track.is_none() && sound.lane().is_none() {
            self.status_message =
                Some("Both source channels are patched off; patch one in a lane header".into());
            return Ok(false);
        }
        self.apply_command(Command::Edit(EditCommand::SourceEdit {
            media,
            source,
            at,
            fill,
            ripple,
            track,
            sound,
        }))?;
        Ok(true)
    }

    /// Drop the open Source tab's marked range at `tick`: on `aimed` for the
    /// channel of its kind and through the patching for the other, covering
    /// unless `insert`.
    pub fn drop_source_on_timeline(
        &mut self,
        aimed: Option<TrackId>,
        tick: i64,
        insert: bool,
    ) -> Result<(), EngineError> {
        let Some(view) = self.sources.focused() else {
            return Ok(());
        };
        let media = MediaId::from_raw(view.media);
        let Some((start, end)) = self.source_range() else {
            self.status_message = Some("the source's marked range is empty".into());
            return Ok(());
        };
        let Some(rate) = self
            .engine
            .lock()
            .project()
            .media(media)
            .map(|item| item.frame_rate)
        else {
            return Ok(());
        };
        if self.place_source(SourcePlacement {
            media,
            source: TimeRange::at_rate(start, end - start, rate),
            at: RationalTime::new(tick.max(0), self.playhead.rate),
            fill: None,
            ripple: insert,
            aimed,
        })? {
            self.status_message = Some(if insert { "Insert" } else { "Overwrite" }.into());
        }
        Ok(())
    }

    /// Whether `media` has a picture and whether it has sound; `None` when it
    /// is not in the pool.
    fn source_channels(&self, media: MediaId) -> Option<(bool, bool)> {
        let engine = self.engine.lock();
        let item = engine.project().media(media)?;
        Some((
            item.has_channel(SourceChannel::Video),
            item.has_channel(SourceChannel::Audio),
        ))
    }

    /// The lane the picture lands on: the lane its header draws lit, or a new
    /// video lane when the sequence has none. `None` when it is patched off.
    fn patched_picture_lane(&mut self) -> Result<Option<TrackId>, EngineError> {
        if self.store.sequence.source_patching.video == Patch::Off {
            return Ok(None);
        }
        match self.store.sequence.patched_lane(SourceChannel::Video) {
            Some(id) => Ok(Some(TrackId::from_raw(id))),
            None => self.ensure_kind_track(TrackKind::Video).map(Some),
        }
    }

    /// Where the sound goes: the lane its header draws lit; with no audio lane
    /// in the sequence, inside the picture when one is placed and on a new
    /// audio lane when none is.
    fn patched_sound(&mut self, with_picture: bool) -> Result<SoundPlacement, EngineError> {
        if self.store.sequence.source_patching.audio == Patch::Off {
            return Ok(SoundPlacement::Off);
        }
        Ok(
            match self.store.sequence.patched_lane(SourceChannel::Audio) {
                Some(id) => SoundPlacement::Lane(TrackId::from_raw(id)),
                None if with_picture => SoundPlacement::InPicture,
                None => SoundPlacement::Lane(self.ensure_kind_track(TrackKind::Audio)?),
            },
        )
    }
}

/// One source edit before its lanes are resolved.
pub(crate) struct SourcePlacement {
    pub media: MediaId,
    /// The range of the source taken, in the source's clock.
    pub source: TimeRange,
    pub at: RationalTime,
    /// The span to retime the source to fill; `None` places it at its length.
    pub fill: Option<RationalTime>,
    pub ripple: bool,
    /// The lane under the pointer, for a drop.
    pub aimed: Option<TrackId>,
}
