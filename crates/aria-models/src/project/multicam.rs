//! Project-level multicam group operations and clip creation.

use super::Project;
use crate::clip::Clip;
use crate::error::ModelError;
use crate::ids::{ClipId, MediaId, MulticamGroupId, TrackId};
use crate::multicam::{MulticamAngle, MulticamGroup};
use crate::time::{RationalTime, TimeRange, check_same_rate, resample};

impl Project {
    /// Create a new multicam group and register it in the project.
    pub fn create_multicam_group(
        &mut self,
        name: impl Into<String>,
        angles: Vec<MulticamAngle>,
    ) -> Result<MulticamGroupId, ModelError> {
        for angle in &angles {
            if !self.media.contains_key(&angle.media) {
                return Err(ModelError::UnknownMedia(angle.media));
            }
        }
        let group = MulticamGroup::new(name, angles)?;
        let id = group.id;
        self.multicam_groups.insert(id, group);
        Ok(id)
    }

    /// Add an existing multicam group structure directly to the project.
    pub fn add_multicam_group(
        &mut self,
        group: MulticamGroup,
    ) -> Result<MulticamGroupId, ModelError> {
        for angle in &group.angles {
            if !self.media.contains_key(&angle.media) {
                return Err(ModelError::UnknownMedia(angle.media));
            }
        }
        group.validate()?;
        let id = group.id;
        self.multicam_groups.insert(id, group);
        Ok(id)
    }

    /// Access a multicam group by ID.
    pub fn multicam_group(&self, id: MulticamGroupId) -> Option<&MulticamGroup> {
        self.multicam_groups.get(&id)
    }

    /// Mutably access a multicam group by ID.
    pub fn multicam_group_mut(&mut self, id: MulticamGroupId) -> Option<&mut MulticamGroup> {
        self.multicam_groups.get_mut(&id)
    }

    /// Iterator over all multicam groups in the project.
    pub fn multicam_group_iter(&self) -> impl Iterator<Item = &MulticamGroup> {
        self.multicam_groups.values()
    }

    /// Total count of multicam groups in the project.
    pub fn multicam_group_count(&self) -> usize {
        self.multicam_groups.len()
    }

    /// Whether any clip on the timeline currently references `group_id`.
    pub fn is_multicam_group_referenced(&self, group_id: MulticamGroupId) -> bool {
        self.timeline()
            .tracks_ordered()
            .flat_map(crate::track::Track::clips)
            .any(|c| c.multicam_group() == Some(group_id))
    }

    /// Remove a multicam group. Refused if any clip still references it.
    pub fn remove_multicam_group(
        &mut self,
        id: MulticamGroupId,
    ) -> Result<MulticamGroup, ModelError> {
        if self.is_multicam_group_referenced(id) {
            return Err(ModelError::InvalidParam(format!(
                "multicam group {id} is still referenced by clips"
            )));
        }
        self.multicam_groups
            .remove(&id)
            .ok_or(ModelError::UnknownMulticamGroup(id))
    }

    /// Add an angle to a multicam group.
    pub fn add_multicam_angle(
        &mut self,
        id: MulticamGroupId,
        angle: MulticamAngle,
    ) -> Result<usize, ModelError> {
        if !self.media.contains_key(&angle.media) {
            return Err(ModelError::UnknownMedia(angle.media));
        }
        let group = self
            .multicam_groups
            .get_mut(&id)
            .ok_or(ModelError::UnknownMulticamGroup(id))?;
        group.add_angle(angle)
    }

    /// Remove an angle from a multicam group by index.
    pub fn remove_multicam_angle(
        &mut self,
        id: MulticamGroupId,
        index: usize,
    ) -> Result<MulticamAngle, ModelError> {
        let group = self
            .multicam_groups
            .get_mut(&id)
            .ok_or(ModelError::UnknownMulticamGroup(id))?;
        group.remove_angle(index)
    }

    /// Switch the active angle of a multicam group at a timeline tick.
    pub fn switch_multicam_angle(
        &mut self,
        id: MulticamGroupId,
        at: i64,
        angle: usize,
    ) -> Result<(), ModelError> {
        let group = self
            .multicam_groups
            .get_mut(&id)
            .ok_or(ModelError::UnknownMulticamGroup(id))?;
        group.switch_angle(at, angle)
    }

    /// Move one angle on the group's common clock, returning the offset it
    /// replaced.
    ///
    /// The previous value comes back because the offset is what an undo has to
    /// restore, and reading it separately would let an edit and its inverse
    /// disagree about what was there.
    pub fn set_multicam_angle_offset(
        &mut self,
        id: MulticamGroupId,
        index: usize,
        sync_offset: i64,
    ) -> Result<i64, ModelError> {
        let group = self
            .multicam_groups
            .get_mut(&id)
            .ok_or(ModelError::UnknownMulticamGroup(id))?;
        group.set_angle_offset(index, sync_offset)
    }

    /// Get the active angle index of a multicam group at a tick.
    pub fn active_multicam_angle_at(
        &self,
        id: MulticamGroupId,
        at: i64,
    ) -> Result<usize, ModelError> {
        let group = self
            .multicam_groups
            .get(&id)
            .ok_or(ModelError::UnknownMulticamGroup(id))?;
        Ok(group.active_angle_at(at))
    }

    /// Get the active media ID of a multicam group at a tick.
    pub fn active_multicam_media_at(
        &self,
        id: MulticamGroupId,
        at: i64,
    ) -> Result<MediaId, ModelError> {
        let group = self
            .multicam_groups
            .get(&id)
            .ok_or(ModelError::UnknownMulticamGroup(id))?;
        group
            .active_media_at(at)
            .ok_or(ModelError::EmptyMulticamGroup)
    }

    /// The span a placement of `id` covers on the group's own clock.
    ///
    /// From zero to the last frame any of the group's angles can still answer
    /// for. An angle's sync offset is where its first frame sits on that clock,
    /// so a camera that started late runs out later; taking the longest reach
    /// rather than the first angle's length is what keeps a cut to a longer
    /// camera from falling off the end of the clip.
    ///
    /// Expressed at the timeline's rate, because that is the clock the placement
    /// and the cut list are addressed in.
    pub fn multicam_group_span(&self, id: MulticamGroupId) -> Result<TimeRange, ModelError> {
        let group = self
            .multicam_groups
            .get(&id)
            .ok_or(ModelError::UnknownMulticamGroup(id))?;
        if group.angles.is_empty() {
            return Err(ModelError::EmptyMulticamGroup);
        }
        let rate = self.timeline().frame_rate;
        let mut end = 0i64;
        for angle in &group.angles {
            let media = self
                .media
                .get(&angle.media)
                .ok_or(ModelError::UnknownMedia(angle.media))?;
            let duration = resample(media.full_range().duration, rate).value;
            end = end.max(angle.sync_offset + duration);
        }
        if end <= 0 {
            return Err(ModelError::InvalidRange);
        }
        Ok(TimeRange::at_rate(0, end, rate))
    }

    /// Place a multicam clip referencing `group_id` on `track_id`.
    pub fn add_multicam_clip(
        &mut self,
        track_id: TrackId,
        group_id: MulticamGroupId,
        source: TimeRange,
        timeline_start: RationalTime,
    ) -> Result<ClipId, ModelError> {
        if !self.multicam_groups.contains_key(&group_id) {
            return Err(ModelError::UnknownMulticamGroup(group_id));
        }
        let tl_rate = self.timeline().frame_rate;
        check_same_rate(source.start.rate, tl_rate)?;
        check_same_rate(timeline_start.rate, tl_rate)?;
        if source.is_empty() {
            return Err(ModelError::InvalidRange);
        }
        let timeline_duration = resample(source.duration, tl_rate);
        let duration_ticks = timeline_duration.value.max(1);
        let timeline = TimeRange::at_rate(timeline_start.value, duration_ticks, tl_rate);

        let clip = Clip::from_multicam(group_id, source, timeline);
        self.timeline_mut().add_clip(track_id, clip)
    }
}
