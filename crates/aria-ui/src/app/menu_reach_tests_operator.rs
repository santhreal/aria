//! Operator: 5 of the cases in [`super`].
//!
//! Split out of `menu_reach_tests.rs` to stay under the file ceiling. A pure
//! move: only `#[test]` fns live here. Every fixture and helper stays in the
//! parent, reached through `use super::*`, so no two children of this parent
//! reference each other.

use super::*;

#[test]
fn save_as_writes_the_project_where_the_operator_said() {
    // The dressings are the census's own, so a row that stops working fails here
    // and in the census against the same state.
    let (state, path) = Answered::SaveAs.state();
    let cx = gpui::TestAppContext::single();
    let entity = entity_of(state, &cx);

    run_row(&entity, &cx, AppMenu::File, "file-save-as");

    cx.update(|app| {
        let state = entity.read(app);
        assert_eq!(state.picks_asked(), 1);
        assert_eq!(state.project_path.as_deref(), Some(path.as_path()));
    });
    assert!(path.exists(), "the project was not written");
}
#[test]
fn import_adds_the_files_the_operator_chose() {
    let (state, staged) = Answered::Import.state();
    let before = state.store.media.len();
    let cx = gpui::TestAppContext::single();
    let entity = entity_of(state, &cx);

    run_row(&entity, &cx, AppMenu::File, "file-import");

    cx.update(|app| {
        let state = entity.read(app);
        assert_eq!(state.picks_asked(), 1);
        assert_eq!(
            state.store.media.len(),
            before + 1,
            "the chosen file is not in the pool"
        );
        assert!(
            state
                .store
                .media
                .iter()
                .any(|item| std::path::Path::new(&*item.path) == staged),
            "the pool gained an entry for some other file"
        );
    });
}
#[test]
fn save_as_template_writes_the_bundle_the_operator_named() {
    let (state, path) = Answered::SaveTemplate.state();
    let cx = gpui::TestAppContext::single();
    let entity = entity_of(state, &cx);

    run_row(&entity, &cx, AppMenu::File, "file-save-template");

    cx.update(|app| assert_eq!(entity.read(app).picks_asked(), 1));
    assert!(path.exists(), "the bundle was not written");
}
#[test]
fn use_template_replaces_the_session_with_the_chosen_bundle() {
    let (state, _) = Answered::UseTemplate.state();
    assert_eq!(clip_count(&state), 0, "the dressing left the session full");
    let cx = gpui::TestAppContext::single();
    let entity = entity_of(state, &cx);

    run_row(&entity, &cx, AppMenu::File, "file-use-template");

    cx.update(|app| {
        let state = entity.read(app);
        assert_eq!(state.picks_asked(), 1);
        assert_eq!(clip_count(state), 1, "the template did not land");
        assert_eq!(state.route, crate::types::UiRoute::Editor);
    });
}
#[test]
fn relink_all_repairs_the_media_from_the_chosen_folder() {
    let (state, _) = Answered::RelinkAll.state();
    assert_eq!(
        state.store.media.iter().filter(|item| item.missing).count(),
        1,
        "the dressing for a relink needs one missing file"
    );
    let cx = gpui::TestAppContext::single();
    let entity = entity_of(state, &cx);

    run_row(&entity, &cx, AppMenu::File, "file-relink-all");

    cx.update(|app| {
        let state = entity.read(app);
        assert_eq!(state.picks_asked(), 1);
        assert!(
            state.store.media.iter().all(|item| !item.missing),
            "the media was not relinked from the chosen folder"
        );
        assert_eq!(state.status_message.as_deref(), Some("Relinked 1"));
    });
}

/// The incident, as a test: **"Open data dir" opened a file-manager window on
/// the machine running the suite.**
///
/// `measured()` above runs every live menu row, this row reveals a folder, and
/// the reveal used to spawn `xdg-open` unconditionally — so `cargo test`
/// launched the operator's file manager. The row still does its whole job here;
/// what changed is that starting another application needs the headed editor's
/// permission, which a test process never gives.
#[test]
fn opening_the_data_dir_asks_for_a_folder_without_launching_anything() {
    let (state, _) = with_clip();
    let data_dir = state.data_dir.clone();
    crate::app::launch::forget_last_folder();
    let cx = gpui::TestAppContext::single();
    let entity = entity_of(state, &cx);

    run_row(&entity, &cx, AppMenu::Help, "help-data");

    assert!(
        !crate::app::launch::desktop_launch_allowed(),
        "a test process must never be allowed to start another application"
    );
    assert_eq!(
        crate::app::launch::last_folder_asked_for().as_deref(),
        Some(data_dir.as_path()),
        "the row still asks for the data dir; only the launch is withheld"
    );
}
