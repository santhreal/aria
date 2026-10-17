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

/// Set (or clear) the clip type a clip is. Accepted on every clip kind, and
/// changes no duration or picture: the type's generator is an ordinary effect
/// in the chain. The inverse is a full-clip restore.
pub fn set_clip_type(
    ctx: &mut ApplyContext<'_>,
    clip: ClipId,
    clip_type: Option<String>,
) -> Result<Box<dyn EditAction>, EngineError> {
    let before = ctx
        .project
        .clip(clip)
        .cloned()
        .ok_or(ModelError::UnknownClip(clip))?;
    ctx.project.set_clip_type(clip, clip_type)?;
    Ok(Box::new(RestoreClipAction { clip: before }))
}

/// Set (or clear) the property a clip's bar draws as its line. Changes no
/// duration, picture, or sound. The inverse is a full-clip restore.
pub fn set_clip_line_param(
    ctx: &mut ApplyContext<'_>,
    clip: ClipId,
    param: Option<aria_models::ClipParam>,
) -> Result<Box<dyn EditAction>, EngineError> {
    let before = ctx
        .project
        .clip(clip)
        .cloned()
        .ok_or(ModelError::UnknownClip(clip))?;
    ctx.project.set_clip_line_param(clip, param)?;
    Ok(Box::new(RestoreClipAction { clip: before }))
}
