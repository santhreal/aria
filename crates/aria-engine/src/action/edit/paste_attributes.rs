//! Atomic clipboard look transfer.

use aria_models::{ClipId, ModelError};

use crate::action::edit::restore_clip::RestoreClipAction;
use crate::action::{ApplyContext, EditAction};
use crate::error::EngineError;

/// Copy the source's animated transform, filter, and effect chain to `target`.
///
/// Every validation and mutation occurs under one engine command. The inverse is
/// one full target snapshot, so undo cannot expose a half-pasted look.
pub(crate) fn execute(
    ctx: &mut ApplyContext<'_>,
    source: ClipId,
    target: ClipId,
) -> Result<Box<dyn EditAction>, EngineError> {
    let source = ctx
        .project
        .clip(source)
        .cloned()
        .ok_or(ModelError::UnknownClip(source))?;
    let before = ctx
        .project
        .clip(target)
        .cloned()
        .ok_or(ModelError::UnknownClip(target))?;

    let result = (|| {
        ctx.project
            .set_animated_transform(target, source.transform.clone())?;
        ctx.project.set_clip_filter(target, source.filter.clone())?;
        ctx.project
            .set_clip_effects(target, source.effects.clone())?;
        Ok::<(), ModelError>(())
    })();
    if let Err(error) = result {
        *ctx.project.timeline_mut().require_clip_mut(target)? = before;
        return Err(error.into());
    }

    Ok(Box::new(RestoreClipAction { clip: before }))
}
