//! Workspaces contributed by plugins: named arrangements of the editor's panes
//! (`PluginContext::register_workspace` and `[[workspaces]]` in a package's
//! `plugin.toml`).
//!
//! A workspace is data. It sets pane sizes, which panes are collapsed, whether
//! the scope strip is open, and which pane has the keyboard; it holds no
//! project state, so applying one never changes the project. A field left
//! unset takes the host's Editing arrangement.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::context::PluginContext;
use crate::error::PluginError;
use crate::listed::{Listed, owner};
use crate::registration::{Contribution, ContributionKind, Registration};

/// The longest workspace id, in bytes.
pub const WORKSPACE_ID_MAX_BYTES: usize = 64;

/// The longest workspace name, in bytes.
pub const WORKSPACE_NAME_MAX_BYTES: usize = 64;

/// The largest pane size a workspace may state, in pixels. The host clamps a
/// size to its own range; this bound only refuses nonsense.
pub const WORKSPACE_MAX_SIZE: f32 = 16_384.0;

/// The pane that has the keyboard once the workspace is applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkspaceFocus {
    Media,
    Preview,
    Timeline,
    Inspector,
    Assistant,
    Mixer,
}

/// The arrangement a workspace restores. `None` takes the host's Editing
/// arrangement for that field.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WorkspacePanes {
    /// Sidebar width in pixels.
    pub sidebar_width: Option<f32>,
    pub sidebar_collapsed: Option<bool>,
    /// Inspector width in pixels.
    pub inspector_width: Option<f32>,
    pub inspector_collapsed: Option<bool>,
    /// Timeline height in pixels.
    pub timeline_height: Option<f32>,
    /// Whether the scope strip is open.
    pub scopes: Option<bool>,
    pub focus: Option<WorkspaceFocus>,
    /// Optional serialized dock tree JSON. When unset, the host maps the pane
    /// fields onto its Editing tree.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tree: Option<String>,
}

/// What a workspace is called and what it arranges.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceSpec {
    /// Unique within the registering plugin: ASCII letters, digits, `-`, `_`
    /// and `.`, not starting with `.`.
    pub id: String,
    /// The Window menu row and the Settings row.
    pub name: String,
    pub panes: WorkspacePanes,
}

/// One registered workspace, as the Window menu and Settings read it.
#[derive(Debug, Clone, PartialEq)]
pub struct PluginWorkspace {
    /// The registering plugin's id, empty for the host.
    pub plugin_id: Arc<str>,
    /// Display name of the registering plugin.
    pub plugin_name: Arc<str>,
    pub spec: Arc<WorkspaceSpec>,
}

impl PluginWorkspace {
    /// The name a host keys this workspace by: `<plugin id>/<workspace id>`.
    pub fn key(&self) -> String {
        format!("{}/{}", self.plugin_id, self.spec.id)
    }
}

/// The workspaces registered on one [`PluginContext`], in registration order.
pub(crate) type Workspaces = Listed<PluginWorkspace>;

fn refused(reason: String) -> PluginError {
    PluginError::WorkspaceRefused(reason)
}

/// Why `spec` is not a workspace the host can apply, or `None` when it is one.
pub(crate) fn spec_fault(spec: &WorkspaceSpec) -> Option<String> {
    let id = &spec.id;
    if id.is_empty() {
        return Some("a workspace needs an id".into());
    }
    if id.len() > WORKSPACE_ID_MAX_BYTES {
        return Some(format!(
            "workspace id `{id}` is longer than {WORKSPACE_ID_MAX_BYTES} bytes"
        ));
    }
    if id.starts_with('.')
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Some(format!(
            "workspace id `{id}` may hold ASCII letters, digits, `-`, `_` and `.`, and may not start with `.`"
        ));
    }
    let name = spec.name.trim();
    if name.is_empty() {
        return Some(format!("{id}: a workspace needs a name"));
    }
    if spec.name.len() > WORKSPACE_NAME_MAX_BYTES || spec.name.contains(['\n', '\r']) {
        return Some(format!(
            "{id}: a workspace name is one line of at most {WORKSPACE_NAME_MAX_BYTES} bytes"
        ));
    }
    let panes = &spec.panes;
    for (field, size) in [
        ("sidebar_width", panes.sidebar_width),
        ("inspector_width", panes.inspector_width),
        ("timeline_height", panes.timeline_height),
    ] {
        if let Some(size) = size
            && !(size.is_finite() && size > 0.0 && size <= WORKSPACE_MAX_SIZE)
        {
            return Some(format!(
                "{id}: {field} is {size}; a pane size is above 0 and at most {WORKSPACE_MAX_SIZE} pixels"
            ));
        }
    }
    if let Some(tree) = &panes.tree
        && tree.trim().is_empty()
    {
        return Some(format!("{id}: tree may not be blank"));
    }
    None
}

impl PluginContext {
    /// Add a workspace to the Window menu and Settings.
    ///
    /// Held on the terms of [`register_theme`](Self::register_theme): a
    /// registration made during mount drops when the plugin unmounts. Refused
    /// when the id is malformed or already registered by this plugin, the
    /// name is blank or longer than [`WORKSPACE_NAME_MAX_BYTES`], or a pane
    /// size is not finite, not above 0, or over [`WORKSPACE_MAX_SIZE`].
    pub fn register_workspace(&self, spec: WorkspaceSpec) -> Result<Registration, PluginError> {
        if let Some(fault) = spec_fault(&spec) {
            return Err(refused(fault));
        }
        let (plugin_id, plugin_name) = owner(self);
        let id = spec.id.clone();
        let item = PluginWorkspace {
            plugin_id: Arc::clone(&plugin_id),
            plugin_name,
            spec: Arc::new(spec),
        };
        let guard = self
            .workspace_registry()
            .insert(&plugin_id, item, |held| held.spec.id == id)
            .ok_or_else(|| {
                refused(format!(
                    "plugin `{}` already registered a workspace named `{id}`",
                    if plugin_id.is_empty() {
                        "host"
                    } else {
                        &*plugin_id
                    },
                ))
            })?;
        let contribution = Contribution::new(ContributionKind::Workspace, id);
        Ok(self.retain_if_mounting(Registration::holding(contribution, guard)))
    }

    /// Every registered workspace, in registration order.
    pub fn workspaces(&self) -> Vec<PluginWorkspace> {
        self.workspace_registry().items()
    }

    /// Counts workspace registrations and removals.
    pub fn workspaces_revision(&self) -> u64 {
        self.workspace_registry().revision()
    }
}

#[cfg(test)]
#[path = "workspaces_tests.rs"]
mod tests;
