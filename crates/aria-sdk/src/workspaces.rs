//! Named workspaces registered by native plugins: the pane arrangement and
//! focus each restores.

use serde::{Deserialize, Serialize};

/// Pane that receives keyboard focus when a workspace is applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkspaceFocus {
    /// The Project panel.
    Media,
    /// The Viewer.
    Preview,
    /// The Timeline.
    Timeline,
    /// Effect Controls.
    Inspector,
    /// The assistant.
    Assistant,
    /// The mixer.
    Mixer,
}

/// Pane arrangement a workspace restores.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkspacePanes {
    /// Sidebar width in pixels, above 0 and at most 16384. `None` keeps the
    /// Editing width.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sidebar_width: Option<f32>,
    /// Collapses the sidebar. `None` shows it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sidebar_collapsed: Option<bool>,
    /// Inspector width in pixels, above 0 and at most 16384. `None` keeps the
    /// Editing width.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inspector_width: Option<f32>,
    /// Collapses the inspector. `None` shows it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inspector_collapsed: Option<bool>,
    /// Timeline height in pixels, above 0 and at most 16384. `None` keeps the
    /// Editing height.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeline_height: Option<f32>,
    /// Opens the scope strip. `None` leaves it closed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scopes: Option<bool>,
    /// The pane that takes the keyboard. `None` selects the Timeline.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub focus: Option<WorkspaceFocus>,
    /// A serialized `DockTree` JSON string. `None` derives the layout from the
    /// pane sizes and visibility.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tree: Option<String>,
}

/// Specification of a named workspace registered during `mount`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceSpec {
    /// Unique within the plugin: 1 to 64 ASCII letters, digits, `-`, `_` and
    /// `.`, not starting with `.`.
    pub id: String,
    /// The Window menu row and the Settings row: one line of at most 64 bytes.
    pub name: String,
    /// The arrangement applied.
    #[serde(default)]
    pub panes: WorkspacePanes,
}

impl WorkspaceSpec {
    /// Build a workspace specification with `id` and display `name`.
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            panes: WorkspacePanes::default(),
        }
    }

    /// Set the pane arrangement for the workspace.
    pub fn with_panes(mut self, panes: WorkspacePanes) -> Self {
        self.panes = panes;
        self
    }
}
