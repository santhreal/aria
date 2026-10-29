//! What a drawn frame holds that its text runs do not show: the bounds of
//! every image and icon, and of every row that wraps its items, and the
//! opacity each picture was painted at.
//!
//! A test turns recording on, draws, and takes the record. Recording is off
//! otherwise, and a build that is not a test records nothing and draws no
//! marker.
//!
//! While a test records, a row that wraps also adds a marker to the window's
//! text runs where it starts to paint: an empty run, zero points in size, over
//! the row's bounds. The runs after it in paint order are the row's own; a
//! list's text scrolled under the row was painted before it. `is_marker`
//! tells a marker from text.
//!
//! A [`picture`] adds a marker of its own: a box of the `PICTURE_MARK` color
//! over its bounds at the picture's own opacity, which the window multiplies by
//! every ancestor's. `painted_pictures` reads them back from the window's
//! quads.

use std::panic::Location;

use gpui::{
    App, Bounds, Element, ElementId, GlobalElementId, InspectorElementId, IntoElement, LayoutId,
    ParentElement, Pixels, StyleRefinement, Styled, Window,
};

/// What a recorded box is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Drawn {
    /// An image or an icon.
    Image,
    /// A row laid out by [`Wraps::wraps`].
    WrapRow,
}

#[cfg(test)]
thread_local! {
    static RECORD: std::cell::RefCell<Option<Vec<(Drawn, Bounds<Pixels>)>>> =
        const { std::cell::RefCell::new(None) };
}

/// Record that a `kind` box was laid out at `bounds`, while a test records.
#[inline]
pub(crate) fn record(kind: Drawn, bounds: Bounds<Pixels>) {
    #[cfg(test)]
    RECORD.with_borrow_mut(|record| {
        if let Some(record) = record {
            record.push((kind, bounds));
        }
    });
    #[cfg(not(test))]
    let _ = (kind, bounds);
}

/// Add a wrap row's marker at `bounds` to the frame's text runs, while a test
/// records.
#[cfg(test)]
fn mark(bounds: Bounds<Pixels>, window: &mut Window) {
    if RECORD.with_borrow(Option::is_some) {
        window.record_text_run(gpui::TextRunLayout {
            font_size: gpui::px(0.0),
            bounds,
            text: gpui::SharedString::default(),
        });
    }
}

/// The color of a picture's marker at full opacity. No chrome paints it.
#[cfg(test)]
const PICTURE_MARK: gpui::Hsla = gpui::Hsla {
    h: 0.123,
    s: 0.987,
    l: 0.456,
    a: 1.0,
};

/// Every picture the last frame painted while a test recorded: its bounds in
/// window pixels and the opacity it was painted at. A picture painted fully
/// transparent paints no marker, so it is absent.
#[cfg(test)]
pub(crate) fn painted_pictures(window: &Window) -> Vec<(Bounds<Pixels>, f32)> {
    let scale = window.scale_factor();
    window
        .painted_quads()
        .iter()
        .filter_map(|quad| {
            let color = quad.background.as_solid()?;
            let marked = [
                (color.h, PICTURE_MARK.h),
                (color.s, PICTURE_MARK.s),
                (color.l, PICTURE_MARK.l),
            ]
            .iter()
            .all(|(painted, mark)| (painted - mark).abs() < 1e-4);
            let b = quad.bounds;
            marked.then(|| {
                (
                    Bounds::new(
                        gpui::point(
                            gpui::px(b.origin.x.0 / scale),
                            gpui::px(b.origin.y.0 / scale),
                        ),
                        gpui::size(
                            gpui::px(b.size.width.0 / scale),
                            gpui::px(b.size.height.0 / scale),
                        ),
                    ),
                    color.a,
                )
            })
        })
        .collect()
}

/// Whether `run` is a wrap row's marker rather than text.
#[cfg(test)]
pub(crate) fn is_marker(run: &gpui::TextRunLayout) -> bool {
    run.font_size == gpui::px(0.0) && run.text.is_empty()
}

/// Start a fresh record.
#[cfg(test)]
pub(crate) fn start() {
    RECORD.set(Some(Vec::new()));
}

/// Stop recording and return what was recorded since [`start`].
#[cfg(test)]
pub(crate) fn take() -> Vec<(Drawn, Bounds<Pixels>)> {
    RECORD.with_borrow_mut(|record| record.take().unwrap_or_default())
}

/// A row whose items move to the next line when it is too narrow for them.
///
/// An item in such a row draws its whole label: the row makes room by
/// wrapping, not by shortening its items. Only a label wider than the whole
/// row ends in an ellipsis, alone on its line.
pub trait Wraps: Styled + ParentElement + Sized {
    /// Lay the row out as a flex row whose items wrap onto further lines.
    fn wraps(self) -> Self {
        let row = self.flex().flex_wrap();
        #[cfg(test)]
        let row = row.child(
            gpui::canvas(
                |bounds, _, _| record(Drawn::WrapRow, bounds),
                |bounds, _, window, _| mark(bounds, window),
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full(),
        );
        row
    }
}

impl<E: Styled + ParentElement> Wraps for E {}

/// An image element, an [`gpui::Img`] with whatever it was given, whose
/// bounds a test reads back.
pub fn picture<E: Element + Styled>(image: E) -> Picture<E> {
    Picture(image)
}

/// An image element that records its bounds as [`Drawn::Image`].
pub struct Picture<E>(E);

impl<E> std::fmt::Debug for Picture<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Picture").finish_non_exhaustive()
    }
}

impl<E: Styled> Styled for Picture<E> {
    fn style(&mut self) -> &mut StyleRefinement {
        self.0.style()
    }
}

impl<E: Element + Styled> IntoElement for Picture<E> {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl<E: Element + Styled> Element for Picture<E> {
    type RequestLayoutState = E::RequestLayoutState;
    type PrepaintState = E::PrepaintState;

    fn id(&self) -> Option<ElementId> {
        self.0.id()
    }

    fn source_location(&self) -> Option<&'static Location<'static>> {
        self.0.source_location()
    }

    fn request_layout(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        self.0.request_layout(global_id, inspector_id, window, cx)
    }

    fn prepaint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        record(Drawn::Image, bounds);
        self.0
            .prepaint(global_id, inspector_id, bounds, request_layout, window, cx)
    }

    fn paint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.0.paint(
            global_id,
            inspector_id,
            bounds,
            request_layout,
            prepaint,
            window,
            cx,
        );
        #[cfg(test)]
        if RECORD.with_borrow(Option::is_some) {
            let opacity = self.0.style().opacity.unwrap_or(1.0);
            window.paint_quad(gpui::fill(bounds, PICTURE_MARK.opacity(opacity)));
        }
    }
}
