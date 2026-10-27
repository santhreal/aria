//! Re-creation of a closed standard frame when a panel opens into it.
//!
//! A standard frame closes when its last panel closes or floats. Opening a
//! panel whose [`DefaultFrame`] frame closed re-creates that frame where the
//! shipped layouts put it, so the panel returns to its column instead of
//! becoming a tab of the largest frame, which is the Timeline's.

use super::{
    Axis, DefaultFrame, DockTree, Frame, FrameId, Node, PanelId, Zone, node_at_path_mut,
    node_has_frame, split_frame_in_node,
};

impl DockTree {
    /// Re-create the frame `preferred.frame()` in `root` holding `panel`, focus
    /// it, and return its id.
    ///
    /// - `Left` becomes the first and `Right` the last child of the outermost
    ///   row holding [`FrameId::CENTER`], at the Editing layout's share of that
    ///   row ([`DockTree::EDITING_LEFT_PX`] and [`DockTree::EDITING_RIGHT_PX`]
    ///   of [`DockTree::DEFAULT_WINDOW_WIDTH`]). With no row around the centre
    ///   frame, the centre frame splits.
    /// - `Bottom` splits [`FrameId::METERS`] with the new frame on its left at
    ///   the Editing lower-row share, or adds a lower row under the whole root
    ///   at [`DockTree::EDITING_TIMELINE_SHARE`] when Audio Meters is not docked.
    ///
    /// Returns `None` for `Center`, and for `Left` or `Right` when the centre
    /// frame is not docked; the caller then opens `panel` in the largest frame.
    /// The caller has checked that no frame holds `preferred.frame()`.
    pub(super) fn restore_frame(
        &mut self,
        panel: PanelId,
        preferred: DefaultFrame,
    ) -> Option<FrameId> {
        let id = preferred.frame();
        let width = Self::DEFAULT_WINDOW_WIDTH;
        match preferred {
            DefaultFrame::Center => return None,
            DefaultFrame::Left => {
                let (left, _, _) = Self::upper_row_weights_for_width(width, true);
                self.insert_beside_center(id, panel, Zone::Left, left)?;
            }
            DefaultFrame::Right => {
                let (_, _, right) = Self::upper_row_weights_for_width(width, true);
                self.insert_beside_center(id, panel, Zone::Right, right)?;
            }
            DefaultFrame::Bottom => {
                let (timeline, _) = Self::lower_row_weights_for_width(width);
                let meters = FrameId::METERS;
                if !split_frame_in_node(&mut self.root, meters, id, panel, Zone::Left, timeline) {
                    self.wrap_root_below(Frame::new(id, vec![panel]));
                }
            }
        }
        self.focused = id;
        self.normalize();
        Some(id)
    }

    /// Open Audio Meters as a tab of [`FrameId::METERS`], re-creating that
    /// frame right of the Timeline frame at the Editing lower-row share, or as
    /// a lower row under the whole root when the Timeline frame is not docked.
    pub(super) fn open_meters(&mut self, panel: PanelId) -> FrameId {
        let id = FrameId::METERS;
        if self.frame(id).is_some() {
            self.add_tab(id, panel);
            return id;
        }
        let (_, meters) = Self::lower_row_weights_for_width(Self::DEFAULT_WINDOW_WIDTH);
        let timeline = FrameId::BOTTOM;
        if !split_frame_in_node(&mut self.root, timeline, id, panel, Zone::Right, meters) {
            self.wrap_root_below(Frame::new(id, vec![panel]));
        }
        self.focused = id;
        self.normalize();
        id
    }

    /// Add a frame `id` holding `panel` at the `zone` edge of the outermost row
    /// holding the centre frame, taking `share` of that row. Returns `None`
    /// when the centre frame is not docked.
    fn insert_beside_center(
        &mut self,
        id: FrameId,
        panel: PanelId,
        zone: Zone,
        share: f32,
    ) -> Option<()> {
        let row = outer_row_path(&self.root, FrameId::CENTER)
            .and_then(|path| node_at_path_mut(&mut self.root, &path));
        let Some(Node::Split { children, .. }) = row else {
            return split_frame_in_node(&mut self.root, FrameId::CENTER, id, panel, zone, share)
                .then_some(());
        };
        let total: f32 = children.iter().map(|(w, _)| *w).sum();
        let added = (
            total * share / (1.0 - share),
            Node::Frame(Frame::new(id, vec![panel])),
        );
        if zone == Zone::Left {
            children.insert(0, added);
        } else {
            children.push(added);
        }
        Some(())
    }

    /// Put `frame` in a lower row under the whole root.
    fn wrap_root_below(&mut self, frame: Frame) {
        let upper = std::mem::replace(
            &mut self.root,
            Node::Split {
                axis: Axis::Column,
                children: Vec::new(),
            },
        );
        self.root = Node::Split {
            axis: Axis::Column,
            children: vec![
                (1.0 - Self::EDITING_TIMELINE_SHARE, upper),
                (Self::EDITING_TIMELINE_SHARE, Node::Frame(frame)),
            ],
        };
    }
}

/// Child path of the outermost [`Axis::Row`] split on the way from `root` to
/// the frame `anchor`, or `None` when `root` does not hold `anchor` or no row
/// split holds it.
fn outer_row_path(root: &Node, anchor: FrameId) -> Option<Vec<usize>> {
    if !node_has_frame(root, anchor) {
        return None;
    }
    let mut path = Vec::new();
    let mut node = root;
    while let Node::Split { axis, children } = node {
        if *axis == Axis::Row {
            return Some(path);
        }
        let pos = children
            .iter()
            .position(|(_, c)| node_has_frame(c, anchor))?;
        path.push(pos);
        node = &children[pos].1;
    }
    None
}

#[cfg(test)]
#[path = "restore_tests.rs"]
mod tests;
