//! Every scroll frame fades each edge on its axis that its content continues
//! past, and no other edge.
//!
//! The class closed here: a scroll frame with no sign that it scrolls. GPUI
//! draws no scroll bar over a scrolling `div`, and of the crate's scroll frames
//! only Home's body and the assistant's thread faded their edges, so the Scopes
//! panel cut its vectorscope at the panel's foot and Effect Controls cut its
//! rows with nothing to show that more lay below.
//!
//! The census reads every production source in the crate and fails on a frame
//! made to scroll anywhere but [`scroll_y`] and [`scroll_x`], and on fewer
//! frames made there than the crate has. The draw cases scroll a column and a
//! row of cells to their start, middle, and end by a caller's handle, and read
//! which edges fade at each: an edge fades only while the content continues
//! past it, and a frame whose content fits fades none. A column turned by the
//! wheel with no caller's handle keeps its place on the next draw.
//!
//! Not covered: the fade's pixels, which the test platform does not raster on
//! Linux.

use gpui::{
    Axis, Context, EdgeFade, InteractiveElement, IntoElement, Modifiers, ParentElement, Render,
    ScrollDelta, ScrollHandle, ScrollWheelEvent, Styled, TestAppContext, TouchPhase,
    VisualTestContext, Window, div, point, px,
};

use super::{SCROLL_FADE, scroll_x, scroll_y};
use crate::test_support::sources::production_sources;

/// The calls and the style that make a `div` scroll, as a method or by path.
const SCROLLS: [&str; 4] = [
    "overflow_scroll(",
    "overflow_x_scroll(",
    "overflow_y_scroll(",
    "Overflow::Scroll",
];

/// Fewest scroll frames the crate makes through [`scroll_y`] and [`scroll_x`].
/// A frame moved to another way of scrolling drops the count below this.
const FRAME_FLOOR: usize = 22;

/// **Every scroll frame the crate makes fades the edges its content continues
/// past.**
#[test]
fn every_scroll_frame_the_crate_makes_fades_its_edges() {
    let mut made = 0;
    let mut bare = Vec::new();
    for (rel, code) in production_sources() {
        if rel == "edge_fade.rs" {
            continue;
        }
        made += code.matches("edge_fade::scroll_y(").count()
            + code.matches("edge_fade::scroll_x(").count();
        for (index, line) in code.lines().enumerate() {
            for call in SCROLLS.iter().filter(|call| line.contains(*call)) {
                bare.push(format!("{rel}:{}: {call}", index + 1));
            }
        }
    }
    assert!(
        bare.is_empty(),
        "{} scroll frames are made without `edge_fade::scroll_y` or `edge_fade::scroll_x`, \
         so they fade no edge their content continues past:\n  {}",
        bare.len(),
        bare.join("\n  ")
    );
    assert!(
        made >= FRAME_FLOOR,
        "{made} scroll frames are made through `edge_fade`, fewer than the {FRAME_FLOOR} the \
         crate has: a frame now scrolls by a call this census does not read"
    );
}

/// Side of the frame in the draw cases.
const VIEW: f32 = 100.0;

/// Side of each cell in the frame.
const CELL: f32 = 25.0;

/// Cells that run to three frames past the first.
const LONG: usize = 16;

/// Cells that fill the frame and run no further.
const FILL: usize = 4;

/// A frame of `cells` on `axis`, scrolled by `handle` when there is one.
struct Frame {
    axis: Axis,
    cells: usize,
    handle: Option<ScrollHandle>,
}

impl Render for Frame {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let cells = (0..self.cells).map(|_| div().flex_none().size(px(CELL)));
        let frame = div()
            .id("frame")
            .debug_selector(|| "frame".into())
            .size(px(VIEW))
            .flex()
            .children(cells);
        let frame = match self.axis {
            Axis::Vertical => scroll_y(frame.flex_col()),
            Axis::Horizontal => scroll_x(frame.flex_row()),
        };
        let frame = match &self.handle {
            Some(handle) => frame.track_scroll(handle),
            None => frame,
        };
        div().size_full().child(frame)
    }
}

/// The fades of one full draw.
fn draw(visual: &mut VisualTestContext) -> Vec<EdgeFade> {
    visual.run_until_parked();
    super::take_painted();
    visual.update(|window, _| window.refresh());
    super::take_painted()
}

/// The bands of the one fade over the frame, as `(before, after)` along
/// `axis`, or zeros when no fade is painted.
fn bands(visual: &mut VisualTestContext, axis: Axis, what: &str) -> (f32, f32) {
    let fades = draw(visual);
    let frame = visual
        .debug_bounds("frame")
        .unwrap_or_else(|| panic!("{what}: the frame was not drawn"));
    let [fade] = fades.as_slice() else {
        assert!(
            fades.is_empty(),
            "{what}: {} fades painted, at most one expected",
            fades.len()
        );
        return (0.0, 0.0);
    };
    assert_eq!(fade.bounds, frame, "{what}: the fade is not over the frame");
    let b = fade.bands;
    let (before, after, across) = match axis {
        Axis::Vertical => (b.top, b.bottom, [b.left, b.right]),
        Axis::Horizontal => (b.left, b.right, [b.top, b.bottom]),
    };
    assert_eq!(across, [px(0.0); 2], "{what}: an edge off the axis fades");
    (f32::from(before), f32::from(after))
}

/// One frame: its axis, its cells, the caller's offset along the axis, and
/// whether the start and the end edges fade there.
struct Case {
    axis: Axis,
    cells: usize,
    scrolled: f32,
    start: bool,
    end: bool,
}

const fn case(axis: Axis, cells: usize, scrolled: f32, start: bool, end: bool) -> Case {
    Case {
        axis,
        cells,
        scrolled,
        start,
        end,
    }
}

/// The furthest a long frame scrolls.
const END: f32 = LONG as f32 * CELL - VIEW;

const CASES: [Case; 8] = [
    case(Axis::Vertical, FILL, 0.0, false, false),
    case(Axis::Vertical, LONG, 0.0, false, true),
    case(Axis::Vertical, LONG, END / 2.0, true, true),
    case(Axis::Vertical, LONG, END, true, false),
    case(Axis::Horizontal, FILL, 0.0, false, false),
    case(Axis::Horizontal, LONG, 0.0, false, true),
    case(Axis::Horizontal, LONG, END / 2.0, true, true),
    case(Axis::Horizontal, LONG, END, true, false),
];

#[gpui::test]
fn a_frame_fades_only_the_edges_its_content_continues_past(cx: &mut TestAppContext) {
    for (index, case) in CASES.iter().enumerate() {
        let handle = ScrollHandle::new();
        let (axis, cells) = (case.axis, case.cells);
        let (_, visual) = crate::test_support::window_view(cx, {
            let handle = handle.clone();
            move |_, _| Frame {
                axis,
                cells,
                handle: Some(handle),
            }
        });
        let what = format!(
            "case {index}: {cells} cells on {axis:?} scrolled {}",
            case.scrolled
        );
        draw(visual);
        let offset = match axis {
            Axis::Vertical => point(px(0.0), px(-case.scrolled)),
            Axis::Horizontal => point(px(-case.scrolled), px(0.0)),
        };
        handle.set_offset(offset);
        let (start, end) = bands(visual, axis, &what);
        assert_eq!(
            handle.offset(),
            offset,
            "{what}: the frame does not scroll by the caller's handle"
        );
        let want = |on: bool| if on { SCROLL_FADE } else { 0.0 };
        assert_eq!(
            (start, end),
            (want(case.start), want(case.end)),
            "{what}: the start and end bands"
        );
    }
}

#[gpui::test]
fn a_frame_keeps_its_place_without_a_callers_handle(cx: &mut TestAppContext) {
    let (_, visual) = crate::test_support::window_view(cx, |_, _| Frame {
        axis: Axis::Vertical,
        cells: LONG,
        handle: None,
    });
    let what = "a column turned by the wheel";
    assert_eq!(
        bands(visual, Axis::Vertical, what),
        (0.0, SCROLL_FADE),
        "{what}: at rest only the end fades"
    );
    let frame = visual
        .debug_bounds("frame")
        .unwrap_or_else(|| panic!("{what}: the frame was not drawn"));
    let position = frame.center();
    visual.simulate_mouse_move(position, None, Modifiers::none());
    visual.simulate_event(ScrollWheelEvent {
        position,
        delta: ScrollDelta::Pixels(point(px(0.0), px(-END / 2.0))),
        modifiers: Modifiers::none(),
        touch_phase: TouchPhase::Moved,
    });
    assert_eq!(
        bands(visual, Axis::Vertical, what),
        (SCROLL_FADE, SCROLL_FADE),
        "{what}: the next draw lost the place the wheel left it at"
    );
}
