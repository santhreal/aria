//! The Viewer's mini ruler: the whole of the tab in front, under the control
//! row. It draws the In/Out range, the sequence markers on Program, and the
//! playhead. A press puts the tab in front on the frame under the pointer; the
//! drag that follows is held on the state (see [`crate::app::viewer_scrub`]),
//! so it keeps going when the pointer leaves the strip.

use std::sync::Arc;

use gpui::{
    Bounds, Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    Styled, canvas, div, fill, point, px, relative, size,
};

use crate::app::AppState;
use crate::fluent::FluentBuilder;
use crate::theme::Theme;
use crate::types::UiMarker;
use aria_models::MarkerColor;

#[cfg(test)]
#[path = "mini_ruler_tests.rs"]
mod tests;

/// The strip's height.
const HEIGHT: f32 = 12.0;

/// What the strip shows, read once per frame.
struct Span {
    /// The tick at the right edge: the sequence end on Program, the take's
    /// last frame on a Source tab. Never zero, so it divides.
    last: i64,
    /// The playhead of the tab in front.
    tick: i64,
    /// The In/Out range, while either mark is set.
    marks: Option<(i64, i64)>,
    /// The sequence markers on Program, none on a Source tab.
    markers: Option<Arc<[UiMarker]>>,
}

impl Span {
    fn of(state: &AppState) -> Self {
        match state.sources.focused() {
            Some(view) => Self {
                last: view.duration_ticks.saturating_sub(1).max(1),
                tick: view.tick,
                marks: (view.mark_in.is_some() || view.mark_out.is_some()).then(|| view.window()),
                markers: None,
            },
            None => Self {
                last: state.store.sequence.duration_ticks.max(1),
                tick: state.playhead.value,
                marks: (state.mark_in.is_some() || state.mark_out.is_some())
                    .then(|| state.work_area()),
                markers: Some(state.store.sequence.markers.clone()),
            },
        }
    }

    /// Where `tick` sits across the strip, from 0 at the left edge to 1 at
    /// the right.
    fn at(&self, tick: i64) -> f32 {
        fraction(tick, self.last)
    }
}

/// Where `tick` sits across a strip ending at `last`, from 0 at the left edge
/// to 1 at the right.
fn fraction(tick: i64, last: i64) -> f32 {
    (tick as f32 / last as f32).clamp(0.0, 1.0)
}

/// The point markers as runs of one colour, `(x, width, colour)` in window
/// pixels, left to right.
///
/// A flag is one pixel wide and the strip maps the whole sequence across its
/// width, so it shows at most one flag per pixel column. Each marker lands in
/// the column under its tick, the last one there gives the column its colour,
/// as the flag painted on top did, and neighbouring columns of one colour join.
/// The runs are bounded by the width in pixels whatever the number of markers.
/// `markers` is in tick order, as the sequence holds them.
fn flag_runs(
    markers: &[UiMarker],
    last: i64,
    left: f32,
    width: f32,
    mut emit: impl FnMut(f32, f32, MarkerColor),
) {
    let mut run: Option<(i32, i32, MarkerColor)> = None;
    let mut settle =
        |column: i32, color: MarkerColor, run: &mut Option<(i32, i32, MarkerColor)>| match run {
            Some((_, end, open)) if *open == color && *end + 1 == column => *end = column,
            _ => {
                if let Some((start, end, open)) = run.replace((column, column, color)) {
                    emit(start as f32, (end - start + 1) as f32, open);
                }
            }
        };
    let mut column: Option<(i32, MarkerColor)> = None;
    for marker in markers.iter().filter(|marker| marker.duration_ticks <= 0) {
        let at = (left + fraction(marker.tick, last) * width).floor() as i32;
        match column {
            Some((open, _)) if open == at => column = Some((at, marker.color)),
            _ => {
                if let Some((done, color)) = column.replace((at, marker.color)) {
                    settle(done, color, &mut run);
                }
            }
        }
    }
    if let Some((done, color)) = column {
        settle(done, color, &mut run);
    }
    if let Some((start, end, open)) = run {
        emit(start as f32, (end - start + 1) as f32, open);
    }
}

/// The sequence markers, painted in one pass over the strip.
///
/// Range markers paint first, one translucent quad each, then the point
/// markers as [`flag_runs`], so a flag inside a range stays on top of it. The
/// row paints in a layer of its own, so its quads do not grow the index the
/// rest of the Viewer is ranked in.
fn marker_flags(markers: Option<Arc<[UiMarker]>>, last: i64) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, (), window, cx| {
            let Some(markers) = markers.as_deref() else {
                return;
            };
            let theme = Theme::of(cx);
            let left = f32::from(bounds.origin.x);
            let width = f32::from(bounds.size.width);
            let quad = |x: f32, w: f32| {
                Bounds::new(
                    point(px(x), bounds.origin.y),
                    size(px(w), bounds.size.height),
                )
            };
            window.paint_layer(bounds, |window| {
                for marker in markers.iter().filter(|marker| marker.duration_ticks > 0) {
                    let start = left + fraction(marker.tick, last) * width;
                    let end = left
                        + fraction(marker.tick.saturating_add(marker.duration_ticks), last) * width;
                    window.paint_quad(fill(
                        quad(start, end - start),
                        theme.marker(marker.color).opacity(0.42),
                    ));
                }
                flag_runs(markers, last, left, width, |x, w, color| {
                    window.paint_quad(fill(quad(x, w), theme.marker(color)));
                });
            });
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

pub(super) fn strip(
    state: &Entity<AppState>,
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let span = Span::of(state.read(cx));
    // A mouse move carries no element bounds, so the strip records where it
    // was painted for the press and the drag to map the pointer through.
    let measure = {
        let state = state.clone();
        canvas(
            move |bounds, _, cx| {
                let left = f32::from(bounds.origin.x);
                let width = f32::from(bounds.size.width);
                state.update(cx, |state, _| state.record_viewer_ruler(left, width));
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full()
    };
    let flags = marker_flags(span.markers.clone(), span.last);
    div()
        .id("viewer-ruler")
        .debug_selector(|| "viewer-ruler".into())
        .relative()
        .flex_none()
        .w_full()
        .h(px(HEIGHT))
        .bg(theme.element_active)
        .cursor(gpui::CursorStyle::ResizeLeftRight)
        .child(measure)
        .when_some(span.marks, |el, (start, end)| {
            let left = span.at(start);
            el.child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(relative(left))
                    .w(relative(span.at(end) - left))
                    .bg(theme.accent.opacity(0.35)),
            )
        })
        .child(flags)
        .child(
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .left(relative(span.at(span.tick)))
                .w(px(2.))
                .bg(theme.playhead),
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, event: &MouseDownEvent, _, cx| {
                let x = f32::from(event.position.x);
                // The Viewer's own press handler, which runs next, makes it
                // the active panel and renders the frame.
                this.state
                    .update(cx, |state, _| state.press_viewer_ruler(x));
                cx.notify();
            }),
        )
}
