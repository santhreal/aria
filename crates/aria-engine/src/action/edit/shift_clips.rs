use aria_models::{RationalTime, TrackId};

use crate::action::{ApplyContext, EditAction};
use crate::error::EngineError;

/// Ripple primitive: shift every clip on `track` with start ≥ `from` by
/// `delta` ticks. Self-inverse with negated delta; when nothing shifted, the
/// inverse re-selects the same (empty) set, so it stays a no-op.
pub struct ShiftClipsAction {
    pub track: TrackId,
    pub from: RationalTime,
    pub delta: RationalTime,
}

pub fn execute(
    ctx: &mut ApplyContext<'_>,
    track: TrackId,
    from: RationalTime,
    delta: RationalTime,
) -> Result<Box<dyn EditAction>, EngineError> {
    let first_new_start = ctx.project.shift_clips(track, from, delta)?;
    // The inverse must select exactly the clips that moved. Their new starts
    // all sit at/after the first shifted clip's new start, while unshifted
    // clips end at/before it (no-overlap invariant) — so that boundary is
    // exact. When nothing moved, the original `from` re-selects nothing.
    Ok(Box::new(ShiftClipsAction {
        track,
        from: first_new_start.unwrap_or(from),
        delta: RationalTime::new(-delta.value, delta.rate),
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

/// Open a hole of `delta` at `from` on `track` and on every other sync-locked
/// lane, so an insert keeps picture and sound in step.
///
/// The target lane always shifts — that *is* the insert. Another lane shifts
/// only while its sync lock is set; clearing the lock is how a lane is pinned
/// against inserts elsewhere. This is the one owner of that rule: ripple
/// insert and a rippled four-point edit both go through it rather than each
/// deciding for itself which lanes ride along.
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
pub fn open_for_insert(
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
pub fn open_lanes_for_insert(
    ctx: &mut ApplyContext<'_>,
    lanes: &[TrackId],
    from: RationalTime,
    delta: RationalTime,
    inverses: &mut Vec<Box<dyn EditAction>>,
) -> Result<(), EngineError> {
    let riders: Vec<TrackId> = ctx
        .project
        .timeline()
        .tracks_ordered()
        .filter(|t| t.sync_lock && !lanes.contains(&t.id))
        .map(|t| t.id)
        .collect();
    for &lane in lanes.iter().chain(&riders) {
        cut_at(ctx, lane, from, inverses)?;
        inverses.push(execute(ctx, lane, from, delta)?);
    }
    Ok(())
}

/// Close the span `from`..`to` on every sync-locked lane other than `track`, so
/// a ripple delete keeps picture and sound in step.
///
/// This is the delete-side half of [`open_for_insert`] and owns the same rider
/// rule: a lane rides along only while its sync lock is set, and the lanes in
/// `excluded` are handled by their own callers. Ripple delete and a rippled
/// four-point edit both come through here rather than each deciding which lanes
/// follow.
///
/// Closing a span is not the mirror image of opening one. Opening a hole moves
/// material aside and preserves all of it; closing a span removes that stretch of
/// timeline, and a rider lane's own material inside it goes with it — that is
/// what keeping the lanes in step means. So each rider lane is cut at both
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
pub fn close_for_ripple_delete(
    ctx: &mut ApplyContext<'_>,
    excluded: &[TrackId],
    from: RationalTime,
    to: RationalTime,
    inverses: &mut Vec<Box<dyn EditAction>>,
) -> Result<(), EngineError> {
    let span = RationalTime::new(from.value - to.value, from.rate);
    let riders: Vec<TrackId> = ctx
        .project
        .timeline()
        .tracks_ordered()
        .filter(|t| !excluded.contains(&t.id) && t.sync_lock)
        .map(|t| t.id)
        .collect();
    for lane in riders {
        cut_at(ctx, lane, from, inverses)?;
        cut_at(ctx, lane, to, inverses)?;
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

/// Split the clip that straddles `at` on `lane`, if there is one, so the hole
/// opens at a cut. A clip that starts or ends exactly at `at` is already cut
/// there and is left alone.
fn cut_at(
    ctx: &mut ApplyContext<'_>,
    lane: TrackId,
    at: RationalTime,
    inverses: &mut Vec<Box<dyn EditAction>>,
) -> Result<(), EngineError> {
    let straddling = ctx.project.timeline().track(lane).and_then(|track| {
        track
            .clips()
            .find(|clip| {
                clip.timeline.start.value < at.value && clip.timeline.end_tick() > at.value
            })
            .map(|clip| clip.id)
    });
    if let Some(clip) = straddling {
        let (_, inverse) = super::split_clip::execute(ctx, clip, at)?;
        inverses.push(inverse);
    }
    Ok(())
}
