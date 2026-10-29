use aria_models::{ClipId, ModelError};

use crate::action::edit::restore_clip::RestoreClipAction;
use crate::action::{ApplyContext, CompoundAction, EditAction};
use crate::error::EngineError;

/// Move a clip's source window without moving the clip. Linked companions slip
/// by the same tick delta as one history entry.
///
/// The inverse restores the whole clip rather than slipping back by `-by`: a
/// slip clamps at the ends of the take, so the opposite slip is not always the
/// same distance.
pub(crate) fn execute(
    ctx: &mut ApplyContext<'_>,
    clip: ClipId,
    by: i64,
) -> Result<Box<dyn EditAction>, EngineError> {
    let primary = ctx.project.require_clip(clip)?;
    let ids = match primary.link {
        Some(link) => ctx.project.linked_clips(link),
        None => vec![clip],
    };

    if ids.len() == 1 {
        return slip_one(ctx, clip, by);
    }

    let mut inverses: Vec<Box<dyn EditAction>> = Vec::with_capacity(ids.len());
    for id in ids {
        match slip_one(ctx, id, by) {
            Ok(inverse) => inverses.push(inverse),
            Err(err) => {
                for inverse in inverses.into_iter().rev() {
                    if inverse.apply(ctx).is_err() {
                        tracing::error!("linked slip rollback failed");
                        break;
                    }
                }
                return Err(err);
            }
        }
    }
    Ok(Box::new(CompoundAction { actions: inverses }))
}

fn slip_one(
    ctx: &mut ApplyContext<'_>,
    clip: ClipId,
    by: i64,
) -> Result<Box<dyn EditAction>, EngineError> {
    let before = ctx
        .project
        .clip(clip)
        .cloned()
        .ok_or(ModelError::UnknownClip(clip))?;
    ctx.project.slip_clip(clip, by)?;
    Ok(Box::new(RestoreClipAction { clip: before }))
}
