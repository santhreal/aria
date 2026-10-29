//! Source patching: which lane a Source tab's picture and sound land on, the
//! lane header button that moves the patch, and the one placement every
//! source edit and media drop goes through.

use aria_commands::{Command, EditCommand, Patch, SoundPlacement, SourceChannel};
use aria_engine::EngineError;
use aria_models::{MediaId, RationalTime, TimeRange, TrackId};

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

    /// Place `placement` through source patching as one `Source edit` undo
    /// step, the lanes it needs created inside it. `Ok(false)` when every
    /// channel the source has is patched off, with the status line naming the
    /// fix.
    pub(crate) fn place_source(&mut self, placement: SourcePlacement) -> Result<bool, EngineError> {
        self.one_step_named(
            |label: &Option<&'static str>| *label,
            |state| {
                let Some(lanes) = state.source_lanes(placement.media, placement.aimed)? else {
                    return Ok(None);
                };
                state.place_source_on(placement, lanes).map(Some)
            },
        )
        .map(|label| label.is_some())
    }

    /// The lanes a source edit of `media` lands on: `aimed` for the channel of
    /// its kind, the patching for the other, and a new lane for a channel the
    /// sequence has no lane of. `Ok(None)` when `media` is not in the pool, or
    /// when every channel it has is patched off, with the status line naming
    /// the fix.
    pub(crate) fn source_lanes(
        &mut self,
        media: MediaId,
        aimed: Option<TrackId>,
    ) -> Result<Option<SourceLanes>, EngineError> {
        let Some(has) = self.source_channels(media) else {
            return Ok(None);
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
        let picture = match (has.0, aimed_at(SourceChannel::Video)) {
            (false, _) => None,
            (true, Some(lane)) => Some(lane),
            (true, None) => self.patched_or_new_lane(SourceChannel::Video)?,
        };
        let sound = match (has.1, aimed_at(SourceChannel::Audio)) {
            (false, _) => SoundPlacement::Off,
            (true, Some(lane)) => SoundPlacement::Lane(lane),
            (true, None) => self
                .patched_or_new_lane(SourceChannel::Audio)?
                .map_or(SoundPlacement::Off, SoundPlacement::Lane),
        };
        if picture.is_none() && sound.lane().is_none() {
            self.status_message =
                Some("Both source channels are patched off; patch one in a lane header".into());
            return Ok(None);
        }
        Ok(Some(SourceLanes { picture, sound }))
    }

    /// Place `placement` on `lanes`, resolved by [`Self::source_lanes`], as one
    /// `Source edit`. `placement.aimed` is not read again. Returns the edit's
    /// history label, which names the undo step of a gesture that also adds
    /// the lanes it lands on.
    pub(crate) fn place_source_on(
        &mut self,
        placement: SourcePlacement,
        lanes: SourceLanes,
    ) -> Result<&'static str, EngineError> {
        let SourcePlacement {
            media,
            source,
            at,
            fill,
            ripple,
            aimed: _,
        } = placement;
        let edit = EditCommand::SourceEdit {
            media,
            source,
            at,
            fill,
            ripple,
            track: lanes.picture,
            sound: lanes.sound,
        };
        let label = edit.label();
        self.apply_command(Command::Edit(edit))?;
        Ok(label)
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
            self.status_message = Some("The source's marked range is empty".into());
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

    /// The lane `channel` lands on: the lane its header draws lit, or a new
    /// lane of its kind when the sequence has none. `None` when it is patched
    /// off.
    fn patched_or_new_lane(
        &mut self,
        channel: SourceChannel,
    ) -> Result<Option<TrackId>, EngineError> {
        if self.store.sequence.source_patching.get(channel) == Patch::Off {
            return Ok(None);
        }
        match self.store.sequence.patched_lane(channel) {
            Some(id) => Ok(Some(TrackId::from_raw(id))),
            None => self.ensure_kind_track(channel.lane_kind()).map(Some),
        }
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

/// The lanes one source edit lands on.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SourceLanes {
    /// The lane the picture lands on; `None` when the source has none or it is
    /// patched off.
    pub picture: Option<TrackId>,
    pub sound: SoundPlacement,
}

impl SourceLanes {
    /// Every lane the edit places a clip on: the picture's, then the sound's.
    pub(crate) fn lanes(self) -> impl Iterator<Item = TrackId> {
        self.picture.into_iter().chain(self.sound.lane())
    }
}
