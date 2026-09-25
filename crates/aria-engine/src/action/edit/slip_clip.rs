use aria_models::{ClipId, ModelError};

use crate::action::edit::restore_clip::RestoreClipAction;
use crate::action::{ApplyContext, EditAction};
use crate::error::EngineError;

/// Move a clip's source window without moving the clip.
/// The inverse restores the whole clip rather than slipping back by `-by`: a
/// slip clamps at the ends of the take, so the opposite slip is not always the
/// same distance.
pub fn execute(
    ctx: &mut ApplyContext<'_>,
    clip: ClipId,
    by: i64,
) -> Result<Box<dyn EditAction>, EngineError> {
    let before = ctx
        .project
        .clip(clip)
        .cloned()
        .ok_or(ModelError::UnknownClip(clip))?;
    ctx.project.slip_clip(clip, by)?;
    Ok(Box::new(RestoreClipAction { clip: before }))
}
