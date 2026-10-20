//! Dockable frame, tab, split, floating-window, and maximize tree.
//!
//! Pure data and operations with no GPUI dependency. Every mutation preserves
//! the tree invariants:
//! - no frame has an empty tab list;
//! - no split has fewer than two children;
//! - no split is nested directly inside a split of the same axis;
//! - every split weight and floating dimension is finite and strictly positive;
//! - a panel appears at most once across the root tree and all floating frames;
//! - every frame's `active` index is within `0..tabs.len()`;
//! - `maximized`, when set, names a frame in `root`.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::interned::intern;

use nodes::{
    activate_in_node, collect_frames, find_frame_mut, find_frame_ref, find_in_node, node_at_path,
    node_at_path_mut, node_has_frame, remove_from_node, retain_in_node, root_panel_count,
    split_frame_in_node,
};

/// Schema version of a persisted [`DockTree`]. A tree with any other version is
/// rejected on load so stale on-disk shapes fall back to the shipped layout.
pub const DOCK_VERSION: u32 = 2;

/// Lowest weight a split child may hold.
pub const MIN_WEIGHT: f32 = 0.02;

/// Lowest width or height a floating frame may hold, in logical pixels.
pub const MIN_FLOAT_W: f32 = 160.0;
pub const MIN_FLOAT_H: f32 = 80.0;

/// The gap between two docked frames, which is also the splitter's hit strip,
/// and the margin between the docked frames and the bars and window edges
/// around them, in logical pixels: every frame edge stands this far from its
/// neighbour.
pub const DOCK_GUTTER: f32 = 6.0;

/// Interned identifier of a dockable panel.
pub type PanelId = &'static str;

pub const PANEL_PROJECT: PanelId = "aria.panel.media";
pub const PANEL_VIEWER: PanelId = "aria.panel.viewer";
pub const PANEL_TIMELINE: PanelId = "aria.panel.timeline";
pub const PANEL_INSPECTOR: PanelId = "aria.panel.inspector";
pub const PANEL_MIXER: PanelId = "aria.panel.mixer";
pub const PANEL_ASSISTANT: PanelId = "aria.panel.assistant";
pub const PANEL_SCOPES: PanelId = "aria.panel.scopes";
pub const PANEL_COLOR: PanelId = "aria.panel.color";
pub const PANEL_INFO: PanelId = "aria.panel.info";
pub const PANEL_EFFECTS: PanelId = "aria.panel.effects";
pub const PANEL_ALIGN: PanelId = "aria.panel.align";
pub const PANEL_CHARACTER: PanelId = "aria.panel.character";
pub const PANEL_PARAGRAPH: PanelId = "aria.panel.paragraph";
pub const PANEL_AUDIO_METERS: PanelId = "aria.panel.audio_meters";
pub const PANEL_JOBS: PanelId = "aria.panel.jobs";
pub const PANEL_HISTORY: PanelId = "aria.panel.history";
pub const PANEL_MARKERS: PanelId = "aria.panel.markers";

/// Stable identifier of a frame inside a [`DockTree`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FrameId(pub u32);

impl FrameId {
    pub const LEFT: Self = Self(1);
    pub const CENTER: Self = Self(2);
    pub const RIGHT: Self = Self(3);
    pub const BOTTOM: Self = Self(4);
    pub const METERS: Self = Self(5);
}

impl fmt::Display for FrameId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Which standard frame a panel opens into when its own frame is present.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DefaultFrame {
    Left,
    Center,
    Right,
    Bottom,
}

impl DefaultFrame {
    pub const fn frame(self) -> FrameId {
        match self {
            Self::Left => FrameId::LEFT,
            Self::Center => FrameId::CENTER,
            Self::Right => FrameId::RIGHT,
            Self::Bottom => FrameId::BOTTOM,
        }
    }
}

/// Default frame for a panel id when no custom registry entry overrides it.
pub fn default_frame_for(panel: PanelId) -> DefaultFrame {
    match panel {
        PANEL_PROJECT | PANEL_INSPECTOR | PANEL_MARKERS | PANEL_SCOPES => DefaultFrame::Left,
        PANEL_VIEWER | PANEL_MIXER | PANEL_ASSISTANT => DefaultFrame::Center,
        PANEL_TIMELINE | PANEL_AUDIO_METERS | "aria.panel.audio-meters" => DefaultFrame::Bottom,
        PANEL_COLOR | PANEL_INFO | PANEL_EFFECTS | PANEL_ALIGN | PANEL_CHARACTER
        | PANEL_PARAGRAPH | PANEL_JOBS | PANEL_HISTORY => DefaultFrame::Right,
        _ => DefaultFrame::Right,
    }
}

/// Direction a [`Node::Split`] lays out its children.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Axis {
    /// Children sit left to right in a horizontal row.
    Row,
    /// Children sit top to bottom in a vertical column.
    Column,
}

/// Drop target within a frame when docking a dragged panel tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Zone {
    /// Add as a tab of the target frame.
    Center,
    /// Split the target frame horizontally, placing the panel on the left.
    Left,
    /// Split the target frame horizontally, placing the panel on the right.
    Right,
    /// Split the target frame vertically, placing the panel on the top.
    Top,
    /// Split the target frame vertically, placing the panel on the bottom.
    Bottom,
}

impl Zone {
    pub const ALL: [Self; 5] = [
        Self::Center,
        Self::Left,
        Self::Right,
        Self::Top,
        Self::Bottom,
    ];
}

fn serialize_tabs<S: Serializer>(tabs: &[PanelId], serializer: S) -> Result<S::Ok, S::Error> {
    tabs.serialize(serializer)
}

fn deserialize_tabs<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<PanelId>, D::Error> {
    let raw = Vec::<String>::deserialize(deserializer)?;
    Ok(raw
        .into_iter()
        .filter(|s| !s.trim().is_empty())
        .map(|s| intern(&s))
        .collect())
}

/// One tabbed frame holding one or more panels.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Frame {
    pub id: FrameId,
    #[serde(
        serialize_with = "serialize_tabs",
        deserialize_with = "deserialize_tabs"
    )]
    pub tabs: Vec<PanelId>,
    pub active: usize,
}

impl Frame {
    pub fn new(id: FrameId, tabs: Vec<PanelId>) -> Self {
        Self {
            id,
            tabs,
            active: 0,
        }
    }

    pub fn with_active(id: FrameId, tabs: Vec<PanelId>, active: usize) -> Self {
        let clamped = if tabs.is_empty() {
            0
        } else {
            active.min(tabs.len() - 1)
        };
        Self {
            id,
            tabs,
            active: clamped,
        }
    }

    /// The currently visible panel tab of this frame.
    pub fn active_panel(&self) -> PanelId {
        self.tabs[self.active.min(self.tabs.len().saturating_sub(1))]
    }
}

/// A node of the docked layout tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Node {
    Split {
        axis: Axis,
        children: Vec<(f32, Node)>,
    },
    Frame(Frame),
}

/// Rectangle of a floating panel window in screen/window coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FloatRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl FloatRect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }
}

impl From<(f32, f32, f32, f32)> for FloatRect {
    fn from((x, y, w, h): (f32, f32, f32, f32)) -> Self {
        Self { x, y, w, h }
    }
}

/// A frame undocked into its own floating window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Floating {
    pub frame: Frame,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

fn default_focused() -> FrameId {
    FrameId::CENTER
}

#[derive(Deserialize)]
struct RawDockTree {
    version: u32,
    root: Node,
    #[serde(default)]
    floating: Vec<Floating>,
    #[serde(default = "default_focused")]
    focused: FrameId,
    #[serde(default)]
    maximized: Option<FrameId>,
}

/// Complete arrangement of docked frames, split ratios, floating frames, and
/// maximized state.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DockTree {
    pub version: u32,
    pub root: Node,
    pub floating: Vec<Floating>,
    pub focused: FrameId,
    pub maximized: Option<FrameId>,
}

impl<'de> Deserialize<'de> for DockTree {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = RawDockTree::deserialize(deserializer)?;
        if raw.version != DOCK_VERSION {
            return Err(serde::de::Error::custom(format!(
                "unsupported dock tree version {}, expected {DOCK_VERSION}",
                raw.version
            )));
        }
        let tree = Self {
            version: raw.version,
            root: raw.root,
            floating: raw.floating,
            focused: raw.focused,
            maximized: raw.maximized,
        };
        tree.check_invariants().map_err(serde::de::Error::custom)?;
        Ok(tree)
    }
}

impl Default for DockTree {
    fn default() -> Self {
        Self::editing()
    }
}

impl DockTree {
    /// Borrow a [`Node`] in `root` by its split child path.
    pub fn node_at(&self, path: &[usize]) -> Option<&Node> {
        node_at_path(&self.root, path)
    }

    /// Mutably borrow a [`Node`] in `root` by its split child path.
    pub fn node_at_mut(&mut self, path: &[usize]) -> Option<&mut Node> {
        node_at_path_mut(&mut self.root, path)
    }

    /// Collect every [`Frame`] in `node` in pre-order.
    pub fn collect_frames_from<'a>(node: &'a Node, out: &mut Vec<&'a Frame>) {
        collect_frames(node, out);
    }

    /// Locate which frame (docked or floating) currently holds `panel`.
    pub fn find(&self, panel: &str) -> Option<FrameId> {
        for frame in self.frames() {
            if frame.tabs.contains(&panel) {
                return Some(frame.id);
            }
        }
        None
    }

    /// Whether `panel` is currently open in any docked or floating frame.
    pub fn is_open(&self, panel: &str) -> bool {
        self.find(panel).is_some()
    }

    /// Whether `panel` is the active tab of its frame.
    pub fn is_active(&self, panel: PanelId) -> bool {
        self.frames()
            .into_iter()
            .any(|frame| frame.active_panel() == panel)
    }

    /// Look up a frame by id across `root` and `floating`.
    pub fn frame(&self, id: FrameId) -> Option<&Frame> {
        if let Some(frame) = find_frame_ref(&self.root, id) {
            return Some(frame);
        }
        self.floating
            .iter()
            .find(|fl| fl.frame.id == id)
            .map(|fl| &fl.frame)
    }

    /// Mutable lookup of a frame by id across `root` and `floating`.
    pub fn frame_mut(&mut self, id: FrameId) -> Option<&mut Frame> {
        if let Some(frame) = find_frame_mut(&mut self.root, id) {
            return Some(frame);
        }
        self.floating
            .iter_mut()
            .find(|fl| fl.frame.id == id)
            .map(|fl| &mut fl.frame)
    }

    /// Make `panel` the active tab of its frame and focus that frame. Returns
    /// `true` if `panel` was found.
    pub fn activate(&mut self, panel: PanelId) -> bool {
        if let Some(id) = activate_in_node(&mut self.root, panel) {
            self.focused = id;
            return true;
        }
        for fl in &mut self.floating {
            if let Some(idx) = fl.frame.tabs.iter().position(|&p| p == panel) {
                fl.frame.active = idx;
                self.focused = fl.frame.id;
                return true;
            }
        }
        false
    }

    /// Close `panel`. If its frame becomes empty, the frame is removed and any
    /// single-child parent split collapses. Refuses to close the very last
    /// panel in `root` so the main window never becomes empty.
    pub fn close(&mut self, panel: PanelId) -> bool {
        let in_root = find_in_node(&self.root, panel).is_some();
        if in_root && root_panel_count(&self.root) <= 1 {
            return false;
        }
        let removed = self.remove_panel_internal(panel);
        if removed {
            self.normalize();
        }
        removed
    }

    /// Close every tab in `frame_id` except `keep_panel`.
    pub fn close_other_tabs(&mut self, frame_id: FrameId, keep_panel: PanelId) -> bool {
        let Some(frame) = self.frame_mut(frame_id) else {
            return false;
        };
        if !frame.tabs.contains(&keep_panel) || frame.tabs.len() <= 1 {
            return false;
        }
        frame.tabs.retain(|&p| p == keep_panel);
        frame.active = 0;
        self.focused = frame_id;
        self.normalize();
        true
    }

    /// Open `panel` if closed and make it the active focused tab, placed as
    /// [`Self::open_in`] places it for `default_frame_for(panel)`.
    pub fn open(&mut self, panel: PanelId) -> FrameId {
        self.open_in(panel, default_frame_for(panel))
    }

    /// Open `panel` if closed and make it the active focused tab. A closed
    /// panel opens into the first of these that applies:
    /// - Audio Meters with a `Bottom` preference: [`FrameId::METERS`],
    ///   re-created right of the Timeline frame when closed.
    /// - The frame `preferred.frame()` when `root` holds it.
    /// - That frame re-created where the shipped layouts put it, when no frame
    ///   holds its id (`dock/restore.rs`).
    /// - The largest frame in `root`.
    pub fn open_in(&mut self, panel: PanelId, preferred: DefaultFrame) -> FrameId {
        let interned = intern(panel);
        if let Some(existing) = self.find(interned) {
            self.activate(interned);
            return existing;
        }
        if interned == PANEL_AUDIO_METERS && preferred == DefaultFrame::Bottom {
            return self.open_meters(interned);
        }
        let preferred_id = preferred.frame();
        let target_id = if find_frame_ref(&self.root, preferred_id).is_some() {
            preferred_id
        } else if self.frame(preferred_id).is_none()
            && let Some(restored) = self.restore_frame(interned, preferred)
        {
            return restored;
        } else {
            self.largest_frame()
        };
        if let Some(frame) = find_frame_mut(&mut self.root, target_id) {
            frame.tabs.push(interned);
            frame.active = frame.tabs.len() - 1;
            self.focused = target_id;
            return target_id;
        }
        let fallback = self.largest_frame();
        if let Some(frame) = find_frame_mut(&mut self.root, fallback) {
            frame.tabs.push(interned);
            frame.active = frame.tabs.len() - 1;
            self.focused = fallback;
        }
        fallback
    }

    /// Append `panel` as a tab of `target` (moving it from any previous frame)
    /// and focus it.
    pub fn add_tab(&mut self, target: FrameId, panel: PanelId) -> bool {
        self.dock(panel, target, Zone::Center) == Some(target)
    }

    /// Reorder a tab inside `frame_id` from `from_idx` to `to_idx`, keeping the
    /// previously active panel active.
    pub fn reorder(&mut self, frame_id: FrameId, from_idx: usize, to_idx: usize) -> bool {
        let Some(frame) = self.frame_mut(frame_id) else {
            return false;
        };
        if from_idx >= frame.tabs.len() || to_idx >= frame.tabs.len() || from_idx == to_idx {
            return false;
        }
        let active_panel = frame.active_panel();
        let tab = frame.tabs.remove(from_idx);
        frame.tabs.insert(to_idx, tab);
        if let Some(new_active) = frame.tabs.iter().position(|&p| p == active_panel) {
            frame.active = new_active;
        }
        self.focused = frame_id;
        true
    }

    /// Move `panel` inside `frame_id` to `to_idx`.
    pub fn reorder_panel(&mut self, frame_id: FrameId, panel: PanelId, to_idx: usize) -> bool {
        let Some(from_idx) = self
            .frame(frame_id)
            .and_then(|f| f.tabs.iter().position(|&p| p == panel))
        else {
            return false;
        };
        let len = self.frame(frame_id).map(|f| f.tabs.len()).unwrap_or(0);
        if len == 0 {
            return false;
        }
        self.reorder(frame_id, from_idx, to_idx.min(len - 1))
    }

    /// Dock `panel` onto `target` at `zone`.
    ///
    /// - [`Zone::Center`] appends `panel` to `target`'s tabs (or activates it if
    ///   already in `target`).
    /// - [`Zone::Left`], [`Zone::Right`], [`Zone::Top`], [`Zone::Bottom`] split
    ///   `target` along the corresponding axis and flatten into the parent split
    ///   when the axes match.
    pub fn dock(&mut self, panel: PanelId, target: FrameId, zone: Zone) -> Option<FrameId> {
        let interned = intern(panel);
        self.frame(target)?;

        if zone == Zone::Center {
            if self.find(interned) == Some(target) {
                self.activate(interned);
                return Some(target);
            }
            let in_root = find_in_node(&self.root, interned).is_some();
            let target_in_root = find_frame_ref(&self.root, target).is_some();
            if in_root && !target_in_root && root_panel_count(&self.root) <= 1 {
                return None;
            }
            self.remove_panel_internal(interned);
            if let Some(frame) = self.frame_mut(target) {
                frame.tabs.push(interned);
                frame.active = frame.tabs.len() - 1;
                self.focused = target;
                self.normalize();
                return Some(target);
            }
            self.normalize();
            return None;
        }

        if self.find(interned) == Some(target)
            && self.frame(target).is_some_and(|f| f.tabs.len() <= 1)
        {
            self.activate(interned);
            return Some(target);
        }

        if find_frame_ref(&self.root, target).is_none() {
            return self.dock(interned, target, Zone::Center);
        }

        self.remove_panel_internal(interned);
        let fresh_id = self.fresh_frame_id();
        if !split_frame_in_node(&mut self.root, target, fresh_id, interned, zone, 0.5) {
            self.normalize();
            return None;
        }
        self.focused = fresh_id;
        self.normalize();
        self.find(interned)
    }

    /// Undock `panel` into a new floating window at `rect`.
    ///
    /// Refuses to float the sole remaining panel in `root` so the main window
    /// always retains at least one docked frame.
    pub fn float(&mut self, panel: PanelId, rect: impl Into<FloatRect>) -> Option<FrameId> {
        let interned = intern(panel);
        let rect = rect.into();
        let in_root = find_in_node(&self.root, interned).is_some();
        if in_root && root_panel_count(&self.root) <= 1 {
            return None;
        }
        if let Some(fl) = self
            .floating
            .iter_mut()
            .find(|fl| fl.frame.tabs.len() == 1 && fl.frame.tabs[0] == interned)
        {
            fl.x = if rect.x.is_finite() { rect.x } else { 80.0 };
            fl.y = if rect.y.is_finite() { rect.y } else { 80.0 };
            fl.w = if rect.w.is_finite() {
                rect.w.max(MIN_FLOAT_W)
            } else {
                480.0
            };
            fl.h = if rect.h.is_finite() {
                rect.h.max(MIN_FLOAT_H)
            } else {
                320.0
            };
            let id = fl.frame.id;
            self.focused = id;
            return Some(id);
        }
        self.remove_panel_internal(interned);
        let fresh_id = self.fresh_frame_id();
        self.floating.push(Floating {
            frame: Frame::new(fresh_id, vec![interned]),
            x: if rect.x.is_finite() { rect.x } else { 80.0 },
            y: if rect.y.is_finite() { rect.y } else { 80.0 },
            w: if rect.w.is_finite() {
                rect.w.max(MIN_FLOAT_W)
            } else {
                480.0
            },
            h: if rect.h.is_finite() {
                rect.h.max(MIN_FLOAT_H)
            } else {
                320.0
            },
        });
        self.focused = fresh_id;
        self.normalize();
        Some(fresh_id)
    }

    /// Every frame in reading order: `root` pre-order (left-to-right,
    /// top-to-bottom), followed by `floating` frames.
    pub fn frames(&self) -> Vec<&Frame> {
        let mut out = Vec::new();
        collect_frames(&self.root, &mut out);
        for fl in &self.floating {
            out.push(&fl.frame);
        }
        out
    }

    /// Every docked frame in `root`, in reading order.
    pub fn root_frames(&self) -> Vec<&Frame> {
        let mut out = Vec::new();
        collect_frames(&self.root, &mut out);
        out
    }

    /// Frame ids in reading order.
    pub fn frame_ids(&self) -> Vec<FrameId> {
        self.frames().into_iter().map(|f| f.id).collect()
    }

    /// Keep only panels matching `keep`, collapsing any emptied frames or
    /// single-child splits.
    pub fn retain_panels(&mut self, mut keep: impl FnMut(PanelId) -> bool) {
        retain_in_node(&mut self.root, &mut keep);
        for fl in &mut self.floating {
            let active = fl.frame.active_panel();
            fl.frame.tabs.retain(|&p| keep(p));
            if let Some(pos) = fl.frame.tabs.iter().position(|&p| p == active) {
                fl.frame.active = pos;
            } else {
                fl.frame.active = 0;
            }
        }
        self.normalize();
    }

    fn remove_panel_internal(&mut self, panel: PanelId) -> bool {
        if remove_from_node(&mut self.root, panel) {
            return true;
        }
        for (idx, fl) in self.floating.iter_mut().enumerate() {
            if let Some(pos) = fl.frame.tabs.iter().position(|&p| p == panel) {
                fl.frame.tabs.remove(pos);
                if fl.frame.tabs.is_empty() {
                    self.floating.remove(idx);
                } else {
                    if pos <= fl.frame.active && fl.frame.active > 0 {
                        fl.frame.active -= 1;
                    }
                    fl.frame.active = fl.frame.active.min(fl.frame.tabs.len() - 1);
                }
                return true;
            }
        }
        false
    }
}

mod editing;
mod focus;
mod invariants;
mod nodes;
mod restore;
mod splits;

#[cfg(test)]
mod tests;
