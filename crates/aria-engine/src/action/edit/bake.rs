use aria_models::{ClipId, MediaId, ModelError};

use crate::action::edit::restore_clip::RestoreClipAction;
use crate::action::{ApplyContext, EditAction};
use crate::error::EngineError;

/// Swap a clip for a rendered intermediate (render and replace).
///
/// The model owns the placement rules and the snapshot; this only records the
/// inverse. A whole-clip restore is the right inverse here for the same reason
/// it is for the audio and pitch edits: the edit rewrites most of the clip, and
/// the clip it rewrote is small enough to keep.
pub fn bake_clip(
    ctx: &mut ApplyContext<'_>,
    clip: ClipId,
    media: MediaId,
) -> Result<Box<dyn EditAction>, EngineError> {
    let before = ctx
        .project
        .clip(clip)
        .cloned()
        .ok_or(ModelError::UnknownClip(clip))?;
    ctx.project.bake_clip(clip, media)?;
    Ok(Box::new(RestoreClipAction { clip: before }))
}

/// Put back what a bake replaced.
pub fn restore_baked_clip(
    ctx: &mut ApplyContext<'_>,
    clip: ClipId,
) -> Result<Box<dyn EditAction>, EngineError> {
    let before = ctx
        .project
        .clip(clip)
        .cloned()
        .ok_or(ModelError::UnknownClip(clip))?;
    ctx.project.restore_baked_clip(clip)?;
    Ok(Box::new(RestoreClipAction { clip: before }))
}
