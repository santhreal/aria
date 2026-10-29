//! Edit > Remove Attributes on one clip, as one atomic edit.

use aria_models::{ClipAttribute, ClipId, ModelError};

use crate::action::edit::restore_clip::RestoreClipAction;
use crate::action::{ApplyContext, EditAction};
use crate::error::EngineError;

/// Return each group in `attributes` on `clip` to a new clip's values.
///
/// The inverse is one full snapshot of the clip, so undo brings back every
/// group and the duration a cleared ramp re-derived. A refusal restores the
/// snapshot, so a half-removed set is never left behind.
pub(crate) fn execute(
    ctx: &mut ApplyContext<'_>,
    clip: ClipId,
    attributes: &[ClipAttribute],
) -> Result<Box<dyn EditAction>, EngineError> {
    let before = ctx
        .project
        .clip(clip)
        .cloned()
        .ok_or(ModelError::UnknownClip(clip))?;
    if let Err(error) = ctx.project.remove_clip_attributes(clip, attributes) {
        *ctx.project.timeline_mut().require_clip_mut(clip)? = before;
        return Err(error.into());
    }
    Ok(Box::new(RestoreClipAction { clip: before }))
}
