//! The session history: a failed undo or redo, history labels, grouped
//! rollback, and a jump that cannot reach its depth.

use super::*;

/// An inverse that always fails, simulating a corrupted/mismatched history
/// entry.
struct FailAction;

impl EditAction for FailAction {
    fn apply(
        self: Box<Self>,
        _ctx: &mut ApplyContext<'_>,
    ) -> Result<Box<dyn EditAction>, EngineError> {
        Err(aria_models::ModelError::InvalidRange.into())
    }
}

struct NoopAction;

impl EditAction for NoopAction {
    fn apply(
        self: Box<Self>,
        _ctx: &mut ApplyContext<'_>,
    ) -> Result<Box<dyn EditAction>, EngineError> {
        Ok(Box::new(NoopAction))
    }
}

fn step(action: Box<dyn EditAction>) -> HistoryStep {
    HistoryStep {
        label: std::borrow::Cow::Borrowed("Test step"),
        action,
    }
}

#[test]
fn failed_undo_clears_both_stacks() {
    let mut engine = Engine::new(EngineConfig::default()).expect("engine");
    // A redo entry that WOULD apply, plus a failing undo entry on top.
    engine.history.push_redo(step(Box::new(NoopAction)));
    engine.history.push_undo(step(Box::new(FailAction)));
    assert!(engine.can_undo() && engine.can_redo());

    assert!(!engine.undo(), "a failing inverse reports no step");
    assert!(
        !engine.can_undo() && !engine.can_redo(),
        "a diverged history is dropped wholesale rather than trusted"
    );
}

#[test]
fn failed_redo_clears_both_stacks() {
    let mut engine = Engine::new(EngineConfig::default()).expect("engine");
    engine.history.push_undo(step(Box::new(NoopAction)));
    engine.history.push_redo(step(Box::new(FailAction)));
    assert!(engine.can_undo() && engine.can_redo());

    assert!(!engine.redo());
    assert!(!engine.can_undo() && !engine.can_redo());
}

#[test]
fn history_entries_borrow_static_and_grouped_labels() {
    let mut engine = Engine::new(EngineConfig::default()).expect("engine");
    engine.history.push_undo(step(Box::new(NoopAction)));
    engine.history.begin_group();
    engine
        .history
        .record_do("Grouped step", Box::new(NoopAction));
    engine
        .history
        .record_do("Second step", Box::new(NoopAction));
    engine.history.commit_group();

    let stored: Vec<(*const u8, usize)> = engine
        .history
        .undo_labels()
        .map(|label| (label.as_ptr(), label.len()))
        .collect();
    let entries = engine.history_entries();
    assert_eq!(entries.len(), stored.len());
    for (entry, (ptr, len)) in entries.iter().zip(stored) {
        assert!(
            matches!(entry.label, Cow::Borrowed(_)),
            "history inspection allocated an owned label"
        );
        assert_eq!((entry.label.as_ptr(), entry.label.len()), (ptr, len));
    }
}

#[test]
fn grouped_rollback_bumps_revision_once_for_every_group_size() {
    for action_count in [1, 2, 8] {
        let mut engine = Engine::new(EngineConfig::default()).expect("engine");
        engine.begin_group();
        for _ in 0..action_count {
            engine.history.record_do("Test step", Box::new(NoopAction));
        }
        let before = engine.revision();

        engine.rollback_group();

        assert_eq!(
            engine.revision(),
            before + 1,
            "{action_count} reverted actions must publish one restored state"
        );
    }
}

#[test]
fn a_jump_that_cannot_reach_its_depth_reports_failure() {
    let mut engine = Engine::new(EngineConfig::default()).expect("engine");
    engine.history.push_undo(step(Box::new(NoopAction)));
    // Nothing to redo, so a depth past the end is unreachable and the walk
    // stops instead of spinning.
    assert!(!engine.history_jump(4));
    assert_eq!(engine.history_depth(), 1);
    assert!(engine.history_jump(0), "walking back is reachable");
    assert_eq!(engine.history_depth(), 0);
    assert_eq!(
        engine
            .history_entries()
            .into_iter()
            .map(|entry| (entry.label, entry.applied))
            .collect::<Vec<_>>(),
        vec![(Cow::Borrowed("Test step"), false)],
        "an undone step stays in the list, marked unapplied"
    );
}
