use aria_models::{ClipId, ModelError, RationalTime, TrackId};

use crate::action::{ApplyContext, CompoundAction, EditAction};
use crate::error::EngineError;

pub struct MoveClipAction {
    pub clip: ClipId,
    pub to_track: TrackId,
    pub start: RationalTime,
}

/// Move `clip` to `to_track` at `start`. Linked companions take the same time
/// delta and stay on their lanes, as one history entry.
pub fn execute(
    ctx: &mut ApplyContext<'_>,
    clip: ClipId,
    to_track: TrackId,
    start: RationalTime,
) -> Result<Box<dyn EditAction>, EngineError> {
    let primary = ctx
        .project
        .clip(clip)
        .ok_or(ModelError::UnknownClip(clip))?;
    let delta = start.value - primary.start().value;
    let ids = match primary.link {
        Some(link) => ctx.project.linked_clips(link),
        None => vec![clip],
    };

    if ids.len() == 1 {
        return move_one(ctx, clip, to_track, start);
    }

    let mut inverses: Vec<Box<dyn EditAction>> = Vec::new();
    for id in ids {
        let original = ctx.project.clip(id).ok_or(ModelError::UnknownClip(id))?;
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
    let from_start = ctx
        .project
        .clip(clip)
        .ok_or(ModelError::UnknownClip(clip))?
        .start();
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
