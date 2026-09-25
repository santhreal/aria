//! Split from `reach_tests.rs`: whole `#[test]` functions, moved verbatim.
//! `use super::*` reaches the parent's private helpers and tables.

use super::*;

#[test]
fn using_a_template_replaces_the_session_with_the_chosen_bundle() {
    let (mut state, _) = with_clip();
    let bundle = scratch("use").join("row.ariat");
    state
        .save_template(bundle.clone(), "Row".into())
        .expect("save a bundle to open");
    // Walk away from it: an empty project, so the clip that lands can only have
    // come from the bundle.
    state.new_project().expect("empty project");
    assert_eq!(clip_count(&state), 0);
    state.supply_pick(Answer::file(bundle));

    run_row("pal-use-template", &mut state);

    assert_eq!(state.picks_asked(), 1, "the row did not ask for a bundle");
    assert_eq!(clip_count(&state), 1, "the template did not land");
    assert_eq!(state.route, crate::types::UiRoute::Editor);
}

#[test]
fn relinking_everything_repairs_the_media_from_the_chosen_folder() {
    let (mut state, _) = with_clip();
    let dir = scratch("relink");
    let moved = scratch("relink-moved");
    let source = dir.join("clip.png");
    std::fs::copy(fixture(), &source).expect("stage a file to lose");
    state.import_media(source.clone()).expect("import it");
    std::fs::rename(&source, moved.join("clip.png")).expect("move it out from under the project");
    crate::projection::invalidate_media_statuses();
    state.rebuild_store();
    assert_eq!(
        state.store.media.iter().filter(|item| item.missing).count(),
        1,
        "the fixture for a relink needs one missing file"
    );
    state.supply_pick(Answer::file(moved));

    run_row("pal-relink", &mut state);

    assert_eq!(state.picks_asked(), 1, "the row did not ask for a folder");
    assert!(
        state.store.media.iter().all(|item| !item.missing),
        "the media was not relinked from the chosen folder"
    );
    assert_eq!(state.status_message.as_deref(), Some("Relinked 1"));
}
