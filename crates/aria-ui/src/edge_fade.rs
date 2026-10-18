//! [`edge_faded`] — wraps a child so its whole subtree paints inside a
//! [`gpui::EdgeFade`] scope.
//!
//! An edge fades only while content continues past it. The top and bottom
//! edges read that from a tracked [`ScrollHandle`]; the left and right edges
//! read it from a handle or from the caller, which states it for a view that
//! scrolls by its own model. There is no unconditional fade: a band over an
//! edge with nothing past it cuts the first or last row of a list that fits.
//!
//! Ported from Comet (<https://github.com/zeronsh/comet>)
//! MIT License — see LICENSES/comet-MIT.txt

use gpui::{
    AnyElement, App, Bounds, EdgeFade, Edges, Element, GlobalElementId, InspectorElementId,
    IntoElement, LayoutId, Pixels, ScrollHandle, Window, px,
};

/// `child`, faded `band` px deep at each edge a builder reports content past.
pub fn edge_faded(band: f32, child: impl IntoElement) -> EdgeFaded {
    EdgeFaded {
        band,
        left: false,
        right: false,
        scroll_y: None,
        scroll_x: None,
        child: child.into_any_element(),
    }
}

pub struct EdgeFaded {
    band: f32,
    left: bool,
    right: bool,
    scroll_y: Option<ScrollHandle>,
    scroll_x: Option<ScrollHandle>,
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

    /// Fade the top and bottom edges while `handle`'s content continues past
    /// them.
    pub fn fade_overflow_y(mut self, handle: &ScrollHandle) -> Self {
        self.scroll_y = Some(handle.clone());
        self
    }

    /// Fade the left and right edges while `handle`'s content continues past
    /// them.
    pub fn fade_overflow_x(mut self, handle: &ScrollHandle) -> Self {
        self.scroll_x = Some(handle.clone());
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
        // The child's prepaint has run, so a handle holds this frame's offset
        // and extent.
        let (top, bottom) = self
            .scroll_y
            .as_ref()
            .map_or((false, false), |scroll| continues(scroll, |at| at.y));
        let (left, right) = self
            .scroll_x
            .as_ref()
            .map_or((false, false), |scroll| continues(scroll, |at| at.x));
        let band = |on: bool| px(if on { self.band } else { 0.0 });
        let fade = EdgeFade {
            bounds,
            bands: Edges {
                top: band(top),
                bottom: band(bottom),
                left: band(left || self.left),
                right: band(right || self.right),
            },
        };
        #[cfg(test)]
        if fade.fades() {
            PAINTED.with(|painted| painted.borrow_mut().push(fade));
        }
        window.with_edge_fade(Some(fade), |window| self.child.paint(window, cx));
    }
}

impl IntoElement for EdgeFaded {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}
