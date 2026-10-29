//! Pure beat-marker edits.
//!
//! Beat analysis is asynchronous decode/DSP work and does not belong in pure
//! command dispatch: the detector runs as a job, and its result lands here as a
//! grid to store. Storing and clearing a grid are pure, and the inverse holds
//! only the prior beat list, so undo cannot replace unrelated clip fields.

use aria_models::ClipId;

use crate::action::{ApplyContext, EditAction};
use crate::error::EngineError;

/// Replace a media clip's beat grid and return an inverse restoring the prior
/// one.
///
/// The model normalises `beats` (sorted, deduped, clamped to the clip's source
/// window), so a detector may hand over raw candidates in any order.
pub(crate) fn set(
    ctx: &mut ApplyContext<'_>,
    clip: ClipId,
    beats: Vec<i64>,
) -> Result<Box<dyn EditAction>, EngineError> {
    let previous = ctx.project.set_clip_beats(clip, beats)?;
    Ok(Box::new(SetClipBeatsAction {
        clip,
        beats: previous,
    }))
}

/// Clear a media clip's beat markers and return an inverse restoring only them.
///
/// An already-empty grid still produces an inverse. At the command layer that
/// makes `ClearBeats` one deterministic, reversible history step whose
/// undo/redo both leave the empty grid unchanged.
pub(crate) fn clear(
    ctx: &mut ApplyContext<'_>,
    clip: ClipId,
) -> Result<Box<dyn EditAction>, EngineError> {
    set(ctx, clip, Vec::new())
}

/// Swap one clip's beat grid with a captured value.
///
/// This is deliberately narrower than a whole-clip snapshot: actions recorded
/// after this one may evolve other clip properties without this inverse owning
/// or overwriting them.
struct SetClipBeatsAction {
    clip: ClipId,
    beats: Vec<i64>,
}

impl EditAction for SetClipBeatsAction {
    fn apply(
        self: Box<Self>,
        ctx: &mut ApplyContext<'_>,
    ) -> Result<Box<dyn EditAction>, EngineError> {
        let Self { clip, beats } = *self;
        let beats = ctx.project.set_clip_beats(clip, beats)?;
        Ok(Box::new(Self { clip, beats }))
    }
}
