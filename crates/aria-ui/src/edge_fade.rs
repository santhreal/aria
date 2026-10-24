//! Fades over the edges of a view that content continues past.
//!
//! [`scroll_y`] and [`scroll_x`] make a scroll frame, which fades each edge on
//! its axis while the frame's content continues past that edge, read from the
//! frame's scroll handle. [`edge_faded`] fades the left and right edges of a
//! view that scrolls by its own model, where the caller states which edges
//! content continues past. There is no unconditional fade: a band over an
//! edge with nothing past it cuts the first or last row of a list that fits.
//!
//! Ported from Comet (<https://github.com/zeronsh/comet>)
//! MIT License — see LICENSES/comet-MIT.txt

use gpui::{
    AnyElement, App, Axis, Bounds, Div, EdgeFade, Edges, Element, ElementId, GlobalElementId,
    InspectorElementId, IntoElement, LayoutId, Pixels, ScrollHandle, Stateful,
    StatefulInteractiveElement as _, Window, div, px,
};

#[cfg(test)]
#[path = "edge_fade_tests.rs"]
mod tests;

/// Depth of the fade over an edge of a scroll frame that content continues
/// past.
pub const SCROLL_FADE: f32 = 24.0;

/// `child`, faded `band` px deep at each edge a builder reports content past.
pub fn edge_faded(band: f32, child: impl IntoElement) -> EdgeFaded {
    EdgeFaded {
        band,
        left: false,
        right: false,
        child: child.into_any_element(),
    }
}

pub struct EdgeFaded {
    band: f32,
    left: bool,
    right: bool,
    child: AnyElement,
}

impl EdgeFaded {
    /// Content continues past the left edge.
    pub fn fade_left(mut self, continues: bool) -> Self {
        self.left = continues;
        self
    }

    /// Content continues past the right edge.
    pub fn fade_right(mut self, continues: bool) -> Self {
        self.right = continues;
        self
    }
}

/// `frame`, scrolled on the vertical axis, with its top and bottom edges faded
/// while its content continues past them.
///
/// Every scroll frame in the crate is made here or by [`scroll_x`], so a frame
/// shows on its edges that it scrolls. The frame keeps its place from one draw
/// to the next under its id. A caller that reads or moves the place passes its
/// handle to [`ScrollFrame::track_scroll`], and not to the frame's own
/// `track_scroll`, which the frame's handle replaces.
pub fn scroll_y(frame: Stateful<Div>) -> ScrollFrame {
    ScrollFrame::new(frame.overflow_y_scroll(), Axis::Vertical)
}

/// `frame`, scrolled on the horizontal axis, with its left and right edges
/// faded while its content continues past them. See [`scroll_y`].
pub fn scroll_x(frame: Stateful<Div>) -> ScrollFrame {
    ScrollFrame::new(frame.overflow_x_scroll(), Axis::Horizontal)
}

/// A frame that scrolls on one axis and fades each edge on it that its content
/// continues past. Made by [`scroll_y`] and [`scroll_x`].
pub struct ScrollFrame {
    id: Option<ElementId>,
    /// The frame, until its layout is requested and it becomes the element
    /// drawn.
    frame: Option<Stateful<Div>>,
    axis: Axis,
    /// The caller's handle, and once laid out the handle the frame scrolls by.
    handle: Option<ScrollHandle>,
}

impl ScrollFrame {
    fn new(frame: Stateful<Div>, axis: Axis) -> Self {
        Self {
            id: Element::id(&frame),
            frame: Some(frame),
            axis,
            handle: None,
        }
    }

    /// Scroll by `handle`, which the caller reads or moves, instead of the
    /// handle the frame keeps under its id.
    pub fn track_scroll(mut self, handle: &ScrollHandle) -> Self {
        self.handle = Some(handle.clone());
        self
    }
}

/// Whether content continues before and after the view on one axis of
/// `handle`. A pixel of slack absorbs layout rounding at either end.
fn continues(scroll: &ScrollHandle, axis: fn(gpui::Point<Pixels>) -> Pixels) -> (bool, bool) {
    let scrolled = -f32::from(axis(scroll.offset()));
    let max_scroll = f32::from(axis(scroll.max_offset()));
    (scrolled > 1.0, scrolled < max_scroll - 1.0)
}

/// The fade over `bounds`, `band` px deep at each edge `at` sets.
pub(crate) fn fade(bounds: Bounds<Pixels>, band: f32, at: Edges<bool>) -> EdgeFade {
    EdgeFade {
        bounds,
        bands: at.map(|&on| px(if on { band } else { 0.0 })),
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
        let fade = fade(
            bounds,
            self.band,
            Edges {
                top: false,
                bottom: false,
                left: self.left,
                right: self.right,
            },
        );
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
        let handle = match (self.handle.clone(), id) {
            (Some(handle), _) => handle,
            // Kept under the frame's id, as a frame keeps its own offset, so
            // the place holds from one draw to the next.
            (None, Some(id)) => window.with_element_state(id, |kept: Option<ScrollHandle>, _| {
                let handle = kept.unwrap_or_default();
                (handle.clone(), handle)
            }),
            (None, None) => ScrollHandle::default(),
        };
        // A layout is requested once a draw; a second request draws nothing.
        let mut child = self.frame.take().map_or_else(
            || div().into_any_element(),
            |frame| frame.track_scroll(&handle).into_any_element(),
        );
        self.handle = Some(handle);
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
            .map_or((false, false), |handle| continues(handle, along));
        let vertical = self.axis == Axis::Vertical;
        let at = Edges {
            top: vertical && before,
            bottom: vertical && after,
            left: !vertical && before,
            right: !vertical && after,
        };
        paint_in(window, Some(fade(bounds, SCROLL_FADE, at)), |window| {
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
