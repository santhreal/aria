//! Applying, capturing and listing a workspace.
//!
//! The arrangement itself is data in [`crate::layouts`]; this is the one place
//! that writes it into the running editor and the one place that reads the
//! running editor back out. Both go field by field through the same list, so a
//! workspace cannot restore a pane that capture does not record.
//!
//! Which workspace is active is not stored. It is measured: the active one is the
//! one the panes are already arranged like. A dragged divider therefore leaves no
//! workspace active, which is the truth, and nothing has to be invalidated.

use crate::layouts::{self, CUSTOM_ID, CUSTOM_NAME, PaneLayout};
use crate::types::UiRoute;

use super::AppState;

/// One workspace, as the Window menu and Settings show it.
pub(crate) struct WorkspaceRow {
    pub(crate) id: &'static str,
    pub(crate) name: &'static str,
    pub(crate) panes: PaneLayout,
    /// Whether the editor is arranged like this row now.
    pub(crate) active: bool,
    /// The operator's own slot, which can be re-saved and forgotten.
    pub(crate) custom: bool,
}

impl AppState {
    /// How the editor is arranged now.
    pub(crate) fn panes(&self) -> PaneLayout {
        PaneLayout {
            sidebar_width: self.ui_settings.sidebar_width,
            sidebar_collapsed: self.ui_settings.sidebar_collapsed,
            inspector_width: self.ui_settings.inspector_width,
            inspector_collapsed: self.ui_settings.inspector_collapsed,
            timeline_height: self.ui_settings.timeline_height,
            scopes_open: self.scopes_open,
            panel: self.active_panel,
            route: self.route,
        }
    }

    /// The arrangement one row would restore, shipped or saved.
    pub(crate) fn workspace_panes(&self, id: &str) -> Option<PaneLayout> {
        if id == CUSTOM_ID {
            return self.ui_settings.workspace;
        }
        layouts::workspace(id).map(|ws| ws.panes)
    }

    /// Every row on offer: the shipped ones, then the saved slot if there is one.
    pub(crate) fn workspace_rows(&self) -> Vec<WorkspaceRow> {
        let here = self.panes();
        let mut rows: Vec<WorkspaceRow> = layouts::WORKSPACES
            .iter()
            .map(|ws| WorkspaceRow {
                id: ws.id,
                name: ws.name,
                panes: ws.panes,
                active: here.arranged_like(&ws.panes),
                custom: false,
            })
            .collect();
        if let Some(saved) = self.ui_settings.workspace {
            rows.push(WorkspaceRow {
                id: CUSTOM_ID,
                name: CUSTOM_NAME,
                panes: saved,
                active: here.arranged_like(&saved),
                custom: true,
            });
        }
        rows
    }

    /// Arrange the editor the way one row says. Reports whether the row exists,
    /// so a row that vanished between the click and the keypress is a no-op
    /// rather than a silent wrong arrangement.
    pub(crate) fn apply_workspace(&mut self, id: &str, cx: &mut gpui::Context<Self>) -> bool {
        let Some(panes) = self.workspace_panes(id) else {
            return false;
        };
        let panes = panes.clamped();
        self.ui_settings.sidebar_width = panes.sidebar_width;
        self.ui_settings.sidebar_collapsed = panes.sidebar_collapsed;
        self.ui_settings.inspector_width = panes.inspector_width;
        self.ui_settings.inspector_collapsed = panes.inspector_collapsed;
        self.ui_settings.timeline_height = panes.timeline_height;
        // Through the toggle, not the flag: opening the strip measures the frame
        // on screen, and a workspace that set the flag itself would show an empty
        // scope until the next redraw.
        if self.scopes_open != panes.scopes_open {
            self.toggle_scopes();
        }
        self.active_panel = panes.panel;
        // Through the guarded verbs, so a workspace saved on the start screen
        // cannot walk away from an unsaved project.
        match panes.route {
            UiRoute::Home if self.route != UiRoute::Home => self.request_go_home(),
            UiRoute::Editor if self.route != UiRoute::Editor => self.enter_editor(),
            _ => {}
        }
        let name = self
            .workspace_rows()
            .into_iter()
            .find(|row| row.id == id)
            .map(|row| row.name)
            .unwrap_or(CUSTOM_NAME);
        self.status_message = Some(format!("{name} workspace"));
        self.persist_settings_later(cx);
        cx.notify();
        true
    }

    /// Keep the current arrangement as the Custom row.
    pub(crate) fn save_workspace(&mut self, cx: &mut gpui::Context<Self>) {
        self.ui_settings.workspace = Some(self.panes());
        self.status_message = Some(format!("Saved the {CUSTOM_NAME} workspace"));
        self.persist_settings_later(cx);
        cx.notify();
    }

    /// Drop the saved slot, which takes its row off the menu.
    pub(crate) fn forget_workspace(&mut self, cx: &mut gpui::Context<Self>) {
        if self.ui_settings.workspace.take().is_none() {
            return;
        }
        self.status_message = Some(format!("Forgot the {CUSTOM_NAME} workspace"));
        self.persist_settings_later(cx);
        cx.notify();
    }
}
