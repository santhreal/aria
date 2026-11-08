use aria_models::{ModelError, RationalTime, TrackId, check_same_rate};

use crate::action::edit::fit_to_fill;
use crate::action::{ApplyContext, CompoundAction, EditAction};
use crate::error::EngineError;

/// Open `[at, at + duration)` on every lane in `tracks` and on every other
/// sync-locked lane, and place nothing in it: Paste Insert sends this, then
/// places its copies in the gap, as one undo step.
///
/// A clip straddling `at` on any of those lanes is cut there and its tail moves
/// with the rest ([`shift_clips::open_lanes_for_insert`]), so a gap opened
/// inside a shot opens at a cut instead of being refused for overlapping it.
///
/// A lane named twice opens once. The first lane named is the one the outcome
/// reports; naming none is refused, since the gap would then open only on the
/// sync-locked lanes.
///
/// Atomic: one `CompoundAction` inverse, and a refused lane reverts the lanes
/// already opened before the error propagates.
///
/// [`shift_clips::open_lanes_for_insert`]: crate::action::edit::shift_clips::open_lanes_for_insert
pub(crate) fn execute(
    ctx: &mut ApplyContext<'_>,
    tracks: &[TrackId],
    at: RationalTime,
    duration: RationalTime,
) -> Result<(TrackId, Box<dyn EditAction>), EngineError> {
    let first = *tracks.first().ok_or(ModelError::InvalidRange)?;
    check_same_rate(duration.rate, ctx.project.timeline().frame_rate)?;
    if duration.value <= 0 {
        return Err(EngineError::Refused("a gap needs a length".into()));
    }
    let mut lanes = tracks.to_vec();
    lanes.sort_unstable();
    lanes.dedup();
    let mut inverses: Vec<Box<dyn EditAction>> = Vec::new();
    match fit_to_fill::open_span(ctx, &lanes, at, duration.value, true, &mut inverses) {
        Ok(()) => Ok((first, Box::new(CompoundAction { actions: inverses }))),
        Err(err) => {
            fit_to_fill::rollback(ctx, inverses);
            Err(err)
        }
    }
}
