use aria_models::{ClipId, ModelError, ParentKeep, RationalTime};

use crate::action::edit::restore_clip::RestoreClipAction;
use crate::action::{ApplyContext, EditAction};
use crate::error::EngineError;

/// Parent `clip` to `parent`, or with `None` release it, keeping its place on
/// the canvas at `at` or snapping it to the parent as `keep` selects. The
/// transform rewrite is the renderer's ([`aria_render::reparented`]), so the
/// place kept is the place drawn. The inverse is a full-clip restore.
pub(crate) fn execute(
    ctx: &mut ApplyContext<'_>,
    clip: ClipId,
    parent: Option<ClipId>,
    at: RationalTime,
    keep: ParentKeep,
) -> Result<Box<dyn EditAction>, EngineError> {
    let before = ctx
        .project
        .clip(clip)
        .cloned()
        .ok_or(ModelError::UnknownClip(clip))?;
    let transform = aria_render::reparented(ctx.project, clip, parent, at, keep)?;
    ctx.project.set_clip_parent(clip, parent, transform)?;
    Ok(Box::new(RestoreClipAction { clip: before }))
}
