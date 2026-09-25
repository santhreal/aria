//! Pool folders: create, rename, move, delete, and re-file a source.
//!
//! Every one of these is a single field write on the project, so each inverse is
//! the same write with the captured value. The one that is not symmetric is
//! deletion: a deleted bin promotes its media and its child bins to its parent,
//! so undoing it has to put the folder back **and** put its contents back into
//! it. The delete captures both lists rather than recomputing them, because by
//! the time the undo runs the operator may have moved other things around and a
//! recomputed list would claim sources that were never in the folder.

use aria_models::{Bin, BinId, MediaId, MediaSource, PoolPosition, TimeRange};

use crate::action::{ApplyContext, EditAction};
use crate::error::EngineError;

/// Create a folder. The inverse deletes it.
pub fn create_bin(
    ctx: &mut ApplyContext<'_>,
    name: String,
    parent: Option<BinId>,
) -> Result<(BinId, Box<dyn EditAction>), EngineError> {
    let id = ctx.project.add_bin(name, parent)?;
    Ok((id, Box::new(DropBinAction { bin: id })))
}

/// Rename a folder. The inverse restores the prior name.
pub fn rename_bin(
    ctx: &mut ApplyContext<'_>,
    bin: BinId,
    name: String,
) -> Result<Box<dyn EditAction>, EngineError> {
    let before = ctx
        .project
        .bin(bin)
        .ok_or(aria_models::ModelError::UnknownBin(bin))?
        .name
        .clone();
    ctx.project.rename_bin(bin, name)?;
    Ok(Box::new(RenameBinAction { bin, name: before }))
}

/// Move a folder. The inverse puts it back under its previous parent.
pub fn move_bin(
    ctx: &mut ApplyContext<'_>,
    bin: BinId,
    parent: Option<BinId>,
) -> Result<Box<dyn EditAction>, EngineError> {
    let before = ctx
        .project
        .bin(bin)
        .ok_or(aria_models::ModelError::UnknownBin(bin))?
        .parent;
    ctx.project.move_bin(bin, parent)?;
    Ok(Box::new(MoveBinAction {
        bin,
        parent: before,
    }))
}

/// Delete a folder, promoting what was inside it. The inverse restores the
/// folder and re-files exactly what it held.
pub fn remove_bin(
    ctx: &mut ApplyContext<'_>,
    bin: BinId,
) -> Result<Box<dyn EditAction>, EngineError> {
    let children: Vec<BinId> = ctx
        .project
        .bin_iter()
        .filter(|item| item.parent == Some(bin))
        .map(|item| item.id)
        .collect();
    let media: Vec<MediaId> = ctx
        .project
        .media_iter()
        .filter(|item| item.bin == Some(bin))
        .map(|item| item.id)
        .collect();
    let removed = ctx.project.remove_bin(bin)?;
    Ok(Box::new(RestoreBinAction {
        bin: removed,
        children,
        media,
    }))
}

/// File a source in a folder. The inverse puts it back where it was.
pub fn move_media_to_bin(
    ctx: &mut ApplyContext<'_>,
    media: MediaId,
    bin: Option<BinId>,
) -> Result<Box<dyn EditAction>, EngineError> {
    let before = ctx.project.move_media_to_bin(media, bin)?;
    Ok(Box::new(MoveMediaToBinAction { media, bin: before }))
}

/// Create a subclip in the pool from a range of parent media. The inverse removes it.
pub fn create_subclip(
    ctx: &mut ApplyContext<'_>,
    parent: MediaId,
    name: String,
    source: TimeRange,
    bin: Option<BinId>,
) -> Result<Box<dyn EditAction>, EngineError> {
    let id = ctx.project.create_subclip(parent, name, source, bin)?;
    Ok(Box::new(DropSubclipAction { media: id }))
}

struct DropBinAction {
    bin: BinId,
}

impl EditAction for DropBinAction {
    fn apply(
        self: Box<Self>,
        ctx: &mut ApplyContext<'_>,
    ) -> Result<Box<dyn EditAction>, EngineError> {
        remove_bin(ctx, self.bin)
    }
}

struct RenameBinAction {
    bin: BinId,
    name: String,
}

impl EditAction for RenameBinAction {
    fn apply(
        self: Box<Self>,
        ctx: &mut ApplyContext<'_>,
    ) -> Result<Box<dyn EditAction>, EngineError> {
        rename_bin(ctx, self.bin, self.name)
    }
}

struct MoveBinAction {
    bin: BinId,
    parent: Option<BinId>,
}

impl EditAction for MoveBinAction {
    fn apply(
        self: Box<Self>,
        ctx: &mut ApplyContext<'_>,
    ) -> Result<Box<dyn EditAction>, EngineError> {
        move_bin(ctx, self.bin, self.parent)
    }
}

struct MoveMediaToBinAction {
    media: MediaId,
    bin: Option<BinId>,
}

impl EditAction for MoveMediaToBinAction {
    fn apply(
        self: Box<Self>,
        ctx: &mut ApplyContext<'_>,
    ) -> Result<Box<dyn EditAction>, EngineError> {
        move_media_to_bin(ctx, self.media, self.bin)
    }
}

/// Place a pool item at `(x, y)` in `bin` (or at the pool root with `None`).
pub fn set_pool_item_position(
    ctx: &mut ApplyContext<'_>,
    media: MediaId,
    bin: Option<BinId>,
    x: f32,
    y: f32,
) -> Result<Box<dyn EditAction>, EngineError> {
    let before = ctx.project.pool_item_position(bin, media);
    ctx.project
        .set_pool_item_position(bin, media, PoolPosition::new(x, y))?;
    Ok(Box::new(SetPoolItemPositionAction {
        media,
        bin,
        position: before,
    }))
}

struct SetPoolItemPositionAction {
    media: MediaId,
    bin: Option<BinId>,
    position: Option<PoolPosition>,
}

impl EditAction for SetPoolItemPositionAction {
    fn apply(
        self: Box<Self>,
        ctx: &mut ApplyContext<'_>,
    ) -> Result<Box<dyn EditAction>, EngineError> {
        let before = ctx.project.pool_item_position(self.bin, self.media);
        if let Some(pos) = self.position {
            ctx.project
                .set_pool_item_position(self.bin, self.media, pos)?;
        } else {
            ctx.project
                .remove_pool_item_position(self.bin, self.media)?;
        }
        Ok(Box::new(SetPoolItemPositionAction {
            media: self.media,
            bin: self.bin,
            position: before,
        }))
    }
}
struct RestoreBinAction {
    bin: Bin,
    children: Vec<BinId>,
    media: Vec<MediaId>,
}

impl EditAction for RestoreBinAction {
    fn apply(
        self: Box<Self>,
        ctx: &mut ApplyContext<'_>,
    ) -> Result<Box<dyn EditAction>, EngineError> {
        let id = self.bin.id;
        ctx.project.restore_bin(self.bin)?;
        // A child that has since been moved elsewhere stays where it is: the
        // undo restores the folder's contents, not the operator's later edits.
        for child in self.children {
            if ctx.project.bin(child).is_some() {
                ctx.project.move_bin(child, Some(id))?;
            }
        }
        for media in self.media {
            if ctx.project.media(media).is_some() {
                ctx.project.move_media_to_bin(media, Some(id))?;
            }
        }
        Ok(Box::new(DropBinAction { bin: id }))
    }
}

struct DropSubclipAction {
    media: MediaId,
}

impl EditAction for DropSubclipAction {
    fn apply(
        self: Box<Self>,
        ctx: &mut ApplyContext<'_>,
    ) -> Result<Box<dyn EditAction>, EngineError> {
        let removed = ctx.project.remove_media(self.media)?;
        Ok(Box::new(RestoreSubclipAction { media: removed }))
    }
}

struct RestoreSubclipAction {
    media: MediaSource,
}

impl EditAction for RestoreSubclipAction {
    fn apply(
        self: Box<Self>,
        ctx: &mut ApplyContext<'_>,
    ) -> Result<Box<dyn EditAction>, EngineError> {
        let id = self.media.id;
        ctx.project.add_media(self.media);
        Ok(Box::new(DropSubclipAction { media: id }))
    }
}
