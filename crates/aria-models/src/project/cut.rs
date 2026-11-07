//! Whether a clip can be cut at a timeline tick.

use crate::clip::CutRule;
use crate::error::ModelError;
use crate::ids::ClipId;

use super::Project;

impl Project {
    /// Whether `clip_id` can be cut at timeline tick `at`: the tick lies
    /// strictly inside the clip, and the two parts render what the whole clip
    /// rendered, or a [`ModelError::Split`] states why not.
    ///
    /// [`Project::split_clip`] applies it before its first change, and so do
    /// the freeze check and the rows that cut, so a row is offered exactly
    /// where its cut applies.
    pub fn split_check(&self, clip_id: ClipId, at: i64) -> Result<(), ModelError> {
        let clip = self
            .timeline()
            .clip(clip_id)
            .ok_or(ModelError::UnknownClip(clip_id))?;
        let start = clip.timeline.start.value;
        CutRule::of(clip, self.timeline().frame_rate)
            .check(at - start, clip.timeline.duration.value)
    }
}
