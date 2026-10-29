//! Time display edit: how the sequence's ruler counts time, swapped as one
//! undoable unit.

use aria_models::TimeDisplay;

use crate::action::{ApplyContext, EditAction};
use crate::error::EngineError;

/// Set the sequence's time display; the inverse restores the prior one.
pub(crate) fn set_time_display(
    ctx: &mut ApplyContext<'_>,
    display: TimeDisplay,
) -> Result<Box<dyn EditAction>, EngineError> {
    let before = ctx.project.time_display();
    ctx.project.set_time_display(display);
    Ok(Box::new(RestoreTimeDisplayAction { display: before }))
}

/// Swap the time display back to a captured setting (set-time-display undo/redo).
struct RestoreTimeDisplayAction {
    display: TimeDisplay,
}

impl EditAction for RestoreTimeDisplayAction {
    fn apply(
        self: Box<Self>,
        ctx: &mut ApplyContext<'_>,
    ) -> Result<Box<dyn EditAction>, EngineError> {
        let current = ctx.project.time_display();
        ctx.project.set_time_display(self.display);
        Ok(Box::new(RestoreTimeDisplayAction { display: current }))
    }
}
