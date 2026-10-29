use aria_models::{ClipId, MediaId, RationalTime, TimeRange, TrackId, resample};

use crate::action::edit::{add_clip, shift_clips};
use crate::action::{ApplyContext, CompoundAction, EditAction};
use crate::error::EngineError;

/// CapCut main-track insert: shift every clip starting at/after `at` right by
/// the new clip's timeline duration, then place the clip in the opened hole.
/// Sync-locked lanes ride along with the shift so picture and sound stay in
/// step. Atomic — if the placement is rejected, the shift is reverted before
/// the error propagates. The inverse removes the clip and closes the hole
/// again.
pub(crate) fn execute(
    ctx: &mut ApplyContext<'_>,
    track: TrackId,
    media: MediaId,
    source: TimeRange,
    at: RationalTime,
) -> Result<(ClipId, Box<dyn EditAction>), EngineError> {
    let tl_rate = ctx.project.timeline().frame_rate;
    // Mirror Project::add_clip's source→timeline resampling so the hole is
    // exactly as wide as the clip the engine will place.
    let duration_ticks = resample(source.duration, tl_rate).value.max(1);
    let delta = RationalTime::new(duration_ticks, tl_rate);

    let mut inverses: Vec<Box<dyn EditAction>> = Vec::new();
    if let Err(err) = shift_clips::open_for_insert(ctx, track, at, delta, &mut inverses) {
        rollback(ctx, inverses);
        return Err(err);
    }
    match add_clip::execute(ctx, track, media, source, at) {
        Ok((id, add_inverse)) => {
            inverses.push(add_inverse);
            Ok((id, Box::new(CompoundAction { actions: inverses })))
        }
        Err(err) => {
            // Re-close the hole so a rejected placement leaves no trace. The
            // shift inverses restore positions that were valid a moment ago;
            // surface the original placement error regardless.
            rollback(ctx, inverses);
            Err(err)
        }
    }
}

/// Revert applied shifts, newest first, so a refused insert leaves no trace.
fn rollback(ctx: &mut ApplyContext<'_>, inverses: Vec<Box<dyn EditAction>>) {
    for inverse in inverses.into_iter().rev() {
        if inverse.apply(ctx).is_err() {
            tracing::error!("ripple-insert rollback failed; hole left open");
        }
    }
}
