//! Bringing an element under the inspector's body into view, and the body
//! back to its top when the tab changes.
//!
//! `ScrollHandle::scroll_to_item` reaches only the children of the element that
//! scrolls, and a block sits several elements below the body. [`mark`] reaches
//! any element: drawn over it, the mark's first prepaint after
//! [`Reveal::request`] reads where the element landed, moves the body so the
//! element is in view, and spends the request. The frame after draws the move.

use gpui::{
    Bounds, ElementId, IntoElement, Pixels, Point, SharedString, Styled, canvas, point, px,
};

use crate::app::Reveal;

/// An element covering its parent, which must be `relative`, that brings the
/// parent into view of `reveal`'s frame on its first prepaint after a request.
pub(super) fn mark(reveal: &Reveal) -> impl IntoElement {
    let reveal = reveal.clone();
    canvas(
        move |bounds, window, _| {
            if reveal.take() {
                let frame = &reveal.frame;
                frame.set_offset(in_view(
                    frame.bounds(),
                    bounds,
                    frame.offset(),
                    frame.max_offset(),
                ));
                // A refresh during a draw is dropped; a requested frame is not.
                window.request_animation_frame();
            }
        },
        |_, (), _, _| {},
    )
    .absolute()
    .inset_0()
}

/// A zero-size element, drawn ahead of `reveal`'s frame, that puts the frame
/// back at its top on the first prepaint that draws a tab other than the one
/// the last prepaint drew. The frame's handle outlives the tab, so a tab would
/// otherwise open at the depth the previous one was scrolled to. Drawn before
/// the frame, the move lands on the frame that draws the new tab, and a reveal
/// requested with the switch moves the frame after it.
pub(super) fn top_on_switch(reveal: &Reveal, tab: SharedString) -> impl IntoElement + use<> {
    let frame = reveal.frame.clone();
    canvas(
        move |_, window, _| {
            let key = ElementId::Name("inspector-shown-tab".into());
            let switched = window.with_global_id(key, |id, window| {
                window.with_element_state(id, |shown: Option<SharedString>, _| {
                    (shown.as_ref() != Some(&tab), tab)
                })
            });
            if switched {
                frame.set_offset(Point::default());
            }
        },
        |_, (), _, _| {},
    )
    .absolute()
}

/// The offset that shows `target` in `view`, from `offset`, with the content
/// scrolling at most `max`. Both bounds are where the last layout put them, and
/// an offset grows negative as the content moves up. A target in view keeps
/// the offset; any other is brought to the top of the view, or as near the top
/// as the content scrolls.
fn in_view(
    view: Bounds<Pixels>,
    target: Bounds<Pixels>,
    offset: Point<Pixels>,
    max: Point<Pixels>,
) -> Point<Pixels> {
    if target.top() >= view.top() && target.bottom() <= view.bottom() {
        return offset;
    }
    let y = offset.y - (target.top() - view.top());
    point(offset.x, y.max(-max.y).min(px(0.0)))
}

#[cfg(test)]
mod tests {
    //! The move [`in_view`] makes, at each place a target can be.
    //!
    //! Closes the class "a revealed element is left out of view, or a visible
    //! one is moved": a target below the view, above it, past the content's
    //! end, taller than the view, and already in view, from a body at rest and
    //! from one already scrolled. Not covered: the frame the move is drawn on,
    //! which `transition_reveal_tests` reads through a drawn window.

    use gpui::{Bounds, Pixels, Point, point, px, size};

    use super::in_view;

    /// A body 300 px tall at y 100 whose content scrolls at most 900 px.
    const VIEW_TOP: f32 = 100.0;
    const VIEW_H: f32 = 300.0;
    const MAX: f32 = 900.0;

    fn view() -> Bounds<Pixels> {
        Bounds::new(point(px(0.0), px(VIEW_TOP)), size(px(320.0), px(VIEW_H)))
    }

    /// A target `height` px tall whose top the last layout put at window y
    /// `top`.
    fn target(top: f32, height: f32) -> Bounds<Pixels> {
        Bounds::new(point(px(12.0), px(top)), size(px(296.0), px(height)))
    }

    /// The offset `in_view` sets from a body scrolled `scrolled` px down.
    fn moved(scrolled: f32, top: f32, height: f32) -> f32 {
        let offset = point(px(7.0), px(-scrolled));
        let max = point(px(0.0), px(MAX));
        let after: Point<Pixels> = in_view(view(), target(top, height), offset, max);
        assert_eq!(after.x, px(7.0), "the reveal moved the body sideways");
        -f32::from(after.y)
    }

    #[test]
    fn a_target_out_of_view_is_brought_to_the_top() {
        // (scrolled, target top, target height, scrolled after)
        let cases: [(f32, f32, f32, f32); 5] = [
            // Below the view, the body at rest: the target's top meets the view's.
            (0.0, 700.0, 120.0, 600.0),
            // Below the view, the body already scrolled 200 px.
            (200.0, 700.0, 120.0, 800.0),
            // Straddling the bottom edge.
            (0.0, 350.0, 120.0, 250.0),
            // Above the view: the body scrolls back up to it.
            (500.0, 40.0, 120.0, 440.0),
            // Taller than the view: its top is what is shown.
            (0.0, 500.0, 800.0, 400.0),
        ];
        for (scrolled, top, height, want) in cases {
            assert_eq!(
                moved(scrolled, top, height),
                want,
                "a {height} px target at y {top} with the body scrolled {scrolled} px"
            );
        }
    }

    #[test]
    fn a_reveal_stops_at_either_end_of_the_content() {
        // Past the end: the body scrolls as far as the content goes.
        assert_eq!(moved(0.0, 1_500.0, 40.0), MAX);
        // Above the start: the body returns to rest and no further.
        assert_eq!(moved(30.0, 0.0, 40.0), 0.0);
    }

    #[test]
    fn a_target_in_view_leaves_the_body_where_it_is() {
        for (scrolled, top, height) in [
            (0.0, VIEW_TOP, 40.0),
            (0.0, VIEW_TOP + VIEW_H - 40.0, 40.0),
            (250.0, 180.0, 120.0),
            (0.0, VIEW_TOP, VIEW_H),
        ] {
            assert_eq!(
                moved(scrolled, top, height),
                scrolled,
                "a {height} px target at y {top} in view moved the body"
            );
        }
    }
}
