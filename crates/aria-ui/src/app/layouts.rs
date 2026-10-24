//! Applying, capturing and listing a workspace.
//!
//! The arrangement itself is data in [`crate::layouts`]; this is the one place
//! that writes it into the running editor and the one place that reads the
//! running editor back out. Both go field by field through the same list, so a
//! workspace cannot restore a pane that capture does not record.
//!
//! Plugin workspaces are read from the plugin context on each call, so a
//! workspace leaves the menu the moment its plugin unloads.
//!
//! The active workspace is the one last chosen, `AppState::active_workspace_id`.
//! A workspace remembers the edits made to it, so a dragged divider or a moved
//! panel is part of the active workspace rather than a departure from it.

use crate::dock::{
    Axis, DOCK_GUTTER, DockTree, Node, PANEL_AUDIO_METERS, PANEL_INSPECTOR, PANEL_PROJECT,
    PANEL_TIMELINE, PANEL_VIEWER, PanelId,
};
use crate::interned::intern;
use crate::layouts::{self, CUSTOM_ID, CUSTOM_NAME, PLUGIN_PREFIX, PaneLayout};
use crate::types::ActivePanel;
use crate::types::UiRoute;

use super::AppState;

/// One workspace, as the Window menu and Settings show it.
pub(crate) struct WorkspaceRow {
    pub(crate) id: &'static str,
    pub(crate) name: &'static str,
    pub(crate) panes: PaneLayout,
    /// Whether this row is the workspace last chosen.
    pub(crate) active: bool,
    /// The custom slot, which can be re-saved and forgotten.
    pub(crate) custom: bool,
    /// The name of the plugin that registered the row, `None` for a shipped
    /// row and the Custom slot.
    pub(crate) plugin: Option<&'static str>,
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
            graph_open: self.timeline_graph_open(),
            panel: self.active_panel,
            route: self.route,
        }
    }

    /// Every dockable panel id currently registered with the host or declared
    /// by a plugin.
    pub(crate) fn registered_panel_ids(&self) -> Vec<PanelId> {
        let mut ids: Vec<PanelId> =
            if let Some(host) = crate::plugins::panel_host::host_of(&self.ctx) {
                host.dock_panels()
                    .into_iter()
                    .map(|entry| entry.id)
                    .collect()
            } else {
                crate::plugins::builtin_dock_panel_ids().collect()
            };
        for entry in self.ctx.panels() {
            let id = intern(&entry.id);
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        for waiting in self.plugin_panels.waiting().iter() {
            let id = intern(&waiting.spec.id);
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        ids
    }

    /// Whether `id` is currently registered as a dockable panel in `PanelHost`
    /// or declared by a loaded plugin.
    pub fn is_dock_panel_registered(&self, id: &str) -> bool {
        if let Some(host) = crate::plugins::panel_host::host_of(&self.ctx) {
            if host.get(id).is_some_and(|entry| entry.dock.is_some()) {
                return true;
            }
        } else if crate::plugins::builtin_dock_panel_ids().any(|builtin| builtin == id) {
            return true;
        }
        if self.ctx.panels().iter().any(|entry| entry.id == id) {
            return true;
        }
        self.plugin_panels
            .waiting()
            .iter()
            .any(|waiting| waiting.spec.id == id)
    }

    /// Whether `node` contains at least one registered dockable panel tab.
    pub fn node_has_registered_panel(&self, node: &Node) -> bool {
        match node {
            Node::Frame(frame) => frame
                .tabs
                .iter()
                .any(|&panel| self.is_dock_panel_registered(panel)),
            Node::Split { children, .. } => children
                .iter()
                .any(|child| self.node_has_registered_panel(&child.1)),
        }
    }

    /// The main window size [`Self::observe_main_window_size`] last recorded,
    /// or zero before the first observation.
    pub(crate) fn last_window_size(&self) -> (f32, f32) {
        self.dock_frame_rects
            .lock()
            .get(&crate::dock::FrameId(0))
            .map(|&[_, _, w, h]| (w, h))
            .unwrap_or((0.0, 0.0))
    }

    /// The shipped or declared [`DockTree`] for workspace `id`, ignoring any
    /// user edits made since it was applied.
    pub(crate) fn default_workspace_tree(&self, id: &str) -> Option<DockTree> {
        self.default_workspace_tree_for_width(id, self.last_window_size().0)
    }

    pub(crate) fn default_workspace_tree_for_width(
        &self,
        id: &str,
        window_w: f32,
    ) -> Option<DockTree> {
        if id == CUSTOM_ID {
            return self.ui_settings.workspace.map(|panes| panes.to_dock_tree());
        }
        let registered = self.registered_panel_ids();
        if let Some(key) = id.strip_prefix(PLUGIN_PREFIX) {
            return self
                .ctx
                .workspaces()
                .iter()
                .find(|ws| ws.key() == key)
                .map(|ws| layouts::plugin_tree(&ws.spec.panes, &registered));
        }
        if let Ok(saved) = layouts::load_named_workspace(&self.data_dir, id) {
            return Some(saved.tree);
        }
        let width = if window_w >= 320.0 {
            window_w
        } else {
            DockTree::DEFAULT_WINDOW_WIDTH
        };
        layouts::workspace_tree_for_width(id, width, &registered)
    }

    /// The [`DockTree`] restoring workspace `id` (remembered edits when
    /// present, otherwise its default tree).
    pub(crate) fn workspace_tree(&self, id: &str) -> Option<DockTree> {
        if let Some(saved) = self.ui_settings.workspace_trees.get(id) {
            let mut tree = saved.clone();
            tree.normalize();
            return Some(tree);
        }
        self.default_workspace_tree(id)
    }

    /// Synchronize legacy pane flags, active keyboard focus, and what the
    /// scopes measure from `dock_tree`.
    pub(crate) fn sync_panes_from_dock_tree(&mut self) {
        self.ui_settings.sidebar_collapsed = !self.dock_tree.is_open(PANEL_PROJECT);
        self.ui_settings.inspector_collapsed = !self.dock_tree.is_open(PANEL_INSPECTOR);
        if let Some(Node::Split {
            axis: Axis::Row,
            children,
        }) = self.dock_tree.node_at(&[0])
            && let Some(first) = children.first()
            && matches!(&first.1, Node::Frame(f) if f.id == crate::dock::FrameId::LEFT)
        {
            let (last_w, _) = self.last_window_size();
            let win_w = if last_w >= 320.0 {
                last_w
            } else {
                DockTree::DEFAULT_WINDOW_WIDTH
            };
            let drawn_count = children
                .iter()
                .filter(|c| self.node_has_registered_panel(&c.1))
                .count();
            // The row spans the window less the dock area's margin on each side
            // and one gutter between each pair of drawn frames.
            let gutters_px = (drawn_count.saturating_sub(1) as f32 + 2.0) * DOCK_GUTTER;
            let frame_pool_w = (win_w - gutters_px).max(1.0);
            self.ui_settings.sidebar_width = (first.0 * frame_pool_w)
                .round()
                .clamp(crate::settings::SIDEBAR_MIN, crate::settings::SIDEBAR_MAX);
        }
        if let Some(frame) = self.dock_tree.frame(self.dock_tree.focused)
            && let Some(panel) = ActivePanel::from_panel_id(frame.active_panel())
        {
            self.active_panel = panel;
        }
        self.sync_scope_demand();
    }

    /// Record an interactive edit to `dock_tree` for the active workspace and
    /// persist UI settings.
    pub(crate) fn record_dock_edit(&mut self, cx: &mut gpui::Context<Self>) {
        self.dock_tree.normalize();
        self.sync_panes_from_dock_tree();
        self.ui_settings.dock_tree = Some(self.dock_tree.clone());
        if !self.active_workspace_id.is_empty() {
            self.ui_settings
                .workspace_trees
                .insert(self.active_workspace_id.clone(), self.dock_tree.clone());
        }
        self.persist_settings_later(cx);
        cx.notify();
    }

    /// Record the main window's size in logical pixels and lay the dock out
    /// for it.
    ///
    /// A shipped workspace with no recorded edit takes the weights it ships
    /// with at the new size, as choosing it again would give it, so its frames
    /// do not depend on the sizes the window passed through, such as the
    /// unmaximized size a first launch draws at before the window manager
    /// maximizes it. Then a frame below its minimum size takes room from its
    /// siblings.
    pub fn observe_main_window_size(&mut self, window_w: f32, window_h: f32) -> bool {
        let (prev_w, prev_h) = self.last_window_size();
        self.dock_frame_rects
            .lock()
            .insert(crate::dock::FrameId(0), [0.0, 0.0, window_w, window_h]);
        if (window_w - prev_w).abs() <= 0.5 && (window_h - prev_h).abs() <= 0.5 {
            return false;
        }
        let shipped = self.unedited_shipped_tree(window_w);
        if let Some(shipped) = &shipped {
            self.dock_tree.adopt_weights(shipped);
        }
        self.rebalance_splits_for_window(window_w, window_h) || shipped.is_some()
    }

    /// The active workspace as it ships for a window `window_w` px wide, while
    /// it is a shipped workspace with no recorded edit.
    fn unedited_shipped_tree(&self, window_w: f32) -> Option<DockTree> {
        let id = if self.active_workspace_id.is_empty() {
            layouts::WORKSPACES[0].id
        } else {
            self.active_workspace_id.as_str()
        };
        if window_w < 320.0 || self.ui_settings.workspace_trees.contains_key(id) {
            return None;
        }
        layouts::workspace_tree_for_width(id, window_w, &[])
    }

    /// Last painted window bounds `[x, y, w, h]` of a docked frame.
    pub fn dock_frame_rect(&self, id: crate::dock::FrameId) -> Option<[f32; 4]> {
        self.dock_frame_rects.lock().get(&id).copied()
    }

    /// Tab label for a dock panel id.
    pub fn dock_panel_title(&self, id: &str) -> &'static str {
        if let Some(host) = crate::plugins::panel_host::host_of(&self.ctx)
            && let Some(entry) = host.get(id)
        {
            return entry.title;
        }
        if let Some(entry) = self.ctx.panels().into_iter().find(|entry| entry.id == id) {
            return intern(&entry.title);
        }
        if let Some(waiting) = self
            .plugin_panels
            .waiting()
            .iter()
            .find(|waiting| waiting.spec.id == id)
        {
            return intern(&waiting.spec.title);
        }
        // Read as the Audio Meters panel, as `default_frame_for` and
        // `ActivePanel::from_panel_id` read it.
        let id = match id {
            "aria.panel.audio-meters" => crate::dock::PANEL_AUDIO_METERS,
            other => other,
        };
        crate::plugins::builtin_panel_title(id).unwrap_or_else(|| intern(id))
    }

    /// Minimum logical pixel size `(min_w, min_h)` for a dock panel id.
    pub fn dock_panel_min_size(&self, id: &str) -> (f32, f32) {
        if let Some(host) = crate::plugins::panel_host::host_of(&self.ctx)
            && let Some(entry) = host.get(id)
            && let Some(dock) = entry.dock
        {
            return dock.min_size;
        }
        match id {
            PANEL_VIEWER => (320.0, 180.0),
            PANEL_TIMELINE => (400.0, 120.0),
            PANEL_AUDIO_METERS => (64.0, 80.0),
            _ => crate::plugins::panel_host::DockSpec::DEFAULT_MIN_SIZE,
        }
    }

    /// Minimum logical pixel size `(min_w, min_h)` for a subtree in `dock_tree`.
    pub fn node_min_size(&self, node: &Node) -> (f32, f32) {
        match node {
            Node::Frame(frame) => {
                let mut min_w = 0.0_f32;
                let mut min_h = 0.0_f32;
                for &tab in &frame.tabs {
                    if self.is_dock_panel_registered(tab) {
                        let (w, h) = self.dock_panel_min_size(tab);
                        min_w = min_w.max(w);
                        min_h = min_h.max(h);
                    }
                }
                if min_w == 0.0 && min_h == 0.0 {
                    for &tab in &frame.tabs {
                        let (w, h) = self.dock_panel_min_size(tab);
                        min_w = min_w.max(w);
                        min_h = min_h.max(h);
                    }
                }
                if min_w == 0.0 && min_h == 0.0 {
                    crate::plugins::panel_host::DockSpec::DEFAULT_MIN_SIZE
                } else {
                    (min_w, min_h)
                }
            }
            Node::Split { axis, children } => {
                let mins: Vec<(f32, f32)> = children
                    .iter()
                    .filter(|c| self.node_has_registered_panel(&c.1))
                    .map(|c| self.node_min_size(&c.1))
                    .collect();
                if mins.is_empty() {
                    return crate::plugins::panel_host::DockSpec::DEFAULT_MIN_SIZE;
                }
                let splitter_allowance = DOCK_GUTTER * (mins.len().saturating_sub(1) as f32);
                match axis {
                    Axis::Row => (
                        mins.iter().map(|(w, _)| *w).sum::<f32>() + splitter_allowance,
                        mins.iter().map(|(_, h)| *h).fold(0.0_f32, f32::max),
                    ),
                    Axis::Column => (
                        mins.iter().map(|(w, _)| *w).fold(0.0_f32, f32::max),
                        mins.iter().map(|(_, h)| *h).sum::<f32>() + splitter_allowance,
                    ),
                }
            }
        }
    }

    /// Drag a splitter at `split_path` between `index` and `index + 1` while
    /// enforcing each child subtree's minimum pixel size across `total_extent_px`.
    pub fn drag_split_clamped(
        &mut self,
        split_path: &[usize],
        index: usize,
        delta_weight: f32,
        total_extent_px: f32,
    ) -> bool {
        let Some(Node::Split { axis, children }) = self.dock_tree.node_at(split_path) else {
            return false;
        };
        if index + 1 >= children.len() {
            return false;
        }
        let axis = *axis;
        let min_a = self.node_min_size(&children[index].1);
        let min_b = self.node_min_size(&children[index + 1].1);
        let min_a_px = match axis {
            Axis::Row => min_a.0,
            Axis::Column => min_a.1,
        };
        let min_b_px = match axis {
            Axis::Row => min_b.0,
            Axis::Column => min_b.1,
        };
        let pair_sum = children[index].0 + children[index + 1].0;
        let extent = total_extent_px.max(1.0);
        let min_w_a = (min_a_px / extent).clamp(0.03, pair_sum * 0.95);
        let min_w_b = (min_b_px / extent).clamp(0.03, pair_sum * 0.95);
        if min_w_a + min_w_b >= pair_sum {
            return false;
        }
        let cur_a = children[index].0;
        let target_a = (cur_a + delta_weight).clamp(min_w_a, pair_sum - min_w_b);
        let clamped_delta = target_a - cur_a;
        if clamped_delta.abs() < 1e-5 {
            return false;
        }
        let Some(Node::Split { children, .. }) = self.dock_tree.node_at_mut(split_path) else {
            return false;
        };
        children[index].0 = target_a;
        children[index + 1].0 = pair_sum - target_a;
        self.sync_panes_from_dock_tree();
        true
    }

    /// Drag a splitter at `split_path` between `index` and `index + 1` by
    /// `delta_px` pixels inside a root dock area of `root_extent_px` along the
    /// split's axis, scaling by the split container's own span fraction so a
    /// nested split moves 1:1 with the pointer.
    pub fn drag_split_by_pixels(
        &mut self,
        split_path: &[usize],
        index: usize,
        delta_px: f32,
        root_extent_px: f32,
    ) -> bool {
        let Some(Node::Split { axis, .. }) = self.dock_tree.node_at(split_path) else {
            return false;
        };
        let axis = *axis;
        let span_px = (root_extent_px.max(100.0)
            * self.dock_tree.split_span_fraction(split_path, axis))
        .max(100.0);
        let delta_weight = delta_px / span_px;
        self.drag_split_clamped(split_path, index, delta_weight, span_px)
    }

    /// Rebalance split weights when a window resize would push a child frame
    /// below its minimum pixel size, taking the needed delta from sibling
    /// frames that still have slack (`docs/spec/layout.md:58-59`).
    pub fn rebalance_splits_for_window(&mut self, avail_w: f32, avail_h: f32) -> bool {
        let root_clone = self.dock_tree.root.clone();
        self.rebalance_node_mins(&root_clone, &[], avail_w, avail_h)
    }

    fn rebalance_node_mins(
        &mut self,
        snapshot: &Node,
        path: &[usize],
        avail_w: f32,
        avail_h: f32,
    ) -> bool {
        let Node::Split { axis, children } = snapshot else {
            return false;
        };
        let extent = match axis {
            Axis::Row => avail_w,
            Axis::Column => avail_h,
        };
        if extent <= 1.0 || children.is_empty() {
            return false;
        }
        let mins: Vec<f32> = children
            .iter()
            .map(|c| {
                if !self.node_has_registered_panel(&c.1) {
                    return 0.0;
                }
                let (mw, mh) = self.node_min_size(&c.1);
                let px = match axis {
                    Axis::Row => mw,
                    Axis::Column => mh,
                };
                (px / extent).min(1.0)
            })
            .collect();
        let total_min: f32 = mins.iter().sum();
        let mut weights: Vec<f32> = children.iter().map(|c| c.0).collect();
        let mut changed = false;
        if total_min > 0.0 && total_min <= 1.0 + 1e-5 {
            let mut deficit = 0.0_f32;
            for (w, &m) in weights.iter_mut().zip(mins.iter()) {
                if *w + 1e-4 < m {
                    deficit += m - *w;
                    *w = m;
                    changed = true;
                }
            }
            if deficit > 0.0 {
                let slack_total: f32 = weights
                    .iter()
                    .zip(mins.iter())
                    .map(|(&w, &m)| (w - m).max(0.0))
                    .sum();
                if slack_total > 1e-5 {
                    for (w, &m) in weights.iter_mut().zip(mins.iter()) {
                        let slack = (*w - m).max(0.0);
                        if slack > 0.0 {
                            let take = (deficit * (slack / slack_total)).min(slack);
                            *w -= take;
                        }
                    }
                }
            }
        }
        if changed
            && let Some(Node::Split {
                children: mut_children,
                ..
            }) = self.dock_tree.node_at_mut(path)
        {
            for (child, new_w) in mut_children.iter_mut().zip(weights.iter().copied()) {
                child.0 = new_w;
            }
        }
        for (idx, child) in children.iter().enumerate() {
            let w = weights[idx];
            let (child_w, child_h) = match axis {
                Axis::Row => (avail_w * w, avail_h),
                Axis::Column => (avail_w, avail_h * w),
            };
            let mut child_path = path.to_vec();
            child_path.push(idx);
            if self.rebalance_node_mins(&child.1, &child_path, child_w, child_h) {
                changed = true;
            }
        }
        changed
    }

    /// Restore the splitter weights at `split_path` from the active workspace's
    /// saved/default [`DockTree`], falling back to equal weights when the saved
    /// tree has no matching split at `split_path`.
    pub fn restore_split_weights(
        &mut self,
        split_path: &[usize],
        cx: &mut gpui::Context<Self>,
    ) -> bool {
        let ws_id = if self.active_workspace_id.is_empty() {
            layouts::WORKSPACES[0].id.to_string()
        } else {
            self.active_workspace_id.clone()
        };
        let (last_w, _) = self.last_window_size();
        let saved = self.default_workspace_tree(&ws_id).unwrap_or_else(|| {
            let width = if last_w >= 320.0 {
                last_w
            } else {
                DockTree::DEFAULT_WINDOW_WIDTH
            };
            DockTree::editing_for_width(width)
        });
        if !self.dock_tree.restore_split_from(split_path, &saved) {
            return false;
        }
        self.record_dock_edit(cx);
        true
    }

    /// Dock a floating frame back into `root` at its panels' default frames and
    /// post a notice when opening its OS window fails (`docs/spec/layout.md:67`).
    pub fn handle_float_open_failure(
        &mut self,
        frame_id: crate::dock::FrameId,
        error_text: impl Into<String>,
        cx: &mut gpui::Context<Self>,
    ) -> bool {
        if !self.redock_floating_frame(frame_id, cx) {
            return false;
        }
        let msg = format!("Could not open floating window: {}", error_text.into());
        self.status_message = Some(msg.clone());
        self.report_error(msg);
        cx.notify();
        true
    }
    /// Activate a dock panel tab and focus its frame.
    pub fn activate_dock_panel(&mut self, panel: PanelId, cx: &mut gpui::Context<Self>) -> bool {
        if !self.dock_tree.activate(panel) {
            return false;
        }
        if let Some(active) = ActivePanel::from_panel_id(panel) {
            self.active_panel = active;
        }
        self.record_dock_edit(cx);
        true
    }

    /// Close a dock panel tab.
    pub fn close_dock_panel(&mut self, panel: PanelId, cx: &mut gpui::Context<Self>) -> bool {
        if !self.dock_tree.close(panel) {
            return false;
        }
        self.record_dock_edit(cx);
        true
    }

    /// Close all tabs in `frame_id` except `keep`.
    pub fn close_other_dock_tabs(
        &mut self,
        frame_id: crate::dock::FrameId,
        keep: PanelId,
        cx: &mut gpui::Context<Self>,
    ) -> bool {
        if !self.dock_tree.close_other_tabs(frame_id, keep) {
            return false;
        }
        self.record_dock_edit(cx);
        true
    }

    /// Float a dock panel into a detached frame.
    pub fn float_dock_panel(
        &mut self,
        panel: PanelId,
        rect: impl Into<crate::dock::FloatRect>,
        cx: &mut gpui::Context<Self>,
    ) -> Option<crate::dock::FrameId> {
        let id = self.dock_tree.float(panel, rect)?;
        self.record_dock_edit(cx);
        Some(id)
    }

    /// Return all tabs in a floating frame back into the main dock tree.
    pub fn redock_floating_frame(
        &mut self,
        frame_id: crate::dock::FrameId,
        cx: &mut gpui::Context<Self>,
    ) -> bool {
        let Some(pos) = self
            .dock_tree
            .floating
            .iter()
            .position(|fl| fl.frame.id == frame_id)
        else {
            return false;
        };
        let floating = self.dock_tree.floating.remove(pos);
        for panel in floating.frame.tabs {
            let default_frame = crate::dock::default_frame_for(panel);
            self.dock_tree.open_in(panel, default_frame);
        }
        self.record_dock_edit(cx);
        true
    }

    /// Close a floating frame and every panel in it, for the window's Close
    /// Window button and the window manager's close control. The panels open
    /// again from the Window menu in their default frames.
    pub fn close_floating_frame(
        &mut self,
        frame_id: crate::dock::FrameId,
        cx: &mut gpui::Context<Self>,
    ) -> bool {
        let Some(pos) = self
            .dock_tree
            .floating
            .iter()
            .position(|fl| fl.frame.id == frame_id)
        else {
            return false;
        };
        self.dock_tree.floating.remove(pos);
        self.record_dock_edit(cx);
        true
    }

    /// The arrangement one row would restore, shipped, plugin, or saved.
    pub(crate) fn workspace_panes(&self, id: &str) -> Option<PaneLayout> {
        if id == CUSTOM_ID {
            return self.ui_settings.workspace;
        }
        if let Some(key) = id.strip_prefix(PLUGIN_PREFIX) {
            return self
                .ctx
                .workspaces()
                .iter()
                .find(|ws| ws.key() == key)
                .map(|ws| layouts::plugin_panes(&ws.spec.panes));
        }
        layouts::workspace(id).map(|ws| ws.panes)
    }

    /// Every row on offer: the shipped ones, then the plugins' in registration
    /// order, then the saved slot if there is one.
    pub(crate) fn workspace_rows(&self) -> Vec<WorkspaceRow> {
        let mut rows: Vec<WorkspaceRow> = layouts::WORKSPACES
            .iter()
            .map(|ws| WorkspaceRow {
                id: ws.id,
                name: ws.name,
                panes: ws.panes,
                active: self.is_active_workspace(ws.id),
                custom: false,
                plugin: None,
            })
            .collect();
        for ws in self.ctx.workspaces() {
            let id = intern(&format!("{PLUGIN_PREFIX}{}", ws.key()));
            rows.push(WorkspaceRow {
                id,
                name: intern(&ws.spec.name),
                panes: layouts::plugin_panes(&ws.spec.panes),
                active: self.is_active_workspace(id),
                custom: false,
                plugin: Some(intern(&ws.plugin_name)),
            });
        }
        if let Some(saved) = self.ui_settings.workspace {
            rows.push(WorkspaceRow {
                id: CUSTOM_ID,
                name: CUSTOM_NAME,
                panes: saved,
                active: self.is_active_workspace(CUSTOM_ID),
                custom: true,
                plugin: None,
            });
        }
        rows
    }

    /// Whether row `id` is the workspace last chosen, which the switcher, the
    /// Window menu and Settings mark. The workspace remembers the edits made
    /// to it, so it stays marked through a dragged divider, a moved panel and
    /// a click on another panel.
    pub(crate) fn is_active_workspace(&self, id: &str) -> bool {
        self.active_workspace_id == id
    }

    /// Arrange the editor the way one row says. Reports whether the row exists,
    /// so a row that vanished between the click and the keypress is a no-op
    /// rather than a silent wrong arrangement.
    pub(crate) fn apply_workspace(&mut self, id: &str, cx: &mut gpui::Context<Self>) -> bool {
        let Some(panes) = self.workspace_panes(id) else {
            return false;
        };
        let panes = panes.clamped();
        let tree = self
            .workspace_tree(id)
            .unwrap_or_else(|| panes.to_dock_tree());
        self.ui_settings.sidebar_width = panes.sidebar_width;
        self.ui_settings.sidebar_collapsed = panes.sidebar_collapsed;
        self.ui_settings.inspector_width = panes.inspector_width;
        self.ui_settings.inspector_collapsed = panes.inspector_collapsed;
        self.ui_settings.timeline_height = panes.timeline_height;
        self.scopes_open = panes.scopes_open;
        self.dock_tree = tree;
        // After both: the strip and the Scopes tab are read together, and a
        // reading starts or stops once rather than per field.
        self.sync_scope_demand();
        self.active_panel = panes.panel;
        self.active_workspace_id = id.to_string();
        self.ui_settings.active_workspace = Some(id.to_string());
        self.ui_settings.dock_tree = Some(self.dock_tree.clone());
        self.workspace_transition_epoch = self.workspace_transition_epoch.wrapping_add(1);
        if id == "win-workspace-audio" {
            self.set_effects_filter(Some(crate::app::effects_browser::Group::AudioEffects));
        } else {
            self.set_effects_filter(None);
        }
        if self.timeline_graph_open() != panes.graph_open {
            self.toggle_timeline_graph_editor();
        }
        // Home goes through the unsaved-changes prompt, so a workspace
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
        self.ui_settings.dock_tree = Some(self.dock_tree.clone());
        self.ui_settings
            .workspace_trees
            .insert(CUSTOM_ID.to_string(), self.dock_tree.clone());
        self.begin_rename(crate::types::RenameTarget::SaveWorkspace);
        self.status_message = Some(format!("Saved the {CUSTOM_NAME} workspace"));
        self.persist_settings_later(cx);
        cx.notify();
    }

    /// Save the current arrangement and dock tree under `<data dir>/workspaces/<name>.json`
    /// from the inline rename prompt (`RenameTarget::SaveWorkspace`).
    pub(crate) fn save_named_workspace(
        &mut self,
        name: &str,
    ) -> Result<(), aria_engine::EngineError> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Ok(());
        }
        match layouts::save_named_workspace(&self.data_dir, trimmed, self.panes(), &self.dock_tree)
        {
            Ok(_) => {
                self.active_workspace_id = trimmed.to_string();
                self.ui_settings.active_workspace = Some(trimmed.to_string());
                self.status_message = Some(format!("Saved workspace {trimmed}"));
                Ok(())
            }
            Err(err) => {
                self.report_error(err);
                Ok(())
            }
        }
    }

    /// Drop the saved slot, which takes its row off the menu.
    pub(crate) fn forget_workspace(&mut self, cx: &mut gpui::Context<Self>) {
        if self.ui_settings.workspace.take().is_none() {
            return;
        }
        self.ui_settings.workspace_trees.remove(CUSTOM_ID);
        self.status_message = Some(format!("Forgot the {CUSTOM_NAME} workspace"));
        self.persist_settings_later(cx);
        cx.notify();
    }

    /// Restore the active workspace to its shipped/saved arrangement, discarding
    /// remembered dock edits for that workspace.
    pub fn reset_workspace_layout(&mut self, cx: &mut gpui::Context<Self>) {
        let target = if self.active_workspace_id.is_empty() {
            layouts::WORKSPACES[0].id.to_string()
        } else {
            self.active_workspace_id.clone()
        };
        self.ui_settings.workspace_trees.remove(&target);
        if !self.apply_workspace(&target, cx) {
            let fallback = layouts::WORKSPACES[0].id;
            self.ui_settings.workspace_trees.remove(fallback);
            let _ = self.apply_workspace(fallback, cx);
        }
        self.status_message = Some("Reset to saved layout".to_string());
        self.persist_settings_later(cx);
        cx.notify();
    }

    /// Save the current arrangement and dock tree under `<data dir>/workspaces/<name>.json`.
    pub fn save_workspace_as(
        &mut self,
        name: &str,
        cx: &mut gpui::Context<Self>,
    ) -> Result<std::path::PathBuf, String> {
        match layouts::save_named_workspace(&self.data_dir, name, self.panes(), &self.dock_tree) {
            Ok(path) => {
                self.status_message = Some(format!("Saved workspace {}", name.trim()));
                cx.notify();
                Ok(path)
            }
            Err(err) => {
                self.report_error(err.clone());
                Err(err)
            }
        }
    }

    /// Load a named workspace from `<data dir>/workspaces/<name>.json` and apply it.
    pub fn load_named_workspace(
        &mut self,
        name: &str,
        cx: &mut gpui::Context<Self>,
    ) -> Result<(), String> {
        let saved = match layouts::load_named_workspace(&self.data_dir, name) {
            Ok(ws) => ws,
            Err(err) => {
                self.report_error(err.clone());
                return Err(err);
            }
        };
        self.apply_saved_workspace(saved, cx);
        Ok(())
    }

    /// Export the current workspace to `path` as JSON.
    pub fn export_workspace(&mut self, path: &std::path::Path) -> Result<(), String> {
        let ws = layouts::SavedWorkspace::new(
            if self.active_workspace_id.is_empty() {
                CUSTOM_NAME
            } else {
                &self.active_workspace_id
            },
            self.panes(),
            self.dock_tree.clone(),
        );
        if let Err(err) = layouts::export_workspace_json(path, &ws) {
            self.report_error(err.clone());
            return Err(err);
        }
        self.status_message = Some(format!("Exported workspace to {}", path.display()));
        Ok(())
    }

    /// Import a workspace JSON file from `path`, store it in `<data dir>/workspaces/`
    /// when a data dir is configured, and apply it.
    pub fn import_workspace(
        &mut self,
        path: &std::path::Path,
        cx: &mut gpui::Context<Self>,
    ) -> Result<String, String> {
        let saved = match layouts::import_workspace_json(path) {
            Ok(ws) => ws,
            Err(err) => {
                self.report_error(err.clone());
                return Err(err);
            }
        };
        let _ =
            layouts::save_named_workspace(&self.data_dir, &saved.name, saved.panes, &saved.tree);
        let name = saved.name.clone();
        self.apply_saved_workspace(saved, cx);
        Ok(name)
    }

    fn apply_saved_workspace(
        &mut self,
        saved: layouts::SavedWorkspace,
        cx: &mut gpui::Context<Self>,
    ) {
        let panes = saved.panes.clamped();
        self.ui_settings.sidebar_width = panes.sidebar_width;
        self.ui_settings.sidebar_collapsed = panes.sidebar_collapsed;
        self.ui_settings.inspector_width = panes.inspector_width;
        self.ui_settings.inspector_collapsed = panes.inspector_collapsed;
        self.ui_settings.timeline_height = panes.timeline_height;
        self.scopes_open = panes.scopes_open;
        let mut tree = saved.tree;
        tree.normalize();
        self.dock_tree = tree;
        self.sync_scope_demand();
        self.active_panel = panes.panel;
        self.active_workspace_id = saved.name.clone();
        self.ui_settings.active_workspace = Some(saved.name.clone());
        self.ui_settings.dock_tree = Some(self.dock_tree.clone());
        self.workspace_transition_epoch = self.workspace_transition_epoch.wrapping_add(1);
        self.status_message = Some(format!("{} workspace", saved.name));
        self.persist_settings_later(cx);
        cx.notify();
    }
}
