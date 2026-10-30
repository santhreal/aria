//! Command dispatch: undoable timeline edits and session project commands.

mod dispatch;
pub(crate) mod edit;
pub(crate) mod project;

pub use dispatch::ApplyOutcome;
pub(crate) use dispatch::dispatch;

use std::borrow::Cow;

use aria_models::Project;

use crate::error::EngineError;

/// Session surface passed to every command handler.
pub(crate) struct ApplyContext<'a> {
    pub project: &'a mut Project,
    pub project_path: &'a mut Option<std::path::PathBuf>,
    pub history: &'a mut History,
}

/// A runtime edit action. Consuming `apply` runs the edit and returns its inverse.
pub(crate) trait EditAction: Send {
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
pub(crate) struct HistoryStep {
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
pub(crate) struct History {
    undo: Vec<HistoryStep>,
    redo: Vec<HistoryStep>,
    limit: usize,
    pending: Option<Vec<HistoryStep>>,
    /// Advanced by every change to `undo` or `redo`; see [`Self::generation`].
    generation: u64,
}

impl History {
    pub(crate) fn new(limit: usize) -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            limit: limit.max(1),
            pending: None,
            generation: 0,
        }
    }

    /// A number that changes whenever the applied or undone steps change: a
    /// recorded step, a committed group, an undo, a redo, a merge, or a clear.
    /// Two equal generations list the same steps under the same labels, so a
    /// reader holding a copy of the list compares this instead of the list.
    ///
    /// A committed group or a merge changes the list without changing the
    /// project, so a project revision does not stand in for this.
    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    fn advance(&mut self) {
        self.generation = self.generation.wrapping_add(1);
    }

    pub(crate) fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub(crate) fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub(crate) fn in_group(&self) -> bool {
        self.pending.is_some()
    }

    /// Applied labels, oldest first, borrowed from their history steps.
    pub(crate) fn undo_labels(&self) -> impl DoubleEndedIterator<Item = &str> + ExactSizeIterator {
        self.undo.iter().map(|step| step.label.as_ref())
    }

    /// Undone labels, oldest first, borrowed from their history steps.
    ///
    /// The redo stack is ordered newest-last, so this reverses it.
    pub(crate) fn redo_labels(&self) -> impl DoubleEndedIterator<Item = &str> + ExactSizeIterator {
        self.redo.iter().rev().map(|step| step.label.as_ref())
    }

    /// Start collecting recorded inverses into a single compound entry.
    /// Groups don't nest; a stray second begin keeps the open group.
    pub(crate) fn begin_group(&mut self) {
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
    pub(crate) fn commit_group(&mut self) {
        debug_assert!(self.pending.is_some(), "no history group open");
        let Some(steps) = self.pending.take() else {
            return;
        };
        let step = match <[HistoryStep; 1]>::try_from(steps) {
            Ok([step]) => step,
            Err(steps) => {
                let Some(first) = steps.first() else {
                    return;
                };
                let label = format!("{} +{}", first.label, steps.len() - 1);
                HistoryStep {
                    label: Cow::Owned(label),
                    action: Box::new(CompoundAction {
                        actions: steps.into_iter().map(|step| step.action).collect(),
                    }),
                }
            }
        };
        self.record_step(step);
    }

    /// Detach the open group's collected inverses without recording them
    /// (for rollback: the caller applies them in reverse order).
    pub(crate) fn take_group(&mut self) -> Vec<Box<dyn EditAction>> {
        debug_assert!(self.pending.is_some(), "no history group open");
        self.pending
            .take()
            .unwrap_or_default()
            .into_iter()
            .map(|step| step.action)
            .collect()
    }

    /// Steps collected by the open group so far; zero with none open.
    pub(crate) fn group_len(&self) -> usize {
        self.pending.as_ref().map_or(0, Vec::len)
    }

    /// Detach the open group's steps after the first `mark`, leaving the group
    /// open with the earlier ones (for a savepoint rollback: the caller applies
    /// them in reverse order).
    pub(crate) fn take_group_from(&mut self, mark: usize) -> Vec<Box<dyn EditAction>> {
        let Some(pending) = self.pending.as_mut() else {
            return Vec::new();
        };
        if mark >= pending.len() {
            return Vec::new();
        }
        pending.drain(mark..).map(|step| step.action).collect()
    }

    /// Close the open group and record it as one undo entry named `label`,
    /// whatever its commands are called. No-op when the group is empty.
    pub(crate) fn commit_group_as(&mut self, label: Cow<'static, str>) {
        debug_assert!(self.pending.is_some(), "no history group open");
        let Some(steps) = self.pending.take() else {
            return;
        };
        let action: Box<dyn EditAction> = match <[HistoryStep; 1]>::try_from(steps) {
            Ok([step]) => step.action,
            Err(steps) if steps.is_empty() => return,
            Err(steps) => Box::new(CompoundAction {
                actions: steps.into_iter().map(|step| step.action).collect(),
            }),
        };
        self.record_step(HistoryStep { label, action });
    }

    pub(crate) fn record_do(&mut self, label: &'static str, inverse: Box<dyn EditAction>) {
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
        self.advance();
    }

    pub(crate) fn pop_undo(&mut self) -> Option<HistoryStep> {
        let step = self.undo.pop();
        if step.is_some() {
            self.advance();
        }
        step
    }

    pub(crate) fn push_redo(&mut self, step: HistoryStep) {
        if self.redo.len() >= self.limit {
            self.redo.remove(0);
        }
        self.redo.push(step);
        self.advance();
    }

    pub(crate) fn pop_redo(&mut self) -> Option<HistoryStep> {
        let step = self.redo.pop();
        if step.is_some() {
            self.advance();
        }
        step
    }

    pub(crate) fn push_undo(&mut self, step: HistoryStep) {
        if self.undo.len() >= self.limit {
            self.undo.remove(0);
        }
        self.undo.push(step);
        self.advance();
    }
    /// Merge the two newest undo steps into one compound step keeping the
    /// earlier step's label.
    pub(crate) fn coalesce_top_two(&mut self) {
        let grouped = self.pending.is_some();
        let stack = self.pending.as_mut().unwrap_or(&mut self.undo);
        if stack.len() < 2 {
            return;
        }
        let newer = stack.pop().expect("len >= 2");
        let older = stack.pop().expect("len >= 2");
        stack.push(HistoryStep {
            label: older.label,
            action: Box::new(CompoundAction {
                actions: vec![older.action, newer.action],
            }),
        });
        // A merge inside an open group changes nothing a reader lists.
        if !grouped {
            self.advance();
        }
    }

    pub(crate) fn clear(&mut self) {
        let listed = !self.undo.is_empty() || !self.redo.is_empty();
        self.undo.clear();
        self.redo.clear();
        self.pending = None;
        if listed {
            self.advance();
        }
    }
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
