//! Edit > Remove Attributes on one clip.

use crate::{ClipAttribute, ClipId, ModelError, Project};

impl Project {
    /// Return each group in `attributes` to the values a new clip starts with,
    /// keyframes and drivers included.
    ///
    /// The speed ramp is cleared first, through
    /// [`Self::set_clip_speed_curve`], because that re-derives the clip's
    /// duration and fails when the longer clip would overlap a neighbour; on
    /// that failure nothing else has changed. A group the clip does not hold is
    /// left as it is.
    pub fn remove_clip_attributes(
        &mut self,
        clip_id: ClipId,
        attributes: &[ClipAttribute],
    ) -> Result<(), ModelError> {
        let clip = self
            .timeline()
            .clip(clip_id)
            .ok_or(ModelError::UnknownClip(clip_id))?;
        if attributes.contains(&ClipAttribute::TimeRemapping)
            && ClipAttribute::TimeRemapping.is_set(clip)
        {
            self.set_clip_speed_curve(clip_id, None)?;
        }
        let clip = self
            .timeline_mut()
            .clip_mut(clip_id)
            .ok_or(ModelError::UnknownClip(clip_id))?;
        for attribute in attributes {
            attribute.clear(clip);
        }
        Ok(())
    }
}
