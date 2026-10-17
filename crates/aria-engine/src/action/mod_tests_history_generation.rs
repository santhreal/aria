//! `History::generation` changes whenever the listed steps change.
//!
//! The class this closes: a cached copy of the history list keyed on the
//! project revision. A committed group and a merged pair change the list
//! without touching the project, so the History panel kept showing the steps
//! from before the commit until some later edit. Every mutator is driven here,
//! including the one that keeps the list's length (a commit at the undo limit).
//!
//! Not caught: a new mutator added to `History` and not listed below. The
//! negative cases pin the operations that must leave the generation alone.

use super::*;

/// The labels a reader lists: applied, then undone.
fn listed(history: &History) -> (Vec<String>, Vec<String>) {
    (
        history.undo_labels().map(str::to_owned).collect(),
        history.redo_labels().map(str::to_owned).collect(),
    )
}

fn named(label: &'static str) -> HistoryStep {
    HistoryStep {
        label: Cow::Borrowed(label),
        action: Box::new(NoopAction),
    }
}

/// Two applied steps and one undone, at a limit of `limit`.
fn stocked(limit: usize) -> History {
    let mut history = History::new(limit);
    history.record_do("a", Box::new(NoopAction));
    history.record_do("b", Box::new(NoopAction));
    let undone = history.pop_undo().expect("b");
    history.push_redo(undone);
    history.record_step(named("c"));
    // `record_step` cleared the redo stack; put one back.
    history.push_redo(named("r"));
    history
}

type Op = fn(&mut History);

#[test]
fn every_change_to_the_list_advances_the_generation() {
    let ops: [(&str, Op); 9] = [
        ("record a step", |h| h.record_do("d", Box::new(NoopAction))),
        ("undo", |h| {
            let step = h.pop_undo().expect("an applied step");
            h.push_redo(step);
        }),
        ("redo", |h| {
            let step = h.pop_redo().expect("an undone step");
            h.push_undo(step);
        }),
        ("commit a group of one", |h| {
            h.begin_group();
            h.record_step(named("g"));
            h.commit_group();
        }),
        ("commit a group of two", |h| {
            h.begin_group();
            h.record_step(named("g"));
            h.record_step(named("h"));
            h.commit_group();
        }),
        ("commit a named group", |h| {
            h.begin_group();
            h.record_step(named("g"));
            h.commit_group_as(Cow::Borrowed("Named"));
        }),
        ("merge the newest two", |h| h.coalesce_top_two()),
        ("clear", |h| h.clear()),
        ("pop an applied step", |h| {
            h.pop_undo();
        }),
    ];
    for (name, op) in ops {
        let mut history = stocked(16);
        let (before, generation) = (listed(&history), history.generation());
        op(&mut history);
        assert_ne!(listed(&history), before, "{name}: the case changes nothing");
        assert_ne!(
            history.generation(),
            generation,
            "{name} changed the list and kept the generation"
        );
    }
}

#[test]
fn a_commit_at_the_limit_keeps_the_length_and_advances_the_generation() {
    let mut history = History::new(2);
    history.record_do("a", Box::new(NoopAction));
    history.record_do("b", Box::new(NoopAction));
    let generation = history.generation();
    history.begin_group();
    history.record_step(named("g"));
    history.record_step(named("h"));
    history.commit_group();
    assert_eq!(
        history.undo_labels().len(),
        2,
        "the oldest step dropped off"
    );
    assert_ne!(history.generation(), generation);
}

#[test]
fn what_a_reader_cannot_see_leaves_the_generation_alone() {
    let ops: [(&str, Op); 5] = [
        ("record inside an open group", |h| {
            h.begin_group();
            h.record_step(named("g"));
        }),
        ("merge inside an open group", |h| {
            h.begin_group();
            h.record_step(named("g"));
            h.record_step(named("h"));
            h.coalesce_top_two();
        }),
        ("commit an empty group", |h| {
            h.begin_group();
            h.commit_group();
        }),
        ("roll back a group", |h| {
            h.begin_group();
            h.record_step(named("g"));
            h.take_group();
        }),
        ("redo with nothing undone", |h| {
            h.pop_redo();
            h.pop_redo();
            h.pop_redo();
        }),
    ];
    for (name, op) in ops {
        let mut history = stocked(16);
        // The redo cases start from an empty redo stack.
        if name.starts_with("redo") {
            history.pop_redo();
        }
        let generation = history.generation();
        op(&mut history);
        assert_eq!(history.generation(), generation, "{name}");
    }
    let mut empty = History::new(4);
    empty.clear();
    assert_eq!(empty.generation(), 0, "clearing an empty history");
}
