use aria_models::{Clip, ClipId, ModelError, RationalTime, TrackId};

use crate::action::edit::shift_clips;
use crate::action::{ApplyContext, CompoundAction, EditAction};
use crate::error::EngineError;

pub struct RippleDeleteSingleAction {
    pub clip: ClipId,
}

pub struct RestoreRippleAction {
    pub track: TrackId,
    pub clip: Clip,
}

pub fn execute(
    ctx: &mut ApplyContext<'_>,
    clip: ClipId,
) -> Result<Box<dyn EditAction>, EngineError> {
    let track = ctx
        .project
        .timeline()
        .track_of(clip)
        .ok_or(ModelError::UnknownClip(clip))?;
    let clip_obj = ctx
        .project
        .timeline()
        .clip(clip)
        .cloned()
        .ok_or(ModelError::UnknownClip(clip))?;
    let gap_start = clip_obj.timeline.start;
    let gap_end = RationalTime::new(
        clip_obj.timeline.end_tick(),
        clip_obj.timeline.duration.rate,
    );

    let mut inverses: Vec<Box<dyn EditAction>> = Vec::new();
    let removed = ctx.project.ripple_delete(clip)?;
    inverses.push(Box::new(RestoreRippleAction {
        track,
        clip: removed,
    }));

    // The rider lanes lose the same stretch of timeline, not just the shift.
    //
    // Shifting a rider from the span's start dragged its material below tick 0
    // and the model refused the whole edit; shifting from the span's end left
    // that material sitting where the following clips were about to land, and the
    // model refused that too. Either way a sync-locked ripple delete could not
    // succeed on the ordinary layout of picture over its own sound. Closing the
    // span on each rider lane is the operation that keeps the lanes in step.
    if let Err(err) =
        shift_clips::close_for_ripple_delete(ctx, &[track], gap_start, gap_end, &mut inverses)
    {
        for inv in inverses.into_iter().rev() {
            if inv.apply(ctx).is_err() {
                tracing::error!("ripple-delete rollback failed");
                break;
            }
        }
        return Err(err);
    }

    if inverses.len() == 1 {
        Ok(inverses.pop().unwrap())
    } else {
        Ok(Box::new(CompoundAction { actions: inverses }))
    }
}

impl EditAction for RippleDeleteSingleAction {
    fn apply(
        self: Box<Self>,
        ctx: &mut ApplyContext<'_>,
    ) -> Result<Box<dyn EditAction>, EngineError> {
        let track = ctx
            .project
            .timeline()
            .track_of(self.clip)
            .ok_or(ModelError::UnknownClip(self.clip))?;
        let removed = ctx.project.ripple_delete(self.clip)?;
        Ok(Box::new(RestoreRippleAction {
            track,
            clip: removed,
        }))
    }
}

impl EditAction for RestoreRippleAction {
    fn apply(
        self: Box<Self>,
        ctx: &mut ApplyContext<'_>,
    ) -> Result<Box<dyn EditAction>, EngineError> {
        let gap_start = self.clip.timeline.start;
        let gap = self.clip.timeline.duration;
        ctx.project.shift_clips(self.track, gap_start, gap)?;
        let id = self.clip.id;
        ctx.project.timeline_mut().add_clip(self.track, self.clip)?;
        Ok(Box::new(RippleDeleteSingleAction { clip: id }))
    }
}
