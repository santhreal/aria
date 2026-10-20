use aria_models::{ClipId, MediaId, Rational, RationalTime, TimeRange, TrackId, resample};

use crate::action::edit::{add_clip, move_clip, overwrite, set_speed, shift_clips};
use crate::action::{ApplyContext, CompoundAction, EditAction};
use crate::error::EngineError;

/// Four-point edit: place `source` at `at` and retime it so it occupies exactly
/// `duration` on the timeline.
///
/// The span is opened before the clip is placed rather than after, because a
/// clip that grew into its neighbour would be refused as an overlap: with
/// `ripple` the lane is pushed right by `duration`, and without it the span is
/// covered the way [`overwrite`] covers one.
///
/// A take *longer* than the span it must fill cannot be placed where it belongs
/// and then shrunk either — at full length it would reach past the span into
/// whatever the lane still holds. Such a take is parked past the end of the
/// lane, retimed there, and moved into the span at its final length, so no
/// intermediate state ever overlaps.
///
/// Atomic: one `CompoundAction` inverse, and a refused step reverts the ones
/// already applied before the error propagates.
pub fn execute(
    ctx: &mut ApplyContext<'_>,
    track: TrackId,
    media: MediaId,
    source: TimeRange,
    at: RationalTime,
    duration: RationalTime,
    ripple: bool,
) -> Result<(ClipId, Box<dyn EditAction>), EngineError> {
    let fill = span_ticks(ctx, duration)?;
    let mut inverses: Vec<Box<dyn EditAction>> = Vec::new();
    let placed = open_span(ctx, &[track], at, fill, ripple, &mut inverses)
        .and_then(|()| place_retimed(ctx, track, media, source, at, fill, &mut inverses));
    match placed {
        Ok(id) => Ok((id, Box::new(CompoundAction { actions: inverses }))),
        Err(err) => {
            rollback(ctx, inverses);
            Err(err)
        }
    }
}

/// `duration` in ticks of the open sequence, refused when it is empty.
pub(crate) fn span_ticks(
    ctx: &ApplyContext<'_>,
    duration: RationalTime,
) -> Result<i64, EngineError> {
    let fill = resample(duration, ctx.project.timeline().frame_rate).value;
    if fill <= 0 {
        return Err(EngineError::Refused(
            "a four-point edit needs a span to fill".into(),
        ));
    }
    Ok(fill)
}

/// Open `[at, at + fill)` on every lane in `lanes`: with `ripple` each lane,
/// and every other sync-locked lane, is pushed right by `fill`; without it the
/// span is covered on each lane the way [`overwrite`] covers one.
pub(crate) fn open_span(
    ctx: &mut ApplyContext<'_>,
    lanes: &[TrackId],
    at: RationalTime,
    fill: i64,
    ripple: bool,
    inverses: &mut Vec<Box<dyn EditAction>>,
) -> Result<(), EngineError> {
    let tl_rate = ctx.project.timeline().frame_rate;
    if ripple {
        return shift_clips::open_lanes_for_insert(
            ctx,
            lanes,
            at,
            RationalTime::new(fill, tl_rate),
            inverses,
        );
    }
    for &lane in lanes {
        clear_span(ctx, lane, at.value, at.value + fill, tl_rate, inverses)?;
    }
    Ok(())
}

/// Place `source` at `at` on `track`, already opened, and retime it to occupy
/// `fill` ticks. A take that already lasts `fill` is placed as it is.
pub(crate) fn place_retimed(
    ctx: &mut ApplyContext<'_>,
    track: TrackId,
    media: MediaId,
    source: TimeRange,
    at: RationalTime,
    fill: i64,
    inverses: &mut Vec<Box<dyn EditAction>>,
) -> Result<ClipId, EngineError> {
    let tl_rate = ctx.project.timeline().frame_rate;
    // Mirror Project::add_clip's source→timeline resampling: the speed is the
    // ratio of what the take is worth on this timeline to what it must fill.
    let source_ticks = resample(source.duration, tl_rate).value.max(1);
    // Where the clip is placed while it is still at full length.
    let park = (source_ticks > fill).then(|| park_tick(ctx, track, at.value + fill));
    let place_at = park.map_or(at, |tick| RationalTime::new(tick, tl_rate));

    let (id, inverse) = add_clip::execute(ctx, track, media, source, place_at)?;
    inverses.push(inverse);
    if source_ticks != fill {
        inverses.push(set_speed::set_speed(
            ctx,
            id,
            Rational::from_ratio_fitted(source_ticks, fill),
            false,
        )?);
    }
    if park.is_some() {
        inverses.push(move_clip::execute(ctx, id, track, at)?);
    }
    Ok(id)
}

/// A tick past everything the lane holds, so a full-length take can be parked
/// there without overlapping anything.
///
/// `floor` keeps the park clear of the span being filled even on an empty lane,
/// so the move into that span is always a move backwards.
fn park_tick(ctx: &ApplyContext<'_>, track: TrackId, floor: i64) -> i64 {
    ctx.project
        .timeline()
        .track(track)
        .map(|lane| {
            lane.clips()
                .map(|clip| clip.timeline.end_tick())
                .max()
                .unwrap_or(0)
        })
        .unwrap_or(0)
        .max(floor)
}

/// Cover `[start, end)` on `track`, reusing overwrite's plan so the two edits
/// cannot disagree about what a cover costs a clip.
fn clear_span(
    ctx: &mut ApplyContext<'_>,
    track: TrackId,
    start: i64,
    end: i64,
    tl_rate: Rational,
    inverses: &mut Vec<Box<dyn EditAction>>,
) -> Result<(), EngineError> {
    for step in overwrite::cover_plan(ctx, track, start, end) {
        overwrite::clear(ctx, step, start, end, tl_rate, inverses)?;
    }
    Ok(())
}

/// Revert applied steps, newest first, so a refused fit leaves no trace.
pub(crate) fn rollback(ctx: &mut ApplyContext<'_>, inverses: Vec<Box<dyn EditAction>>) {
    for inverse in inverses.into_iter().rev() {
        if inverse.apply(ctx).is_err() {
            tracing::error!("fit-to-fill rollback failed; the lane may hold a partial edit");
            break;
        }
    }
}
