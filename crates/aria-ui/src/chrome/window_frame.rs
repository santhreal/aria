//! The window's frame and title.
//!
//! Linux compositors that draw no decorations, GNOME's among them, leave a
//! client-decorated window with no border to resize it from and no title bar
//! to move it by. [`framed`] lays a resize band [`BAND`] wide along every side
//! the window is not tiled against, over the edge of the content, and edges
//! those sides with a hairline. The bar's empty stretches move the window.
//!
//! The content keeps the window's origin and size under every frame. A
//! position read from a pointer event is a window position, and the shell
//! places its menus and scrims at such positions, so a frame that shifted the
//! content would shift every one of them away from the pointer.

use gpui::{
    AnyElement, Bounds, CursorStyle, Decorations, InteractiveElement, IntoElement, MouseButton,
    ParentElement, Pixels, ResizeEdge, SharedString, Size, Styled, Tiling, Window, div, point, px,
    size,
};

use crate::fluent::FluentBuilder;
use crate::theme::Theme;

/// The width of a resize band along a side, in logical pixels.
pub(crate) const BAND: f32 = 5.0;

/// The side of the square a corner resizes from, in logical pixels: wider
/// than [`BAND`], so a diagonal resize does not need a five pixel target.
pub(crate) const CORNER: f32 = 12.0;

/// Who draws the window's frame, which sets what the bar does with a press.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Frame {
    /// The platform or the window manager draws the frame and its controls:
    /// macOS, and Linux under server decorations or with no compositor.
    Platform,
    /// Windows with its title bar hidden: the bar draws the window controls
    /// and reports its empty stretches as the caption, so the system moves,
    /// snaps, and maximizes the window from them.
    Caption,
    /// Linux client decorations: the bar draws the window controls and moves
    /// the window, and a band along every side not tiled resizes it.
    Client(Tiling),
}

impl Frame {
    /// The frame `window` has now. Maximizing or snapping the window changes
    /// its tiling, so this is read on every draw.
    pub(crate) fn of(window: &Window) -> Self {
        if cfg!(target_os = "macos") {
            Self::Platform
        } else if cfg!(target_os = "windows") {
            Self::Caption
        } else {
            match window.window_decorations() {
                Decorations::Server => Self::Platform,
                Decorations::Client { tiling } => Self::Client(tiling),
            }
        }
    }

    /// Whether the bar draws minimize, maximize, and close.
    pub(crate) fn draws_controls(self) -> bool {
        !matches!(self, Self::Platform)
    }
}

/// One resize band: the edge or corner a press on it resizes from, and where
/// it sits in the window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Handle {
    pub(crate) edge: ResizeEdge,
    pub(crate) bounds: Bounds<Pixels>,
}

/// The resize bands of a client frame `window` in size and tiled as `tiling`.
///
/// A side tiled against the screen edge or another window has no band. A
/// corner whose two sides are not tiled is a [`CORNER`] square, and the bands
/// of those sides stop at it, so no point is on two bands.
pub(crate) fn handles(window: Size<Pixels>, tiling: Tiling) -> [Option<Handle>; 8] {
    let (w, h) = (f32::from(window.width), f32::from(window.height));
    let (top, right, bottom, left) = (!tiling.top, !tiling.right, !tiling.bottom, !tiling.left);
    // A corner square only fits a window wider and taller than two of them.
    let fits = w >= 2.0 * CORNER && h >= 2.0 * CORNER;
    let corner = |a: bool, b: bool| fits && a && b;
    let (tl, tr, br, bl) = (
        corner(top, left),
        corner(top, right),
        corner(bottom, right),
        corner(bottom, left),
    );
    let from = |cut: bool| if cut { CORNER } else { 0.0 };
    let band = |on: bool, edge: ResizeEdge, x: f32, y: f32, bw: f32, bh: f32| {
        (on && bw > 0.0 && bh > 0.0).then(|| Handle {
            edge,
            bounds: Bounds::new(point(px(x), px(y)), size(px(bw), px(bh))),
        })
    };
    let (c, b) = (CORNER, BAND);
    [
        band(tl, ResizeEdge::TopLeft, 0.0, 0.0, c, c),
        band(
            top,
            ResizeEdge::Top,
            from(tl),
            0.0,
            w - from(tl) - from(tr),
            b,
        ),
        band(tr, ResizeEdge::TopRight, w - c, 0.0, c, c),
        band(
            right,
            ResizeEdge::Right,
            w - b,
            from(tr),
            b,
            h - from(tr) - from(br),
        ),
        band(br, ResizeEdge::BottomRight, w - c, h - c, c, c),
        band(
            bottom,
            ResizeEdge::Bottom,
            from(bl),
            h - b,
            w - from(bl) - from(br),
            b,
        ),
        band(bl, ResizeEdge::BottomLeft, 0.0, h - c, c, c),
        band(
            left,
            ResizeEdge::Left,
            0.0,
            from(tl),
            b,
            h - from(tl) - from(bl),
        ),
    ]
}

/// The pointer a resize band shows.
pub(crate) fn resize_cursor(edge: ResizeEdge) -> CursorStyle {
    match edge {
        ResizeEdge::Top | ResizeEdge::Bottom => CursorStyle::ResizeUpDown,
        ResizeEdge::Left | ResizeEdge::Right => CursorStyle::ResizeLeftRight,
        ResizeEdge::TopLeft | ResizeEdge::BottomRight => CursorStyle::ResizeUpLeftDownRight,
        ResizeEdge::TopRight | ResizeEdge::BottomLeft => CursorStyle::ResizeUpRightDownLeft,
    }
}

/// `content` inside the window's frame.
///
/// Under [`Frame::Client`] a hairline edges every side not tiled and the
/// bands of [`handles`] lie over the content's edge: a press on one resizes
/// the window and reaches nothing under it, and a wheel still scrolls what is
/// under it. Under any other frame the content is returned as it is.
pub(crate) fn framed(
    content: AnyElement,
    frame: Frame,
    theme: &Theme,
    window: &Window,
) -> AnyElement {
    let Frame::Client(tiling) = frame else {
        return content;
    };
    let bands = handles(window.viewport_size(), tiling);
    div()
        .id("window-frame")
        .size_full()
        .relative()
        .child(content)
        // Drawn over the content rather than round it: a border on the box
        // would move the content in by its width.
        .child(
            div()
                .absolute()
                .inset_0()
                .border_color(theme.border_strong)
                .when(!tiling.top, |el| el.border_t_1())
                .when(!tiling.right, |el| el.border_r_1())
                .when(!tiling.bottom, |el| el.border_b_1())
                .when(!tiling.left, |el| el.border_l_1()),
        )
        .children(bands.into_iter().flatten().map(|band| {
            let edge = band.edge;
            div()
                .absolute()
                .left(band.bounds.origin.x)
                .top(band.bounds.origin.y)
                .w(band.bounds.size.width)
                .h(band.bounds.size.height)
                .block_mouse_except_scroll()
                .cursor(resize_cursor(edge))
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    cx.stop_propagation();
                    window.start_window_resize(edge);
                })
        }))
        .into_any_element()
}

/// What the platform title of a window reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum WindowTitle {
    /// The Home screen.
    Home,
    /// The editor on a project, and whether it has unsaved changes.
    Project { name: SharedString, dirty: bool },
    /// A floating panel window, by its panel's title.
    Panel(&'static str),
}

impl WindowTitle {
    /// The title text: the project's name with a leading `*` while it has
    /// unsaved changes, or the panel's, then the application's.
    pub(crate) fn text(&self) -> String {
        match self {
            Self::Home => "Aria".to_owned(),
            Self::Project { name, dirty } => {
                format!("{}{name} - Aria", if *dirty { "*" } else { "" })
            }
            Self::Panel(panel) => format!("{panel} - Aria"),
        }
    }

    /// Whether the window has unsaved changes, for the platform's edited mark.
    pub(crate) fn edited(&self) -> bool {
        matches!(self, Self::Project { dirty: true, .. })
    }
}

#[cfg(test)]
#[path = "window_frame_tests.rs"]
mod window_frame_tests;
