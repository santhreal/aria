use aria_models::{ModelError, RationalTime, TimeError, Timeline, TrackId};

use crate::action::{ApplyContext, EditAction};
use crate::error::EngineError;

/// Ripple primitive: shift every clip on `track` with start ≥ `from` by
/// `delta` ticks. Self-inverse with negated delta; when nothing shifted, the
/// inverse re-selects the same (empty) set, so it stays a no-op.
pub(crate) struct ShiftClipsAction {
    pub track: TrackId,
    pub from: RationalTime,
    pub delta: RationalTime,
}

pub(crate) fn execute(
    ctx: &mut ApplyContext<'_>,
    track: TrackId,
    from: RationalTime,
    delta: RationalTime,
) -> Result<Box<dyn EditAction>, EngineError> {
    let neg_delta = delta.value.checked_neg().ok_or(TimeError::Overflow)?;
    if let Some(lane) = ctx.project.timeline().track(track) {
        for clip in lane.clips() {
            if clip.timeline.start.value >= from.value {
                let new_start = clip
                    .timeline
                    .start
                    .value
                    .checked_add(delta.value)
                    .ok_or(TimeError::Overflow)?;
                if new_start < 0 {
                    return Err(ModelError::InvalidRange.into());
                }
                new_start
                    .checked_add(clip.timeline.duration.value)
                    .ok_or(TimeError::Overflow)?;
            }
        }
    }
    let first_new_start = ctx.project.shift_clips(track, from, delta)?;
    // The inverse must select exactly the clips that moved. Their new starts
    // all sit at/after the first shifted clip's new start, while unshifted
    // clips end at/before it (no-overlap invariant) — so that boundary is
    // exact. When nothing moved, the original `from` re-selects nothing.
    Ok(Box::new(ShiftClipsAction {
        track,
        from: first_new_start.unwrap_or(from),
        delta: RationalTime::new(neg_delta, delta.rate),
    }))
}

impl EditAction for ShiftClipsAction {
    fn apply(
        self: Box<Self>,
        ctx: &mut ApplyContext<'_>,
    ) -> Result<Box<dyn EditAction>, EngineError> {
        execute(ctx, self.track, self.from, self.delta)
    }
}

/// The lanes that ride along with a ripple on `addressed`: every sync-locked
/// lane `addressed` does not name, in stack order. A lane follows an edit
/// elsewhere exactly while its sync lock is set. The single definition of that
/// rule: inserts, ripple deletes, ripple moves, and the transitions guard of a
/// rippling command all select their riders here.
pub(crate) fn rider_lanes(timeline: &Timeline, addressed: &[TrackId]) -> Vec<TrackId> {
    timeline
        .tracks_ordered()
        .filter(|track| track.sync_lock && !addressed.contains(&track.id))
        .map(|track| track.id)
        .collect()
}

/// Open a hole of `delta` at `from` on `track` and on every other sync-locked
/// lane, so an insert keeps picture and sound in step.
///
/// The target lane always shifts — that *is* the insert. Another lane shifts
/// only while it is one of the [`rider_lanes`]; clearing its sync lock is how a
/// lane is pinned against inserts elsewhere. Ripple insert and a rippled
/// four-point edit both go through here rather than each deciding for itself
/// which lanes ride along.
///
/// A lane whose clip straddles `from` is cut there first. Shifting selects
/// clips by start tick, so without the cut an insert landing inside a clip
/// moved nothing and was then refused for overlapping it: a pool item
/// dropped halfway through a shot got an error instead of an
/// insert. The cut is part of the hole, so it is one lane's decision here
/// rather than each caller's.
///
/// Inverses are pushed onto `inverses` in the order applied, so a caller
/// reverts them newest-first. A refused lane leaves the earlier ones applied
/// and on `inverses` for exactly that.
pub(crate) fn open_for_insert(
    ctx: &mut ApplyContext<'_>,
    track: TrackId,
    from: RationalTime,
    delta: RationalTime,
    inverses: &mut Vec<Box<dyn EditAction>>,
) -> Result<(), EngineError> {
    open_lanes_for_insert(ctx, &[track], from, delta, inverses)
}

/// [`open_for_insert`] for an edit that lands on several lanes at once, such
/// as a source edit placing picture and sound: every lane in `lanes` shifts,
/// and so does every other sync-locked lane, each exactly once.
pub(crate) fn open_lanes_for_insert(
    ctx: &mut ApplyContext<'_>,
    lanes: &[TrackId],
    from: RationalTime,
    delta: RationalTime,
    inverses: &mut Vec<Box<dyn EditAction>>,
) -> Result<(), EngineError> {
    let lanes: Vec<TrackId> = lanes
        .iter()
        .copied()
        .chain(rider_lanes(ctx.project.timeline(), lanes))
        .collect();
    cut_at(ctx, &lanes, from, inverses)?;
    for lane in lanes {
        inverses.push(execute(ctx, lane, from, delta)?);
    }
    Ok(())
}

/// Close the span `from`..`to` on every sync-locked lane other than `track`, so
/// a ripple delete keeps picture and sound in step.
///
/// This is the delete-side half of [`open_for_insert`] and takes its riders
/// from the same [`rider_lanes`]: a lane rides along only while its sync lock
/// is set, and the lanes in `excluded` are handled by their own callers. Ripple
/// delete and a rippled four-point edit both come through here rather than
/// each deciding which lanes follow.
///
/// Closing a span is not the mirror image of opening one. Opening a hole moves
/// material aside and preserves all of it; closing a span removes that stretch of
/// timeline, and a rider lane's own material inside it goes with it — that is
/// what keeping the lanes in step means. So every rider lane is cut at both
/// boundaries, every clip then lying wholly inside the span is removed, and what
/// followed the span moves up by its length. Cutting first means a clip
/// straddling either boundary loses only the part inside the span.
///
/// The removals are undoable with everything else in the gesture: inverses are
/// pushed in the order applied, so one history entry restores every lane.
///
/// The shift here cannot be refused. Nothing starts inside the span once the
/// removals are done, so the first clip that moves lands at or after `from`,
/// which is at or after the end of the last clip left behind and never below
/// tick 0.
pub(crate) fn close_for_ripple_delete(
    ctx: &mut ApplyContext<'_>,
    excluded: &[TrackId],
    from: RationalTime,
    to: RationalTime,
    inverses: &mut Vec<Box<dyn EditAction>>,
) -> Result<(), EngineError> {
    let span_ticks = from
        .value
        .checked_sub(to.value)
        .ok_or(TimeError::Overflow)?;
    let span = RationalTime::new(span_ticks, from.rate);
    let riders = rider_lanes(ctx.project.timeline(), excluded);
    cut_at(ctx, &riders, from, inverses)?;
    cut_at(ctx, &riders, to, inverses)?;
    for lane in riders {
        let covered: Vec<_> = ctx
            .project
            .timeline()
            .track(lane)
            .into_iter()
            .flat_map(|t| t.clips())
            .filter(|clip| {
                clip.timeline.start.value >= from.value && clip.timeline.end_tick() <= to.value
            })
            .map(|clip| clip.id)
            .collect();
        for clip in covered {
            inverses.push(Box::new(super::remove_clip::RemoveClipAction { clip }).apply(ctx)?);
        }
        inverses.push(execute(ctx, lane, to, span)?);
    }
    Ok(())
}

/// Split the clip that straddles `at` on each of `lanes`, if there is one, so
/// the hole opens at a cut, and link the tails whose heads share a link
/// ([`Project::split_tail_links`]). A clip that starts or ends exactly at `at`
/// is already cut there and is left alone.
///
/// [`Project::split_tail_links`]: aria_models::Project::split_tail_links
pub(crate) fn cut_at(
    ctx: &mut ApplyContext<'_>,
    lanes: &[TrackId],
    at: RationalTime,
    inverses: &mut Vec<Box<dyn EditAction>>,
) -> Result<(), EngineError> {
    let mut cuts = Vec::new();
    for &lane in lanes {
        let straddling = ctx.project.timeline().track(lane).and_then(|track| {
            track
                .clips()
                .find(|clip| {
                    clip.timeline.start.value < at.value && clip.timeline.end_tick() > at.value
                })
                .map(|clip| clip.id)
        });
        if let Some(clip) = straddling {
            let (tail, inverse) = super::split_clip::execute(ctx, clip, at)?;
            inverses.push(inverse);
            cuts.push((clip, tail));
        }
    }
    for tails in ctx.project.split_tail_links(&cuts) {
        inverses.push(super::link_clips::execute(ctx, &tails)?);
    }
    Ok(())
}
