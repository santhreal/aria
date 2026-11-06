//! Sequence frame rate edit: resamples timeline placements, keyframes, transitions,
//! and markers, swapped as one undoable unit.

use aria_models::{ModelError, Rational, TimeError, Timeline};

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
    if let Err(err) = ctx
        .project
        .resample_frame_rate(frame_rate)
        .and_then(|()| validate_resampled_timeline(ctx.project.timeline()))
    {
        *ctx.project.timeline_mut() = before;
        return Err(err.into());
    }
    Ok(Box::new(RestoreSequenceRateAction { previous: before }))
}

fn validate_resampled_timeline(timeline: &Timeline) -> Result<(), ModelError> {
    for marker in timeline.markers() {
        if marker.tick.value < 0 || marker.duration.value < 0 {
            return Err(ModelError::InvalidRange);
        }
        marker
            .tick
            .value
            .checked_add(marker.duration.value)
            .ok_or(TimeError::Overflow)?;
    }
    for track in timeline.tracks_ordered() {
        let mut prev_end: Option<i64> = None;
        for clip in track.clips_ordered() {
            if clip.timeline.start.value < 0 || clip.timeline.duration.value <= 0 {
                return Err(ModelError::InvalidRange);
            }
            let end = clip.timeline.end()?.value;
            if let Some(last_end) = prev_end
                && last_end > clip.timeline.start.value
            {
                return Err(ModelError::Overlap(track.id));
            }
            prev_end = Some(end);
        }
    }
    Ok(())
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
