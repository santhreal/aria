//! Multicam group creation, angle switching, and where each camera sits on the
//! group's clock.

use aria_commands::{Command, EditCommand};
use aria_engine::EngineError;
use aria_models::{MediaId, MulticamAngle, MulticamGroupId};

use super::AppState;
use super::decline::Decline;
use super::multicam_sync;

impl AppState {
    /// Create a multicam camera group from media items.
    pub fn create_multicam_group(
        &mut self,
        name: impl Into<String>,
        angles: Vec<MulticamAngle>,
    ) -> Result<Option<MulticamGroupId>, EngineError> {
        let name = name.into();
        self.apply_command(Command::Edit(EditCommand::CreateMulticamGroup {
            name,
            angles,
        }))?;
        // The engine assigns the id, and the projection is rebuilt by
        // `apply_command` above, so the new group is the last row it carries.
        Ok(self
            .store
            .multicam
            .last()
            .map(|group| MulticamGroupId::from_raw(group.id)))
    }

    /// Every selected pool item, in the order the operator picked them.
    ///
    /// The order is the angle order of a group built from it, which is why the
    /// selection keeps pick order rather than the order the list happens to be
    /// sorted in.
    pub fn selected_pool_media_ids(&self) -> Vec<MediaId> {
        self.pool_selection
            .ids()
            .iter()
            .map(|id| MediaId::from_raw(*id))
            .collect()
    }

    /// Switch the active angle of a multicam group at the current playhead position.
    pub fn switch_multicam_angle(
        &mut self,
        group: MulticamGroupId,
        angle: usize,
    ) -> Result<(), EngineError> {
        let at = self.store.playhead_ticks;
        self.apply_command(Command::Edit(EditCommand::SwitchMulticamAngle {
            group,
            at,
            angle,
        }))?;
        Ok(())
    }

    /// Put `group` on a video lane at the playhead.
    ///
    /// The span comes from the model rather than from the first angle: a group
    /// is as long as its longest camera reaches, and the model owns that
    /// derivation because the offsets and the media durations are both its
    /// facts.
    ///
    /// The lane is the one a media drop would land on, unless the span is
    /// already occupied there, in which case the group gets a lane of its own on
    /// top. A placement is an add rather than a ripple or an overwrite, so the
    /// alternative is a button that refuses on any full timeline, and a group
    /// that arrives beside the material it is meant to replace is what an
    /// operator can act on.
    pub fn place_multicam_group(&mut self, group: MulticamGroupId) -> Result<(), EngineError> {
        let source = {
            let engine = self.engine.lock();
            engine.project().multicam_group_span(group)?
        };
        let at = self.store.playhead_ticks.max(0);
        let target = self.drop_target_track(aria_models::TrackKind::Video, None)?;
        let end = at + source.duration.value;
        let occupied = self
            .spans_on(target)
            .into_iter()
            .any(|(from, to)| from < end && at < to);
        let track = if occupied {
            self.add_lane_of_kind(aria_models::TrackKind::Video, Some(0))?
        } else {
            target
        };
        let start = aria_models::RationalTime::new(at, self.playhead.rate);
        self.apply_command(Command::Edit(EditCommand::AddMulticamClip {
            track,
            group,
            source,
            start,
        }))?;
        Ok(())
    }

    /// Multicam groups as the projection carries them, newest last.
    ///
    /// A handle: the strip draws it without a borrow of the store and without
    /// copying every group on every frame it is open.
    pub fn multicam_groups(&self) -> std::sync::Arc<[crate::types::UiMulticamGroup]> {
        std::sync::Arc::clone(&self.store.multicam)
    }

    /// Move one angle to an absolute offset on its group's clock.
    pub fn set_multicam_angle_offset(
        &mut self,
        group: MulticamGroupId,
        angle: usize,
        offset: i64,
    ) -> Result<(), EngineError> {
        self.apply_command(Command::Edit(EditCommand::SetMulticamSync {
            group,
            angle,
            offset,
        }))?;
        Ok(())
    }

    /// Move one angle by `frames` frames of the sequence rate.
    ///
    /// The offset it moves from is the projected one, which is the value the
    /// operator is looking at: reading it back out of the engine would let a
    /// nudge act on a state the sidebar never drew.
    pub fn nudge_multicam_angle_offset(
        &mut self,
        group: MulticamGroupId,
        angle: usize,
        frames: i64,
    ) -> Result<(), EngineError> {
        let Some(current) = self
            .multicam_groups()
            .iter()
            .find(|row| row.id == group.raw())
            .and_then(|row| row.angles.get(angle))
            .map(|angle| angle.sync_offset)
        else {
            return Ok(());
        };
        self.set_multicam_angle_offset(group, angle, current + frames)
    }

    /// Place every camera of `group` against its first camera, measured from
    /// the sound they recorded.
    ///
    /// One history entry for the whole group: a half-synced group is worse than
    /// an unsynced one, because the offsets that did land have to be undone one
    /// at a time. The peaks are the ones the timeline decodes, so this reads
    /// what is already there and asks for no decode of its own; an angle whose
    /// file has not been read yet is reported rather than waited for.
    pub fn sync_multicam_group_by_audio(
        &mut self,
        group: MulticamGroupId,
    ) -> Result<(), EngineError> {
        let ticks_per_sample = |per_second: f64| {
            if per_second <= 0.0 {
                0.0
            } else {
                self.store.sequence.frame_rate.as_f64() / per_second
            }
        };
        let measurement = {
            // The handle is bound first: a row borrowed straight out of the
            // temporary would outlive it.
            let groups = self.multicam_groups();
            let Some(row) = groups.iter().find(|row| row.id == group.raw()) else {
                return Ok(());
            };
            let sounds: Vec<Result<&[f32], Decline>> = row
                .angles
                .iter()
                .map(|angle| match self.waveforms.get(angle.media) {
                    Some(waveform) => Ok(waveform.peaks.as_slice()),
                    None if self.waveforms.refused(angle.media) => Err(Decline::NoAngleSound),
                    None => Err(Decline::AnglesStillDecoding),
                })
                .collect();
            let per_second = row
                .angles
                .first()
                .and_then(|angle| self.waveforms.get(angle.media))
                .map_or(0.0, |waveform| waveform.per_second);
            let reference_offset = row.angles.first().map_or(0, |angle| angle.sync_offset);
            multicam_sync::measure(&sounds, reference_offset, ticks_per_sample(per_second))
        };
        let measurement = match measurement {
            Ok(measurement) => measurement,
            Err(why) => {
                self.decline(why);
                return Ok(());
            }
        };
        let moved: Vec<(usize, i64)> = measurement
            .moved
            .iter()
            .map(|measured| (measured.angle, measured.offset))
            .collect();
        self.engine.lock().begin_group();
        let result = moved
            .iter()
            .try_for_each(|(angle, offset)| self.set_multicam_angle_offset(group, *angle, *offset));
        match result {
            Ok(()) => {
                self.engine.lock().commit_group();
                self.sync_from_engine();
                let placed = moved.len();
                self.status_message = Some(match measurement.refused {
                    Some(why) => format!("Synced {placed} angle(s): {}", why.reason()),
                    None => format!("Synced {placed} angle(s) by sound"),
                });
                Ok(())
            }
            Err(err) => {
                // A group with some cameras moved and some not is out of sync in
                // a way the operator cannot see, so nothing lands.
                self.engine.lock().rollback_group();
                self.sync_from_engine();
                Err(err)
            }
        }
    }
}
#[cfg(test)]
#[path = "multicam_tests.rs"]
mod tests;
