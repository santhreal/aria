//! Sequence overview and zoom scroll bar along the bottom of the timeline.
//!
//! Dragging the viewport window scrolls the sequence; dragging its left or
//! right handle zooms the timeline in or out; clicking outside the window
//! centers the viewport on the clicked tick. Clip spans are coalesced in pixel
//! space so sequences with thousands of clips build a small, bounded number of
//! overview segments. Under the outline column the bar holds the add-track
//! button and the zoom readout, which fits the sequence on a click.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    CursorStyle, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, ParentElement, StatefulInteractiveElement, Styled, div, px,
};

use aria_models::TrackKind;

use crate::app::AppState;
use crate::fluent::FluentBuilder;
use crate::motion;
use crate::theme::Theme;
use crate::tooltip::WithTip as _;
use crate::types::UiEditorStore;

use super::ticks_to_px;

/// Height of the footer: one 24 px control row plus the 1 px rule above it.
pub(super) const OVERVIEW_H: f32 = Theme::ICON_HIT + 1.0;
/// Inset of a footer control's label inside its hit box: the control steps
/// out by this much so its glyph stands on the column's edge, as the header's
/// timecode does.
const CONTROL_INSET: f32 = Theme::SPACE_XS;
const HANDLE_W: f32 = 6.0;
const MAX_OVERVIEW_SPANS: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OverviewDrag {
    Pan,
    LeftHandle,
    RightHandle,
}

struct TrackSpanCache {
    clips: std::sync::Weak<Vec<crate::types::UiClip>>,
    ranges: Rc<[(i64, i64)]>,
    mask: Option<((i64, u32), u64)>,
}

thread_local! {
    static OVERVIEW_DRAG: Cell<Option<OverviewDrag>> = const { Cell::new(None) };
    static SPANS: RefCell<Option<(SpanKey, Rc<[(f32, f32)]>)>> = const { RefCell::new(None) };
    static TRACK_MASKS: RefCell<std::collections::HashMap<u64, TrackSpanCache>> =
        RefCell::new(std::collections::HashMap::new());
}

#[cfg(test)]
thread_local! {
    static OVERVIEW_CLIP_PROBES: Cell<usize> = const { Cell::new(0) };
}

#[cfg(test)]
fn reset_overview_clip_probes() {
    OVERVIEW_CLIP_PROBES.with(|n| n.set(0));
}

#[cfg(test)]
fn overview_clip_probes() -> usize {
    OVERVIEW_CLIP_PROBES.with(Cell::get)
}

/// What the coalesced overview spans are a function of. Clip times are written
/// only when a store is projected, so a store that kept its allocation and its
/// projection count kept its spans.
#[derive(Clone, Copy, PartialEq)]
struct SpanKey {
    store: *const UiEditorStore,
    projections: u64,
    tracks: usize,
    duration: i64,
    view_w: u32,
}

/// [`coalesce_overview_spans`] for `store`, walked again only when the store
/// was re-projected or the strip changed width: a scroll, zoom or playhead
/// frame reads the last result, and a re-projection reuses the bucket mask of
/// every lane whose `UiClips` allocation did not move.
fn overview_spans(
    store: &Arc<UiEditorStore>,
    projections: u64,
    duration: i64,
    view_w: f32,
) -> Rc<[(f32, f32)]> {
    let key = SpanKey {
        store: Arc::as_ptr(store),
        projections,
        tracks: store.sequence.tracks.len(),
        duration,
        view_w: view_w.to_bits(),
    };
    SPANS.with_borrow_mut(|memo| match memo {
        Some((held, spans)) if *held == key => spans.clone(),
        _ => {
            let spans: Rc<[(f32, f32)]> = coalesce_overview_spans(store, duration, view_w).into();
            *memo = Some((key, spans.clone()));
            spans
        }
    })
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

/// The `[start, end]` tick intervals that `clips` cover, merged where they
/// touch or overlap. `clips` is in start order, as a lane holds them.
fn coalesce_track_ranges(clips: &[crate::types::UiClip]) -> Rc<[(i64, i64)]> {
    let mut ranges: Vec<(i64, i64)> = Vec::new();
    for clip in clips {
        #[cfg(test)]
        OVERVIEW_CLIP_PROBES.with(|n| n.set(n.get() + 1));
        let start = clip.timeline_start_ticks;
        let end = clip.timeline_end_ticks.max(start);
        if let Some(last) = ranges.last_mut()
            && start <= last.1
        {
            last.1 = last.1.max(end);
        } else {
            ranges.push((start, end));
        }
    }
    ranges.into()
}

fn range_bucket_mask(ranges: &[(i64, i64)], duration: i64, view_w: f32, bucket_w: f32) -> u64 {
    let mut mask = 0u64;
    for &(start, end_tick) in ranges {
        let x0 = x_at(start, duration, view_w).clamp(0.0, view_w);
        let x1 = x_at(end_tick, duration, view_w).clamp(x0, view_w);
        let b0 = ((x0 / bucket_w).floor() as usize).min(MAX_OVERVIEW_SPANS - 1);
        let b1 = ((x1 / bucket_w).ceil() as usize).min(MAX_OVERVIEW_SPANS);
        let end = b1.max(b0 + 1).min(MAX_OVERVIEW_SPANS);
        let count = end - b0;
        let bits = if count >= 64 {
            u64::MAX
        } else {
            ((1u64 << count) - 1) << b0
        };
        mask |= bits;
    }
    mask
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
    let geom = (duration, view_w.to_bits());
    let combined = TRACK_MASKS.with_borrow_mut(|prev| {
        let mut next = std::collections::HashMap::with_capacity(store.sequence.tracks.len());
        let mut all = 0u64;
        for track in &store.sequence.tracks {
            let (weak, ranges, cached) = match prev.remove(&track.id) {
                Some(entry) if track.clips.is(&entry.clips) => {
                    (entry.clips, entry.ranges, entry.mask)
                }
                _ => (
                    track.clips.downgrade(),
                    coalesce_track_ranges(&track.clips),
                    None,
                ),
            };
            let mask = match cached {
                Some((held_geom, mask)) if held_geom == geom => mask,
                _ => range_bucket_mask(&ranges, duration, view_w, bucket_w),
            };
            all |= mask;
            next.insert(
                track.id,
                TrackSpanCache {
                    clips: weak,
                    ranges,
                    mask: Some((geom, mask)),
                },
            );
        }
        *prev = next;
        all
    });
    let mut merged = Vec::new();
    let mut idx = 0;
    while idx < MAX_OVERVIEW_SPANS {
        if (combined & (1u64 << idx)) == 0 {
            idx += 1;
            continue;
        }
        let start = idx;
        while idx < MAX_OVERVIEW_SPANS && (combined & (1u64 << idx)) != 0 {
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
    store: &Arc<UiEditorStore>,
    pps: f32,
    fps: f32,
    scroll_x: f32,
    view_w: f32,
    add_name: String,
    zoom: f32,
    state: &gpui::Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let outline = state.read(cx).outline_drawn_width();
    let duration = store.sequence.duration_ticks;
    let spans = overview_spans(store, state.read(cx).projections, duration, view_w);
    let full_px = ticks_to_px(duration, fps, pps).max(1.0);
    let (window_x, window_w) = window_on_strip(scroll_x, full_px, view_w);
    let accent = theme.accent;
    let raised = theme.surface_raised;
    let wash = theme.wash(0.35);
    let border = theme.border;
    let fit_key = motion::MotionKey::new("timeline-zoom-fit", "");

    div()
        .id("sequence-overview")
        .debug_selector(|| "sequence-overview".into())
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
                        .debug_selector(|| "add-track".into())
                        .ml(px(-CONTROL_INSET))
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
                        .h(px(Theme::ICON_HIT))
                        .mr(px(-CONTROL_INSET))
                        .px(px(CONTROL_INSET))
                        .flex()
                        .items_center()
                        .rounded(px(Theme::SPACE_XS))
                        .text_size(Theme::TEXT_SMALL)
                        .text_color(motion::hover_blend(fit_key, theme.text_muted, theme.text))
                        .bg(motion::hover_blend(
                            fit_key,
                            theme.wash(0.0),
                            theme.element_hover,
                        ))
                        .on_hover(motion::hover_listener(fit_key))
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
                        this.redraw(crate::shell::Moved::View, cx);
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
                    this.redraw(crate::shell::Moved::View, cx);
                }))
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|_, _: &MouseUpEvent, _, _| {
                        OVERVIEW_DRAG.set(None);
                    }),
                )
                .children(spans.iter().map(|&(x, w)| {
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

    /// WHY: `coalesce_overview_spans` previously walked every clip on every
    /// track whenever `projections` advanced. This test closes that class by
    /// verifying that when one lane's `UiClips` allocation is replaced and the
    /// remaining lanes keep their shared `UiClips`, only the changed lane's
    /// clips are inspected.
    ///
    /// Does NOT catch: layout width changes (`view_w`), which intentionally
    /// recompute bucket masks for the new strip width.
    #[test]
    fn coalesce_overview_spans_reuses_unchanged_lane_masks() {
        let mut state = crate::app::premiere_tests::with_sequence(3, 15);
        let last = state.store.sequence.tracks[0]
            .clips
            .last()
            .expect("tail clip");
        let clip = aria_models::ClipId::from_raw(last.id);
        let start = last.timeline_start_ticks;
        let dur = state.store.sequence.duration_ticks;
        let _ = overview_spans(&state.store, state.projections, dur, 320.0);

        state
            .apply_command(aria_commands::Command::Edit(
                aria_commands::EditCommand::TrimClip {
                    clip,
                    timeline: aria_models::TimeRange::at_rate(
                        start,
                        12,
                        aria_models::Rational::FPS_24,
                    ),
                },
            ))
            .unwrap();

        reset_overview_clip_probes();
        let next_dur = state.store.sequence.duration_ticks;
        let spans = overview_spans(&state.store, state.projections, next_dur, 320.0);
        assert!(!spans.is_empty());
        assert_eq!(
            overview_clip_probes(),
            15,
            "expected only the 15 clips on the modified lane to be scanned, got {}",
            overview_clip_probes()
        );
    }
}
