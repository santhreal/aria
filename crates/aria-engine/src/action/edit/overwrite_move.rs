use aria_models::{Clip, ClipId, ModelError, RationalTime, TimeRange, TrackId, check_same_rate};

use crate::action::edit::insert_clip::InsertClipAction;
use crate::action::edit::overwrite::{clear, cover_plan, rollback};
use crate::action::edit::remove_clip::RemoveClipAction;
use crate::action::{ApplyContext, CompoundAction, EditAction};
use crate::error::EngineError;

/// Where one clip of the move lands.
struct Landing {
    id: ClipId,
    from_track: TrackId,
    to_track: TrackId,
    start: i64,
    duration: i64,
}

/// Move `clip` to `to_track` at `start`, covering what it lands on. Linked and
/// grouped companions take the same time delta on their own lanes and cover
/// what they land on there. One history entry.
pub fn execute(
    ctx: &mut ApplyContext<'_>,
    clip: ClipId,
    to_track: TrackId,
    start: RationalTime,
) -> Result<Box<dyn EditAction>, EngineError> {
    let ids = ctx.project.move_companions(clip);
    move_over(ctx, clip, to_track, start, &ids)
}

/// [`execute`] without links: clips grouped with `clip` take the same time
/// delta, linked clips stay where they are.
pub fn execute_unlinked(
    ctx: &mut ApplyContext<'_>,
    clip: ClipId,
    to_track: TrackId,
    start: RationalTime,
) -> Result<Box<dyn EditAction>, EngineError> {
    let ids = ctx.project.group_companions(clip);
    move_over(ctx, clip, to_track, start, &ids)
}

/// Lift every clip of `ids`, clear each landing span with the overwrite rule,
/// and place each clip on its span.
///
/// Every clip is lifted before any span is cleared, so a clip landing on its
/// own old span, or on a companion's, covers nothing of the move. A landing is
/// cleared only of clips outside the move, and two clips of the move that land
/// on each other are refused. A refused move leaves no trace.
fn move_over(
    ctx: &mut ApplyContext<'_>,
    clip: ClipId,
    to_track: TrackId,
    start: RationalTime,
    ids: &[ClipId],
) -> Result<Box<dyn EditAction>, EngineError> {
    let rate = ctx.project.timeline().frame_rate;
    check_same_rate(start.rate, rate)?;
    if start.value < 0 {
        return Err(ModelError::InvalidRange.into());
    }
    let delta = start.value - ctx.project.require_clip(clip)?.start().value;

    let mut landings = Vec::with_capacity(ids.len());
    for &id in ids {
        let timeline = ctx.project.timeline();
        let member = timeline.require_clip(id)?;
        let from_track = timeline.track_of(id).ok_or(ModelError::UnknownClip(id))?;
        let (to, at) = if id == clip {
            (to_track, start.value)
        } else {
            (from_track, (member.start().value + delta).max(0))
        };
        landings.push(Landing {
            id,
            from_track,
            to_track: to,
            start: at,
            duration: member.timeline.duration.value,
        });
    }

    let mut inverses: Vec<Box<dyn EditAction>> = Vec::new();
    let mut lifted: Vec<Clip> = Vec::with_capacity(landings.len());
    for landing in &landings {
        let Some(clip) = ctx.project.remove_clip(landing.id) else {
            rollback(ctx, inverses);
            return Err(ModelError::UnknownClip(landing.id).into());
        };
        inverses.push(Box::new(InsertClipAction {
            track: landing.from_track,
            clip: clip.clone(),
        }));
        lifted.push(clip);
    }

    for landing in &landings {
        let end = landing.start + landing.duration;
        for step in cover_plan(ctx, landing.to_track, landing.start, end) {
            if let Err(err) = clear(ctx, step, landing.start, end, rate, &mut inverses) {
                rollback(ctx, inverses);
                return Err(err);
            }
        }
    }

    for (landing, mut clip) in landings.iter().zip(lifted) {
        clip.timeline = TimeRange::at_rate(landing.start, landing.duration, rate);
        if let Err(err) = ctx.project.timeline_mut().add_clip(landing.to_track, clip) {
            rollback(ctx, inverses);
            return Err(err.into());
        }
        inverses.push(Box::new(RemoveClipAction { clip: landing.id }));
    }
    Ok(Box::new(CompoundAction { actions: inverses }))
}
