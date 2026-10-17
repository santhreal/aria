use aria_models::{ClipId, ModelError, TimeRange};

use crate::action::edit::restore_clip::RestoreClipAction;
use crate::action::{ApplyContext, CompoundAction, EditAction};
use crate::error::EngineError;

/// Trim `clip` to `timeline`. Linked companions take the same start and end
/// deltas on their own lanes as one history entry.
pub fn execute(
    ctx: &mut ApplyContext<'_>,
    clip: ClipId,
    timeline: TimeRange,
) -> Result<Box<dyn EditAction>, EngineError> {
    let primary = ctx
        .project
        .clip(clip)
        .ok_or(ModelError::UnknownClip(clip))?;
    let start_delta = timeline.start.value - primary.timeline.start.value;
    let end_delta = timeline.end_tick() - primary.timeline.end_tick();
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
            let other = ctx.project.clip(id).ok_or(ModelError::UnknownClip(id))?;
            let new_start = other.timeline.start.value + start_delta;
            let new_end = other.timeline.end_tick() + end_delta;
            if new_start < 0 || new_end <= new_start {
                rollback(ctx, inverses);
                return Err(EngineError::from(ModelError::InvalidRange));
            }
            TimeRange::at_rate(new_start, new_end - new_start, timeline.start.rate)
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
