//! Mask stack edits. All four take a full-clip snapshot as their inverse, the
//! `set_effect` shape: removing or moving a mask also moves the drivers on its
//! properties, and the snapshot restores both.

use aria_models::{ClipId, Mask, ModelError};

use crate::action::edit::restore_clip::RestoreClipAction;
use crate::action::{ApplyContext, EditAction};
use crate::error::EngineError;

pub fn add_mask(
    ctx: &mut ApplyContext<'_>,
    clip: ClipId,
    mask: Mask,
) -> Result<Box<dyn EditAction>, EngineError> {
    let before = snapshot(ctx, clip)?;
    ctx.project.add_clip_mask(clip, mask)?;
    Ok(Box::new(RestoreClipAction { clip: before }))
}

pub fn set_mask(
    ctx: &mut ApplyContext<'_>,
    clip: ClipId,
    index: u32,
    mask: Mask,
) -> Result<Box<dyn EditAction>, EngineError> {
    let before = snapshot(ctx, clip)?;
    ctx.project.set_clip_mask(clip, index, mask)?;
    Ok(Box::new(RestoreClipAction { clip: before }))
}

pub fn remove_mask(
    ctx: &mut ApplyContext<'_>,
    clip: ClipId,
    index: u32,
) -> Result<Box<dyn EditAction>, EngineError> {
    let before = snapshot(ctx, clip)?;
    ctx.project.remove_clip_mask(clip, index)?;
    Ok(Box::new(RestoreClipAction { clip: before }))
}

pub fn move_mask(
    ctx: &mut ApplyContext<'_>,
    clip: ClipId,
    from_index: u32,
    to_index: u32,
) -> Result<Box<dyn EditAction>, EngineError> {
    let before = snapshot(ctx, clip)?;
    ctx.project.move_clip_mask(clip, from_index, to_index)?;
    Ok(Box::new(RestoreClipAction { clip: before }))
}

fn snapshot(ctx: &ApplyContext<'_>, clip: ClipId) -> Result<aria_models::Clip, EngineError> {
    Ok(ctx
        .project
        .clip(clip)
        .cloned()
        .ok_or(ModelError::UnknownClip(clip))?)
}
