//! Walks and edits over a [`Node`](super::Node) tree, used by the
//! [`DockTree`](super::DockTree) operations.

use std::collections::BTreeSet;

use super::{Axis, Frame, FrameId, Node, PanelId, Zone};

pub(super) fn node_has_frame(node: &Node, target: FrameId) -> bool {
    find_frame_ref(node, target).is_some()
}

pub(super) fn find_frame_ref(node: &Node, target: FrameId) -> Option<&Frame> {
    match node {
        Node::Frame(frame) => (frame.id == target).then_some(frame),
        Node::Split { children, .. } => children
            .iter()
            .find_map(|(_, child)| find_frame_ref(child, target)),
    }
}

pub(super) fn find_frame_mut(node: &mut Node, target: FrameId) -> Option<&mut Frame> {
    match node {
        Node::Frame(frame) => (frame.id == target).then_some(frame),
        Node::Split { children, .. } => children
            .iter_mut()
            .find_map(|(_, child)| find_frame_mut(child, target)),
    }
}

pub(super) fn root_panel_count(node: &Node) -> usize {
    match node {
        Node::Frame(frame) => frame.tabs.len(),
        Node::Split { children, .. } => children
            .iter()
            .map(|(_, child)| root_panel_count(child))
            .sum(),
    }
}

pub(super) fn remove_from_node(node: &mut Node, panel: PanelId) -> bool {
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

/// Split the frame `target` in `node` by `zone`, adding a frame `fresh` holding
/// `panel` at weight `share`; the existing frame keeps `1 - share`.
pub(super) fn split_frame_in_node(
    node: &mut Node,
    target: FrameId,
    fresh: FrameId,
    panel: PanelId,
    zone: Zone,
    share: f32,
) -> bool {
    match node {
        Node::Frame(frame) if frame.id == target => {
            let existing = (1.0 - share, Node::Frame(frame.clone()));
            let added = (share, Node::Frame(Frame::new(fresh, vec![panel])));
            let (axis, children) = match zone {
                Zone::Left => (Axis::Row, vec![added, existing]),
                Zone::Right => (Axis::Row, vec![existing, added]),
                Zone::Top => (Axis::Column, vec![added, existing]),
                Zone::Bottom => (Axis::Column, vec![existing, added]),
                Zone::Center => return false,
            };
            *node = Node::Split { axis, children };
            true
        }
        Node::Frame(_) => false,
        Node::Split { children, .. } => {
            for (_, child) in children {
                if split_frame_in_node(child, target, fresh, panel, zone, share) {
                    return true;
                }
            }
            false
        }
    }
}

pub(super) fn node_at_path<'a>(node: &'a Node, path: &[usize]) -> Option<&'a Node> {
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

pub(super) fn node_at_path_mut<'a>(node: &'a mut Node, path: &[usize]) -> Option<&'a mut Node> {
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

pub(super) fn first_frame_id(node: &Node) -> Option<FrameId> {
    match node {
        Node::Frame(frame) => Some(frame.id),
        Node::Split { children, .. } => children.first().and_then(|(_, c)| first_frame_id(c)),
    }
}

pub(super) fn find_split_weights_by_signature(
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

pub(super) fn collect_frames<'a>(node: &'a Node, out: &mut Vec<&'a Frame>) {
    match node {
        Node::Frame(frame) => out.push(frame),
        Node::Split { children, .. } => {
            for (_, child) in children {
                collect_frames(child, out);
            }
        }
    }
}
pub(super) fn collect_frame_ids(node: &Node, out: &mut BTreeSet<FrameId>) {
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

pub(super) fn find_in_node(node: &Node, panel: PanelId) -> Option<FrameId> {
    match node {
        Node::Frame(frame) => frame.tabs.contains(&panel).then_some(frame.id),
        Node::Split { children, .. } => children
            .iter()
            .find_map(|(_, child)| find_in_node(child, panel)),
    }
}

pub(super) fn activate_in_node(node: &mut Node, panel: PanelId) -> Option<FrameId> {
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

pub(super) fn largest_in_node(node: &Node, share: f32, best: &mut (FrameId, f32)) {
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

pub(super) fn retain_in_node(node: &mut Node, keep: &mut impl FnMut(PanelId) -> bool) {
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
