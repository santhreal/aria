//! Atomic clipboard look transfer.

use aria_models::{Clip, ClipId, ModelError};

use crate::action::edit::restore_clip::RestoreClipAction;
use crate::action::{ApplyContext, EditAction};
use crate::error::EngineError;

/// Copy `copy`'s animated transform, filter, and effect chain to `target`.
///
/// Every validation and mutation occurs under one engine command. The inverse is
/// one full target snapshot, so undo cannot expose a half-pasted look.
pub(crate) fn execute(
    ctx: &mut ApplyContext<'_>,
    copy: Clip,
    target: ClipId,
) -> Result<Box<dyn EditAction>, EngineError> {
    let before = ctx
        .project
        .clip(target)
        .cloned()
        .ok_or(ModelError::UnknownClip(target))?;

    let result = (|| {
        ctx.project.set_animated_transform(target, copy.transform)?;
        ctx.project.set_clip_filter(target, copy.filter)?;
        ctx.project.set_clip_effects(target, copy.effects)?;
        Ok::<(), ModelError>(())
    })();
    if let Err(error) = result {
        *ctx.project.timeline_mut().require_clip_mut(target)? = before;
        return Err(error.into());
    }

    Ok(Box::new(RestoreClipAction { clip: before }))
}
