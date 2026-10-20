use aria_models::{ClipId, LabelColor, ModelError};

use crate::action::edit::restore_clip::RestoreClipAction;
use crate::action::{ApplyContext, EditAction};
use crate::error::EngineError;

/// Set (or clear) a clip's label colour (Premiere label colours). Accepted on
/// every clip kind, since a label is an editorial grouping, not a property of
/// the content, and changes no duration, so there is no neighbour check. The
/// inverse is a full-clip restore, like the other flag edits.
pub fn set_label(
    ctx: &mut ApplyContext<'_>,
    clip: ClipId,
    label: Option<LabelColor>,
) -> Result<Box<dyn EditAction>, EngineError> {
    let before = ctx
        .project
        .clip(clip)
        .cloned()
        .ok_or(ModelError::UnknownClip(clip))?;
    ctx.project.set_clip_label(clip, label)?;
    Ok(Box::new(RestoreClipAction { clip: before }))
}
