//! Command dispatch: undoable timeline edits and session project commands.

mod dispatch;
mod edit;
pub(crate) mod project;

pub use dispatch::{ApplyOutcome, dispatch};

use std::borrow::Cow;

use aria_models::Project;

use crate::error::EngineError;

/// Session surface passed to every command handler.
pub struct ApplyContext<'a> {
    pub project: &'a mut Project,
    pub project_path: &'a mut Option<std::path::PathBuf>,
    pub history: &'a mut History,
}

/// A runtime edit action. Consuming `apply` runs the edit and returns its inverse.
pub trait EditAction: Send {
    fn apply(
        self: Box<Self>,
        ctx: &mut ApplyContext<'_>,
    ) -> Result<Box<dyn EditAction>, EngineError>;
}

/// Inverses for a multi-command gesture, undone/redone as one history entry.
///
/// `actions` holds the inverses in the order their commands were executed;
/// `apply` runs them in reverse (last command reverted first) and returns a
/// compound of the produced inverses, so the entry oscillates like any single
/// action. Also the inverse shape of engine-internal composites
/// (e.g. `RippleInsert` = shift + add).
pub(crate) struct CompoundAction {
    pub(crate) actions: Vec<Box<dyn EditAction>>,
}

impl EditAction for CompoundAction {
    fn apply(
        self: Box<Self>,
        ctx: &mut ApplyContext<'_>,
    ) -> Result<Box<dyn EditAction>, EngineError> {
        let mut inverses = Vec::with_capacity(self.actions.len());
        for action in self.actions.into_iter().rev() {
            match action.apply(ctx) {
                Ok(inverse) => inverses.push(inverse),
                Err(e) => {
                    // A partially-applied compound would corrupt state (some
                    // inner edits done, others not). Revert what we did apply
                    // — running the produced inverses in reverse — so the
                    // compound is all-or-nothing.
                    for inverse in inverses.into_iter().rev() {
                        if inverse.apply(ctx).is_err() {
                            tracing::error!(
                                "compound rollback failed after a partial apply; \
                                 state may be inconsistent"
                            );
                            break;
                        }
                    }
                    return Err(e);
                }
            }
        }
        Ok(Box::new(CompoundAction { actions: inverses }))
    }
}

/// One undoable step: the inverse action, and the name of the command that
/// produced it.
///
/// The label travels with the action rather than in a parallel list, so a
/// history surface cannot show a name that belongs to a different entry.
pub struct HistoryStep {
    pub label: Cow<'static, str>,
    pub action: Box<dyn EditAction>,
}

/// Inverse-command undo/redo stacks.
///
/// A *group* collects the inverses of several dispatched commands into one
/// undo entry (one gesture = one Ctrl+Z): [`begin_group`](Self::begin_group),
/// then dispatch the batch, then [`commit_group`](Self::commit_group). While a
/// group is open, recorded inverses are held aside and the redo stack is left
/// untouched, so an abandoned group (see `Engine::rollback_group`) leaves
/// history exactly as it was.
#[derive(Default)]
pub struct History {
    undo: Vec<HistoryStep>,
    redo: Vec<HistoryStep>,
    limit: usize,
    pending: Option<Vec<HistoryStep>>,
}

impl History {
    pub fn new(limit: usize) -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            limit: limit.max(1),
            pending: None,
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn in_group(&self) -> bool {
        self.pending.is_some()
    }

    /// Applied labels, oldest first, borrowed from their history steps.
    pub fn undo_labels(&self) -> impl DoubleEndedIterator<Item = &str> + ExactSizeIterator {
        self.undo.iter().map(|step| step.label.as_ref())
    }

    /// Undone labels, oldest first, borrowed from their history steps.
    ///
    /// The redo stack is ordered newest-last, so this reverses it.
    pub fn redo_labels(&self) -> impl DoubleEndedIterator<Item = &str> + ExactSizeIterator {
        self.redo.iter().rev().map(|step| step.label.as_ref())
    }

    /// Start collecting recorded inverses into a single compound entry.
    /// Groups don't nest; a stray second begin keeps the open group.
    pub fn begin_group(&mut self) {
        debug_assert!(self.pending.is_none(), "history group already open");
        if self.pending.is_none() {
            self.pending = Some(Vec::new());
        }
    }

    /// Close the open group and push it as one undo entry. No-op when the
    /// group is empty (the gesture made no edits) or no group is open.
    ///
    /// The entry is named after the gesture's first command, with the count of
    /// the rest: one gesture reads as one line, and `Move clip +2` says the
    /// undo covers more than the name alone.
    pub fn commit_group(&mut self) {
        debug_assert!(self.pending.is_some(), "no history group open");
        let Some(mut steps) = self.pending.take() else {
            return;
        };
        match steps.len() {
            0 => {}
            1 => {
                let step = steps.pop().expect("len checked");
                self.record_step(step);
            }
            n => {
                let label = steps
                    .first()
                    .map(|step| format!("{} +{}", step.label, n - 1))
                    .unwrap_or_else(|| "Edit".to_string());
                let actions = steps.into_iter().map(|step| step.action).collect();
                self.record_step(HistoryStep {
                    label: Cow::Owned(label),
                    action: Box::new(CompoundAction { actions }),
                });
            }
        }
    }

    /// Detach the open group's collected inverses without recording them
    /// (for rollback: the caller applies them in reverse order).
    pub fn take_group(&mut self) -> Vec<Box<dyn EditAction>> {
        debug_assert!(self.pending.is_some(), "no history group open");
        self.pending
            .take()
            .unwrap_or_default()
            .into_iter()
            .map(|step| step.action)
            .collect()
    }

    pub fn record_do(&mut self, label: &'static str, inverse: Box<dyn EditAction>) {
        self.record_step(HistoryStep {
            label: Cow::Borrowed(label),
            action: inverse,
        });
    }

    fn record_step(&mut self, step: HistoryStep) {
        if let Some(pending) = self.pending.as_mut() {
            // Redo clearing waits for commit_group so a rolled-back group
            // leaves the redo stack intact.
            pending.push(step);
            return;
        }
        if self.undo.len() >= self.limit {
            self.undo.remove(0);
        }
        self.undo.push(step);
        self.redo.clear();
    }

    pub fn pop_undo(&mut self) -> Option<HistoryStep> {
        self.undo.pop()
    }

    pub fn push_redo(&mut self, step: HistoryStep) {
        if self.redo.len() >= self.limit {
            self.redo.remove(0);
        }
        self.redo.push(step);
    }

    pub fn pop_redo(&mut self) -> Option<HistoryStep> {
        self.redo.pop()
    }

    pub fn push_undo(&mut self, step: HistoryStep) {
        if self.undo.len() >= self.limit {
            self.undo.remove(0);
        }
        self.undo.push(step);
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.pending = None;
    }
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
