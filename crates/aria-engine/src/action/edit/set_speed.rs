use aria_models::{ClipId, ModelError, Rational};

use crate::action::edit::restore_clip::RestoreClipAction;
use crate::action::{ApplyContext, CompoundAction, EditAction};
use crate::error::EngineError;

/// Retime a media clip (CapCut speed, M1). Linked companions take the same
/// speed and direction as one history entry.
///
/// The model re-derives the timeline duration (source ÷ speed) and validates
/// speed positivity, media backing, and neighbor overlap. The inverse is a
/// full-clip restore — speed, reversed, and the re-derived timeline range all
/// roll back in one shot, like the transform and param edits.
pub(crate) fn set_speed(
    ctx: &mut ApplyContext<'_>,
    clip: ClipId,
    speed: Rational,
    reversed: bool,
) -> Result<Box<dyn EditAction>, EngineError> {
    let primary = ctx.project.require_clip(clip)?;
    let ids = match primary.link {
        Some(link) => ctx.project.linked_clips(link),
        None => vec![clip],
    };
    let max_duration = ids
        .iter()
        .filter_map(|&id| ctx.project.clip_room_after(id))
        .min();

    if ids.len() == 1 {
        return set_speed_one(ctx, clip, speed, reversed, max_duration);
    }

    let mut inverses: Vec<Box<dyn EditAction>> = Vec::with_capacity(ids.len());
    for id in ids {
        match set_speed_one(ctx, id, speed, reversed, max_duration) {
            Ok(inverse) => inverses.push(inverse),
            Err(err) => {
                for inverse in inverses.into_iter().rev() {
                    if inverse.apply(ctx).is_err() {
                        tracing::error!("linked speed rollback failed");
                        break;
                    }
                }
                return Err(err);
            }
        }
    }
    Ok(Box::new(CompoundAction { actions: inverses }))
}

fn set_speed_one(
    ctx: &mut ApplyContext<'_>,
    clip: ClipId,
    speed: Rational,
    reversed: bool,
    max_duration: Option<i64>,
) -> Result<Box<dyn EditAction>, EngineError> {
    let before = ctx
        .project
        .clip(clip)
        .cloned()
        .ok_or(ModelError::UnknownClip(clip))?;
    ctx.project
        .set_clip_speed_bounded(clip, speed, reversed, max_duration)?;
    Ok(Box::new(RestoreClipAction { clip: before }))
}
