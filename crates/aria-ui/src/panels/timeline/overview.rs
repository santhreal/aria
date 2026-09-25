//! Sequence overview under the ruler: the whole cut in one strip, click to pan.
//!
//! The ruler is already at the file ceiling. Mapping lives here so a click and
//! a draw cannot disagree about where a tick sits on the strip.

use gpui::{
    InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    StatefulInteractiveElement, Styled, div, px,
};

use crate::app::AppState;
use crate::theme::Theme;
use crate::types::UiEditorStore;

use super::{HEADER_W, ticks_to_px};

pub(super) const OVERVIEW_H: f32 = 12.0;

/// Tick under `x` on a strip that maps `[0, duration]` onto `[0, width]`.
pub(super) fn tick_at(x: f32, width: f32, duration: i64) -> i64 {
    if width <= 0.0 || duration <= 0 {
        return 0;
    }
    let tick = ((x / width) * duration as f32).floor() as i64;
    tick.clamp(0, duration)
}

/// Pixel of `tick` on the same strip.
pub(super) fn x_at(tick: i64, duration: i64, width: f32) -> f32 {
    if duration <= 0 {
        return 0.0;
    }
    (tick as f32 / duration as f32) * width
}

/// Scroll offset that puts `tick` in the middle of the timeline body.
pub(super) fn scroll_to_center(tick: i64, fps: f32, pps: f32, view_w: f32) -> f32 {
    (ticks_to_px(tick, fps, pps) - view_w * 0.5).max(0.0)
}

/// Pixel window of the zoomed timeline on a strip that maps the whole cut.
pub(super) fn window_on_strip(scroll_x: f32, full_px: f32, view_w: f32) -> (f32, f32) {
    let strip = view_w.max(1.0);
    let full = full_px.max(1.0);
    let x = (scroll_x / full) * strip;
    let w = ((view_w / full) * strip).clamp(2.0, strip.max(2.0));
    (x.max(0.0), w)
}

pub(super) fn overview(
    theme: &Theme,
    store: &UiEditorStore,
    pps: f32,
    fps: f32,
    scroll_x: f32,
    view_w: f32,
    _state: &gpui::Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let duration = store.sequence.duration_ticks;
    let spans: Vec<(i64, i64)> = store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| {
            track
                .clips
                .iter()
                .map(|clip| (clip.timeline_start_ticks, clip.timeline_end_ticks))
        })
        .collect();
    let full_px = ticks_to_px(duration, fps, pps).max(1.0);
    let (window_x, window_w) = window_on_strip(scroll_x, full_px, view_w);
    let accent = theme.accent;
    let raised = theme.surface_raised;
    let wash = theme.wash(0.35);
    let border = theme.border;

    div()
        .id("sequence-overview")
        .h(px(OVERVIEW_H))
        .w_full()
        .flex()
        .flex_row()
        .border_b_1()
        .border_color(border)
        .bg(theme.surface)
        .child(
            div()
                .w(px(HEADER_W))
                .h_full()
                .border_r_1()
                .border_color(border),
        )
        .child(
            div()
                .id("sequence-overview-body")
                .flex_1()
                .h_full()
                .relative()
                .overflow_hidden()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        let Some(left) = this.state.read(cx).timeline_content_left() else {
                            return;
                        };
                        let view_w = this.state.read(cx).timeline_viewport_px();
                        let x = f32::from(event.position.x) - left;
                        if x < 0.0 {
                            return;
                        }
                        let tick = tick_at(x, view_w, duration);
                        let to = scroll_to_center(tick, fps, pps, view_w);
                        this.state.update(cx, |state, _| {
                            state.scroll_timeline_to(to, true);
                        });
                        cx.notify();
                    }),
                )
                .children(spans.into_iter().filter_map(|(start, end)| {
                    let x = x_at(start, duration, view_w);
                    let w = (x_at(end, duration, view_w) - x).max(1.0);
                    if w < 0.5 {
                        return None;
                    }
                    Some(
                        div()
                            .absolute()
                            .left(px(x))
                            .top(px(2.0))
                            .h(px(OVERVIEW_H - 4.0))
                            .w(px(w))
                            .bg(raised),
                    )
                }))
                .child(
                    div()
                        .id("sequence-overview-window")
                        .absolute()
                        .left(px(window_x))
                        .top(px(0.0))
                        .h_full()
                        .w(px(window_w))
                        .border_1()
                        .border_color(accent)
                        .bg(wash),
                ),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_click_halfway_along_a_minute_lands_on_the_midpoint() {
        assert_eq!(tick_at(100.0, 200.0, 1800), 900);
    }

    #[test]
    fn a_click_past_the_end_clamps() {
        assert_eq!(tick_at(400.0, 200.0, 1800), 1800);
        assert_eq!(tick_at(-8.0, 200.0, 1800), 0);
    }

    #[test]
    fn an_empty_or_sizeless_strip_stays_at_zero() {
        assert_eq!(tick_at(10.0, 200.0, 0), 0);
        assert_eq!(tick_at(10.0, 0.0, 1800), 0);
    }

    #[test]
    fn scroll_centering_puts_the_tick_in_the_middle() {
        let mid = scroll_to_center(50, 24.0, 50.0, 200.0);
        assert!((mid - (ticks_to_px(50, 24.0, 50.0) - 100.0)).abs() < 0.01);
        assert_eq!(scroll_to_center(0, 24.0, 50.0, 200.0), 0.0);
    }

    #[test]
    fn the_viewport_window_is_half_the_strip_when_zoomed_two_to_one() {
        let (x, w) = window_on_strip(0.0, 400.0, 200.0);
        assert!((x - 0.0).abs() < 0.01);
        assert!((w - 100.0).abs() < 0.01);
        let (x, w) = window_on_strip(200.0, 400.0, 200.0);
        assert!((x - 100.0).abs() < 0.01);
        assert!((w - 100.0).abs() < 0.01);
    }
}
