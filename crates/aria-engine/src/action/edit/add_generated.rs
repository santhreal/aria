use aria_models::{ClipId, Generator, TimeRange, TrackId};

use crate::action::edit::remove_clip::RemoveClipAction;
use crate::action::{ApplyContext, EditAction};
use crate::error::EngineError;

pub fn execute(
    ctx: &mut ApplyContext<'_>,
    track: TrackId,
    generator: Generator,
    timeline: TimeRange,
) -> Result<(ClipId, Box<dyn EditAction>), EngineError> {
    let id = ctx.project.add_generated(track, generator, timeline)?;
    Ok((id, Box::new(RemoveClipAction { clip: id })))
}
