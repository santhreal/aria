//! The shipped Editing layout and the split weights that fit it to a window
//! width.

use super::{
    Axis, DOCK_VERSION, DockTree, Frame, FrameId, Node, PANEL_AUDIO_METERS, PANEL_EFFECTS,
    PANEL_INSPECTOR, PANEL_PROJECT, PANEL_TIMELINE, PANEL_VIEWER,
};

impl DockTree {
    /// Default window width in logical pixels (`docs/spec/layout.md`).
    pub const DEFAULT_WINDOW_WIDTH: f32 = 1440.0;
    /// Default width of the left frame in the Editing workspace (`400 px`):
    /// wide enough for an Effect Controls row to draw its whole keyframe strip
    /// beside its label, value and keyframe buttons.
    pub const EDITING_LEFT_PX: f32 = 400.0;
    /// Default width of the right column in the Editing workspace (`300 px`).
    pub const EDITING_RIGHT_PX: f32 = 300.0;
    /// Default width of the Audio Meters frame in the Editing workspace (`64 px`).
    pub const EDITING_METERS_PX: f32 = 64.0;
    /// Share of the Editing column the lower row takes (`45 %`).
    pub const EDITING_TIMELINE_SHARE: f32 = 0.45;

    /// The shipped Editing layout (`docs/spec/layout.md` window diagram), six
    /// panels:
    /// - Column split: upper area `55 %`, lower area `45 %`.
    /// - Upper row: left frame [`Self::EDITING_LEFT_PX`] (`Project | Effect
    ///   Controls`), centre frame (`Viewer`), and right column
    ///   [`Self::EDITING_RIGHT_PX`] (`Effects & Presets`).
    /// - Lower row: `Timeline` on the left, `Audio Meters` at `64 px` on the
    ///   right.
    ///
    /// Every other panel opens from the Window menu into its
    /// [`default_frame_for`](super::default_frame_for) frame.
    pub fn editing() -> Self {
        Self::editing_for_width(Self::DEFAULT_WINDOW_WIDTH)
    }

    /// Build the shipped Editing layout with row weights that draw its frames
    /// at the spec's pixel widths ([`Self::EDITING_LEFT_PX`] left,
    /// [`Self::EDITING_RIGHT_PX`] right, [`Self::EDITING_METERS_PX`] audio
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

    /// Compute normalized weights `(left, center, right)` that draw an upper
    /// row's left frame [`Self::EDITING_LEFT_PX`] wide and its optional right
    /// frame [`Self::EDITING_RIGHT_PX`] wide at `window_w`.
    pub fn upper_row_weights_for_width(window_w: f32, has_right: bool) -> (f32, f32, f32) {
        let frames = if has_right { 3 } else { 2 };
        let avail = Self::frame_pool(window_w.max(640.0), frames);
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

    /// Compute normalized weights `(timeline, meters)` that draw the lower
    /// row's Audio Meters frame [`Self::EDITING_METERS_PX`] wide at
    /// `window_w`.
    pub fn lower_row_weights_for_width(window_w: f32) -> (f32, f32) {
        let avail = Self::frame_pool(window_w.max(464.0), 2);
        let meters_px = Self::EDITING_METERS_PX.clamp(48.0, (avail - 400.0).max(48.0));
        let timeline_px = (avail - meters_px).max(400.0);
        let sum = timeline_px + meters_px;
        (timeline_px / sum, meters_px / sum)
    }
}
