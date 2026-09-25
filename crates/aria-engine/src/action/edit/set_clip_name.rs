use aria_models::{ClipId, ModelError};

use crate::action::edit::restore_clip::RestoreClipAction;
use crate::action::{ApplyContext, EditAction};
use crate::error::EngineError;

/// Set (or clear) a clip's operator-given name. Accepted on every clip kind — a
/// name is what the operator calls the clip, not a property of the content — and
/// changes no duration, so there is no neighbour check. The inverse is a
/// full-clip restore, like the other flag edits.
pub fn set_clip_name(
    ctx: &mut ApplyContext<'_>,
    clip: ClipId,
    name: Option<String>,
) -> Result<Box<dyn EditAction>, EngineError> {
    let before = ctx
        .project
        .clip(clip)
        .cloned()
        .ok_or(ModelError::UnknownClip(clip))?;
    ctx.project.set_clip_name(clip, name)?;
    Ok(Box::new(RestoreClipAction { clip: before }))
}
