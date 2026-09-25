//! Workspaces: a named arrangement of the editor.
//!
//! A workspace is data. The four shipped ones are the rows of [`WORKSPACES`], the
//! Window menu draws one entry per row, and Settings lists the same rows, so a
//! fifth arrangement is one row here and nothing else. What a row cannot express
//! is deliberate: a workspace names panes, not project state, so applying one can
//! never lose an edit.
//!
//! The operator's own arrangement is the same shape, saved in
//! `ui-settings.json` as one slot rather than a list, because the name of a
//! saved arrangement would have to be an owned string and a menu row's label is
//! `&'static str` — a list of user-named rows is a menu the registry cannot
//! draw. One slot, called Custom, is what the static bar can honestly offer.

use serde::{Deserialize, Serialize};

use crate::settings::{
    INSPECTOR_DEFAULT, INSPECTOR_MAX, INSPECTOR_MIN, SIDEBAR_DEFAULT, SIDEBAR_MAX, SIDEBAR_MIN,
    TIMELINE_DEFAULT_HEIGHT, TIMELINE_MIN_HEIGHT,
};
use crate::types::{ActivePanel, UiRoute};

/// Every measurement a workspace restores.
///
/// Persisted for the Custom slot, so the fields carry serde: a saved arrangement
/// written by an older build is read back with the missing ones defaulted rather
/// than dropping the whole slot.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PaneLayout {
    pub sidebar_width: f32,
    pub sidebar_collapsed: bool,
    pub inspector_width: f32,
    pub inspector_collapsed: bool,
    pub timeline_height: f32,
    pub scopes_open: bool,
    /// Which pane the keyboard is aimed at.
    pub panel: ActivePanel,
    /// Where the operator was. Home is a route, so an arrangement saved on the
    /// start screen restores the start screen.
    pub route: UiRoute,
}

/// One named arrangement.
pub struct Workspace {
    /// Also the id of its Window-menu row: one id, so the menu and the lookup
    /// cannot name different workspaces.
    pub id: &'static str,
    pub name: &'static str,
    pub panes: PaneLayout,
}

/// The operator's own slot.
pub const CUSTOM_ID: &str = "win-workspace-custom";
/// The row that captures the current arrangement into that slot.
pub const SAVE_ID: &str = "win-workspace-save";
/// The label the Custom slot is drawn with, in the menu and in Settings.
pub const CUSTOM_NAME: &str = "Custom";

/// What ships. Order is menu order.
pub const WORKSPACES: [Workspace; 4] = [
    Workspace {
        id: "win-workspace-editing",
        name: "Editing",
        panes: PaneLayout {
            sidebar_width: SIDEBAR_DEFAULT,
            sidebar_collapsed: false,
            inspector_width: INSPECTOR_DEFAULT,
            inspector_collapsed: false,
            timeline_height: TIMELINE_DEFAULT_HEIGHT,
            scopes_open: false,
            panel: ActivePanel::Timeline,
            route: UiRoute::Editor,
        },
    },
    Workspace {
        id: "win-workspace-colour",
        name: "Colour",
        panes: PaneLayout {
            sidebar_width: SIDEBAR_MIN,
            sidebar_collapsed: true,
            inspector_width: 700.0,
            inspector_collapsed: false,
            timeline_height: 200.0,
            // The one arrangement that opens the scopes: grading without a
            // waveform is guesswork.
            scopes_open: true,
            panel: ActivePanel::Preview,
            route: UiRoute::Editor,
        },
    },
    Workspace {
        id: "win-workspace-audio",
        name: "Audio",
        panes: PaneLayout {
            sidebar_width: 240.0,
            sidebar_collapsed: false,
            inspector_width: 440.0,
            inspector_collapsed: false,
            // Tall, because audio work is lane work.
            timeline_height: 420.0,
            scopes_open: false,
            panel: ActivePanel::Timeline,
            route: UiRoute::Editor,
        },
    },
    Workspace {
        id: "win-workspace-effects",
        name: "Effects",
        panes: PaneLayout {
            sidebar_width: SIDEBAR_MIN,
            sidebar_collapsed: true,
            inspector_width: INSPECTOR_MAX,
            inspector_collapsed: false,
            timeline_height: 240.0,
            scopes_open: false,
            panel: ActivePanel::Inspector,
            route: UiRoute::Editor,
        },
    },
];

impl Default for PaneLayout {
    /// The shipped Editing arrangement: the one a saved slot falls back to field
    /// by field, so a file from an older build reads as the default editor rather
    /// than as zeroes.
    fn default() -> Self {
        WORKSPACES[0].panes
    }
}

impl PaneLayout {
    /// The same arrangement with every measurement inside the bounds the panes
    /// accept. The bounds live in [`crate::settings`], which owns them; a saved
    /// slot from a build with different limits is pulled in rather than refused.
    pub fn clamped(self) -> Self {
        Self {
            sidebar_width: self.sidebar_width.clamp(SIDEBAR_MIN, SIDEBAR_MAX),
            inspector_width: self.inspector_width.clamp(INSPECTOR_MIN, INSPECTOR_MAX),
            timeline_height: self.timeline_height.max(TIMELINE_MIN_HEIGHT),
            ..self
        }
    }

    /// Whether two arrangements put the panes in the same place.
    ///
    /// Ignores the route: which screen the operator is on is where they are, not
    /// how the editor is arranged, and a workspace must not stop being the active
    /// one because they went Home. Measurements compare within half a pixel, so a
    /// workspace stays recognised after a clamp rounds it.
    pub fn arranged_like(&self, other: &Self) -> bool {
        (self.sidebar_width - other.sidebar_width).abs() < 0.5
            && self.sidebar_collapsed == other.sidebar_collapsed
            && (self.inspector_width - other.inspector_width).abs() < 0.5
            && self.inspector_collapsed == other.inspector_collapsed
            && (self.timeline_height - other.timeline_height).abs() < 0.5
            && self.scopes_open == other.scopes_open
            && self.panel == other.panel
    }
}

/// The shipped workspace with this id.
pub fn workspace(id: &str) -> Option<&'static Workspace> {
    WORKSPACES.iter().find(|ws| ws.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shipped_workspace_has_its_own_id_and_name() {
        let mut ids: Vec<&str> = WORKSPACES.iter().map(|ws| ws.id).collect();
        ids.push(CUSTOM_ID);
        ids.push(SAVE_ID);
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), ids.len(), "duplicate row id in {ids:?}");
        for id in ids {
            assert!(id.starts_with("win-workspace-"), "{id} is off-prefix");
        }
        let mut names: Vec<&str> = WORKSPACES.iter().map(|ws| ws.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), WORKSPACES.len());
        assert!(!names.contains(&CUSTOM_NAME));
    }

    /// A shipped arrangement the panes would clamp is one that never draws the
    /// way it reads here.
    #[test]
    fn every_shipped_arrangement_is_already_inside_the_pane_bounds() {
        for ws in &WORKSPACES {
            assert_eq!(
                ws.panes.clamped(),
                ws.panes,
                "{} is outside the pane bounds",
                ws.id
            );
        }
    }

    #[test]
    fn a_saved_slot_from_a_wilder_build_is_pulled_in() {
        let wild = PaneLayout {
            sidebar_width: 9000.0,
            inspector_width: 1.0,
            timeline_height: 4.0,
            ..PaneLayout::default()
        }
        .clamped();
        assert_eq!(wild.sidebar_width, SIDEBAR_MAX);
        assert_eq!(wild.inspector_width, INSPECTOR_MIN);
        assert_eq!(wild.timeline_height, TIMELINE_MIN_HEIGHT);
    }

    #[test]
    fn the_default_arrangement_is_the_first_shipped_one() {
        assert_eq!(PaneLayout::default(), WORKSPACES[0].panes);
        assert_eq!(
            workspace(WORKSPACES[0].id).map(|ws| ws.name),
            Some("Editing")
        );
        assert!(workspace("win-workspace-nothing").is_none());
        assert!(
            workspace(CUSTOM_ID).is_none(),
            "the slot is not a shipped row"
        );
    }

    #[test]
    fn where_the_operator_is_does_not_change_which_workspace_is_arranged() {
        let editing = WORKSPACES[0].panes;
        let home = PaneLayout {
            route: UiRoute::Home,
            ..editing
        };
        assert!(editing.arranged_like(&home));
        assert_ne!(editing, home);
        let dragged = PaneLayout {
            timeline_height: editing.timeline_height + 40.0,
            ..editing
        };
        assert!(!editing.arranged_like(&dragged));
        let rounded = PaneLayout {
            sidebar_width: editing.sidebar_width + 0.25,
            ..editing
        };
        assert!(editing.arranged_like(&rounded));
    }

    /// A workspace that opened the scopes and one that did not are different
    /// arrangements even when every pane measures the same.
    #[test]
    fn the_scope_strip_is_part_of_an_arrangement() {
        let editing = WORKSPACES[0].panes;
        let with_scopes = PaneLayout {
            scopes_open: true,
            ..editing
        };
        assert!(!editing.arranged_like(&with_scopes));
    }
}
