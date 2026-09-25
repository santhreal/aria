//! Row: 6 of the cases in [`super`].
//!
//! Split out of `menu_reach_tests.rs` to stay under the file ceiling. A pure
//! move: only `#[test]` fns live here. Every fixture and helper stays in the
//! parent, reached through `use super::*`, so no two children of this parent
//! reference each other.

use super::*;

#[test]
fn the_table_names_every_menu_row_and_nothing_else() {
    let live: BTreeSet<String> = live_rows().into_iter().map(|(_, id)| id).collect();
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
    for (menu, id) in live_rows() {
        let Some((kind, dress)) = row_of(&id) else {
            continue;
        };
        let produced = labels_from(menu, &id, dress);
        match (kind, produced.is_empty()) {
            (Edits, true) => wrong.push(format!(
                "{id} declared Edits against {dress:?} and sent nothing"
            )),
            (NoEdit, false) => wrong.push(format!(
                "{id} declared NoEdit against {dress:?} and sent {produced:?}"
            )),
            _ => {}
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
#[test]
fn every_command_the_menu_sends_is_declared_reachable() {
    // The inverse direction of `Reach`: a command an operator demonstrably sends
    // from the bar must not be declared internal.
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
    let cx = gpui::TestAppContext::single();
    let mut undeclared = Vec::new();
    for (menu, id) in live_rows() {
        let entity = entity_of(with_clip().0, &cx);
        run_row(&entity, &cx, menu, &id);
        let asked = cx.update(|app| entity.read(app).picks_asked());
        if asked > 0 && !declared.contains(id.as_str()) {
            undeclared.push(id);
        }
    }
    assert!(
        undeclared.is_empty(),
        "these rows ask for a path and no test answers one: {undeclared:?}"
    );
}
#[test]
fn opening_a_project_asks_for_one_and_loads_what_was_chosen() {
    // The dressing writes a second project of two clips and answers with it, so
    // the load is visible in the clip count. The same dressing the census drives.
    let (state, path) = Answered::Open.state();
    assert_eq!(clip_count(&state), 1);
    let cx = gpui::TestAppContext::single();
    let entity = entity_of(state, &cx);

    run_row(&entity, &cx, AppMenu::File, "file-open");

    cx.update(|app| {
        let state = entity.read(app);
        assert_eq!(state.picks_asked(), 1, "the row did not ask for a project");
        assert_eq!(state.project_path.as_deref(), Some(path.as_path()));
        assert_eq!(clip_count(state), 2, "the chosen project did not load");
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
