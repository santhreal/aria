//! The Window menu: one row per dock panel, the workspaces, and the palette,
//! home and report rows.
//!
//! [`super::menu_items::items_for`] dispatches here, so the bar, the palette
//! and the tests reach these rows through the one registry.

use std::borrow::Cow;

use crate::app::{AppState, DialogKind};
use crate::types::ActivePanel;

use super::menu_items::{MenuItem, MenuItems, RowRun};

const WORKSPACES: &str = "Workspaces";

/// Id prefix of the Window rows generated one per registered dock panel that
/// no row in [`PANEL_ROWS`] opens. The rest of the id is the panel id.
pub(crate) const PANEL_PREFIX: &str = "win-panel:";

/// The Window panel rows written by hand: each row's id, the dock panel it
/// opens, and what a press runs. A row is labeled with its panel's title and checked while the
/// panel is open, front tab or not. Every other registered panel, built in or
/// declared by a plugin, gets a generated row.
pub(crate) const PANEL_ROWS: &[(&str, crate::dock::PanelId, RowRun)] = &[
    ("win-media", crate::dock::PANEL_PROJECT, |_, s, _| {
        s.toggle_dock_panel(crate::dock::PANEL_PROJECT);
    }),
    ("win-effects", crate::dock::PANEL_EFFECTS, |_, s, _| {
        s.open_effects_panel()
    }),
    ("win-program", crate::dock::PANEL_VIEWER, |_, s, _| {
        s.focus_pane(ActivePanel::Preview)
    }),
    ("win-timeline", crate::dock::PANEL_TIMELINE, |_, s, _| {
        s.focus_pane(ActivePanel::Timeline)
    }),
    ("win-inspector", crate::dock::PANEL_INSPECTOR, |_, s, _| {
        s.toggle_dock_panel(crate::dock::PANEL_INSPECTOR);
    }),
    ("win-assistant", crate::dock::PANEL_ASSISTANT, |_, s, _| {
        s.toggle_pane_tab(ActivePanel::Ai);
    }),
    ("win-mixer", crate::dock::PANEL_MIXER, |_, s, _| {
        s.toggle_pane_tab(ActivePanel::Mixer);
    }),
    ("win-jobs", crate::dock::PANEL_JOBS, |_, s, _| {
        s.toggle_dock_panel(crate::dock::PANEL_JOBS);
    }),
];

/// The check of a [`PANEL_ROWS`] row: its panel is open.
fn panel_row_open(id: &str, state: &AppState) -> bool {
    PANEL_ROWS
        .iter()
        .find(|(row, ..)| *row == id)
        .is_some_and(|&(_, panel, _)| state.dock_tree.is_open(panel))
}

/// The binding that brings a panel forward, drawn beside its Window row.
const PANEL_KEYS: &[(crate::dock::PanelId, &str)] = &[
    (crate::dock::PANEL_EFFECTS, "key-pane-effects"),
    (crate::dock::PANEL_TIMELINE, "key-pane-timeline"),
    (crate::dock::PANEL_ASSISTANT, "key-assistant"),
    (crate::dock::PANEL_MIXER, "key-mixer"),
    (crate::dock::PANEL_INFO, "key-pane-info"),
    (crate::dock::PANEL_AUDIO_METERS, "key-pane-audio-meters"),
    (crate::dock::PANEL_HISTORY, "key-history"),
    (crate::dock::PANEL_SCOPES, "key-scopes"),
];

/// The Window row opening `panel`, labeled with its title and drawing the
/// chord [`PANEL_KEYS`] gives it.
fn panel_row(
    state: &AppState,
    id: &'static str,
    panel: crate::dock::PanelId,
    run: RowRun,
) -> MenuItem {
    let row = MenuItem::new(id, state.dock_panel_title(panel), run);
    match PANEL_KEYS.iter().find(|&&(keyed, _)| keyed == panel) {
        Some(&(_, key)) => row.key(key),
        None => row,
    }
}

/// The Window rows after the panels.
static WINDOW_TOOLS: &[MenuItem] = &[
    MenuItem::new("win-palette", "Command Palette", |_, s, _| {
        s.open_dialog(DialogKind::Palette)
    })
    .key("key-palette"),
    MenuItem::new("win-home", "Home", |_, s, _| s.request_go_home()),
];

pub(super) fn window_items(state: &AppState) -> MenuItems {
    let mut items = Vec::new();
    // One row per workspace from the list Settings draws: the shipped ones,
    // the plugins', and the saved slot once there is something saved in it.
    // Each row is told its own id, which is the workspace's id.
    for ws in state.workspace_rows() {
        items.push(
            MenuItem::new(ws.id, ws.name, |id, s, cx| {
                s.apply_workspace(id, cx);
            })
            .checked(|id, s| s.is_active_workspace(id))
            .under(WORKSPACES),
        );
    }
    items.push(
        MenuItem::new(crate::layouts::SAVE_ID, "Save Workspace", |_, s, cx| {
            s.save_workspace(cx)
        })
        .under(WORKSPACES),
    );
    items.push(
        MenuItem::new(
            crate::layouts::RESET_ID,
            "Reset to Saved Layout",
            |_, s, cx| s.reset_workspace_layout(cx),
        )
        .under(WORKSPACES),
    );
    items.extend(
        PANEL_ROWS
            .iter()
            .map(|&(id, panel, run)| panel_row(state, id, panel, run).checked(panel_row_open)),
    );
    // One row per other registered dock panel, so a panel added to the registry
    // or declared by a plugin is opened from the menu with no row written for
    // it. The id holds the panel id, so one `fn` runs every row.
    for panel in state.registered_panel_ids() {
        if PANEL_ROWS.iter().any(|&(_, opened, _)| opened == panel) {
            continue;
        }
        let id = crate::interned::joined(PANEL_PREFIX, panel);
        items.push(
            panel_row(state, id, panel, |id, s, _| {
                if let Some(panel) = id.strip_prefix(PANEL_PREFIX) {
                    s.toggle_dock_panel(crate::interned::intern(panel));
                }
            })
            .checked(|id, s| {
                id.strip_prefix(PANEL_PREFIX)
                    .is_some_and(|panel| s.dock_tree.is_open(panel))
            }),
        );
    }
    items.extend_from_slice(WINDOW_TOOLS);
    // A report on a project with no sources and no clips is a column of zeroes,
    // so the item is offered once there is something to report on.
    if state.project_report_available() {
        items.push(MenuItem::new(
            "win-project-report",
            "Project Report",
            |_, s, _| s.open_dialog(DialogKind::ProjectReport),
        ));
    }
    Cow::Owned(items)
}
