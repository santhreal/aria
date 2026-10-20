//! The shipped Editing layout and the split weights that fit it to a window
//! width.

use super::{
    Axis, DOCK_VERSION, DockTree, Frame, FrameId, Node, PANEL_AUDIO_METERS, PANEL_EFFECTS,
    PANEL_INSPECTOR, PANEL_PROJECT, PANEL_TIMELINE, PANEL_VIEWER,
};

impl DockTree {
    /// Default window width in logical pixels (`docs/spec/layout.md`).
    pub const DEFAULT_WINDOW_WIDTH: f32 = 1440.0;
    /// Default width of the left frame in the Editing workspace (`320 px`).
    pub const EDITING_LEFT_PX: f32 = 320.0;
    /// Default width of the right column in the Editing workspace (`300 px`).
    pub const EDITING_RIGHT_PX: f32 = 300.0;
    /// Default width of the Audio Meters frame in the Editing workspace (`64 px`).
    pub const EDITING_METERS_PX: f32 = 64.0;
    /// Share of the Editing column the lower row takes (`45 %`).
    pub const EDITING_TIMELINE_SHARE: f32 = 0.45;

    /// The shipped Editing layout (`docs/spec/layout.md` window diagram), six
    /// panels:
    /// - Column split: upper area `55 %`, lower area `45 %`.
    /// - Upper row: left frame `320 px` (`Project | Effect Controls`), centre
    ///   frame (`Viewer`), and right column `300 px` (`Effects & Presets`).
    /// - Lower row: `Timeline` on the left, `Audio Meters` at `64 px` on the
    ///   right.
    ///
    /// Every other panel opens from the Window menu into its
    /// [`default_frame_for`](super::default_frame_for) frame.
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
                        1.0 - Self::EDITING_TIMELINE_SHARE,
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
                                    Node::Frame(Frame::new(FrameId::CENTER, vec![PANEL_VIEWER])),
                                ),
                                (
                                    right_w,
                                    Node::Frame(Frame::new(FrameId::RIGHT, vec![PANEL_EFFECTS])),
                                ),
                            ],
                        },
                    ),
                    (
                        Self::EDITING_TIMELINE_SHARE,
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
}
