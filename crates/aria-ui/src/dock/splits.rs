//! Split weights, splitter drags, and the maximized frame.

use super::nodes::{
    find_split_weights_by_signature, first_frame_id, node_at_path, node_at_path_mut, node_has_frame,
};
use super::{Axis, DockTree, FrameId, MIN_WEIGHT, Node};

impl DockTree {
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

    /// Give every split the weights of the split in `shipped` that
    /// [`Self::restore_split_from`] matches it with: one on the same [`Axis`]
    /// whose children start with the same [`FrameId`]s. A split `shipped` has
    /// no match for keeps its weights.
    pub fn adopt_weights(&mut self, shipped: &DockTree) {
        adopt_weights_in(&mut self.root, &shipped.root);
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
}

fn adopt_weights_in(node: &mut Node, shipped: &Node) {
    let Node::Split { axis, children } = node else {
        return;
    };
    let signature: Vec<Option<FrameId>> = children.iter().map(|(_, c)| first_frame_id(c)).collect();
    if let Some(weights) = find_split_weights_by_signature(shipped, *axis, &signature) {
        for ((weight, _), next) in children.iter_mut().zip(weights) {
            *weight = next;
        }
    }
    for (_, child) in children.iter_mut() {
        adopt_weights_in(child, shipped);
    }
}
