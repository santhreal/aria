use aria_models::{TrackId, Transition};

use crate::action::{ApplyContext, EditAction};
use crate::error::EngineError;

/// Swap every track's transition set back to a captured snapshot. Used both as
/// the direct inverse of transition edits and, compounded with a structural
/// edit's inverse, to restore junctions pruned when an edit broke an abutment.
pub(crate) struct RestoreTransitionsAction {
    pub snapshot: Vec<(TrackId, Vec<Transition>)>,
}

impl EditAction for RestoreTransitionsAction {
    fn apply(
        self: Box<Self>,
        ctx: &mut ApplyContext<'_>,
    ) -> Result<Box<dyn EditAction>, EngineError> {
        let current = ctx.project.transitions_snapshot();
        ctx.project.restore_transitions(self.snapshot);
        Ok(Box::new(RestoreTransitionsAction { snapshot: current }))
    }
}

/// Compound a structural edit's primary inverse with a post-apply transition
/// snapshot so that both undo and redo finish moving/restoring clips before
/// pruning and restoring transitions on the affected lanes.
pub(crate) struct WithTransitionsAction {
    pub primary: Box<dyn EditAction>,
    pub snapshot: Vec<(TrackId, Vec<Transition>)>,
}

impl EditAction for WithTransitionsAction {
    fn apply(
        self: Box<Self>,
        ctx: &mut ApplyContext<'_>,
    ) -> Result<Box<dyn EditAction>, EngineError> {
        let current: Vec<(TrackId, Vec<Transition>)> = self
            .snapshot
            .iter()
            .map(|(track_id, _)| {
                let transitions = ctx
                    .project
                    .timeline()
                    .track(*track_id)
                    .map(|t| t.transitions().to_vec())
                    .unwrap_or_default();
                (*track_id, transitions)
            })
            .collect();
        let inverse_primary = self.primary.apply(ctx)?;
        ctx.project.prune_dead_transitions();
        ctx.project.restore_transitions(self.snapshot);
        Ok(Box::new(WithTransitionsAction {
            primary: inverse_primary,
            snapshot: current,
        }))
    }
}
