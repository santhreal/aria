//! What Make Subclip adds to the pool.
//!
//! The class these close: every subclip of one file was named "<file>
//! Subclip", so the pool drew several rows that could not be told apart, and
//! the call returned the parent's id rather than the entry it made. Each
//! press now adds one entry under the first free numbered name and returns
//! that entry.
//!
//! Not covered: the menu row and the Source tab button, which call this and
//! drop the result, and the window each subclip takes, which
//! `subclip_sound_tests` and the render subclip tests assert.

use aria_models::MediaId;
use aria_plugin::PluginContext;

use crate::app::AppState;
use crate::scratch_pool::scratch;
use crate::types::ActivePanel;

/// A new project with the 24 fps shot in the pool.
fn project() -> (AppState, MediaId) {
    let shot = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/fixture-640x360-24fps-2s-av.mp4");
    let mut state = AppState::new(PluginContext::new(), scratch("subclips")).expect("state");
    state.new_project().expect("project");
    let media = state.import_media(shot).expect("import the shot");
    (state, media)
}

/// [`project`], with the shot open in a Source tab and source ticks 12..23
/// marked.
fn marked() -> (AppState, MediaId) {
    let (mut state, media) = project();
    state.select_pool_item(media.raw());
    state.active_panel = ActivePanel::Media;
    assert!(state.toggle_source_monitor(), "the monitor opens");
    state.hover_pool_item(media.raw(), 12);
    state.set_source_in();
    state.hover_pool_item(media.raw(), 23);
    state.set_source_out();
    (state, media)
}

/// Every pool entry as (id, parent, drawn name), by id.
fn pool(state: &AppState) -> Vec<(MediaId, Option<MediaId>, String)> {
    let engine = state.engine.lock();
    let mut rows: Vec<_> = engine
        .project()
        .media_iter()
        .map(|media| (media.id, media.parent, media.display_name().to_owned()))
        .collect();
    rows.sort_by_key(|row| row.0);
    rows
}

#[test]
fn each_subclip_of_a_file_gets_its_own_name_and_is_returned() {
    let (mut state, media) = marked();
    let stem = format!("{} Subclip", pool(&state)[0].2);
    let mut expected = pool(&state);
    for name in [stem.clone(), format!("{stem} 2"), format!("{stem} 3")] {
        let made = state
            .create_subclip_from_source()
            .expect("make a subclip")
            .expect("a source range is marked");
        assert_ne!(made, media, "the new entry is returned, not its parent");
        expected.push((made, Some(media), name.clone()));
        expected.sort_by_key(|row| row.0);
        assert_eq!(pool(&state), expected, "one entry named {name}");
        assert_eq!(
            state.status_message.as_deref(),
            Some(format!("Created {name}").as_str())
        );
    }
}

#[test]
fn a_freed_name_is_taken_again_and_a_renamed_entry_holds_its_name() {
    let (mut state, media) = marked();
    let stem = format!("{} Subclip", pool(&state)[0].2);
    let first = state
        .create_subclip_from_source()
        .expect("make")
        .expect("marked");
    let second = state
        .create_subclip_from_source()
        .expect("make")
        .expect("marked");
    state
        .apply_command(aria_commands::Command::Project(
            aria_commands::ProjectCommand::RemoveMedia { media: first },
        ))
        .expect("remove the first subclip");
    let third = state
        .create_subclip_from_source()
        .expect("make")
        .expect("marked");
    let named = |id| {
        pool(&state)
            .into_iter()
            .find(|row| row.0 == id)
            .map(|row| (row.1, row.2))
    };
    assert_eq!(named(second), Some((Some(media), format!("{stem} 2"))));
    assert_eq!(named(third), Some((Some(media), stem)));
}

#[test]
fn with_no_source_open_nothing_is_made() {
    let (mut state, _) = project();
    let before = pool(&state);
    assert_eq!(state.create_subclip_from_source().expect("no error"), None);
    assert_eq!(pool(&state), before);
}
