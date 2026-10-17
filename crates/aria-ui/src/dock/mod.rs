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

use std::collections::BTreeSet;
use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::interned::intern;

/// Schema version of a persisted [`DockTree`]. A tree with any other version is
/// rejected on load so stale on-disk shapes fall back to the shipped layout.
pub const DOCK_VERSION: u32 = 2;

/// Lowest weight a split child may hold.
pub const MIN_WEIGHT: f32 = 0.02;

/// Lowest width or height a floating frame may hold, in logical pixels.
pub const MIN_FLOAT_W: f32 = 160.0;
pub const MIN_FLOAT_H: f32 = 80.0;

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
    /// Default window width in logical pixels (`docs/spec/layout.md`).
    pub const DEFAULT_WINDOW_WIDTH: f32 = 1440.0;
    /// Default width of the left frame in the Editing workspace (`320 px`).
    pub const EDITING_LEFT_PX: f32 = 320.0;
    /// Default width of the right column in the Editing workspace (`300 px`).
    pub const EDITING_RIGHT_PX: f32 = 300.0;
    /// Default width of the Audio Meters frame in the Editing workspace (`64 px`).
    pub const EDITING_METERS_PX: f32 = 64.0;

    /// The shipped Editing layout (`docs/spec/layout.md` window diagram):
    /// - Column split: upper area `55 %`, lower area `45 %`.
    /// - Upper row: left frame `320 px` (`Project | Effect Controls`), centre
    ///   frame (`Viewer`, with `Audio Track Mixer` and `Assistant` tabs), and
    ///   right column `300 px` (`Info | Effects & Presets | Character` as one
    ///   tabbed frame per `docs/spec/layout.md`: "A frame holds one or more
    ///   panels. Each panel has a tab.").
    /// - Lower row: `Timeline` on the left, `Audio Meters` at `64 px` on the
    ///   right.
    pub fn editing() -> Self {
        Self::editing_for_width(Self::DEFAULT_WINDOW_WIDTH)
    }

    /// Build the shipped Editing layout with row weights derived from the
    /// spec's pixel widths (`320 px` left, `300 px` right, `64 px` audio
    /// meters) at `window_w`.
    pub fn editing_for_width(window_w: f32) -> Self {
        let (left_w, center_w, right_w) = Self::upper_row_weights_for_width(window_w, true);
        let (timeline_w, meters_w) = Self::lower_row_weights_for_width(window_w);
        Self {
            version: DOCK_VERSION,
            root: Node::Split {
                axis: Axis::Column,
                children: vec![
                    (
                        0.55,
                        Node::Split {
                            axis: Axis::Row,
                            children: vec![
                                (
                                    left_w,
                                    Node::Frame(Frame::new(
                                        FrameId::LEFT,
                                        vec![PANEL_PROJECT, PANEL_INSPECTOR],
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
                                        vec![PANEL_INFO, PANEL_EFFECTS, PANEL_CHARACTER],
                                        1,
                                    )),
                                ),
                            ],
                        },
                    ),
                    (
                        0.45,
                        Node::Split {
                            axis: Axis::Row,
                            children: vec![
                                (
                                    timeline_w,
                                    Node::Frame(Frame::new(FrameId::BOTTOM, vec![PANEL_TIMELINE])),
                                ),
                                (
                                    meters_w,
                                    Node::Frame(Frame::new(
                                        FrameId::METERS,
                                        vec![PANEL_AUDIO_METERS],
                                    )),
                                ),
                            ],
                        },
                    ),
                ],
            },
            floating: Vec::new(),
            focused: FrameId::CENTER,
            maximized: None,
        }
    }

    /// Compute normalized weights `(left, center, right)` for an upper row with
    /// a `320 px` left frame and optional `300 px` right frame at `window_w`.
    pub fn upper_row_weights_for_width(window_w: f32, has_right: bool) -> (f32, f32, f32) {
        let avail = window_w.max(640.0);
        let left_px = Self::EDITING_LEFT_PX.clamp(160.0, (avail - 320.0).max(160.0));
        if has_right {
            let right_px =
                Self::EDITING_RIGHT_PX.clamp(160.0, (avail - left_px - 320.0).max(160.0));
            let center_px = (avail - left_px - right_px).max(320.0);
            let sum = left_px + center_px + right_px;
            (left_px / sum, center_px / sum, right_px / sum)
        } else {
            let center_px = (avail - left_px).max(320.0);
            let sum = left_px + center_px;
            (left_px / sum, center_px / sum, 0.0)
        }
    }

    /// Compute normalized weights `(timeline, meters)` for the lower row with
    /// a `64 px` Audio Meters frame on the right at `window_w`.
    pub fn lower_row_weights_for_width(window_w: f32) -> (f32, f32) {
        let avail = window_w.max(464.0);
        let meters_px = Self::EDITING_METERS_PX.clamp(48.0, (avail - 400.0).max(48.0));
        let timeline_px = (avail - meters_px).max(400.0);
        let sum = timeline_px + meters_px;
        (timeline_px / sum, meters_px / sum)
    }

    /// Scale the standard Editing/Animation pixel-width frames (`LEFT` = 320 px,
    /// `RIGHT` = 300 px, `METERS` = 64 px) to match `window_w`.
    pub fn apply_spec_px_for_width(&mut self, window_w: f32) {
        if window_w <= 0.0 {
            return;
        }
        let (left_w, center_w, right_w) = Self::upper_row_weights_for_width(window_w, true);
        let (timeline_w, meters_w) = Self::lower_row_weights_for_width(window_w);
        if let Node::Split {
            axis: Axis::Column,
            children,
        } = &mut self.root
        {
            for (_, row_node) in children.iter_mut() {
                if let Node::Split {
                    axis: Axis::Row,
                    children: row_children,
                } = row_node
                {
                    if row_children.len() == 3
                        && matches!(&row_children[0].1, Node::Frame(f) if f.id == FrameId::LEFT)
                        && matches!(&row_children[1].1, Node::Frame(f) if f.id == FrameId::CENTER)
                        && matches!(&row_children[2].1, Node::Frame(f) if f.id == FrameId::RIGHT)
                    {
                        row_children[0].0 = left_w;
                        row_children[1].0 = center_w;
                        row_children[2].0 = right_w;
                    } else if row_children.len() == 2
                        && matches!(&row_children[0].1, Node::Frame(f) if f.id == FrameId::BOTTOM)
                        && matches!(&row_children[1].1, Node::Frame(f) if f.id == FrameId::METERS)
                    {
                        row_children[0].0 = timeline_w;
                        row_children[1].0 = meters_w;
                    }
                }
            }
        }
    }

    /// Parse a JSON [`DockTree`], rejecting any mismatched version or broken
    /// invariant.
    pub fn from_json(text: &str) -> Result<Self, String> {
        serde_json::from_str::<Self>(text).map_err(|err| err.to_string())
    }

    /// Parse a JSON [`DockTree`], falling back to [`Self::editing`] when the
    /// text fails to parse or carries another version.
    pub fn parse_or_editing(text: &str) -> Self {
        Self::from_json(text).unwrap_or_else(|_| Self::editing())
    }

    /// Serialize this tree to pretty JSON.
    pub fn to_json(&self) -> Result<String, String> {
        serde_json::to_string_pretty(self).map_err(|err| err.to_string())
    }

    /// Verify every structural invariant of the tree.
    pub fn check_invariants(&self) -> Result<(), String> {
        if self.version != DOCK_VERSION {
            return Err(format!(
                "dock version {} does not match DOCK_VERSION ({DOCK_VERSION})",
                self.version
            ));
        }
        let mut seen_panels = BTreeSet::new();
        let mut seen_frames = BTreeSet::new();
        let mut root_frames = BTreeSet::new();
        check_node(
            &self.root,
            None,
            &mut seen_panels,
            &mut seen_frames,
            &mut root_frames,
        )?;
        if let Some(max_id) = self.maximized
            && !root_frames.contains(&max_id)
        {
            return Err(format!(
                "maximized frame {max_id} is not present in the root tree"
            ));
        }
        for fl in &self.floating {
            if !fl.w.is_finite() || !fl.h.is_finite() || fl.w < MIN_FLOAT_W || fl.h < MIN_FLOAT_H {
                return Err(format!(
                    "floating frame {} has invalid size {}x{}",
                    fl.frame.id, fl.w, fl.h
                ));
            }
            if !fl.x.is_finite() || !fl.y.is_finite() {
                return Err(format!(
                    "floating frame {} has non-finite origin ({}, {})",
                    fl.frame.id, fl.x, fl.y
                ));
            }
            check_frame(&fl.frame, &mut seen_panels, &mut seen_frames)?;
        }
        if !seen_frames.contains(&self.focused) {
            return Err(format!(
                "focused frame {} is not present in the tree",
                self.focused
            ));
        }
        Ok(())
    }

    /// Repair any broken invariant in place so programmatic or migrated edits
    /// always leave a valid [`DockTree`].
    pub fn normalize(&mut self) {
        self.version = DOCK_VERSION;
        let mut seen_panels = BTreeSet::new();
        let mut seen_frames = BTreeSet::new();
        let mut next_raw = max_frame_raw(&self.root, &self.floating)
            .saturating_add(1)
            .max(10);

        let old_root = std::mem::replace(
            &mut self.root,
            Node::Frame(Frame::new(FrameId::CENTER, vec![PANEL_VIEWER])),
        );
        self.root = normalize_node(old_root, &mut seen_panels, &mut seen_frames, &mut next_raw)
            .unwrap_or_else(|| {
                seen_panels.insert(PANEL_VIEWER);
                let id = if seen_frames.insert(FrameId::CENTER) {
                    FrameId::CENTER
                } else {
                    let fresh = FrameId(next_raw);
                    next_raw += 1;
                    seen_frames.insert(fresh);
                    fresh
                };
                Node::Frame(Frame::new(id, vec![PANEL_VIEWER]))
            });

        let mut root_frames = BTreeSet::new();
        collect_frame_ids(&self.root, &mut root_frames);
        if let Some(max_id) = self.maximized
            && !root_frames.contains(&max_id)
        {
            self.maximized = None;
        }

        let old_floating = std::mem::take(&mut self.floating);
        for mut fl in old_floating {
            let active_panel = fl.frame.tabs.get(fl.frame.active).copied();
            fl.frame
                .tabs
                .retain(|panel| !panel.trim().is_empty() && seen_panels.insert(*panel));
            if fl.frame.tabs.is_empty() {
                continue;
            }
            if !seen_frames.insert(fl.frame.id) {
                fl.frame.id = FrameId(next_raw);
                next_raw += 1;
                seen_frames.insert(fl.frame.id);
            }
            if let Some(active) = active_panel
                && let Some(pos) = fl.frame.tabs.iter().position(|&p| p == active)
            {
                fl.frame.active = pos;
            } else {
                fl.frame.active = fl.frame.active.min(fl.frame.tabs.len() - 1);
            }
            if !fl.x.is_finite() {
                fl.x = 80.0;
            }
            if !fl.y.is_finite() {
                fl.y = 80.0;
            }
            if !fl.w.is_finite() || fl.w < MIN_FLOAT_W {
                fl.w = 480.0;
            }
            if !fl.h.is_finite() || fl.h < MIN_FLOAT_H {
                fl.h = 320.0;
            }
            self.floating.push(fl);
        }

        if !seen_frames.contains(&self.focused) {
            self.focused = self
                .frames()
                .first()
                .map(|f| f.id)
                .unwrap_or(FrameId::CENTER);
        }
    }

    /// Allocate a fresh [`FrameId`] not used anywhere in this tree.
    pub fn fresh_frame_id(&self) -> FrameId {
        FrameId(
            max_frame_raw(&self.root, &self.floating)
                .saturating_add(1)
                .max(10),
        )
    }

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
            if frame.tabs.iter().any(|tab| *tab == panel) {
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

    /// Open `panel` if closed (into `default_frame_for(panel)` when that frame
    /// exists in `root`, or into the largest frame in `root` otherwise), and
    /// make it the active focused tab.
    pub fn open(&mut self, panel: PanelId) -> FrameId {
        if panel == PANEL_AUDIO_METERS
            && self.find(panel).is_none()
            && self.frame(FrameId::METERS).is_some()
        {
            self.add_tab(FrameId::METERS, panel);
            return FrameId::METERS;
        }
        self.open_in(panel, default_frame_for(panel))
    }

    /// Open `panel` using an explicit [`DefaultFrame`] preference.
    pub fn open_in(&mut self, panel: PanelId, preferred: DefaultFrame) -> FrameId {
        let interned = intern(panel);
        if let Some(existing) = self.find(interned) {
            self.activate(interned);
            return existing;
        }
        let target_id = if find_frame_ref(&self.root, preferred.frame()).is_some() {
            preferred.frame()
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
        if !split_frame_in_node(&mut self.root, target, fresh_id, interned, zone) {
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

    /// Update the child weights of the [`Node::Split`] addressed by `split_path`
    /// (child indices from `root`).
    pub fn set_weights(&mut self, split_path: &[usize], weights: &[f32]) -> bool {
        let Some(node) = node_at_path_mut(&mut self.root, split_path) else {
            return false;
        };
        let Node::Split { children, .. } = node else {
            return false;
        };
        if children.len() != weights.len() || weights.is_empty() {
            return false;
        }
        if weights.iter().any(|w| !w.is_finite()) {
            return false;
        }
        let clamped: Vec<f32> = weights.iter().map(|w| w.max(MIN_WEIGHT)).collect();
        let sum: f32 = clamped.iter().sum();
        for ((weight, _), next) in children.iter_mut().zip(clamped) {
            *weight = if sum > 0.0 {
                (next / sum).max(MIN_WEIGHT)
            } else {
                1.0 / weights.len() as f32
            };
        }
        true
    }

    /// Adjust the splitter between `children[index]` and `children[index + 1]`
    /// at `split_path` by `delta_weight`.
    pub fn drag_split_at(&mut self, split_path: &[usize], index: usize, delta_weight: f32) -> bool {
        let Some(mut weights) = self.split_weights(split_path) else {
            return false;
        };
        if index + 1 >= weights.len() {
            return false;
        }
        let pair = weights[index] + weights[index + 1];
        let min_w = MIN_WEIGHT.min(pair * 0.45);
        let next_a = (weights[index] + delta_weight).clamp(min_w, (pair - min_w).max(min_w));
        let next_b = (pair - next_a).max(min_w);
        weights[index] = next_a;
        weights[index + 1] = next_b;
        self.set_weights(split_path, &weights)
    }

    /// Read the weights of the [`Node::Split`] at `split_path`.
    pub fn split_weights(&self, split_path: &[usize]) -> Option<Vec<f32>> {
        let node = node_at_path(&self.root, split_path)?;
        match node {
            Node::Split { children, .. } => Some(children.iter().map(|(w, _)| *w).collect()),
            Node::Frame(_) => None,
        }
    }

    /// Fraction of the root dock area's size along `axis` occupied by the split
    /// container at `split_path`, computed from ancestor split weights along
    /// `axis` so nested splitters convert pixel travel against their own
    /// container span rather than the full window viewport.
    pub fn split_span_fraction(&self, split_path: &[usize], axis: Axis) -> f32 {
        let mut frac_w = 1.0_f32;
        let mut frac_h = 1.0_f32;
        let mut cur = &self.root;
        for &step in split_path {
            let Node::Split {
                axis: step_axis,
                children,
            } = cur
            else {
                break;
            };
            let sum: f32 = children.iter().map(|(w, _)| *w).sum::<f32>().max(1e-5);
            let Some((w, next)) = children.get(step) else {
                break;
            };
            let share = (*w / sum).clamp(0.02, 1.0);
            match step_axis {
                Axis::Row => frac_w *= share,
                Axis::Column => frac_h *= share,
            }
            cur = next;
        }
        match axis {
            Axis::Row => frac_w.clamp(0.02, 1.0),
            Axis::Column => frac_h.clamp(0.02, 1.0),
        }
    }

    /// Restore the child weights of the [`Node::Split`] at `split_path` from
    /// `saved` (the workspace's saved [`DockTree`]).
    ///
    /// Looks up `split_path` in `saved` first; if the tree topology shifted,
    /// matches a split on the same [`Axis`] in `saved` whose children share the
    /// same leading [`FrameId`]s. Falls back to equal weights (`1.0 / len`) only
    /// when `saved` has no matching split (such as an ad-hoc split created by
    /// dragging a tab to a drop zone).
    pub fn restore_split_from(&mut self, split_path: &[usize], saved: &DockTree) -> bool {
        let Some(Node::Split { axis, children }) = node_at_path(&self.root, split_path) else {
            return false;
        };
        let axis = *axis;
        let len = children.len();
        if let Some(saved_weights) = saved.split_weights(split_path)
            && saved_weights.len() == len
        {
            return self.set_weights(split_path, &saved_weights);
        }
        let signature: Vec<Option<FrameId>> =
            children.iter().map(|(_, c)| first_frame_id(c)).collect();
        if let Some(matched) = find_split_weights_by_signature(&saved.root, axis, &signature) {
            return self.set_weights(split_path, &matched);
        }
        let equal = vec![1.0 / len as f32; len];
        self.set_weights(split_path, &equal)
    }

    /// Toggle whether `frame` is maximized to fill the editor area.
    pub fn toggle_maximize(&mut self, frame: FrameId) -> bool {
        if self.maximized == Some(frame) {
            self.maximized = None;
            return true;
        }
        if node_has_frame(&self.root, frame) {
            self.maximized = Some(frame);
            return true;
        }
        false
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

    /// Advance `focused` by `delta` steps through frames in reading order
    /// (`root` pre-order, then `floating`), wrapping around.
    pub fn cycle_focus(&mut self, delta: isize) -> FrameId {
        let ids = self.frame_ids();
        if ids.is_empty() {
            return self.focused;
        }
        let len = ids.len() as isize;
        let at = ids.iter().position(|&id| id == self.focused).unwrap_or(0) as isize;
        let next = ((at + delta).rem_euclid(len)) as usize;
        self.focused = ids[next];
        self.focused
    }

    /// Advance `focused` to the next frame in reading order (`Ctrl+Tab`).
    pub fn next_frame(&mut self) -> FrameId {
        self.cycle_focus(1)
    }

    /// Move `focused` to the previous frame in reading order (`Ctrl+Shift+Tab`).
    pub fn prev_frame(&mut self) -> FrameId {
        self.cycle_focus(-1)
    }

    /// Next frame after `current` in reading order (wrapping around).
    pub fn next_frame_from(&self, current: FrameId) -> Option<FrameId> {
        let ids = self.frame_ids();
        if ids.is_empty() {
            return None;
        }
        let at = ids.iter().position(|&id| id == current).unwrap_or(0);
        Some(ids[(at + 1) % ids.len()])
    }

    /// Previous frame before `current` in reading order (wrapping around).
    pub fn prev_frame_from(&self, current: FrameId) -> Option<FrameId> {
        let ids = self.frame_ids();
        if ids.is_empty() {
            return None;
        }
        let at = ids.iter().position(|&id| id == current).unwrap_or(0);
        Some(ids[(at + ids.len() - 1) % ids.len()])
    }

    /// The frame in `root` with the largest area share.
    pub fn largest_frame(&self) -> FrameId {
        let mut best = (FrameId::CENTER, -1.0f32);
        largest_in_node(&self.root, 1.0, &mut best);
        best.0
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

fn check_frame(
    frame: &Frame,
    seen_panels: &mut BTreeSet<PanelId>,
    seen_frames: &mut BTreeSet<FrameId>,
) -> Result<(), String> {
    if !seen_frames.insert(frame.id) {
        return Err(format!("duplicate frame id {}", frame.id));
    }
    if frame.tabs.is_empty() {
        return Err(format!("frame {} has no tabs", frame.id));
    }
    if frame.active >= frame.tabs.len() {
        return Err(format!(
            "frame {} active index {} out of bounds for {} tabs",
            frame.id,
            frame.active,
            frame.tabs.len()
        ));
    }
    for &panel in &frame.tabs {
        if panel.trim().is_empty() {
            return Err(format!("frame {} holds an empty panel id", frame.id));
        }
        if !seen_panels.insert(panel) {
            return Err(format!("panel {panel} appears more than once in the tree"));
        }
    }
    Ok(())
}

fn check_node(
    node: &Node,
    parent_axis: Option<Axis>,
    seen_panels: &mut BTreeSet<PanelId>,
    seen_frames: &mut BTreeSet<FrameId>,
    root_frames: &mut BTreeSet<FrameId>,
) -> Result<(), String> {
    match node {
        Node::Frame(frame) => {
            check_frame(frame, seen_panels, seen_frames)?;
            root_frames.insert(frame.id);
            Ok(())
        }
        Node::Split { axis, children } => {
            if children.len() < 2 {
                return Err(format!(
                    "split on {axis:?} has {} child(ren); splits require at least 2",
                    children.len()
                ));
            }
            if parent_axis == Some(*axis) {
                return Err(format!(
                    "split on {axis:?} is nested directly inside another {axis:?} split"
                ));
            }
            for (weight, child) in children {
                if !weight.is_finite() || *weight <= 0.0 {
                    return Err(format!(
                        "split on {axis:?} has non-positive weight {weight}"
                    ));
                }
                check_node(child, Some(*axis), seen_panels, seen_frames, root_frames)?;
            }
            Ok(())
        }
    }
}

fn normalize_node(
    node: Node,
    seen_panels: &mut BTreeSet<PanelId>,
    seen_frames: &mut BTreeSet<FrameId>,
    next_raw: &mut u32,
) -> Option<Node> {
    match node {
        Node::Frame(mut frame) => {
            let active_panel = frame.tabs.get(frame.active).copied();
            frame
                .tabs
                .retain(|panel| !panel.trim().is_empty() && seen_panels.insert(*panel));
            if frame.tabs.is_empty() {
                return None;
            }
            if !seen_frames.insert(frame.id) {
                frame.id = FrameId(*next_raw);
                *next_raw += 1;
                seen_frames.insert(frame.id);
            }
            if let Some(active) = active_panel
                && let Some(pos) = frame.tabs.iter().position(|&p| p == active)
            {
                frame.active = pos;
            } else {
                frame.active = frame.active.min(frame.tabs.len() - 1);
            }
            Some(Node::Frame(frame))
        }
        Node::Split { axis, children } => {
            let mut flat: Vec<(f32, Node)> = Vec::new();
            for (weight, child) in children {
                let w = if weight.is_finite() && weight > 0.0 {
                    weight.max(MIN_WEIGHT)
                } else {
                    1.0
                };
                let Some(norm_child) = normalize_node(child, seen_panels, seen_frames, next_raw)
                else {
                    continue;
                };
                match norm_child {
                    Node::Split {
                        axis: child_axis,
                        children: grand,
                    } if child_axis == axis => {
                        let grand_sum: f32 = grand.iter().map(|(gw, _)| *gw).sum();
                        let scale = if grand_sum > 0.0 { w / grand_sum } else { w };
                        for (gw, gnode) in grand {
                            flat.push(((gw * scale).max(MIN_WEIGHT), gnode));
                        }
                    }
                    other => flat.push((w, other)),
                }
            }
            match flat.len() {
                0 => None,
                1 => Some(flat.pop().expect("one child").1),
                _ => {
                    let sum: f32 = flat.iter().map(|(w, _)| *w).sum();
                    if sum > 0.0 {
                        for (w, _) in &mut flat {
                            *w = (*w / sum).max(MIN_WEIGHT);
                        }
                    }
                    Some(Node::Split {
                        axis,
                        children: flat,
                    })
                }
            }
        }
    }
}

fn max_frame_raw(root: &Node, floating: &[Floating]) -> u32 {
    let mut max = 0u32;
    max_in_node(root, &mut max);
    for fl in floating {
        max = max.max(fl.frame.id.0);
    }
    max
}

fn max_in_node(node: &Node, max: &mut u32) {
    match node {
        Node::Frame(frame) => *max = (*max).max(frame.id.0),
        Node::Split { children, .. } => {
            for (_, child) in children {
                max_in_node(child, max);
            }
        }
    }
}

fn node_has_frame(node: &Node, target: FrameId) -> bool {
    find_frame_ref(node, target).is_some()
}

fn find_frame_ref(node: &Node, target: FrameId) -> Option<&Frame> {
    match node {
        Node::Frame(frame) => (frame.id == target).then_some(frame),
        Node::Split { children, .. } => children
            .iter()
            .find_map(|(_, child)| find_frame_ref(child, target)),
    }
}

fn find_frame_mut(node: &mut Node, target: FrameId) -> Option<&mut Frame> {
    match node {
        Node::Frame(frame) => (frame.id == target).then_some(frame),
        Node::Split { children, .. } => children
            .iter_mut()
            .find_map(|(_, child)| find_frame_mut(child, target)),
    }
}

fn root_panel_count(node: &Node) -> usize {
    match node {
        Node::Frame(frame) => frame.tabs.len(),
        Node::Split { children, .. } => children
            .iter()
            .map(|(_, child)| root_panel_count(child))
            .sum(),
    }
}

fn remove_from_node(node: &mut Node, panel: PanelId) -> bool {
    match node {
        Node::Frame(frame) => {
            if let Some(pos) = frame.tabs.iter().position(|&p| p == panel) {
                frame.tabs.remove(pos);
                if !frame.tabs.is_empty() {
                    if pos <= frame.active && frame.active > 0 {
                        frame.active -= 1;
                    }
                    frame.active = frame.active.min(frame.tabs.len() - 1);
                }
                return true;
            }
            false
        }
        Node::Split { children, .. } => {
            for (_, child) in children {
                if remove_from_node(child, panel) {
                    return true;
                }
            }
            false
        }
    }
}

fn split_frame_in_node(
    node: &mut Node,
    target: FrameId,
    fresh: FrameId,
    panel: PanelId,
    zone: Zone,
) -> bool {
    match node {
        Node::Frame(frame) if frame.id == target => {
            let existing = Node::Frame(frame.clone());
            let added = Node::Frame(Frame::new(fresh, vec![panel]));
            let (axis, children) = match zone {
                Zone::Left => (Axis::Row, vec![(0.5, added), (0.5, existing)]),
                Zone::Right => (Axis::Row, vec![(0.5, existing), (0.5, added)]),
                Zone::Top => (Axis::Column, vec![(0.5, added), (0.5, existing)]),
                Zone::Bottom => (Axis::Column, vec![(0.5, existing), (0.5, added)]),
                Zone::Center => return false,
            };
            *node = Node::Split { axis, children };
            true
        }
        Node::Frame(_) => false,
        Node::Split { children, .. } => {
            for (_, child) in children {
                if split_frame_in_node(child, target, fresh, panel, zone) {
                    return true;
                }
            }
            false
        }
    }
}

fn node_at_path<'a>(node: &'a Node, path: &[usize]) -> Option<&'a Node> {
    let Some((&head, tail)) = path.split_first() else {
        return Some(node);
    };
    match node {
        Node::Split { children, .. } => {
            let (_, child) = children.get(head)?;
            node_at_path(child, tail)
        }
        Node::Frame(_) => None,
    }
}

fn node_at_path_mut<'a>(node: &'a mut Node, path: &[usize]) -> Option<&'a mut Node> {
    let Some((&head, tail)) = path.split_first() else {
        return Some(node);
    };
    match node {
        Node::Split { children, .. } => {
            let (_, child) = children.get_mut(head)?;
            node_at_path_mut(child, tail)
        }
        Node::Frame(_) => None,
    }
}

fn first_frame_id(node: &Node) -> Option<FrameId> {
    match node {
        Node::Frame(frame) => Some(frame.id),
        Node::Split { children, .. } => children.first().and_then(|(_, c)| first_frame_id(c)),
    }
}

fn find_split_weights_by_signature(
    node: &Node,
    want_axis: Axis,
    signature: &[Option<FrameId>],
) -> Option<Vec<f32>> {
    let Node::Split { axis, children } = node else {
        return None;
    };
    if *axis == want_axis && children.len() == signature.len() {
        let matches = children
            .iter()
            .zip(signature.iter())
            .all(|((_, c), want)| want.is_some() && first_frame_id(c) == *want);
        if matches {
            return Some(children.iter().map(|(w, _)| *w).collect());
        }
    }
    for (_, child) in children {
        if let Some(found) = find_split_weights_by_signature(child, want_axis, signature) {
            return Some(found);
        }
    }
    None
}

fn collect_frames<'a>(node: &'a Node, out: &mut Vec<&'a Frame>) {
    match node {
        Node::Frame(frame) => out.push(frame),
        Node::Split { children, .. } => {
            for (_, child) in children {
                collect_frames(child, out);
            }
        }
    }
}
fn collect_frame_ids(node: &Node, out: &mut BTreeSet<FrameId>) {
    match node {
        Node::Frame(frame) => {
            out.insert(frame.id);
        }
        Node::Split { children, .. } => {
            for (_, child) in children {
                collect_frame_ids(child, out);
            }
        }
    }
}

fn find_in_node(node: &Node, panel: PanelId) -> Option<FrameId> {
    match node {
        Node::Frame(frame) => frame.tabs.contains(&panel).then_some(frame.id),
        Node::Split { children, .. } => children
            .iter()
            .find_map(|(_, child)| find_in_node(child, panel)),
    }
}

fn activate_in_node(node: &mut Node, panel: PanelId) -> Option<FrameId> {
    match node {
        Node::Frame(frame) => {
            if let Some(idx) = frame.tabs.iter().position(|&p| p == panel) {
                frame.active = idx;
                return Some(frame.id);
            }
            None
        }
        Node::Split { children, .. } => children
            .iter_mut()
            .find_map(|(_, child)| activate_in_node(child, panel)),
    }
}

fn largest_in_node(node: &Node, share: f32, best: &mut (FrameId, f32)) {
    match node {
        Node::Frame(frame) => {
            if share > best.1 {
                *best = (frame.id, share);
            }
        }
        Node::Split { children, .. } => {
            let total: f32 = children.iter().map(|(w, _)| *w).sum();
            for (w, child) in children {
                let frac = if total > 0.0 { *w / total } else { 0.0 };
                largest_in_node(child, share * frac, best);
            }
        }
    }
}

fn retain_in_node(node: &mut Node, keep: &mut impl FnMut(PanelId) -> bool) {
    match node {
        Node::Frame(frame) => {
            let active = frame.active_panel();
            frame.tabs.retain(|&p| keep(p));
            if let Some(pos) = frame.tabs.iter().position(|&p| p == active) {
                frame.active = pos;
            } else {
                frame.active = 0;
            }
        }
        Node::Split { children, .. } => {
            for (_, child) in children {
                retain_in_node(child, keep);
            }
        }
    }
}

#[cfg(test)]
mod tests;
