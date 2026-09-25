//! Canvas settings edit (M1): aspect preset, background color, and explicit
//! pixel size, swapped as one undoable unit.

use aria_models::{CanvasAspect, CanvasSettings};

use crate::action::{ApplyContext, EditAction};
use crate::error::EngineError;

/// Set the project canvas; the inverse restores the prior settings. An explicit
/// `resolution` is refused here rather than clamped: a delivery size the
/// operator typed is either the one they meant or a mistake worth naming, and
/// every later stage (preview, export, compositor) then trusts the stored size.
pub fn set_canvas(
    ctx: &mut ApplyContext<'_>,
    aspect: CanvasAspect,
    background: [u8; 3],
    resolution: Option<[u32; 2]>,
) -> Result<Box<dyn EditAction>, EngineError> {
    if let Some([w, h]) = resolution
        && !CanvasSettings::resolution_is_valid([w, h])
    {
        return Err(EngineError::Model(aria_models::ModelError::InvalidParam(
            format!(
                "canvas {w}×{h} is not encodable: each edge must be even and between \
                 {min} and {max} pixels",
                min = CanvasSettings::MIN_DIM,
                max = CanvasSettings::MAX_DIM,
            ),
        )));
    }
    let timeline = ctx.project.timeline_mut();
    let before = timeline.canvas();
    let target = CanvasSettings {
        aspect,
        background,
        resolution,
    };
    if before == target {
        return Ok(Box::new(NoopCanvasAction));
    }
    timeline.set_canvas(target);
    Ok(Box::new(RestoreCanvasAction { settings: before }))
}

struct NoopCanvasAction;

impl EditAction for NoopCanvasAction {
    fn apply(
        self: Box<Self>,
        _ctx: &mut ApplyContext<'_>,
    ) -> Result<Box<dyn EditAction>, EngineError> {
        Ok(self)
    }
}

/// Swap the canvas back to a captured snapshot (set-canvas undo/redo).
struct RestoreCanvasAction {
    settings: CanvasSettings,
}

impl EditAction for RestoreCanvasAction {
    fn apply(
        self: Box<Self>,
        ctx: &mut ApplyContext<'_>,
    ) -> Result<Box<dyn EditAction>, EngineError> {
        let timeline = ctx.project.timeline_mut();
        let current = timeline.canvas();
        timeline.set_canvas(self.settings);
        Ok(Box::new(RestoreCanvasAction { settings: current }))
    }
}
