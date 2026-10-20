use aria_models::{TrackDisplay, TrackId};

use crate::action::{ApplyContext, EditAction};
use crate::error::EngineError;

/// The undoable unit for a lane's persisted view options (height / waveform /
/// filmstrip).
///
/// One gesture changes one option, so the action snapshots the whole previous
/// [`TrackDisplay`] and its inverse is a plain swap. Height is clamped here, at
/// the one place that writes it, so a drag can ask for any pixel value.
pub struct SetTrackDisplayAction {
    pub track: TrackId,
    pub display: TrackDisplay,
}

pub fn execute(
    ctx: &mut ApplyContext<'_>,
    track: TrackId,
    height: Option<u32>,
    waveform: Option<bool>,
    filmstrip: Option<bool>,
    collapsed: Option<bool>,
) -> Result<Box<dyn EditAction>, EngineError> {
    let t = ctx.project.timeline_mut().require_track_mut(track)?;
    let inverse = Box::new(SetTrackDisplayAction {
        track,
        display: t.display,
    });
    if let Some(v) = height {
        t.display.height = TrackDisplay::clamp_height(v);
    }
    if let Some(v) = waveform {
        t.display.waveform = v;
    }
    if let Some(v) = filmstrip {
        t.display.filmstrip = v;
    }
    if let Some(v) = collapsed {
        t.display.collapsed = v;
    }
    Ok(inverse)
}

impl EditAction for SetTrackDisplayAction {
    fn apply(
        self: Box<Self>,
        ctx: &mut ApplyContext<'_>,
    ) -> Result<Box<dyn EditAction>, EngineError> {
        execute(
            ctx,
            self.track,
            Some(self.display.height),
            Some(self.display.waveform),
            Some(self.display.filmstrip),
            Some(self.display.collapsed),
        )
    }
}
