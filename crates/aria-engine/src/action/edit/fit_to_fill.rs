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
    let tl_rate = ctx.project.timeline().frame_rate;
    let fill = resample(duration, tl_rate).value;
    if fill <= 0 {
        return Err(EngineError::Refused(
            "a four-point edit needs a span to fill".into(),
        ));
    }
    // Mirror Project::add_clip's source→timeline resampling: the speed is the
    // ratio of what the take is worth on this timeline to what it must fill.
    let source_ticks = resample(source.duration, tl_rate).value.max(1);
    let start = at.value;

    let mut inverses: Vec<Box<dyn EditAction>> = Vec::new();
    let opened = if ripple {
        shift_clips::open_for_insert(
            ctx,
            track,
            at,
            RationalTime::new(fill, tl_rate),
            &mut inverses,
        )
    } else {
        clear_span(ctx, track, start, start + fill, tl_rate, &mut inverses)
    };
    if let Err(err) = opened {
        rollback(ctx, inverses);
        return Err(err);
    }

    // Where the clip is placed while it is still at full length.
    let park = (source_ticks > fill).then(|| park_tick(ctx, track, start + fill));
    let place_at = park.map_or(at, |tick| RationalTime::new(tick, tl_rate));

    let id = match add_clip::execute(ctx, track, media, source, place_at) {
        Ok((id, inverse)) => {
            inverses.push(inverse);
            id
        }
        Err(err) => {
            rollback(ctx, inverses);
            return Err(err);
        }
    };

    if source_ticks != fill {
        let speed = ratio(source_ticks, fill);
        match set_speed::set_speed(ctx, id, speed, false) {
            Ok(inverse) => inverses.push(inverse),
            Err(err) => {
                rollback(ctx, inverses);
                return Err(err);
            }
        }
    }

    if park.is_some() {
        match move_clip::execute(ctx, id, track, at) {
            Ok(inverse) => inverses.push(inverse),
            Err(err) => {
                rollback(ctx, inverses);
                return Err(err);
            }
        }
    }

    Ok((id, Box::new(CompoundAction { actions: inverses })))
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

/// `num : den` as a rate, reduced so it fits the `i32` terms a [`Rational`] is
/// made of.
///
/// A span long enough to overflow those terms is halved until it fits rather
/// than refused: what matters is the ratio, and a tick count that large is
/// already beyond the precision a speed is read at.
fn ratio(num: i64, den: i64) -> Rational {
    let divisor = gcd(num, den).max(1);
    let (mut num, mut den) = (num / divisor, den / divisor);
    while num > i64::from(i32::MAX) || den > i64::from(i32::MAX) {
        num = (num / 2).max(1);
        den = (den / 2).max(1);
    }
    Rational::new(num as i32, den as i32)
}

fn gcd(a: i64, b: i64) -> i64 {
    let (mut a, mut b) = (a.abs(), b.abs());
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
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
fn rollback(ctx: &mut ApplyContext<'_>, inverses: Vec<Box<dyn EditAction>>) {
    for inverse in inverses.into_iter().rev() {
        if inverse.apply(ctx).is_err() {
            tracing::error!("fit-to-fill rollback failed; the lane may hold a partial edit");
            break;
        }
    }
}
