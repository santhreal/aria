use aria_models::{ClipId, ModelError, TimeError, TimeRange};

use crate::action::edit::restore_clip::RestoreClipAction;
use crate::action::{ApplyContext, CompoundAction, EditAction};
use crate::error::EngineError;

/// Trim `clip` to `timeline`. Linked companions take the same start and end
/// deltas on their own lanes as one history entry.
pub(crate) fn execute(
    ctx: &mut ApplyContext<'_>,
    clip: ClipId,
    timeline: TimeRange,
) -> Result<Box<dyn EditAction>, EngineError> {
    let primary = ctx.project.require_clip(clip)?;
    let timeline_end = timeline.end()?.value;
    let primary_end = primary.timeline.end()?.value;
    let start_delta = timeline
        .start
        .value
        .checked_sub(primary.timeline.start.value)
        .ok_or(TimeError::Overflow)?;
    let end_delta = timeline_end
        .checked_sub(primary_end)
        .ok_or(TimeError::Overflow)?;
    let ids = match primary.link {
        Some(link) => ctx.project.linked_clips(link),
        None => vec![clip],
    };

    if ids.len() == 1 {
        return trim_one(ctx, clip, timeline);
    }

    let mut inverses: Vec<Box<dyn EditAction>> = Vec::with_capacity(ids.len());
    for id in ids {
        let target = if id == clip {
            timeline
        } else {
            let other = ctx.project.require_clip(id)?;
            let other_end = other.timeline.end()?.value;
            let new_start = other
                .timeline
                .start
                .value
                .checked_add(start_delta)
                .ok_or(TimeError::Overflow)?;
            let new_end = other_end
                .checked_add(end_delta)
                .ok_or(TimeError::Overflow)?;
            let new_dur = new_end.checked_sub(new_start).ok_or(TimeError::Overflow)?;
            // The model refuses a companion taken before the sequence start
            // or down to nothing.
            TimeRange::at_rate(new_start, new_dur, timeline.start.rate)
        };
        match trim_one(ctx, id, target) {
            Ok(inverse) => inverses.push(inverse),
            Err(err) => {
                rollback(ctx, inverses);
                return Err(err);
            }
        }
    }
    Ok(Box::new(CompoundAction { actions: inverses }))
}

/// Trim a single clip without propagating to its link group.
pub(crate) fn trim_one(
    ctx: &mut ApplyContext<'_>,
    clip: ClipId,
    timeline: TimeRange,
) -> Result<Box<dyn EditAction>, EngineError> {
    let before = ctx
        .project
        .clip(clip)
        .cloned()
        .ok_or(ModelError::UnknownClip(clip))?;
    ctx.project.trim_clip(clip, timeline)?;
    Ok(Box::new(RestoreClipAction { clip: before }))
}

fn rollback(ctx: &mut ApplyContext<'_>, inverses: Vec<Box<dyn EditAction>>) {
    for inverse in inverses.into_iter().rev() {
        if inverse.apply(ctx).is_err() {
            tracing::error!("linked trim rollback failed");
            break;
        }
    }
}
