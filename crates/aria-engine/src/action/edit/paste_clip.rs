use aria_models::{Clip, ClipId, RationalTime, TrackId};

use crate::action::edit::remove_clip::RemoveClipAction;
use crate::action::{ApplyContext, EditAction};
use crate::error::EngineError;

/// Place a clipboard copy on `track` at `start` and return its undo action.
pub(crate) fn execute(
    ctx: &mut ApplyContext<'_>,
    copy: Clip,
    track: TrackId,
    start: RationalTime,
) -> Result<(ClipId, Box<dyn EditAction>), EngineError> {
    let placed = ctx.project.place_copy(copy, track, start, None)?;
    Ok((placed, Box::new(RemoveClipAction { clip: placed })))
}
