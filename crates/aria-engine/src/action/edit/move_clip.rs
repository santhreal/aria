use aria_models::{ClipId, ModelError, RationalTime, TrackId};

use crate::action::{ApplyContext, CompoundAction, EditAction};
use crate::error::EngineError;

pub(crate) struct MoveClipAction {
    pub clip: ClipId,
    pub to_track: TrackId,
    pub start: RationalTime,
}

/// Move `clip` to `to_track` at `start`. Linked and grouped companions take
/// the same time delta and stay on their lanes, as one history entry.
pub(crate) fn execute(
    ctx: &mut ApplyContext<'_>,
    clip: ClipId,
    to_track: TrackId,
    start: RationalTime,
) -> Result<Box<dyn EditAction>, EngineError> {
    let ids = ctx.project.move_companions(clip);
    move_with(ctx, clip, to_track, start, ids)
}

/// Move `clip` to `to_track` at `start` without its links: clips grouped with
/// it take the same time delta, linked clips stay where they are.
pub(crate) fn execute_unlinked(
    ctx: &mut ApplyContext<'_>,
    clip: ClipId,
    to_track: TrackId,
    start: RationalTime,
) -> Result<Box<dyn EditAction>, EngineError> {
    if ctx.project.clip(clip).is_none() {
        return Err(ModelError::UnknownClip(clip).into());
    }
    let ids = ctx.project.group_companions(clip);
    move_with(ctx, clip, to_track, start, ids)
}

/// Move `clip` to `to_track` at `start` and every other clip of `ids` by the
/// same time delta on its own lane, as one history entry.
fn move_with(
    ctx: &mut ApplyContext<'_>,
    clip: ClipId,
    to_track: TrackId,
    start: RationalTime,
    mut ids: Vec<ClipId>,
) -> Result<Box<dyn EditAction>, EngineError> {
    let primary = ctx.project.require_clip(clip)?;
    let delta = start.value - primary.start().value;
    ids.sort_by_key(|&id| {
        let tick = ctx.project.clip(id).map_or(0, |c| c.start().value);
        if delta > 0 { -tick } else { tick }
    });

    if ids.len() == 1 {
        return move_one(ctx, clip, to_track, start);
    }

    let mut inverses: Vec<Box<dyn EditAction>> = Vec::new();
    for id in ids {
        let original = ctx.project.require_clip(id)?;
        let from_track = ctx
            .project
            .timeline()
            .track_of(id)
            .ok_or(ModelError::UnknownClip(id))?;
        let dest = if id == clip {
            start
        } else {
            RationalTime::new((original.start().value + delta).max(0), start.rate)
        };
        let dest_track = if id == clip { to_track } else { from_track };
        match move_one(ctx, id, dest_track, dest) {
            Ok(inverse) => inverses.push(inverse),
            Err(err) => {
                for inverse in inverses.into_iter().rev() {
                    if inverse.apply(ctx).is_err() {
                        tracing::error!("linked move rollback failed");
                        break;
                    }
                }
                return Err(err);
            }
        }
    }
    Ok(Box::new(CompoundAction { actions: inverses }))
}

/// Move `clip` alone: its linked companions stay where they are.
fn move_one(
    ctx: &mut ApplyContext<'_>,
    clip: ClipId,
    to_track: TrackId,
    start: RationalTime,
) -> Result<Box<dyn EditAction>, EngineError> {
    let from_track = ctx
        .project
        .timeline()
        .track_of(clip)
        .ok_or(ModelError::UnknownClip(clip))?;
    let from_start = ctx.project.require_clip(clip)?.start();
    ctx.project.move_clip(clip, to_track, start)?;
    Ok(Box::new(MoveClipAction {
        clip,
        to_track: from_track,
        start: from_start,
    }))
}

impl EditAction for MoveClipAction {
    fn apply(
        self: Box<Self>,
        ctx: &mut ApplyContext<'_>,
    ) -> Result<Box<dyn EditAction>, EngineError> {
        move_one(ctx, self.clip, self.to_track, self.start)
    }
}
