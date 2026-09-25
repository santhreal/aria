use aria_models::{ClipId, ModelError, TimeRange};

use crate::action::edit::restore_clip::RestoreClipAction;
use crate::action::{ApplyContext, EditAction};
use crate::error::EngineError;

pub fn execute(
    ctx: &mut ApplyContext<'_>,
    clip: ClipId,
    timeline: TimeRange,
) -> Result<Box<dyn EditAction>, EngineError> {
    let before = ctx
        .project
        .clip(clip)
        .cloned()
        .ok_or(ModelError::UnknownClip(clip))?;
    ctx.project.trim_clip(clip, timeline)?;
    Ok(Box::new(RestoreClipAction { clip: before }))
}
