use aria_models::{ClipId, MediaId, RationalTime, TimeRange, TrackId};

use crate::action::edit::remove_clip::RemoveClipAction;
use crate::action::{ApplyContext, EditAction};
use crate::error::EngineError;

pub(crate) fn execute(
    ctx: &mut ApplyContext<'_>,
    track: TrackId,
    media: MediaId,
    source: TimeRange,
    start: RationalTime,
) -> Result<(ClipId, Box<dyn EditAction>), EngineError> {
    let id = ctx.project.add_clip(track, media, source, start)?;
    Ok((id, Box::new(RemoveClipAction { clip: id })))
}
