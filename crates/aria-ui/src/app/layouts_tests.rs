//! What a workspace restores, measured against the running state.
//!
//! Every case drives the real verbs through a `gpui` entity, because applying a
//! workspace persists settings and asks the window to redraw — a test that called
//! the fields directly would prove nothing about the clicked row.
//!
//! The subjects are read out of `layouts::WORKSPACES` at run time, so a fifth
//! shipped arrangement is covered by these cases without an edit here, and one
//! that cannot be applied turns them red.

use aria_plugin::PluginContext;
use gpui::AppContext;

use super::AppState;
use crate::chrome::menu_items::items_of;
use crate::layouts::{self, CUSTOM_ID, CUSTOM_NAME, PaneLayout, SAVE_ID};
use crate::plugins::UiPlugin;
use crate::settings::{SIDEBAR_MAX, TIMELINE_MIN_HEIGHT, UiSettings};
use crate::types::{ActivePanel, AppMenu, UiRoute};

fn dir() -> std::path::PathBuf {
    crate::scratch_pool::scratch("layouts")
}

/// Mounts the ui plugin, so the menu the cases read is the registry the bar
/// reads rather than a list assembled for the test.
fn state_in(dir: &std::path::Path) -> AppState {
    let ctx = PluginContext::new();
    ctx.mount(&UiPlugin).expect("the ui plugin needs nothing");
    AppState::new(ctx, dir.to_path_buf()).expect("state")
}

/// Run one closure against the state through an entity, and hand the state back
/// as an entity so a case can act more than once.
fn entity(state: AppState, cx: &gpui::TestAppContext) -> gpui::Entity<AppState> {
    cx.update(|app| app.new(|_| state))
}

fn apply(entity: &gpui::Entity<AppState>, cx: &gpui::TestAppContext, id: &str) -> bool {
    cx.update(|app| {
        entity.update(app, |state: &mut AppState, cx| {
            state.apply_workspace(id, cx)
        })
    })
}

fn panes_of(entity: &gpui::Entity<AppState>, cx: &gpui::TestAppContext) -> PaneLayout {
    cx.update(|app| entity.read(app).panes())
}

fn window_labels(entity: &gpui::Entity<AppState>, cx: &gpui::TestAppContext) -> Vec<&'static str> {
    cx.update(|app| {
        items_of(AppMenu::Window, entity.read(app))
            .into_iter()
            .map(|item| item.label)
            .collect()
    })
}

fn window_ids(entity: &gpui::Entity<AppState>, cx: &gpui::TestAppContext) -> Vec<&'static str> {
    cx.update(|app| {
        items_of(AppMenu::Window, entity.read(app))
            .into_iter()
            .map(|item| item.id)
            .collect()
    })
}

#[path = "layouts_tests_dock.rs"]
mod dock;
#[path = "layouts_tests_plugins.rs"]
mod plugins;
#[path = "layouts_tests_resize.rs"]
mod resize;
#[path = "layouts_tests_workspaces.rs"]
mod workspaces;
