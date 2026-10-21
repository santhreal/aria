//! Sequence markers on the ruler, and markers attached to clips.

use super::{Marker, MarkerColor, MarkerKind, Timeline};
use crate::error::ModelError;
use crate::ids::{ClipId, MarkerId};
use crate::time::{RationalTime, resample};
use crate::track::Track;

impl Timeline {
    /// Ruler markers in `(tick, id)` order.
    pub fn markers(&self) -> &[Marker] {
        &self.markers
    }

    /// Find a marker by id across both sequence ruler markers and clip markers.
    pub fn marker(&self, id: MarkerId) -> Option<&Marker> {
        self.markers.iter().find(|m| m.id == id).or_else(|| {
            self.tracks
                .values()
                .flat_map(Track::clips)
                .find_map(|clip| clip.marker(id))
        })
    }

    /// Return the owning [`ClipId`] when `id` is a clip marker, or `None` when
    /// it is a sequence marker (or absent).
    pub fn marker_clip(&self, id: MarkerId) -> Option<ClipId> {
        self.tracks
            .values()
            .flat_map(Track::clips)
            .find_map(|clip| clip.marker(id).map(|_| clip.id))
    }

    /// Total number of markers in the sequence (ruler markers plus clip markers).
    pub fn marker_count(&self) -> usize {
        self.markers.len()
            + self
                .tracks
                .values()
                .flat_map(Track::clips)
                .map(|clip| clip.markers.len())
                .sum::<usize>()
    }

    /// Insert a sequence marker on the ruler, keeping `(tick, id)` order. The
    /// tick and duration are resampled to the timeline rate so every stored
    /// marker shares it. Rejects negative positions, negative durations, range
    /// durations that run past [`Self::duration`], and duplicate ids.
    pub fn add_marker(&mut self, mut marker: Marker) -> Result<MarkerId, ModelError> {
        if marker.tick.value < 0 || !marker.tick.rate.is_valid() || !marker.duration.rate.is_valid()
        {
            return Err(ModelError::InvalidRange);
        }
        if marker.duration.value < 0 {
            return Err(ModelError::InvalidParam(format!(
                "marker duration must be non-negative (got {})",
                marker.duration.value
            )));
        }
        if self.marker(marker.id).is_some() {
            return Err(ModelError::InvalidRange);
        }
        marker.tick = resample(marker.tick, self.frame_rate);
        marker.duration = resample(marker.duration, self.frame_rate);
        if marker.duration.value > 0 {
            let seq_end = self.duration().value;
            let marker_end = marker.tick.value + marker.duration.value;
            if marker_end > seq_end {
                return Err(ModelError::InvalidParam(format!(
                    "marker range end ({marker_end}) exceeds sequence end ({seq_end})"
                )));
            }
        }
        let id = marker.id;
        let at = self
            .markers
            .partition_point(|m| (m.tick.value, m.id) <= (marker.tick.value, marker.id));
        self.markers.insert(at, marker);
        Ok(id)
    }

    /// Insert a marker on `clip_id` in clip time, keeping `(tick, id)` order
    /// and validating against the clip's source range.
    pub fn add_clip_marker(
        &mut self,
        clip_id: ClipId,
        marker: Marker,
    ) -> Result<MarkerId, ModelError> {
        if self.marker(marker.id).is_some() {
            return Err(ModelError::InvalidRange);
        }
        let clip = self.require_clip_mut(clip_id)?;
        clip.add_marker(marker)
    }

    /// Attach an existing sequence marker at its timeline tick onto `clip_id`
    /// in clip time.
    pub fn attach_marker_to_clip(
        &mut self,
        id: MarkerId,
        clip_id: ClipId,
    ) -> Result<(), ModelError> {
        let index = self
            .markers
            .iter()
            .position(|m| m.id == id)
            .ok_or(ModelError::UnknownMarker(id))?;
        let marker = self.markers[index].clone();
        let clip = self.require_clip(clip_id)?;
        let clip_tick = clip
            .clip_time_at_timeline_tick(marker.tick.value)
            .ok_or_else(|| {
                ModelError::InvalidParam(format!(
                    "marker tick ({}) is outside clip {clip_id:?} timeline span",
                    marker.tick.value
                ))
            })?;
        let clip_duration = resample(marker.duration, clip_tick.rate);
        let mut clip_marker = marker.clone();
        clip_marker.tick = clip_tick;
        clip_marker.duration = clip_duration;
        self.markers.remove(index);
        if let Err(err) = self
            .clip_mut(clip_id)
            .expect("clip checked above")
            .add_marker(clip_marker)
        {
            let restored = self.add_marker(marker);
            debug_assert!(restored.is_ok(), "re-inserting sequence marker");
            return Err(err);
        }
        Ok(())
    }

    /// Remove a marker by id (from the sequence ruler or from a clip),
    /// returning it for undo capture.
    pub fn remove_marker(&mut self, id: MarkerId) -> Option<Marker> {
        if let Some(index) = self.markers.iter().position(|m| m.id == id) {
            return Some(self.markers.remove(index));
        }
        let clip_id = self.marker_clip(id)?;
        self.clip_mut(clip_id)?.remove_marker(id)
    }

    /// Move / rename / recolor / re-annotate a marker in one shot (preserving
    /// its existing `duration` and `kind`).
    pub fn set_marker(
        &mut self,
        id: MarkerId,
        tick: RationalTime,
        name: String,
        color: MarkerColor,
        comment: String,
    ) -> Result<(), ModelError> {
        let current = self.marker(id).ok_or(ModelError::UnknownMarker(id))?;
        let duration = current.duration;
        let kind = current.kind;
        self.edit_marker(id, tick, duration, name, color, comment, kind)
    }

    /// Update all fields (`tick`, `duration`, `name`, `color`, `comment`,
    /// `kind`) of a sequence or clip marker in one atomic operation.
    #[allow(clippy::too_many_arguments)]
    pub fn edit_marker(
        &mut self,
        id: MarkerId,
        tick: RationalTime,
        duration: RationalTime,
        name: String,
        color: MarkerColor,
        comment: String,
        kind: MarkerKind,
    ) -> Result<(), ModelError> {
        if let Some(clip_id) = self.marker_clip(id) {
            return self
                .require_clip_mut(clip_id)?
                .edit_marker(id, tick, duration, name, color, comment, kind);
        }
        if tick.value < 0 || !tick.rate.is_valid() || !duration.rate.is_valid() {
            return Err(ModelError::InvalidRange);
        }
        if duration.value < 0 {
            return Err(ModelError::InvalidParam(format!(
                "marker duration must be non-negative (got {})",
                duration.value
            )));
        }
        let mut marker = self
            .remove_marker(id)
            .ok_or(ModelError::UnknownMarker(id))?;
        let before = marker.clone();
        marker.tick = tick;
        marker.duration = duration;
        marker.name = name;
        marker.color = color;
        marker.comment = comment;
        marker.kind = kind;
        match self.add_marker(marker) {
            Ok(_) => Ok(()),
            Err(e) => {
                // Validation failed: put the original back untouched.
                let restored = self.add_marker(before);
                debug_assert!(restored.is_ok(), "re-inserting the removed marker");
                Err(e)
            }
        }
    }
}
