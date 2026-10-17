use aria_models::{Clip, ClipId, MediaId, ModelError, RationalTime, TimeRange, TrackId, resample};

use crate::action::edit::{add_clip, remove_clip::RemoveClipAction, trim_clip};
use crate::action::{ApplyContext, CompoundAction, EditAction};
use crate::error::EngineError;

/// Overwrite: clear the span the new clip will occupy on `track`, then place it.
///
/// Overwrite is a cover, not a ripple — nothing downstream moves, so the clip
/// lands at exactly `at` and the material already there is cut back to make room
/// for it. A clip wholly inside the span is removed, one that reaches into the
/// span is trimmed back to the edge it crosses, and one that spans the whole
/// window is cut at both edges so its two ends survive on either side.
///
/// Atomic: every step runs through the primitive actions, and a rejected step
/// reverts the ones already applied before the error propagates. The inverse is
/// one history entry that puts the covered material back.
pub fn execute(
    ctx: &mut ApplyContext<'_>,
    track: TrackId,
    media: MediaId,
    source: TimeRange,
    at: RationalTime,
) -> Result<(ClipId, Box<dyn EditAction>), EngineError> {
    let tl_rate = ctx.project.timeline().frame_rate;
    // Mirror Project::add_clip's source→timeline resampling so the span cleared
    // is exactly the span the clip will occupy.
    let duration_ticks = resample(source.duration, tl_rate).value.max(1);
    let start = at.value;
    let end = start + duration_ticks;

    let mut inverses: Vec<Box<dyn EditAction>> = Vec::new();
    for step in cover_plan(ctx, track, start, end) {
        if let Err(err) = clear(ctx, step, start, end, tl_rate, &mut inverses) {
            rollback(ctx, inverses);
            return Err(err);
        }
    }
    match add_clip::execute(ctx, track, media, source, at) {
        Ok((id, add_inverse)) => {
            inverses.push(add_inverse);
            Ok((id, Box::new(CompoundAction { actions: inverses })))
        }
        Err(err) => {
            rollback(ctx, inverses);
            Err(err)
        }
    }
}

/// What one clip on the lane has to give up for the span.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Cover {
    /// Wholly inside the span: it has nothing left to show.
    Remove(ClipId),
    /// Reaches into the span from one side: keep the part outside it.
    Trim(ClipId, i64, i64),
    /// Spans the whole window: cut at both edges, drop the middle.
    Straddle(ClipId),
}

/// What clearing `[start, end)` on `track` costs each clip, read from the
/// project once so the loop that edits is not iterating what it mutates.
///
/// Public to the crate because it is the whole rule of overwrite: which clips
/// are touched, and how, is asserted directly rather than through six timelines.
pub(crate) fn cover_plan(
    ctx: &ApplyContext<'_>,
    track: TrackId,
    start: i64,
    end: i64,
) -> Vec<Cover> {
    let Some(lane) = ctx.project.timeline().track(track) else {
        return Vec::new();
    };
    let mut plan = Vec::new();
    for clip in lane.clips_ordered() {
        let (clip_start, clip_end) = (clip.timeline.start.value, clip.timeline.end_tick());
        if clip_end <= start || clip_start >= end {
            continue;
        }
        plan.push(match (clip_start >= start, clip_end <= end) {
            (true, true) => Cover::Remove(clip.id),
            (false, true) => Cover::Trim(clip.id, clip_start, start),
            (true, false) => Cover::Trim(clip.id, end, clip_end),
            (false, false) => Cover::Straddle(clip.id),
        });
    }
    plan
}

/// An atomic action for undoing a straddle clear.
pub struct StraddleClearAction {
    pub track: TrackId,
    pub original: Clip,
    pub head_clip: Clip,
    pub tail_clip: Clip,
}

impl EditAction for StraddleClearAction {
    fn apply(
        self: Box<Self>,
        ctx: &mut ApplyContext<'_>,
    ) -> Result<Box<dyn EditAction>, EngineError> {
        ctx.project.timeline_mut().remove_clip(self.tail_clip.id);
        ctx.project
            .timeline_mut()
            .replace_clip(self.original.clone())?;
        Ok(Box::new(ReapplyStraddleAction {
            track: self.track,
            original: self.original,
            head_clip: self.head_clip,
            tail_clip: self.tail_clip,
        }))
    }
}

pub struct ReapplyStraddleAction {
    pub track: TrackId,
    pub original: Clip,
    pub head_clip: Clip,
    pub tail_clip: Clip,
}

impl EditAction for ReapplyStraddleAction {
    fn apply(
        self: Box<Self>,
        ctx: &mut ApplyContext<'_>,
    ) -> Result<Box<dyn EditAction>, EngineError> {
        ctx.project
            .timeline_mut()
            .replace_clip(self.head_clip.clone())?;
        ctx.project
            .timeline_mut()
            .add_clip(self.track, self.tail_clip.clone())?;
        Ok(Box::new(StraddleClearAction {
            track: self.track,
            original: self.original,
            head_clip: self.head_clip,
            tail_clip: self.tail_clip,
        }))
    }
}

/// Apply one plan step, pushing the inverses it produced.
///
/// Crate-visible so a four-point edit clears its span exactly the way an
/// overwrite does: two rules for what a cover costs a clip would be one rule
/// too many.
pub(crate) fn clear(
    ctx: &mut ApplyContext<'_>,
    step: Cover,
    start: i64,
    end: i64,
    tl_rate: aria_models::Rational,
    inverses: &mut Vec<Box<dyn EditAction>>,
) -> Result<(), EngineError> {
    match step {
        Cover::Remove(clip) => {
            inverses.push(Box::new(RemoveClipAction { clip }).apply(ctx)?);
        }
        Cover::Trim(clip, from, to) => {
            let keep = TimeRange::at_rate(from, to - from, tl_rate);
            inverses.push(trim_clip::trim_one(ctx, clip, keep)?);
        }
        Cover::Straddle(clip) => {
            let original = ctx
                .project
                .timeline()
                .clip(clip)
                .cloned()
                .ok_or(ModelError::UnknownClip(clip))?;
            let track = ctx
                .project
                .timeline()
                .track_of(clip)
                .ok_or(ModelError::UnknownClip(clip))?;

            // Cut 1 at start
            let rest = match ctx
                .project
                .split_clip(clip, RationalTime::new(start, tl_rate))
            {
                Ok(r) => r,
                Err(err) => return Err(err.into()),
            };

            // Cut 2 at end
            let tail = match ctx
                .project
                .split_clip(rest, RationalTime::new(end, tl_rate))
            {
                Ok(t) => t,
                Err(err) => {
                    // Rollback Cut 1
                    ctx.project.timeline_mut().remove_clip(rest);
                    ctx.project.timeline_mut().replace_clip(original)?;
                    return Err(err.into());
                }
            };

            // Remove the covered middle slice
            ctx.project.timeline_mut().remove_clip(rest);

            let head_clip = ctx
                .project
                .timeline()
                .clip(clip)
                .cloned()
                .ok_or(ModelError::UnknownClip(clip))?;
            let tail_clip = ctx
                .project
                .timeline()
                .clip(tail)
                .cloned()
                .ok_or(ModelError::UnknownClip(tail))?;

            inverses.push(Box::new(StraddleClearAction {
                track,
                original,
                head_clip,
                tail_clip,
            }));
        }
    }
    Ok(())
}

/// Revert applied steps, newest first, so a refused overwrite leaves no trace.
fn rollback(ctx: &mut ApplyContext<'_>, inverses: Vec<Box<dyn EditAction>>) {
    for inverse in inverses.into_iter().rev() {
        if inverse.apply(ctx).is_err() {
            tracing::error!("overwrite rollback failed; the lane may hold a partial edit");
            break;
        }
    }
}
