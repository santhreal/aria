//! The Viewer's mini ruler: the whole of the tab in front, under the control
//! row. It draws the In/Out range, the sequence markers on Program, and the
//! playhead. A press puts the tab in front on the frame under the pointer; the
//! drag that follows is held on the state (see [`crate::app::viewer_scrub`]),
//! so it keeps going when the pointer leaves the strip.

use gpui::{
    Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement, Styled,
    canvas, div, px, relative,
};

use crate::app::AppState;
use crate::fluent::FluentBuilder;
use crate::theme::Theme;
use crate::types::UiMarker;

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
    markers: Option<std::sync::Arc<[UiMarker]>>,
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
        (tick as f32 / self.last as f32).clamp(0.0, 1.0)
    }
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
    let markers = span.markers.iter().flat_map(|markers| markers.iter());
    let flags = markers.map(|marker| {
        let swatch = theme.marker(marker.color);
        let left = span.at(marker.tick);
        let range = marker.duration_ticks > 0;
        div()
            .absolute()
            .top_0()
            .bottom_0()
            .left(relative(left))
            .when(range, |el| {
                let right = span.at(marker.tick.saturating_add(marker.duration_ticks));
                el.w(relative(right - left)).bg(swatch.opacity(0.42))
            })
            .when(!range, |el| el.w(px(1.)).bg(swatch))
    });
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
        .children(flags)
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
