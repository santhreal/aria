//! Operator: the File rows that ask for a path, from the cases in [`super`].
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
/// Save a Copy writes the chosen file and leaves the open project on its own
/// path, so the next Ctrl+S still writes where it did before.
#[test]
fn save_a_copy_writes_the_copy_and_keeps_the_open_path() {
    let (state, path) = Answered::SaveCopy.state();
    let open = state.project_path.clone();
    let cx = gpui::TestAppContext::single();
    let entity = entity_of(state, &cx);

    run_row(&entity, &cx, AppMenu::File, "file-save-copy");

    cx.update(|app| {
        let state = entity.read(app);
        assert_eq!(state.picks_asked(), 1);
        assert_eq!(
            state.project_path, open,
            "the copy took over the project path"
        );
    });
    assert!(path.exists(), "the copy was not written");
}
/// Export Frame asks for a file and starts a job that writes the frame there.
/// The render itself is covered by `aria-render`'s `export_frame` tests.
#[test]
fn export_frame_starts_a_job_for_the_chosen_file() {
    let (state, path) = Answered::ExportFrame.state();
    let cx = gpui::TestAppContext::single();
    let entity = entity_of(state, &cx);

    run_row(&entity, &cx, AppMenu::File, "file-export-frame");

    cx.update(|app| {
        let state = entity.read(app);
        assert_eq!(state.picks_asked(), 1, "the row did not ask for a file");
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .expect("a name");
        let labels: Vec<String> = state
            .jobs
            .snapshot()
            .into_iter()
            .map(|job| job.label)
            .collect();
        assert!(
            labels
                .iter()
                .any(|label| *label == format!("Exporting frame to {name}")),
            "no job exports to {name}: {labels:?}"
        );
    });
}
/// A path typed with no extension still names a PNG: the job writes
/// `still.png`, not an extensionless file no viewer opens.
#[test]
fn export_frame_to_a_bare_name_writes_a_png() {
    let (mut state, _) = with_clip();
    state.supply_pick(Answer::file(own_scratch("export-frame-bare").join("still")));
    let cx = gpui::TestAppContext::single();
    let entity = entity_of(state, &cx);

    run_row(&entity, &cx, AppMenu::File, "file-export-frame");

    cx.update(|app| {
        let state = entity.read(app);
        assert_eq!(state.picks_asked(), 1, "the row did not ask for a path");
        let labels: Vec<String> = state
            .jobs
            .snapshot()
            .into_iter()
            .map(|job| job.label)
            .collect();
        assert!(
            labels
                .iter()
                .any(|label| label == "Exporting frame to still.png"),
            "the job does not write still.png: {labels:?}"
        );
    });
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
#[test]
fn import_timeline_reads_the_chosen_file_as_one_step() {
    let (state, path) = Answered::ImportTimeline.state();
    assert_eq!(clip_count(&state), 0, "the dressing left the session full");
    let cx = gpui::TestAppContext::single();
    let entity = entity_of(state, &cx);

    run_row(&entity, &cx, AppMenu::File, "file-import-timeline");

    let file = path.file_name().expect("a file").to_string_lossy();
    cx.update(|app| {
        let state = entity.read(app);
        assert_eq!(state.picks_asked(), 1);
        assert_eq!(state.last_error, None);
        assert_eq!(clip_count(state), 1, "the timeline did not land");
        // A plugin's transaction is named after the plugin that ran it.
        assert_eq!(
            state.next_undo_label().as_deref(),
            Some(format!("aria.interchange: Import {file}").as_str())
        );
    });
}
#[test]
fn export_timeline_writes_the_file_the_operator_named() {
    let (state, path) = Answered::ExportTimeline.state();
    let cx = gpui::TestAppContext::single();
    let entity = entity_of(state, &cx);

    run_row(&entity, &cx, AppMenu::File, "file-export-timeline");

    cx.update(|app| {
        let state = entity.read(app);
        assert_eq!(state.picks_asked(), 1);
        assert_eq!(state.last_error, None);
    });
    assert!(path.exists(), "the timeline was not written");
}
#[test]
fn import_reads_a_chosen_timeline_file_into_the_timeline_not_the_pool() {
    // The dressing answers the next pick with the timeline file, so the
    // general Import row is handed it in place of the timeline row.
    let (state, path) = Answered::ImportTimeline.state();
    let cx = gpui::TestAppContext::single();
    let entity = entity_of(state, &cx);

    run_row(&entity, &cx, AppMenu::File, "file-import");

    cx.update(|app| {
        let state = entity.read(app);
        assert_eq!(state.last_error, None);
        assert_eq!(clip_count(state), 1, "the timeline file did not land");
        assert!(
            state
                .store
                .media
                .iter()
                .all(|item| std::path::Path::new(&*item.path) != path),
            "the timeline file went to the media pool"
        );
    });
}
#[test]
fn install_plugin_shows_the_prompt_for_the_chosen_archive_and_runs_nothing() {
    let (state, _) = Answered::InstallPlugin.state();
    let cx = gpui::TestAppContext::single();
    let entity = entity_of(state, &cx);

    run_row(&entity, &cx, AppMenu::File, "file-install-plugin");

    cx.update(|app| {
        let state = entity.read(app);
        assert_eq!(state.picks_asked(), 1);
        assert_eq!(state.dialog, crate::app::DialogKind::InstallPlugin);
        assert_eq!(
            state.install_prompt().map(|prompt| prompt.id),
            Some("com.example.menu".to_owned())
        );
        assert!(!state.ctx.is_mounted("com.example.menu"));
    });
}

/// View > Compare > Still asks for an image and compares the Program frame
/// with the one chosen, once it is read.
#[test]
fn compare_with_still_reads_the_chosen_file() {
    let (state, path) = Answered::CompareStill.state();
    let cx = gpui::TestAppContext::single();
    let entity = entity_of(state, &cx);

    run_row(&entity, &cx, AppMenu::View, "view-compare-still");
    cx.run_until_parked();

    cx.update(|app| {
        let state = entity.read(app);
        assert_eq!(state.picks_asked(), 1, "the row did not ask for a file");
        assert_eq!(state.last_error, None);
        assert_eq!(
            state.viewer_compare.reference(),
            Some(&crate::app::viewer_compare::Reference::Still { path })
        );
        assert!(
            state.viewer_compare.picture().is_some(),
            "the still was never read"
        );
    });
}

/// The incident, as a test: **"Open data dir" opened a file-manager window on
/// the machine running the suite.**
///
/// `measured()` above runs every live menu row, this row reveals a folder, and
/// the reveal used to spawn `xdg-open` unconditionally — so `cargo test`
/// launched the desktop file manager. The row still does its whole job here;
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
