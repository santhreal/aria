//! Sequence frame rate edit: resamples timeline placements, keyframes, transitions,
//! and markers, swapped as one undoable unit.

use aria_models::{ModelError, Rational, Timeline};

use crate::action::{ApplyContext, EditAction};
use crate::error::EngineError;

/// Set the sequence frame rate; the inverse restores the prior timeline snapshot.
pub(crate) fn set_sequence_rate(
    ctx: &mut ApplyContext<'_>,
    frame_rate: Rational,
) -> Result<Box<dyn EditAction>, EngineError> {
    if !frame_rate.is_valid() {
        return Err(EngineError::Model(ModelError::InvalidParam(format!(
            "frame rate must have positive numerator and denominator (got {}/{})",
            frame_rate.num, frame_rate.den
        ))));
    }
    let before = ctx.project.timeline().clone();
    ctx.project.resample_frame_rate(frame_rate)?;
    Ok(Box::new(RestoreSequenceRateAction { previous: before }))
}

/// Swap the timeline back to a captured snapshot (set-sequence-rate undo/redo).
struct RestoreSequenceRateAction {
    previous: Timeline,
}

impl EditAction for RestoreSequenceRateAction {
    fn apply(
        self: Box<Self>,
        ctx: &mut ApplyContext<'_>,
    ) -> Result<Box<dyn EditAction>, EngineError> {
        let current = ctx.project.timeline().clone();
        *ctx.project.timeline_mut() = self.previous;
        Ok(Box::new(RestoreSequenceRateAction { previous: current }))
    }
}
