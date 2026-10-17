//! Which panel the keyboard is in, for the keys that act on a panel rather
//! than on a key route.
//!
//! The class these close: several dock panels share one key route (Effects
//! with the pool, Scopes with the Viewer, Info and the text panels with the
//! Inspector, Jobs, History and Markers with the Timeline, the meters with the
//! Mixer), and a key that read the route acted on the route's first panel
//! instead of the one in front. Space over the Effects panel opened the source
//! monitor as if over the pool, and Ctrl+W over Scopes closed the Viewer. The
//! built-in panels are enumerated from their registry, so a panel registered
//! later is covered without an edit here.
//!
//! Not covered: panels a plugin registers, and the clicks that move focus.
//! These tests start from the state those clicks leave behind.

use crate::app::AppState;
use crate::dock::{PANEL_PROJECT, PANEL_TIMELINE};
use crate::plugins::builtin_dock_panel_ids;
use crate::scratch_pool::scratch;
use crate::types::{ActivePanel, SidebarTab};
use aria_plugin::PluginContext;

/// A pool with one still in it, selected, and the sidebar on its Media tab.
fn with_selection() -> AppState {
    let mut state = AppState::new(PluginContext::new(), scratch("panel-focus")).expect("state");
    state.new_project().expect("project");
    let still = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/texture.png");
    let media = state.import_media(still).expect("import").raw();
    state.select_pool_item(media);
    state.sidebar_tab = SidebarTab::Media;
    state
}

/// Press Space, report whether it opened the source monitor rather than
/// playing the sequence, and put both back for the next press.
fn space_previews(state: &mut AppState) -> bool {
    assert!(
        !state.playing && state.sources.focused().is_none(),
        "each press starts stopped, on Program"
    );
    state.space();
    let previewed = state.sources.focused().is_some();
    assert_ne!(previewed, state.playing, "Space does one of the two");
    if state.playing {
        state.toggle_playback();
    }
    state.show_program();
    previewed
}

#[test]
fn space_previews_only_where_the_pool_is_in_front() {
    let mut state = with_selection();
    let mut panels = 0;
    for panel in builtin_dock_panel_ids() {
        state.focus_dock_panel(panel);
        assert_eq!(
            state.keyed_panel(),
            panel,
            "the keys follow the tab in front"
        );
        assert_eq!(
            space_previews(&mut state),
            panel == PANEL_PROJECT,
            "Space over {panel}"
        );
        panels += 1;
    }
    assert!(
        panels > 6,
        "every built-in panel is visited, not one per route"
    );
}

#[test]
fn space_previews_only_on_the_sidebar_tabs_that_list_the_pool() {
    let mut state = with_selection();
    for &tab in SidebarTab::ALL {
        state.focus_dock_panel(PANEL_PROJECT);
        state.sidebar_tab = tab;
        assert_eq!(
            space_previews(&mut state),
            matches!(tab, SidebarTab::Media | SidebarTab::Audio),
            "Space over the {tab:?} tab"
        );
    }
}

#[test]
fn space_over_the_pool_with_nothing_selected_plays_the_sequence() {
    let mut state = with_selection();
    state.focus_dock_panel(PANEL_PROJECT);
    state.pool_selection.clear();
    state.status_message = None;
    assert!(!space_previews(&mut state));
    assert_eq!(state.status_message, None, "no request to select first");
}

#[test]
fn a_click_in_the_pool_gives_it_the_keys_while_another_frame_is_focused() {
    let mut state = with_selection();
    state.focus_dock_panel(PANEL_TIMELINE);
    // The sidebar's mouse-down moves the route and leaves the frame focus.
    state.active_panel = ActivePanel::Media;
    assert_eq!(state.keyed_panel(), PANEL_PROJECT);
    assert!(space_previews(&mut state));
}

#[test]
fn ctrl_w_closes_the_panel_in_front_and_no_other() {
    let mut state = with_selection();
    let fresh = state.dock_tree.clone();
    for panel in builtin_dock_panel_ids() {
        state.dock_tree = fresh.clone();
        state.focus_dock_panel(panel);
        let open: Vec<_> = builtin_dock_panel_ids()
            .filter(|other| *other != panel && state.dock_tree.is_open(other))
            .collect();
        state.close_focused().expect("close");
        assert!(
            !state.dock_tree.is_open(panel),
            "Ctrl+W over {panel} leaves it open"
        );
        for other in open {
            assert!(
                state.dock_tree.is_open(other),
                "Ctrl+W over {panel} closed {other}"
            );
        }
    }
}
