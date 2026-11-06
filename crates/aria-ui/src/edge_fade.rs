//! Fades over the edges of a view that content continues past.
//!
//! [`scroll_y`] and [`scroll_x`] make a scroll frame, which fades each edge on
//! its axis while the frame's content continues past that edge, read from the
//! frame's scroll handle. [`scroll_list`] makes one over rows of one height,
//! drawing only the rows in view. [`edge_faded`] fades the edges of a view that
//! scrolls by its own model, such as the Timeline's ruler and lanes, where the
//! caller states how far content continues past each edge. A fade is as deep as
//! the content past its edge, up to the view's band: a view scrolled a few
//! pixels off a row fades those pixels, not a band over the row's text, and a
//! list that fits fades nothing.
//!
//! Ported from Comet (<https://github.com/zeronsh/comet>)
//! MIT License — see LICENSES/comet-MIT.txt

use std::ops::Range;

use gpui::{
    AnyElement, App, Axis, Bounds, Div, EdgeFade, Edges, Element, ElementId, GlobalElementId,
    InspectorElementId, IntoElement, LayoutId, Pixels, ScrollHandle, ScrollStrategy, Stateful,
    StatefulInteractiveElement as _, Styled as _, UniformList, UniformListScrollHandle, Window,
    div, px, uniform_list,
};

#[cfg(test)]
#[path = "edge_fade_tests.rs"]
mod tests;

/// Depth of the fade over an edge of a scroll frame that content continues
/// past by this much or more.
pub const SCROLL_FADE: f32 = 24.0;

/// `child`, faded at each edge a builder reports content past: as deep as the
/// content past the edge, up to `band` px.
pub fn edge_faded(band: f32, child: impl IntoElement) -> EdgeFaded {
    EdgeFaded {
        band,
        past: Edges::default(),
        child: child.into_any_element(),
    }
}

pub struct EdgeFaded {
    band: f32,
    /// How far content continues past each edge, in px.
    past: Edges<f32>,
    child: AnyElement,
}

impl std::fmt::Debug for EdgeFaded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EdgeFaded")
            .field("band", &self.band)
            .field("past", &self.past)
            .finish_non_exhaustive()
    }
}

impl EdgeFaded {
    /// Content continues `past` px beyond the left edge.
    pub fn fade_left(mut self, past: f32) -> Self {
        self.past.left = past;
        self
    }

    /// Content continues `past` px beyond the right edge.
    pub fn fade_right(mut self, past: f32) -> Self {
        self.past.right = past;
        self
    }

    /// Content continues `past` px beyond the top edge.
    pub fn fade_top(mut self, past: f32) -> Self {
        self.past.top = past;
        self
    }

    /// Content continues `past` px beyond the bottom edge.
    pub fn fade_bottom(mut self, past: f32) -> Self {
        self.past.bottom = past;
        self
    }
}

/// `frame`, scrolled on the vertical axis, with its top and bottom edges faded
/// while its content continues past them.
///
/// Every scroll frame in the crate is made here, by [`scroll_x`] or by
/// [`scroll_list`], so a frame shows on its edges that it scrolls. The frame
/// keeps its place from one draw to the next under its id. A caller that reads
/// or moves the place passes its handle to [`ScrollFrame::track_scroll`], and
/// not to the frame's own `track_scroll`, which the frame's handle replaces.
pub fn scroll_y(frame: Stateful<Div>) -> ScrollFrame {
    ScrollFrame::new(Scrolled::Div(frame.overflow_y_scroll()), Axis::Vertical)
}

/// `frame`, scrolled on the horizontal axis, with its left and right edges
/// faded while its content continues past them. See [`scroll_y`].
pub fn scroll_x(frame: Stateful<Div>) -> ScrollFrame {
    ScrollFrame::new(Scrolled::Div(frame.overflow_x_scroll()), Axis::Horizontal)
}

/// `rows` rows `row` px tall, scrolled on the vertical axis in a frame at most
/// `tallest` px tall, with its top and bottom edges faded while rows continue
/// past them. `render` draws the rows of a range, and only the rows in view are
/// drawn, so a list of a thousand rows costs a draw what a list of ten does.
///
/// The frame is laid out at its height, the rows' or `tallest`, whichever is
/// less, in any parent. A list measured by its rows instead counts every row
/// toward the height of a flex parent while it draws `tallest`, so a font list
/// under a title's Text tab grew the tab by every installed family and the
/// wheel scrolled the tab into blank ground.
///
/// Row `reveal` is brought into view on the first draw and on each draw it
/// differs from the row brought in last, centered when it was out of view.
/// Between those draws the frame keeps the place the wheel moved it to, under
/// `id`.
pub fn scroll_list<R: IntoElement>(
    id: impl Into<ElementId>,
    rows: usize,
    row: Pixels,
    tallest: Pixels,
    reveal: Option<usize>,
    render: impl Fn(Range<usize>, &mut Window, &mut App) -> Vec<R> + 'static,
) -> ScrollFrame {
    let list = uniform_list(id, rows, render)
        .w_full()
        .h((row * rows as f32).min(tallest));
    ScrollFrame::new(Scrolled::List { list, reveal }, Axis::Vertical)
}

/// What a scroll frame scrolls.
enum Scrolled {
    /// A `div` with every child laid out, scrolled by the frame's handle.
    Div(Stateful<Div>),
    /// Rows of one height, only those in view laid out, scrolled by the list
    /// handle the frame keeps, and the row to bring into view.
    List {
        list: UniformList,
        reveal: Option<usize>,
    },
}

impl Scrolled {
    fn id(&self) -> Option<ElementId> {
        match self {
            Self::Div(frame) => Element::id(frame),
            Self::List { list, .. } => Element::id(list),
        }
    }
}

/// What a list frame keeps from one draw to the next: the list's handle and
/// the row it last brought into view.
#[derive(Default)]
struct KeptList {
    handle: UniformListScrollHandle,
    revealed: Option<usize>,
}

/// A frame that scrolls on one axis and fades each edge on it that its content
/// continues past. Made by [`scroll_y`], [`scroll_x`] and [`scroll_list`].
pub struct ScrollFrame {
    id: Option<ElementId>,
    /// What the frame scrolls, until its layout is requested and it becomes the
    /// element drawn.
    frame: Option<Scrolled>,
    axis: Axis,
    /// The caller's handle, and once laid out the handle the frame scrolls by.
    handle: Option<ScrollHandle>,
}

impl std::fmt::Debug for ScrollFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScrollFrame")
            .field("id", &self.id)
            .field("axis", &self.axis)
            .finish_non_exhaustive()
    }
}

impl ScrollFrame {
    fn new(frame: Scrolled, axis: Axis) -> Self {
        Self {
            id: frame.id(),
            frame: Some(frame),
            axis,
            handle: None,
        }
    }

    /// Scroll by `handle`, which the caller reads or moves, instead of the
    /// handle the frame keeps under its id. A frame made by [`scroll_list`]
    /// scrolls by the list handle it keeps and takes none.
    pub fn track_scroll(mut self, handle: &ScrollHandle) -> Self {
        debug_assert!(
            !matches!(self.frame, Some(Scrolled::List { .. })),
            "a list frame scrolls by the list handle it keeps"
        );
        self.handle = Some(handle.clone());
        self
    }
}

/// How far content continues before and after the view on one axis of
/// `handle`, in px.
fn continues(scroll: &ScrollHandle, axis: fn(gpui::Point<Pixels>) -> Pixels) -> (f32, f32) {
    let scrolled = -f32::from(axis(scroll.offset()));
    let max_scroll = f32::from(axis(scroll.max_offset()));
    (scrolled, max_scroll - scrolled)
}

/// Depth of the fade over an edge content continues `past` px beyond, in a
/// view whose fade is `band` px deep: the content past the edge, up to the
/// band. A pixel of slack absorbs layout rounding, so content within a pixel
/// of the edge fades nothing.
pub(crate) fn depth(band: f32, past: f32) -> f32 {
    if past > 1.0 { past.min(band) } else { 0.0 }
}

/// The fade over `bounds` with content `past` each edge by that many px, at
/// most `band` px deep at any edge.
pub(crate) fn fade(bounds: Bounds<Pixels>, band: f32, past: Edges<f32>) -> EdgeFade {
    EdgeFade {
        bounds,
        bands: past.map(|&past| px(depth(band, past))),
    }
}

#[cfg(test)]
thread_local! {
    static PAINTED: std::cell::RefCell<Vec<EdgeFade>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Every fade painted on this thread since the last call, in paint order.
/// Fades with no band are left out: they paint nothing.
#[cfg(test)]
pub(crate) fn take_painted() -> Vec<EdgeFade> {
    PAINTED.with(|painted| std::mem::take(&mut *painted.borrow_mut()))
}

/// Runs `paint` inside `fade`. Every element in the crate that paints in a
/// fade paints through this, so a test reads each fade applied from
/// [`take_painted`].
pub(crate) fn paint_in<R>(
    window: &mut Window,
    fade: Option<EdgeFade>,
    paint: impl FnOnce(&mut Window) -> R,
) -> R {
    #[cfg(test)]
    if let Some(fade) = fade.filter(EdgeFade::fades) {
        PAINTED.with(|painted| painted.borrow_mut().push(fade));
    }
    window.with_edge_fade(fade, paint)
}

impl Element for EdgeFaded {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<gpui::ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.child.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let fade = fade(bounds, self.band, self.past);
        paint_in(window, Some(fade), |window| self.child.paint(window, cx));
    }
}

impl IntoElement for EdgeFaded {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for ScrollFrame {
    type RequestLayoutState = AnyElement;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        self.id.clone()
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, AnyElement) {
        // A layout is requested once a draw; a second request draws nothing.
        let mut child = match self.frame.take() {
            Some(Scrolled::Div(frame)) => {
                let handle = match (self.handle.clone(), id) {
                    (Some(handle), _) => handle,
                    // Kept under the frame's id, as a frame keeps its own
                    // offset, so the place holds from one draw to the next.
                    (None, Some(id)) => {
                        window.with_element_state(id, |kept: Option<ScrollHandle>, _| {
                            let handle = kept.unwrap_or_default();
                            (handle.clone(), handle)
                        })
                    }
                    (None, None) => ScrollHandle::default(),
                };
                let child = frame.track_scroll(&handle).into_any_element();
                self.handle = Some(handle);
                child
            }
            Some(Scrolled::List { list, reveal }) => {
                let list_handle = match id {
                    // Kept under the frame's id with the row last brought into
                    // view, so the wheel's place holds until that row changes.
                    Some(id) => window.with_element_state(id, |kept: Option<KeptList>, _| {
                        let mut kept = kept.unwrap_or_default();
                        if reveal != kept.revealed {
                            if let Some(row) = reveal {
                                kept.handle.scroll_to_item(row, ScrollStrategy::Center);
                            }
                            kept.revealed = reveal;
                        }
                        (kept.handle.clone(), kept)
                    }),
                    None => UniformListScrollHandle::new(),
                };
                self.handle = Some(list_handle.0.borrow().base_handle.clone());
                list.track_scroll(&list_handle).into_any_element()
            }
            None => div().into_any_element(),
        };
        (child.request_layout(window, cx), child)
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        child: &mut AnyElement,
        window: &mut Window,
        cx: &mut App,
    ) {
        child.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        child: &mut AnyElement,
        _prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let along: fn(gpui::Point<Pixels>) -> Pixels = match self.axis {
            Axis::Vertical => |at| at.y,
            Axis::Horizontal => |at| at.x,
        };
        // The child's prepaint has run, so the handle holds this draw's offset
        // and extent.
        let (before, after) = self
            .handle
            .as_ref()
            .map_or((0.0, 0.0), |handle| continues(handle, along));
        let (on_y, on_x) = match self.axis {
            Axis::Vertical => ((before, after), (0.0, 0.0)),
            Axis::Horizontal => ((0.0, 0.0), (before, after)),
        };
        let past = Edges {
            top: on_y.0,
            bottom: on_y.1,
            left: on_x.0,
            right: on_x.1,
        };
        paint_in(window, Some(fade(bounds, SCROLL_FADE, past)), |window| {
            child.paint(window, cx)
        });
    }
}

impl IntoElement for ScrollFrame {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}
