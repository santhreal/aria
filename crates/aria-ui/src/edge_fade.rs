//! [`edge_faded`] — wraps a child so its whole subtree paints inside a
//! [`gpui::EdgeFade`] scope.
//!
//! Ported from Comet (<https://github.com/zeronsh/comet>)
//! MIT License — see LICENSES/comet-MIT.txt

use gpui::{
    AnyElement, App, Bounds, EdgeFade, Edges, Element, GlobalElementId, InspectorElementId,
    IntoElement, LayoutId, Pixels, ScrollHandle, Window, px,
};

pub fn edge_faded(band: f32, top: bool, bottom: bool, child: impl IntoElement) -> EdgeFaded {
    EdgeFaded {
        band,
        band_top: None,
        band_bottom: None,
        inset_top: 0.0,
        top,
        bottom,
        left: false,
        right: false,
        scroll_y: None,
        scroll_x: None,
        child: child.into_any_element(),
    }
}

pub struct EdgeFaded {
    band: f32,
    band_top: Option<f32>,
    band_bottom: Option<f32>,
    inset_top: f32,
    top: bool,
    bottom: bool,
    left: bool,
    right: bool,
    scroll_y: Option<ScrollHandle>,
    scroll_x: Option<ScrollHandle>,
    child: AnyElement,
}

impl EdgeFaded {
    pub fn fade_left(mut self, on: bool) -> Self {
        self.left = on;
        self
    }

    pub fn fade_right(mut self, on: bool) -> Self {
        self.right = on;
        self
    }

    pub fn band_top(mut self, px: f32) -> Self {
        self.band_top = Some(px);
        self
    }

    pub fn band_bottom(mut self, px: f32) -> Self {
        self.band_bottom = Some(px);
        self
    }

    pub fn fade_overflow_y(mut self, handle: &ScrollHandle) -> Self {
        self.scroll_y = Some(handle.clone());
        self
    }

    pub fn fade_overflow_x(mut self, handle: &ScrollHandle) -> Self {
        self.scroll_x = Some(handle.clone());
        self
    }

    pub fn inset_top(mut self, px: f32) -> Self {
        self.inset_top = px;
        self
    }
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
        let (mut top, mut bottom) = (self.top, self.bottom);
        if let Some(scroll) = &self.scroll_y {
            let scrolled = -f32::from(scroll.offset().y);
            let max_scroll = f32::from(scroll.max_offset().y);
            top &= scrolled > 1.0;
            bottom &= scrolled < max_scroll - 1.0;
        }
        let (mut left, mut right) = (self.left, self.right);
        if let Some(scroll) = &self.scroll_x {
            let scrolled = -f32::from(scroll.offset().x);
            let max_scroll = f32::from(scroll.max_offset().x);
            left &= scrolled > 1.0;
            right &= scrolled < max_scroll - 1.0;
        }
        let band = |on: bool, width: f32| px(if on { width } else { 0.0 });
        let bands = Edges {
            top: band(top, self.band_top.unwrap_or(self.band)),
            bottom: band(bottom, self.band_bottom.unwrap_or(self.band)),
            left: band(left, self.band),
            right: band(right, self.band),
        };
        let fade = {
            let mut bounds = bounds;
            bounds.origin.y += px(self.inset_top);
            bounds.size.height -= px(self.inset_top);
            EdgeFade { bounds, bands }
        };
        window.with_edge_fade(Some(fade), |window| self.child.paint(window, cx));
    }
}

impl IntoElement for EdgeFaded {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}
