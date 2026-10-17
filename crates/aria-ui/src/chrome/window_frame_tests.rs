//! The window's own frame resizes from every free side and moves no content.
//!
//! The class closed here: a side the window is not tiled against with no band
//! to resize it from, a band on a tiled side, a point two bands claim, a band
//! that resizes a side far from where it lies, a press on a band that also
//! reaches the content under it, and a frame that moves the content off the
//! window's origin, which would put every menu the shell opens at a pointer
//! position beside the pointer. Every one of the sixteen tilings is
//! enumerated, at sample points along each side, through each corner, and in
//! the middle of the window.
//!
//! Not covered: the platform's response to a resize or move request, which a
//! test window does not implement, and the pixels of the hairline.

use gpui::{
    Bounds, Context, InteractiveElement, IntoElement, Modifiers, MouseButton, Pixels, ResizeEdge,
    Size, Styled, TestAppContext, Tiling, Window, div, point, px, size,
};

use super::{BAND, CORNER, Frame, Handle, WindowTitle, framed, handles, resize_cursor};
use crate::theme::Theme;

/// Every tiling: bit 0 top, 1 right, 2 bottom, 3 left.
fn tilings() -> impl Iterator<Item = Tiling> {
    (0u8..16).map(|bits| Tiling {
        top: bits & 1 != 0,
        right: bits & 2 != 0,
        bottom: bits & 4 != 0,
        left: bits & 8 != 0,
    })
}

/// The sides an edge or a corner resizes, as `[top, right, bottom, left]`.
fn sides(edge: ResizeEdge) -> [bool; 4] {
    match edge {
        ResizeEdge::Top => [true, false, false, false],
        ResizeEdge::TopRight => [true, true, false, false],
        ResizeEdge::Right => [false, true, false, false],
        ResizeEdge::BottomRight => [false, true, true, false],
        ResizeEdge::Bottom => [false, false, true, false],
        ResizeEdge::BottomLeft => [false, false, true, true],
        ResizeEdge::Left => [false, false, false, true],
        ResizeEdge::TopLeft => [true, false, false, true],
    }
}

/// How far `at` is from each side of `window`, as `[top, right, bottom, left]`.
fn reach(at: [f32; 2], window: Size<Pixels>) -> [f32; 4] {
    let (w, h) = (f32::from(window.width), f32::from(window.height));
    [at[1], w - at[0], h - at[1], at[0]]
}

/// Offsets from each end of an axis `len` long: inside a band, between a
/// band and a corner, past a corner, and the middle.
fn samples(len: f32) -> Vec<f32> {
    let near = [
        0.5,
        BAND - 0.5,
        BAND + 0.5,
        CORNER - 0.5,
        CORNER + 0.5,
        3.0 * CORNER,
    ];
    let mut out: Vec<f32> = near.iter().flat_map(|d| [*d, len - d]).collect();
    out.push(len / 2.0);
    out
}

const SIZES: [Size<Pixels>; 3] = [
    size(px(1440.), px(900.)),
    size(px(1024.), px(640.)),
    size(px(480.), px(320.)),
];

#[test]
fn every_free_side_resizes_from_its_edge_and_nothing_else_does() {
    for window in SIZES {
        for tiling in tilings() {
            let bands: Vec<Handle> = handles(window, tiling).into_iter().flatten().collect();
            let tiled = [tiling.top, tiling.right, tiling.bottom, tiling.left];
            for band in &bands {
                assert_eq!(resize_cursor(band.edge), cursor_for(band.edge));
            }
            for y in samples(f32::from(window.height)) {
                for x in samples(f32::from(window.width)) {
                    let at = point(px(x), px(y));
                    let on: Vec<&Handle> = bands
                        .iter()
                        .filter(|band| band.bounds.contains(&at))
                        .collect();
                    assert!(
                        on.len() <= 1,
                        "{tiling:?} at ({x}, {y}): two bands claim one point"
                    );
                    let near = reach([x, y], window);
                    // Every free side within a band's width of the point.
                    let wanted: Vec<usize> =
                        (0..4).filter(|&n| !tiled[n] && near[n] < BAND).collect();
                    match on.first() {
                        None => assert!(
                            wanted.is_empty(),
                            "{tiling:?} at ({x}, {y}): free sides {wanted:?} have no band here"
                        ),
                        Some(band) => {
                            let resized = sides(band.edge);
                            for n in wanted {
                                assert!(
                                    resized[n],
                                    "{tiling:?} at ({x}, {y}): the {:?} band does not resize side {n}",
                                    band.edge
                                );
                            }
                            for n in (0..4).filter(|&n| resized[n]) {
                                assert!(
                                    !tiled[n],
                                    "{tiling:?}: the {:?} band resizes tiled side {n}",
                                    band.edge
                                );
                                assert!(
                                    near[n] < CORNER,
                                    "{tiling:?} at ({x}, {y}): the {:?} band resizes side {n}, {} px away",
                                    band.edge,
                                    near[n]
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

fn cursor_for(edge: ResizeEdge) -> gpui::CursorStyle {
    let [top, right, bottom, left] = sides(edge);
    match (top || bottom, left || right) {
        (true, false) => gpui::CursorStyle::ResizeUpDown,
        (false, true) => gpui::CursorStyle::ResizeLeftRight,
        _ if (top && left) || (bottom && right) => gpui::CursorStyle::ResizeUpLeftDownRight,
        _ => gpui::CursorStyle::ResizeUpRightDownLeft,
    }
}

#[test]
fn only_a_frame_the_app_draws_has_window_controls() {
    assert!(!Frame::Platform.draws_controls());
    assert!(Frame::Caption.draws_controls());
    for tiling in tilings() {
        assert!(Frame::Client(tiling).draws_controls(), "{tiling:?}");
    }
}

#[test]
fn the_title_names_the_project_and_marks_unsaved_changes() {
    let cases = [
        (WindowTitle::Home, "Aria", false),
        (
            WindowTitle::Project {
                name: "untitled".into(),
                dirty: false,
            },
            "untitled - Aria",
            false,
        ),
        (
            WindowTitle::Project {
                name: "Trailer cut".into(),
                dirty: true,
            },
            "*Trailer cut - Aria",
            true,
        ),
        (WindowTitle::Panel("Info"), "Info - Aria", false),
    ];
    for (title, text, edited) in cases {
        assert_eq!(title.text(), text);
        assert_eq!(title.edited(), edited, "{text}");
    }
}

/// A window whose whole content is one probe that counts the presses it
/// receives, inside the frame under test.
struct Framed {
    frame: Frame,
    presses: usize,
}

impl gpui::Render for Framed {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let probe = div()
            .id("framed-probe")
            .size_full()
            .debug_selector(|| "framed-probe".into())
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.presses += 1),
            )
            .into_any_element();
        framed(probe, self.frame, &theme, window)
    }
}

#[gpui::test]
fn the_content_fills_the_window_and_a_band_press_never_reaches_it(cx: &mut TestAppContext) {
    cx.update(|app| {
        Theme::install(
            crate::theme::Appearance::Dark,
            Default::default(),
            None,
            app,
        );
    });
    let frames = [Frame::Platform, Frame::Caption]
        .into_iter()
        .chain(tilings().map(Frame::Client));
    for frame in frames {
        let (view, visual) = cx.add_window_view(move |_, _| Framed { frame, presses: 0 });
        visual.run_until_parked();
        let window = visual.update(|window, _| window.viewport_size());
        let probe = visual
            .debug_bounds("framed-probe")
            .unwrap_or_else(|| panic!("{frame:?}: the content was not drawn"));
        assert_eq!(
            probe,
            Bounds::new(point(px(0.), px(0.)), window),
            "{frame:?}: the frame moved or shrank the content"
        );
        let bands: Vec<Handle> = match frame {
            Frame::Client(tiling) => handles(window, tiling).into_iter().flatten().collect(),
            Frame::Platform | Frame::Caption => Vec::new(),
        };
        for band in &bands {
            visual.simulate_click(band.bounds.center(), Modifiers::none());
        }
        assert_eq!(
            visual.read(|app| view.read(app).presses),
            0,
            "{frame:?}: a press on a resize band reached the content"
        );
        let middle = point(window.width / 2., window.height / 2.);
        visual.simulate_click(middle, Modifiers::none());
        assert_eq!(
            visual.read(|app| view.read(app).presses),
            1,
            "{frame:?}: a press in the middle of the window missed the content"
        );
    }
}
