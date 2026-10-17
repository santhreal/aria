//! Sequence overview and zoom scroll bar along the bottom of the timeline.
//!
//! Dragging the viewport window scrolls the sequence; dragging its left or
//! right handle zooms the timeline in or out; clicking outside the window
//! centers the viewport on the clicked tick. Clip spans are coalesced in pixel
//! space so sequences with thousands of clips build a small, bounded number of
//! overview segments. Under the outline column the bar holds the add-track
//! button and the zoom readout, which fits the sequence on a click.

use std::cell::Cell;

use gpui::{
    CursorStyle, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, ParentElement, StatefulInteractiveElement, Styled, div, px,
};

use aria_models::TrackKind;

use crate::app::AppState;
use crate::fluent::FluentBuilder;
use crate::theme::Theme;
use crate::tooltip::WithTip as _;
use crate::types::UiEditorStore;

use super::ticks_to_px;

pub(super) const OVERVIEW_H: f32 = 18.0;
const HANDLE_W: f32 = 6.0;
const MAX_OVERVIEW_SPANS: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OverviewDrag {
    Pan,
    LeftHandle,
    RightHandle,
}

thread_local! {
    static OVERVIEW_DRAG: Cell<Option<OverviewDrag>> = const { Cell::new(None) };
}

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
    let w = ((view_w / full) * strip).clamp(12.0, strip.max(12.0));
    (x.clamp(0.0, (strip - w).max(0.0)), w)
}

/// Coalesce clip `[start, end]` intervals into at most `MAX_OVERVIEW_SPANS`
/// pixel intervals `(x, w)` so 10,000 clips never build 10,000 overview nodes.
pub(super) fn coalesce_overview_spans(
    store: &UiEditorStore,
    duration: i64,
    view_w: f32,
) -> Vec<(f32, f32)> {
    if duration <= 0 || view_w <= 0.0 {
        return Vec::new();
    }
    let bucket_w = (view_w / MAX_OVERVIEW_SPANS as f32).max(2.0);
    let mut buckets = [false; MAX_OVERVIEW_SPANS];
    for track in &store.sequence.tracks {
        for clip in &track.clips {
            let x0 = x_at(clip.timeline_start_ticks, duration, view_w).clamp(0.0, view_w);
            let x1 = x_at(clip.timeline_end_ticks, duration, view_w).clamp(x0, view_w);
            let b0 = ((x0 / bucket_w).floor() as usize).min(MAX_OVERVIEW_SPANS - 1);
            let b1 = ((x1 / bucket_w).ceil() as usize).min(MAX_OVERVIEW_SPANS);
            for slot in &mut buckets[b0..b1.max(b0 + 1).min(MAX_OVERVIEW_SPANS)] {
                *slot = true;
            }
        }
    }
    let mut merged = Vec::new();
    let mut idx = 0;
    while idx < MAX_OVERVIEW_SPANS {
        if !buckets[idx] {
            idx += 1;
            continue;
        }
        let start = idx;
        while idx < MAX_OVERVIEW_SPANS && buckets[idx] {
            idx += 1;
        }
        let x = start as f32 * bucket_w;
        let w = ((idx - start) as f32 * bucket_w).min(view_w - x).max(1.0);
        merged.push((x, w));
    }
    merged
}

pub(super) fn overview(
    theme: &Theme,
    store: &UiEditorStore,
    pps: f32,
    fps: f32,
    scroll_x: f32,
    view_w: f32,
    add_name: String,
    zoom: f32,
    state: &gpui::Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let outline = state.read(cx).ui_settings.outline_width;
    let duration = store.sequence.duration_ticks;
    let spans = coalesce_overview_spans(store, duration, view_w);
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
        .border_t_1()
        .border_color(border)
        .bg(theme.surface)
        .child(
            div()
                .w(px(outline))
                .h_full()
                .flex_shrink_0()
                .border_r_1()
                .border_color(border)
                .px(px(Theme::SPACE_SM))
                .flex()
                .items_center()
                .justify_between()
                .child(
                    crate::components::icon_button("add-track", crate::icons::PLUS, theme)
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                let name = add_name.clone();
                                this.state.update(cx, |state, _| {
                                    if let Err(err) = state.add_track(TrackKind::Video, name) {
                                        state.report_error(&err);
                                    }
                                });
                                cx.notify();
                            }),
                        ),
                )
                .child(
                    div()
                        .id("timeline-zoom-fit")
                        .debug_selector(|| "timeline-zoom-fit".into())
                        .text_size(Theme::TEXT_SMALL)
                        .font_family(theme.font_mono.clone())
                        .text_color(theme.text_muted)
                        .cursor(CursorStyle::PointingHand)
                        .active(|style| style.opacity(0.92))
                        .when_some(crate::tooltip::builder("fit-timeline"), |el, tip| {
                            el.with_tip(tip)
                        })
                        // The zoom is the fit control: a click on the number
                        // fits the sequence to the window.
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                this.state.update(cx, |state, _| state.zoom_fit_sequence());
                                cx.notify();
                            }),
                        )
                        .child(format!("{:.0}%", zoom * 100.0)),
                ),
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
                        OVERVIEW_DRAG.set(Some(OverviewDrag::Pan));
                        let tick = tick_at(x, view_w, duration);
                        let to = scroll_to_center(tick, fps, pps, view_w);
                        this.state.update(cx, |state, _| {
                            state.scroll_timeline_to(to, true);
                        });
                        cx.notify();
                    }),
                )
                .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                    let Some(mode) = OVERVIEW_DRAG.get() else {
                        return;
                    };
                    if !event.dragging() {
                        OVERVIEW_DRAG.set(None);
                        return;
                    }
                    let Some(left) = this.state.read(cx).timeline_content_left() else {
                        return;
                    };
                    let view_w = this.state.read(cx).timeline_viewport_px().max(1.0);
                    let norm = ((f32::from(event.position.x) - left) / view_w).clamp(0.0, 1.0);
                    this.state.update(cx, |state, _| match mode {
                        OverviewDrag::Pan => state.zoom_bar_pan(norm),
                        OverviewDrag::LeftHandle => state.zoom_bar_drag_handle(true, norm),
                        OverviewDrag::RightHandle => state.zoom_bar_drag_handle(false, norm),
                    });
                    cx.notify();
                }))
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|_, _: &MouseUpEvent, _, _| {
                        OVERVIEW_DRAG.set(None);
                    }),
                )
                .children(spans.into_iter().map(|(x, w)| {
                    div()
                        .absolute()
                        .left(px(x))
                        .top(px(2.0))
                        .h(px(OVERVIEW_H - 4.0))
                        .w(px(w))
                        .bg(raised)
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
                        .bg(wash)
                        .cursor(CursorStyle::OpenHand)
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|_, _: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                OVERVIEW_DRAG.set(Some(OverviewDrag::Pan));
                            }),
                        )
                        .child(
                            div()
                                .id("sequence-overview-handle-left")
                                .absolute()
                                .left_0()
                                .top_0()
                                .bottom_0()
                                .w(px(HANDLE_W))
                                .bg(accent)
                                .cursor(CursorStyle::ResizeLeftRight)
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|_, _: &MouseDownEvent, _, cx| {
                                        cx.stop_propagation();
                                        OVERVIEW_DRAG.set(Some(OverviewDrag::LeftHandle));
                                    }),
                                ),
                        )
                        .child(
                            div()
                                .id("sequence-overview-handle-right")
                                .absolute()
                                .right_0()
                                .top_0()
                                .bottom_0()
                                .w(px(HANDLE_W))
                                .bg(accent)
                                .cursor(CursorStyle::ResizeLeftRight)
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|_, _: &MouseDownEvent, _, cx| {
                                        cx.stop_propagation();
                                        OVERVIEW_DRAG.set(Some(OverviewDrag::RightHandle));
                                    }),
                                ),
                        ),
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
