//! Sequence overview and zoom scroll bar along the bottom of the timeline.
//!
//! The strip draws the sequence's clips as a band and the stretch on screen as
//! a thumb: a capsule with a round knob at each end that zooms the view.
//! A press anywhere on it is held on the state ([`crate::app::zoom_bar`]),
//! which reads the grip, scrolls, and zooms; the shell carries the drag past
//! the bar. A wheel turned over the strip zooms about the playhead; Shift or a
//! sideways swipe scrolls the timeline as it does anywhere on it. Clip spans
//! are coalesced in pixel space so sequences with thousands of clips build a
//! small, bounded number of overview segments. Under the outline column the
//! bar holds the add-track button and the zoom readout, which fits the
//! sequence on a click.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    CursorStyle, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    ScrollWheelEvent, StatefulInteractiveElement, Styled, div, px,
};

use aria_models::TrackKind;

use crate::app::AppState;
use crate::app::zoom_bar::{HANDLE_W, ZoomBarGrip, zoom_bar_grip_w};
use crate::motion;
use crate::theme::Theme;
use crate::tooltip::WithTip as _;
use crate::types::UiEditorStore;
use gpui::prelude::FluentBuilder;

/// Height of the footer: one 24 px control row plus the 1 px rule above it.
pub(super) const OVERVIEW_H: f32 = Theme::ICON_HIT + 1.0;
/// Inset of a footer control's label inside its hit box: the control steps
/// out by this much so its glyph stands on the column's edge, as the header's
/// timecode does.
const CONTROL_INSET: f32 = Theme::SPACE_XS;
const MAX_OVERVIEW_SPANS: usize = 64;
/// Height of the band the sequence's clips are drawn as along the zoom bar.
const CLIP_BAND_H: f32 = 4.0;
/// Top of the thumb, and of the track it rides, in the control row.
const THUMB_TOP: f32 = (Theme::ICON_HIT - HANDLE_W) / 2.0;

struct TrackSpanCache {
    clips: std::sync::Weak<Vec<crate::types::UiClip>>,
    ranges: Rc<[(i64, i64)]>,
    mask: Option<((i64, u32), u64)>,
}

thread_local! {
    static SPANS: RefCell<Option<(SpanKey, Rc<[(f32, f32)]>)>> = const { RefCell::new(None) };
    static TRACK_MASKS: RefCell<std::collections::HashMap<u64, TrackSpanCache>> =
        RefCell::new(std::collections::HashMap::new());
}

#[cfg(test)]
thread_local! {
    static OVERVIEW_CLIP_PROBES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
fn reset_overview_clip_probes() {
    OVERVIEW_CLIP_PROBES.with(|n| n.set(0));
}

#[cfg(test)]
fn overview_clip_probes() -> usize {
    OVERVIEW_CLIP_PROBES.with(std::cell::Cell::get)
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

/// Pixel of `tick` on a strip that maps `[0, duration]` onto `[0, width]`.
pub(super) fn x_at(tick: i64, duration: i64, width: f32) -> f32 {
    if duration <= 0 {
        return 0.0;
    }
    (tick as f32 / duration as f32) * width
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
    let ((window_x, window_w), clip_share) = state.read(cx).zoom_bar_thumb(scroll_x, zoom);
    let held = state.read(cx).zoom_bar_grip();
    let knob_d = zoom_bar_grip_w(window_w);
    let thumb_key = motion::MotionKey::new("zoom-bar-thumb", "");
    let thumb_fill = if held == Some(ZoomBarGrip::Thumb) {
        theme.wash(0.26)
    } else {
        motion::hover_blend(thumb_key, theme.wash(0.14), theme.wash(0.2))
    };
    let band = theme.text_faint;
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
                        .child(zoom_label(zoom)),
                ),
        )
        .child(
            div()
                .id("sequence-overview-body")
                .debug_selector(|| "sequence-overview-body".into())
                .flex_1()
                .h_full()
                .relative()
                .overflow_hidden()
                // The state reads the grip from where the press lands; the
                // shell carries the drag from here, past the bar's edges.
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, event: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        let x = f32::from(event.position.x);
                        this.state.update(cx, |state, _| state.press_zoom_bar(x));
                        this.redraw(crate::shell::Moved::View, cx);
                    }),
                )
                .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _, cx| {
                    let delta = event.delta.pixel_delta(px(crate::transport::WHEEL_LINE_PX));
                    if event.modifiers.shift || f32::from(delta.x).abs() > f32::from(delta.y).abs()
                    {
                        return;
                    }
                    cx.stop_propagation();
                    let Some(factor) = super::wheel_zoom_factor(event) else {
                        return;
                    };
                    if this
                        .state
                        .update(cx, |state, _| state.wheel_zoom_bar(factor))
                    {
                        this.redraw(crate::shell::Moved::View, cx);
                    }
                }))
                // The track the thumb rides, the clips as a band along it, and
                // the thumb over both.
                .child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .top(px(THUMB_TOP))
                        .h(px(HANDLE_W))
                        .rounded(px(HANDLE_W / 2.0))
                        .bg(theme.hairline),
                )
                .children(spans.iter().map(|&(x, w)| {
                    div()
                        .absolute()
                        .left(px(x * clip_share))
                        .top(px((Theme::ICON_HIT - CLIP_BAND_H) / 2.0))
                        .h(px(CLIP_BAND_H))
                        .w(px((w * clip_share).max(1.0)))
                        .rounded(px(CLIP_BAND_H / 2.0))
                        .bg(band)
                }))
                .child(
                    div()
                        .id("sequence-overview-window")
                        .debug_selector(|| "sequence-overview-window".into())
                        .absolute()
                        .left(px(window_x))
                        .top(px(THUMB_TOP))
                        .h(px(HANDLE_W))
                        .w(px(window_w))
                        .rounded(px(HANDLE_W / 2.0))
                        .bg(thumb_fill)
                        .on_hover(motion::hover_listener(thumb_key))
                        .cursor(CursorStyle::OpenHand)
                        .child(
                            knob(
                                "sequence-overview-handle-left",
                                knob_d,
                                held,
                                ZoomBarGrip::Start,
                                theme,
                            )
                            .left_0(),
                        )
                        .child(
                            knob(
                                "sequence-overview-handle-right",
                                knob_d,
                                held,
                                ZoomBarGrip::End,
                                theme,
                            )
                            .right_0(),
                        ),
                ),
        )
}

/// The round knob `d` pixels across at the `end` of the zoom bar's thumb:
/// muted at rest, lit under the pointer, and in the accent while a press holds
/// it.
fn knob(
    id: impl Into<motion::ControlId>,
    d: f32,
    held: Option<ZoomBarGrip>,
    end: ZoomBarGrip,
    theme: &Theme,
) -> gpui::Stateful<gpui::Div> {
    let id = id.into();
    let key = id.motion("zoom-bar-knob");
    let ink = if held == Some(end) {
        theme.accent
    } else {
        motion::hover_blend(key, theme.text_muted, theme.text)
    };
    div()
        .id(id.element("zoom-bar-knob"))
        .when_some(id.fixed_name(), |el, name| {
            el.debug_selector(move || name.to_owned())
        })
        .absolute()
        .top(px((HANDLE_W - d) / 2.0))
        .size(px(d))
        .rounded(px(d / 2.0))
        .bg(ink)
        .on_hover(motion::hover_listener(key))
        .cursor(CursorStyle::ResizeLeftRight)
}

/// The zoom readout: a whole percentage from 10% up, and two significant digits
/// below it, so the fit of a long sequence does not read as 0%.
fn zoom_label(zoom: f32) -> String {
    let percent = zoom * 100.0;
    let magnitude = percent.log10().floor() as i32;
    let decimals = 1_i32.saturating_sub(magnitude).clamp(0, 6) as usize;
    format!("{percent:.decimals$}%")
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// The readout states every zoom the timeline draws as a number that is
    /// not zero, down to the fit of a day-long sequence. The class: a whole
    /// percentage, which read 0% for any zoom under half a percent.
    #[test]
    fn the_zoom_readout_never_reads_zero() {
        for (zoom, label) in [
            (crate::transport::ZOOM_MAX, "1600%"),
            (1.0, "100%"),
            (crate::transport::ZOOM_MIN, "10%"),
            (0.05, "5.0%"),
            (0.0013, "0.13%"),
        ] {
            assert_eq!(zoom_label(zoom), label, "zoom {zoom}");
        }
        // A day of sequence across a 1000 px view.
        let mut zoom = crate::transport::ZOOM_MAX;
        while zoom > 1000.0 / (86_400.0 * crate::transport::TIMELINE_BASE_PPS) {
            let label = zoom_label(zoom);
            let read: f32 = label.trim_end_matches('%').parse().expect("a number");
            assert!(
                read > 0.0 && (read - zoom * 100.0).abs() <= zoom * 100.0 * 0.05,
                "zoom {zoom} reads {label}"
            );
            zoom /= crate::transport::ZOOM_WHEEL_STEP;
        }
    }
}
