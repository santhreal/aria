//! Row: 6 of the cases in [`super`].
//!
//! Split out of `menu_reach_tests.rs` to stay under the file ceiling. A pure
//! move: only `#[test]` fns live here. Every fixture and helper stays in the
//! parent, reached through `use super::*`, so no two children of this parent
//! reference each other.

use super::*;

#[test]
fn the_table_names_every_menu_row_and_nothing_else() {
    let live: BTreeSet<String> = live_rows()
        .into_iter()
        .map(|(_, id)| id)
        .filter(|id| by_prefix(id).is_none())
        .collect();
    let declared: BTreeSet<String> = ROWS.iter().map(|(id, ..)| id.to_string()).collect();
    let missing: Vec<_> = live.difference(&declared).collect();
    let stale: Vec<_> = declared.difference(&live).collect();
    assert!(
        missing.is_empty() && stale.is_empty(),
        "undeclared rows {missing:?}, declared rows that no longer exist {stale:?}"
    );
}
#[test]
fn every_row_does_what_it_says() {
    // Collected rather than asserted one at a time: one run should name every
    // row that drifted, not the first.
    let mut wrong = Vec::new();
    for row in &CENSUS.rows {
        match (row.kind, row.wrote.is_empty()) {
            (Edits, true) => wrong.push(format!(
                "{} declared Edits against {:?} and sent nothing",
                row.id, row.dress
            )),
            (NoEdit, false) => wrong.push(format!(
                "{} declared NoEdit against {:?} and sent {:?}",
                row.id, row.dress, row.wrote
            )),
            _ => {}
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
#[test]
fn every_command_the_menu_sends_is_declared_reachable() {
    // The inverse direction of `Reach`: a command the bar demonstrably sends
    // must not be declared internal.
    let labels = measured();
    for cmd in every_edit_command() {
        if labels.contains(cmd.label()) {
            assert!(
                cmd.reach().is_operator(),
                "{} is declared unreachable but the menu bar sends it",
                cmd.label()
            );
        }
    }
}
#[test]
fn every_row_that_asks_for_a_path_is_driven_with_an_answer() {
    // Derived from the live menus rather than declared twice: a row is run and
    // the state is asked whether it asked for a path. A new row that opens a
    // dialog is red here until it is declared and driven. Reaching this line at
    // all is the other half of the proof: a row that blocked on a dialog would
    // never return.
    let declared: BTreeSet<&str> = PICK_ROWS.iter().map(|(id, _)| *id).collect();
    let undeclared: Vec<&String> = CENSUS
        .asking_picks
        .iter()
        .filter(|id| !declared.contains(id.as_str()))
        .collect();
    assert!(
        undeclared.is_empty(),
        "these rows ask for a path and no test answers one: {undeclared:?}"
    );
}
#[test]
fn every_row_that_opens_a_form_is_committed() {
    // Derived from the live menus: each row runs against its own dressing, and a
    // row that leaves a dialog up whose Enter runs something must be driven with
    // Enter pressed or exempted with a reason. A new form row is red here until
    // someone decides which.
    let committed: BTreeSet<&str> = FORM_ROWS.iter().map(|(id, _)| *id).collect();
    let exempt: BTreeSet<&str> = FORM_EXEMPT.iter().map(|(id, _)| *id).collect();
    let forms: BTreeSet<String> = CENSUS
        .rows
        .iter()
        .filter(|row| row.opened_form)
        .map(|row| row.id.clone())
        .collect();
    let undeclared: Vec<&String> = forms
        .iter()
        .filter(|id| !committed.contains(id.as_str()) && !exempt.contains(id.as_str()))
        .collect();
    let stale: Vec<&&str> = committed
        .iter()
        .chain(&exempt)
        .filter(|id| !forms.contains(**id))
        .collect();
    assert!(
        undeclared.is_empty() && stale.is_empty(),
        "rows that open a form nothing commits: {undeclared:?}; declared rows that \
         open no form: {stale:?}"
    );
}
#[test]
fn opening_a_project_asks_for_one_and_loads_what_was_chosen() {
    // The dressing writes a second project of two clips and answers with it, so
    // the load is visible in the clip count. The same dressing the census drives.
    let (state, path) = Answered::Open.state();
    assert_eq!(state.store.sequence.clip_count(), 1);
    let cx = gpui::TestAppContext::single();
    let entity = entity_of(state, &cx);

    run_row(&entity, &cx, AppMenu::File, "file-open");

    cx.update(|app| {
        let state = entity.read(app);
        assert_eq!(state.picks_asked(), 1, "the row did not ask for a project");
        assert_eq!(state.project_path.as_deref(), Some(path.as_path()));
        assert_eq!(
            state.store.sequence.clip_count(),
            2,
            "the chosen project did not load"
        );
    });
}
#[test]
fn opening_a_project_with_unsaved_work_asks_the_prompt_before_the_dialog() {
    // The guard comes first on purpose: cancelling the prompt must cost no file
    // dialog, so the pick happens after the answer and not before it.
    let (mut state, _) = with_clip();
    assert!(state.store.is_dirty, "the fixture must have unsaved edits");
    state.supply_pick(Answer::file(scratch("guard").join("never.aria")));
    let cx = gpui::TestAppContext::single();
    let entity = entity_of(state, &cx);

    run_row(&entity, &cx, AppMenu::File, "file-open");

    cx.update(|app| {
        let state = entity.read(app);
        assert_eq!(state.dialog, crate::app::DialogKind::UnsavedChanges);
        assert_eq!(
            state.picks_asked(),
            0,
            "the dialog opened before the prompt"
        );
    });
    cx.update(|app| entity.update(app, |state: &mut AppState, _| state.cancel_pending()));
    cx.update(|app| {
        assert_eq!(
            entity.read(app).picks_asked(),
            0,
            "a cancelled prompt still opened a dialog"
        );
    });
}
