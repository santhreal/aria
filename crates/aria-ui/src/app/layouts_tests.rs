//! What a workspace restores, measured against the running state.
//!
//! Every case drives the real verbs through a `gpui` entity, because applying a
//! workspace persists settings and asks the window to redraw — a test that called
//! the fields directly would prove nothing about the row the operator clicks.
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

#[test]
fn every_shipped_workspace_arranges_the_panes_it_names() {
    let cx = gpui::TestAppContext::single();
    let subject = entity(state_in(&dir()), &cx);
    for ws in &layouts::WORKSPACES {
        assert!(apply(&subject, &cx, ws.id), "{} was refused", ws.id);
        let here = panes_of(&subject, &cx);
        assert_eq!(here.sidebar_width, ws.panes.sidebar_width, "{}", ws.id);
        assert_eq!(
            here.sidebar_collapsed, ws.panes.sidebar_collapsed,
            "{}",
            ws.id
        );
        assert_eq!(here.inspector_width, ws.panes.inspector_width, "{}", ws.id);
        assert_eq!(
            here.inspector_collapsed, ws.panes.inspector_collapsed,
            "{}",
            ws.id
        );
        assert_eq!(here.timeline_height, ws.panes.timeline_height, "{}", ws.id);
        assert_eq!(here.scopes_open, ws.panes.scopes_open, "{}", ws.id);
        assert_eq!(here.panel, ws.panes.panel, "{}", ws.id);
        assert_eq!(here.route, ws.panes.route, "{}", ws.id);
        assert_eq!(
            cx.update(|app| active_id(&subject, app)),
            Some(ws.id),
            "{} did not read back as the active workspace",
            ws.id
        );
    }
}

/// The active row as the Settings list marks it: the list the UI draws is the
/// only place activeness is decided, so a case must not compute it a second way.
fn active_id(subject: &gpui::Entity<AppState>, app: &mut gpui::App) -> Option<&'static str> {
    subject
        .read(app)
        .workspace_rows()
        .into_iter()
        .find(|row| row.active)
        .map(|row| row.id)
}

#[test]
fn a_dragged_divider_leaves_no_workspace_active() {
    let cx = gpui::TestAppContext::single();
    let subject = entity(state_in(&dir()), &cx);
    let editing = layouts::WORKSPACES[0].id;
    assert!(apply(&subject, &cx, editing));
    cx.update(|app| {
        subject.update(app, |state: &mut AppState, _| {
            state.ui_settings.timeline_height += 37.0;
        })
    });
    assert_eq!(cx.update(|app| active_id(&subject, app)), None);
}

/// The scope strip is opened through `toggle_scopes`, which measures the frame.
/// Setting the flag instead would leave the reading behind when it closes.
#[test]
fn a_workspace_that_closes_the_scopes_drops_the_reading_with_them() {
    let cx = gpui::TestAppContext::single();
    let subject = entity(state_in(&dir()), &cx);
    let colour = layouts::WORKSPACES
        .iter()
        .find(|ws| ws.panes.scopes_open)
        .expect("one shipped workspace opens the scopes");
    assert!(apply(&subject, &cx, colour.id));
    assert!(cx.update(|app| subject.read(app).scopes_open));
    let editing = layouts::WORKSPACES[0].id;
    assert!(apply(&subject, &cx, editing));
    assert!(cx.update(|app| {
        let state = subject.read(app);
        !state.scopes_open && state.scope_textures.is_none()
    }));
}

#[test]
fn the_saved_slot_is_offered_only_once_it_holds_something() {
    let cx = gpui::TestAppContext::single();
    let subject = entity(state_in(&dir()), &cx);
    assert!(!window_ids(&subject, &cx).contains(&CUSTOM_ID));
    assert!(window_ids(&subject, &cx).contains(&SAVE_ID));
    cx.update(|app| subject.update(app, |state: &mut AppState, cx| state.save_workspace(cx)));
    assert!(window_labels(&subject, &cx).contains(&CUSTOM_NAME));
    cx.update(|app| subject.update(app, |state: &mut AppState, cx| state.forget_workspace(cx)));
    assert!(!window_ids(&subject, &cx).contains(&CUSTOM_ID));
}

/// Capture and apply must cover the same fields: an arrangement that is saved and
/// restored has to come back exactly, including the pane the keyboard was aimed
/// at and the screen it was saved on.
#[test]
fn a_saved_arrangement_comes_back_whole_after_a_relaunch() {
    let scratch = dir();
    let cx = gpui::TestAppContext::single();
    let subject = entity(state_in(&scratch), &cx);
    let wanted = PaneLayout {
        sidebar_width: 311.0,
        sidebar_collapsed: true,
        inspector_width: 433.0,
        inspector_collapsed: true,
        timeline_height: 377.0,
        scopes_open: false,
        panel: ActivePanel::Inspector,
        route: UiRoute::Home,
    };
    cx.update(|app| {
        subject.update(app, |state: &mut AppState, cx| {
            state.ui_settings.sidebar_width = wanted.sidebar_width;
            state.ui_settings.sidebar_collapsed = wanted.sidebar_collapsed;
            state.ui_settings.inspector_width = wanted.inspector_width;
            state.ui_settings.inspector_collapsed = wanted.inspector_collapsed;
            state.ui_settings.timeline_height = wanted.timeline_height;
            state.active_panel = wanted.panel;
            state.save_workspace(cx);
            state.ui_settings.save(&state.data_dir).expect("save");
        })
    });
    // A second launch against the same dir: the slot has to survive the file, not
    // just the session.
    let relaunched = entity(state_in(&scratch), &cx);
    assert!(apply(&relaunched, &cx, layouts::WORKSPACES[0].id));
    assert!(apply(&relaunched, &cx, CUSTOM_ID));
    let here = panes_of(&relaunched, &cx);
    assert_eq!(
        here, wanted,
        "the saved arrangement did not come back whole"
    );
}

#[test]
fn a_saved_slot_from_a_wilder_build_is_pulled_inside_the_pane_bounds() {
    let scratch = dir();
    let settings = UiSettings {
        workspace: Some(PaneLayout {
            sidebar_width: 9000.0,
            timeline_height: 1.0,
            ..PaneLayout::default()
        }),
        ..UiSettings::default()
    };
    settings.save(&scratch).expect("save");
    let cx = gpui::TestAppContext::single();
    let subject = entity(state_in(&scratch), &cx);
    assert!(apply(&subject, &cx, CUSTOM_ID));
    let here = panes_of(&subject, &cx);
    assert_eq!(here.sidebar_width, SIDEBAR_MAX);
    assert_eq!(here.timeline_height, TIMELINE_MIN_HEIGHT);
}

#[test]
fn a_workspace_nobody_ships_is_refused_and_changes_nothing() {
    let cx = gpui::TestAppContext::single();
    let subject = entity(state_in(&dir()), &cx);
    let before = panes_of(&subject, &cx);
    assert!(!apply(&subject, &cx, "win-workspace-nothing"));
    // The slot is empty, so its row is not on offer and applying it is refused
    // rather than restoring the default arrangement under the operator.
    assert!(!apply(&subject, &cx, CUSTOM_ID));
    assert_eq!(panes_of(&subject, &cx), before);
}

/// Where the operator is goes with the arrangement, through the guarded verbs:
/// a slot saved on the start screen restores the start screen.
#[test]
fn an_arrangement_saved_at_home_takes_the_operator_home() {
    let cx = gpui::TestAppContext::single();
    let subject = entity(state_in(&dir()), &cx);
    cx.update(|app| subject.update(app, |state: &mut AppState, cx| state.save_workspace(cx)));
    assert_eq!(panes_of(&subject, &cx).route, UiRoute::Home);
    assert!(apply(&subject, &cx, layouts::WORKSPACES[0].id));
    assert_eq!(cx.update(|app| subject.read(app).route), UiRoute::Editor);
    assert!(apply(&subject, &cx, CUSTOM_ID));
    assert_eq!(cx.update(|app| subject.read(app).route), UiRoute::Home);
}

/// The Window menu draws the table, in the table's order, and nothing else.
#[test]
fn the_menu_family_is_the_workspace_table() {
    let cx = gpui::TestAppContext::single();
    let subject = entity(state_in(&dir()), &cx);
    let ids = window_ids(&subject, &cx);
    let shipped: Vec<&str> = layouts::WORKSPACES.iter().map(|ws| ws.id).collect();
    let drawn: Vec<&str> = ids
        .iter()
        .copied()
        .filter(|id| id.starts_with("win-workspace-") && *id != SAVE_ID)
        .collect();
    assert_eq!(drawn, shipped, "menu order must follow the table");
    let labels = window_labels(&subject, &cx);
    for ws in &layouts::WORKSPACES {
        assert!(labels.contains(&ws.name), "{} is not drawn", ws.name);
    }
}

/// Clicking the menu row is what an operator does, so the row is what is driven
/// here: the closure has to reach the verb with its own id.
#[test]
fn the_menu_row_arranges_the_panes_it_names() {
    let cx = gpui::TestAppContext::single();
    let subject = entity(state_in(&dir()), &cx);
    let wanted = layouts::WORKSPACES
        .iter()
        .find(|ws| ws.panes.sidebar_collapsed)
        .expect("one shipped workspace collapses the rail");
    let ran = cx.update(|app| {
        subject.update(app, |state: &mut AppState, cx| {
            crate::chrome::menu_items::run_item(AppMenu::Window, wanted.id, state, cx)
        })
    });
    assert!(ran, "{} did not run", wanted.id);
    assert!(panes_of(&subject, &cx).sidebar_collapsed);
    let saved = cx.update(|app| {
        subject.update(app, |state: &mut AppState, cx| {
            crate::chrome::menu_items::run_item(AppMenu::Window, SAVE_ID, state, cx)
        })
    });
    assert!(saved);
    assert_eq!(
        cx.update(|app| subject.read(app).ui_settings.workspace),
        Some(panes_of(&subject, &cx)),
        "Save Workspace must keep exactly what is on screen"
    );
}
