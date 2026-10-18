//! Workspaces: a named arrangement of the editor.
//!
//! A workspace is data. The four shipped ones are the rows of [`WORKSPACES`], the
//! Window menu draws one entry per row, and Settings lists the same rows, so a
//! fifth arrangement is one row here and nothing else. What a row cannot express
//! is deliberate: a workspace names panes, not project state, so applying one can
//! never lose an edit.
//!
//! Plugins add rows through `PluginContext::register_workspace` or a package's
//! `[[workspaces]]`. Their names are owned strings, interned once for the menu
//! the way plugin menu rows are; see [`plugin_panes`].
//!
//! The operator's own arrangement is the same shape, saved in
//! `ui-settings.json` as one slot rather than a list, because the name of a
//! saved arrangement would have to be an owned string and a menu row's label is
//! `&'static str` — a list of user-named rows is a menu the registry cannot
//! draw. One slot, called Custom, is what the static bar can honestly offer.

use serde::{Deserialize, Serialize};

use std::io;
use std::path::{Path, PathBuf};

use crate::dock::{
    Axis, DOCK_VERSION, DefaultFrame, DockTree, Frame, FrameId, Node, PANEL_ALIGN, PANEL_ASSISTANT,
    PANEL_AUDIO_METERS, PANEL_COLOR, PANEL_EFFECTS, PANEL_INFO, PANEL_INSPECTOR, PANEL_MIXER,
    PANEL_PROJECT, PANEL_SCOPES, PANEL_TIMELINE, PANEL_VIEWER, PanelId,
};
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
#[derive(Debug, Clone, Copy)]
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
/// The row that resets the current workspace to its saved/shipped layout.
pub const RESET_ID: &str = "win-workspace-reset";
/// The label the Custom slot is drawn with, in the menu and in Settings.
pub const CUSTOM_NAME: &str = "Custom";

/// Subdirectory inside the profile data dir holding named workspace JSON files.
pub const WORKSPACES_DIR: &str = "workspaces";

/// What ships. Order is menu order.
pub const WORKSPACES: [Workspace; 5] = [
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
        id: "win-workspace-animation",
        name: "Animation",
        panes: PaneLayout {
            sidebar_width: 280.0,
            sidebar_collapsed: false,
            inspector_width: 480.0,
            inspector_collapsed: false,
            timeline_height: 360.0,
            scopes_open: false,
            panel: ActivePanel::Timeline,
            route: UiRoute::Editor,
        },
    },
    Workspace {
        id: "win-workspace-colour",
        name: "Color",
        panes: PaneLayout {
            sidebar_width: SIDEBAR_MIN,
            sidebar_collapsed: true,
            inspector_width: 700.0,
            inspector_collapsed: false,
            timeline_height: 200.0,
            // The Scopes panel is in front instead, so the strip stays shut and
            // the frame is measured once.
            scopes_open: false,
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

    /// Build a [`DockTree`] whose weights and open frames reflect this pane
    /// arrangement.
    pub fn to_dock_tree(&self) -> DockTree {
        let clamped = self.clamped();
        let left_w = if clamped.sidebar_collapsed {
            0.0
        } else {
            clamped.sidebar_width
        };
        let right_w = if clamped.inspector_collapsed {
            0.0
        } else {
            clamped.inspector_width
        };
        let center_w = (1280.0 - left_w - right_w).max(320.0);
        let timeline_r = (clamped.timeline_height / 720.0).clamp(0.15, 0.85);
        let viewer_r = 1.0 - timeline_r;
        let center_col = Node::Split {
            axis: Axis::Column,
            children: vec![
                (
                    viewer_r,
                    Node::Frame(Frame::new(FrameId::CENTER, vec![PANEL_VIEWER])),
                ),
                (
                    timeline_r,
                    Node::Frame(Frame::new(FrameId::BOTTOM, vec![PANEL_TIMELINE])),
                ),
            ],
        };
        let mut row_children = Vec::with_capacity(3);
        if !clamped.sidebar_collapsed {
            row_children.push((
                left_w,
                Node::Frame(Frame::new(FrameId::LEFT, vec![PANEL_PROJECT])),
            ));
        }
        row_children.push((center_w, center_col));
        if !clamped.inspector_collapsed {
            row_children.push((
                right_w,
                Node::Frame(Frame::new(FrameId::RIGHT, vec![PANEL_INSPECTOR])),
            ));
        }
        let mut tree = DockTree {
            version: DOCK_VERSION,
            root: Node::Split {
                axis: Axis::Row,
                children: row_children,
            },
            floating: Vec::new(),
            maximized: None,
            focused: FrameId::BOTTOM,
        };
        tree.normalize();
        let target_panel = clamped.panel.panel_id();
        if tree.is_open(target_panel) {
            tree.activate(target_panel);
        } else {
            tree.open_in(target_panel, DefaultFrame::Center);
        }
        tree
    }
}

/// The shipped workspace with this id.
pub fn workspace(id: &str) -> Option<&'static Workspace> {
    WORKSPACES.iter().find(|ws| ws.id == id)
}

/// Build the shipped [`DockTree`] for `id`.
///
/// A tab whose panel is not registered stays in the tree, so a workspace gains
/// the panel once a plugin registers it; the shell skips unregistered tabs when
/// it builds a drawn frame and omits a frame whose tabs are all unregistered.
pub fn workspace_tree(id: &str, _registered_panels: &[PanelId]) -> Option<DockTree> {
    workspace_tree_for_width(id, DockTree::DEFAULT_WINDOW_WIDTH, _registered_panels)
}

/// Build the shipped [`DockTree`] for `id` with default weights derived from
/// the spec's pixel widths at `window_w`.
pub fn workspace_tree_for_width(
    id: &str,
    window_w: f32,
    _registered_panels: &[PanelId],
) -> Option<DockTree> {
    let (left_w, center_w, right_w) = DockTree::upper_row_weights_for_width(window_w, true);
    let mut tree = match id {
        "win-workspace-editing" => {
            let mut t = DockTree::editing_for_width(window_w);
            t.focused = FrameId::BOTTOM;
            t
        }
        "win-workspace-animation" => DockTree {
            version: DOCK_VERSION,
            root: Node::Split {
                axis: Axis::Column,
                children: vec![
                    (
                        0.50,
                        Node::Split {
                            axis: Axis::Row,
                            children: vec![
                                (
                                    left_w,
                                    Node::Frame(Frame::new(
                                        FrameId::LEFT,
                                        vec![PANEL_INSPECTOR, PANEL_PROJECT],
                                    )),
                                ),
                                (
                                    center_w,
                                    Node::Frame(Frame::new(
                                        FrameId::CENTER,
                                        vec![PANEL_VIEWER, PANEL_MIXER, PANEL_ASSISTANT],
                                    )),
                                ),
                                (
                                    right_w,
                                    Node::Frame(Frame::with_active(
                                        FrameId::RIGHT,
                                        vec![PANEL_INFO, PANEL_EFFECTS, PANEL_ALIGN],
                                        1,
                                    )),
                                ),
                            ],
                        },
                    ),
                    (
                        0.50,
                        Node::Frame(Frame::new(FrameId::BOTTOM, vec![PANEL_TIMELINE])),
                    ),
                ],
            },
            floating: Vec::new(),
            maximized: None,
            focused: FrameId::BOTTOM,
        },
        "win-workspace-colour" => DockTree {
            version: DOCK_VERSION,
            root: Node::Split {
                axis: Axis::Column,
                children: vec![
                    (
                        0.65,
                        Node::Split {
                            axis: Axis::Row,
                            children: vec![
                                (
                                    0.38,
                                    Node::Frame(Frame::new(
                                        FrameId::CENTER,
                                        vec![PANEL_VIEWER, PANEL_MIXER, PANEL_ASSISTANT],
                                    )),
                                ),
                                (
                                    0.32,
                                    Node::Frame(Frame::new(
                                        FrameId::LEFT,
                                        vec![PANEL_SCOPES, PANEL_PROJECT],
                                    )),
                                ),
                                (
                                    0.30,
                                    Node::Frame(Frame::new(
                                        FrameId::RIGHT,
                                        vec![PANEL_COLOR, PANEL_INSPECTOR, PANEL_EFFECTS],
                                    )),
                                ),
                            ],
                        },
                    ),
                    (
                        0.35,
                        Node::Frame(Frame::new(FrameId::BOTTOM, vec![PANEL_TIMELINE])),
                    ),
                ],
            },
            floating: Vec::new(),
            maximized: None,
            focused: FrameId::CENTER,
        },
        "win-workspace-audio" => DockTree {
            version: DOCK_VERSION,
            root: Node::Split {
                axis: Axis::Column,
                children: vec![
                    (
                        0.45,
                        Node::Split {
                            axis: Axis::Row,
                            children: vec![
                                (
                                    0.22,
                                    Node::Frame(Frame::new(
                                        FrameId::LEFT,
                                        vec![PANEL_VIEWER, PANEL_PROJECT],
                                    )),
                                ),
                                (
                                    0.50,
                                    Node::Frame(Frame::new(
                                        FrameId::CENTER,
                                        vec![PANEL_MIXER, PANEL_ASSISTANT],
                                    )),
                                ),
                                (
                                    0.28,
                                    Node::Frame(Frame::new(
                                        FrameId::RIGHT,
                                        vec![PANEL_AUDIO_METERS, PANEL_EFFECTS, PANEL_INSPECTOR],
                                    )),
                                ),
                            ],
                        },
                    ),
                    (
                        0.55,
                        Node::Frame(Frame::new(FrameId::BOTTOM, vec![PANEL_TIMELINE])),
                    ),
                ],
            },
            floating: Vec::new(),
            maximized: None,
            focused: FrameId::BOTTOM,
        },
        "win-workspace-effects" => DockTree {
            version: DOCK_VERSION,
            root: Node::Split {
                axis: Axis::Column,
                children: vec![
                    (
                        0.60,
                        Node::Split {
                            axis: Axis::Row,
                            children: vec![
                                (
                                    0.32,
                                    Node::Frame(Frame::new(
                                        FrameId::LEFT,
                                        vec![PANEL_EFFECTS, PANEL_PROJECT],
                                    )),
                                ),
                                (
                                    0.36,
                                    Node::Frame(Frame::new(
                                        FrameId::CENTER,
                                        vec![PANEL_VIEWER, PANEL_MIXER, PANEL_ASSISTANT],
                                    )),
                                ),
                                (
                                    0.32,
                                    Node::Frame(Frame::new(
                                        FrameId::RIGHT,
                                        vec![PANEL_INSPECTOR, PANEL_INFO],
                                    )),
                                ),
                            ],
                        },
                    ),
                    (
                        0.40,
                        Node::Frame(Frame::new(FrameId::BOTTOM, vec![PANEL_TIMELINE])),
                    ),
                ],
            },
            floating: Vec::new(),
            maximized: None,
            focused: FrameId::RIGHT,
        },
        _ => return None,
    };
    tree.normalize();
    Some(tree)
}

/// The Window-menu id of a plugin workspace is this prefix and the
/// workspace's `<plugin id>/<workspace id>` key.
pub const PLUGIN_PREFIX: &str = "win-workspace-plugin:";

/// The arrangement a plugin workspace restores, inside the bounds the panes
/// accept. A field the plugin left unset takes the Editing arrangement, and
/// the route is always the editor: a workspace arranges editing panes.
pub fn plugin_panes(panes: &aria_plugin::WorkspacePanes) -> PaneLayout {
    let editing = PaneLayout::default();
    PaneLayout {
        sidebar_width: panes.sidebar_width.unwrap_or(editing.sidebar_width),
        sidebar_collapsed: panes.sidebar_collapsed.unwrap_or(editing.sidebar_collapsed),
        inspector_width: panes.inspector_width.unwrap_or(editing.inspector_width),
        inspector_collapsed: panes
            .inspector_collapsed
            .unwrap_or(editing.inspector_collapsed),
        timeline_height: panes.timeline_height.unwrap_or(editing.timeline_height),
        scopes_open: panes.scopes.unwrap_or(editing.scopes_open),
        panel: panes.focus.map_or(editing.panel, focus_panel),
        route: UiRoute::Editor,
    }
    .clamped()
}

/// The pane a plugin workspace's focus names.
fn focus_panel(focus: aria_plugin::WorkspaceFocus) -> ActivePanel {
    use aria_plugin::WorkspaceFocus;
    match focus {
        WorkspaceFocus::Media => ActivePanel::Media,
        WorkspaceFocus::Preview => ActivePanel::Preview,
        WorkspaceFocus::Timeline => ActivePanel::Timeline,
        WorkspaceFocus::Inspector => ActivePanel::Inspector,
        WorkspaceFocus::Assistant => ActivePanel::Ai,
        WorkspaceFocus::Mixer => ActivePanel::Mixer,
    }
}

/// Resolve the [`DockTree`] for a plugin workspace: parses its declared `tree`
/// JSON when present and valid, or maps `panes` onto the Editing tree.
pub fn plugin_tree(panes: &aria_plugin::WorkspacePanes, registered_panels: &[PanelId]) -> DockTree {
    if let Some(json) = &panes.tree
        && let Ok(mut tree) = DockTree::from_json(json)
    {
        if !registered_panels.is_empty() {
            tree.retain_panels(|panel_id| registered_panels.contains(&panel_id));
        }
        return tree;
    }
    let mut tree = plugin_panes(panes).to_dock_tree();
    if !registered_panels.is_empty() {
        tree.retain_panels(|panel_id| registered_panels.contains(&panel_id));
    }
    tree
}

/// On-disk workspace file stored in `<data dir>/workspaces/<name>.json` or
/// imported/exported as JSON.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedWorkspace {
    pub version: u32,
    pub name: String,
    #[serde(default)]
    pub panes: PaneLayout,
    pub tree: DockTree,
}

impl SavedWorkspace {
    pub fn new(name: impl Into<String>, panes: PaneLayout, mut tree: DockTree) -> Self {
        tree.normalize();
        Self {
            version: DOCK_VERSION,
            name: name.into(),
            panes: panes.clamped(),
            tree,
        }
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    pub fn from_json(json: &str) -> Result<Self, String> {
        #[derive(Deserialize)]
        struct Probe {
            version: Option<u32>,
        }
        let probe: Probe =
            serde_json::from_str(json).map_err(|e| format!("invalid workspace JSON: {e}"))?;
        match probe.version {
            Some(DOCK_VERSION) => {}
            Some(other) => {
                return Err(format!(
                    "unsupported workspace version {other} (expected {DOCK_VERSION})"
                ));
            }
            None => return Err("missing workspace version field".to_string()),
        }
        let mut ws: Self =
            serde_json::from_str(json).map_err(|e| format!("invalid workspace JSON: {e}"))?;
        let trimmed = ws.name.trim();
        if trimmed.is_empty() {
            return Err("workspace name may not be blank".to_string());
        }
        ws.name = trimmed.to_string();
        ws.panes = ws.panes.clamped();
        ws.tree.normalize();
        Ok(ws)
    }
}

/// Validate a workspace file stem and return `<data dir>/workspaces/<name>.json`.
pub fn workspace_file_path(data_dir: &Path, name: &str) -> Result<PathBuf, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("workspace name may not be empty".to_string());
    }
    if trimmed.len() > 64 {
        return Err("workspace name may not exceed 64 bytes".to_string());
    }
    if trimmed.starts_with('.')
        || trimmed
            .chars()
            .any(|c| matches!(c, '/' | '\\' | '\0' | '\n' | '\r' | ':'))
    {
        return Err(format!(
            "workspace name `{trimmed}` contains invalid path characters"
        ));
    }
    Ok(data_dir
        .join(WORKSPACES_DIR)
        .join(format!("{trimmed}.json")))
}

/// Write `<data dir>/workspaces/<name>.json`.
pub fn save_named_workspace(
    data_dir: &Path,
    name: &str,
    panes: PaneLayout,
    tree: &DockTree,
) -> Result<PathBuf, String> {
    let path = workspace_file_path(data_dir, name)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("could not create {}: {e}", parent.display()))?;
    }
    let ws = SavedWorkspace::new(name.trim(), panes, tree.clone());
    let json = ws
        .to_json()
        .map_err(|e| format!("could not serialize workspace: {e}"))?;
    std::fs::write(&path, json).map_err(|e| format!("could not write {}: {e}", path.display()))?;
    Ok(path)
}

/// Read `<data dir>/workspaces/<name>.json`.
pub fn load_named_workspace(data_dir: &Path, name: &str) -> Result<SavedWorkspace, String> {
    let path = workspace_file_path(data_dir, name)?;
    import_workspace_json(&path)
}

/// Export a workspace as JSON to `path`.
pub fn export_workspace_json(path: &Path, ws: &SavedWorkspace) -> Result<(), String> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("could not create {}: {e}", parent.display()))?;
    }
    let json = ws
        .to_json()
        .map_err(|e| format!("could not serialize workspace: {e}"))?;
    std::fs::write(path, json).map_err(|e| format!("could not write {}: {e}", path.display()))
}

/// Import a workspace from a JSON file at `path`.
pub fn import_workspace_json(path: &Path) -> Result<SavedWorkspace, String> {
    let text = std::fs::read_to_string(path).map_err(|e| {
        if e.kind() == io::ErrorKind::NotFound {
            format!("workspace file {} not found", path.display())
        } else {
            format!("could not read {}: {e}", path.display())
        }
    })?;
    SavedWorkspace::from_json(&text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shipped_workspace_has_its_own_id_and_name() {
        let mut ids: Vec<&str> = WORKSPACES.iter().map(|ws| ws.id).collect();
        ids.push(CUSTOM_ID);
        ids.push(SAVE_ID);
        ids.push(RESET_ID);
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
}
