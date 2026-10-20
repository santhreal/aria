//! Parsing and serializing a [`DockTree`], and checking and repairing the tree
//! invariants listed in [`super`].

use std::collections::BTreeSet;

use super::nodes::collect_frame_ids;
use super::{
    Axis, DOCK_VERSION, DockTree, Floating, Frame, FrameId, MIN_FLOAT_H, MIN_FLOAT_W, MIN_WEIGHT,
    Node, PANEL_VIEWER, PanelId,
};

impl DockTree {
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
