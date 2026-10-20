//! Focus cycling through frames in reading order, and the largest frame.

use super::nodes::largest_in_node;
use super::{DockTree, FrameId};

impl DockTree {
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
}
