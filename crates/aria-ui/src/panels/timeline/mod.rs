//! Timeline: header, split video/audio lane groups, clip bodies, and the zoom
//! scroll bar.
//!
//! This module owns the panel's frame — geometry constants, tick/pixel
//! conversions, the ruler's tick spacing, and scroll/zoom handlers — and
//! composes the header (timecode, search and toggles over the outline column;
//! ruler, work area and render bar over the time area), the video lane group,
//! the draggable A/V divider, the audio lane group, and the zoom scroll bar
//! along the bottom.

mod clip;
mod drop;
mod graph;
mod header;
mod headers;
mod lanes;
mod media;
mod overview;
mod property_rows;
mod render_bar;
pub(crate) mod ruler;
mod sides;

#[cfg(test)]
mod empty_timeline_tests;
#[cfg(test)]
#[path = "offscreen_tests.rs"]
mod offscreen_tests;

use gpui::prelude::FluentBuilder;
use gpui::{
    CursorStyle, Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    ParentElement, ScrollDelta, ScrollWheelEvent, Styled, div, px,
};

use crate::app::AppState;
use crate::motion::glide;
use crate::theme::Theme;
use crate::timeline_playhead;
use crate::types::ActivePanel;

use lanes::lane_group;
use overview::overview;
use ruler::ruler;

/// Height of the timeline header: two 24 px rows over the outline column, and
/// over the time area the ruler's ticks, the work area bar under them, and the
/// render bar along the bottom; plus the 1 px rule under the header.
pub(super) const RULER_H: f32 = 2.0 * header::HEADER_ROW_H + 1.0;
/// Height of the ruler's ticks and labels at the top of the header.
pub(super) const RULER_TICKS_H: f32 = 30.0;
/// Height of a lane group that has no lanes: one empty-lane strip that names
/// the missing side, so the other side takes the panel.
pub(super) const EMPTY_SIDE_H: f32 = Theme::ROW_HEIGHT;
pub(super) const AV_DIVIDER_H: f32 = 6.0;
/// Width of the grab band on the outline column's right border. The band
/// ends at the border, so the first pixel of the content is the content's.
const OUTLINE_EDGE_W: f32 = 6.0;
pub(super) const BASE_PPS: f32 = crate::transport::TIMELINE_BASE_PPS;
/// Pixels per waveform bar.
pub(super) const BAR_STEP: f32 = 3.0;
/// Width of a filmstrip cell: a 16:9 frame at the cell height.
pub(super) const CELL_W: f32 = 60.0;
/// Height of a collapsed lane: enough to read the cut pattern, not enough for
/// a clip label.
pub(super) const COLLAPSED_H: f32 = 20.0;

/// The spec the drawn scroll offset travels on: the release spec after a pan
/// was let go, and otherwise the zoom's, so a tick held in place across an
/// eased zoom (the playhead under `=`) stays put while both travel.
fn scroll_spec(fling: bool) -> gpui::motion::MotionSpec {
    if fling { glide::FLING } else { glide::ZOOM }
}

pub fn render(
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let theme = Theme::of(cx);
    let store = state.read(cx).store.clone();
    let reduced = cx.reduce_motion();
    let zoom = glide::glide(
        crate::motion::MotionKey::new("timeline-zoom", ""),
        store.zoom,
        glide::ZOOM,
        reduced || !state.read(cx).zoom_eased,
    );
    let scroll_x = glide::glide(
        crate::motion::MotionKey::new("timeline-scroll", ""),
        store.scroll_x.max(0.0),
        scroll_spec(state.read(cx).scroll_fling),
        reduced || !state.read(cx).scroll_eased,
    );
    let pps = BASE_PPS * zoom;
    let outline = state.read(cx).outline_drawn_width();
    let fps = store.sequence.ticks_per_second();
    let (
        snap_guide,
        visible_width,
        tool,
        track_dragging,
        drop_row,
        av_split,
        video_scroll_y,
        audio_scroll_y,
        panel_h,
    ) = {
        let app = state.read(cx);
        let h = app.timeline_frame.map(|f| f[3]).unwrap_or(320.0);
        (
            app.snap_guide,
            app.timeline_viewport_px(),
            app.tool,
            app.track_dragging(),
            app.track_drop_row,
            app.timeline_av_split,
            app.video_scroll_y,
            app.audio_scroll_y,
            h,
        )
    };
    let view_start = ((scroll_x / pps.max(1.0)) * fps).floor() as i64;
    let view_end = (((scroll_x + visible_width) / pps.max(1.0)) * fps).ceil() as i64;
    let next_video = store
        .sequence
        .tracks
        .iter()
        .filter(|t| matches!(t.kind, crate::types::UiTrackKind::Video))
        .count()
        + 1;
    let add_name = format!("V{next_video}");
    let no_clips = store
        .sequence
        .tracks
        .iter()
        .all(|track| track.clips.is_empty());
    let cursor = tool.cursor();
    let total_rows = store.sequence.tracks.len();
    let visual_rows = state.read(cx).visual_lane_count();
    let graph_open = state.read(cx).timeline_graph_open();
    // The layer paints the playhead over the body unless the body draws
    // something that reads it or covers its line; then the body draws it.
    let layered = timeline_playhead::layered(state.read(cx));
    state
        .read(cx)
        .timeline_playhead
        .begin(layered, timeline_playhead::View { pps, fps, scroll_x });
    let playhead_at = (!layered).then(|| state.read(cx).store.playhead_ticks as f32);
    let graph_h = if graph_open {
        crate::app::property_rows::GRAPH_EDITOR_H
    } else {
        0.0
    };
    // `panel_h` is the panel inside its top border. The sides share what the
    // header, the two drop strips, the divider, the graph editor and the
    // overview leave of it; a frame too short for those leaves the sides
    // nothing, never a stack taller than the panel.
    let stack_h =
        (panel_h - RULER_H - 2.0 * Theme::SPACE_SM - AV_DIVIDER_H - graph_h - overview::OVERVIEW_H)
            .max(0.0);
    // A frame too short for any of the stack puts the graph editor straight
    // under the ruler, and its top rule would stand on the ruler's bottom rule
    // as one doubled line. The ruler's rule then separates the two alone.
    let graph_ruled = panel_h - RULER_H - graph_h - overview::OVERVIEW_H >= 2.0;
    let (video_side, audio_side) = sides::sides(
        visual_rows,
        total_rows.saturating_sub(visual_rows),
        av_split,
    );
    let video_viewport_h = video_side.viewport(stack_h);
    let audio_viewport_h = audio_side.viewport(stack_h);

    let video_group = lane_group(
        &theme,
        state,
        &store,
        0..visual_rows,
        false,
        true,
        video_scroll_y,
        video_viewport_h,
        track_dragging,
        drop_row,
        pps,
        fps,
        scroll_x,
        view_start,
        view_end,
        playhead_at,
        snap_guide,
        reduced,
        cx,
    );
    let audio_group = lane_group(
        &theme,
        state,
        &store,
        visual_rows..total_rows,
        true,
        false,
        audio_scroll_y,
        audio_viewport_h,
        track_dragging,
        drop_row,
        pps,
        fps,
        scroll_x,
        view_start,
        view_end,
        playhead_at,
        snap_guide,
        reduced,
        cx,
    );

    let one_sided = video_side.is_empty() || audio_side.is_empty();

    div()
        .id("timeline")
        .size_full()
        .relative()
        .flex()
        .flex_col()
        .bg(theme.surface)
        .cursor(cursor)
        .on_scroll_wheel(cx.listener(move |this, event: &ScrollWheelEvent, _, cx| {
            handle_scroll(this, event, cx);
        }))
        .on_mouse_down(
            MouseButton::Middle,
            cx.listener(|this, event: &MouseDownEvent, _, cx| {
                this.start_pan(f32::from(event.position.x), MouseButton::Middle, cx);
            }),
        )
        .child(measure(state))
        .child(ruler(
            &theme,
            &store.sequence,
            playhead_at,
            pps,
            fps,
            scroll_x,
            visible_width,
            view_start,
            view_end,
            snap_guide,
            state,
            cx,
        ))
        .child(
            div()
                .id("timeline-stack")
                .flex_1()
                .min_h_0()
                .w_full()
                .flex()
                .flex_col()
                .relative()
                .overflow_hidden()
                .child(drop::edge_strip(
                    &theme, true, scroll_x, outline, fps, pps, cx,
                ))
                .child(
                    sides::frame(video_viewport_h)
                        .child(video_group)
                        .when(video_side.is_empty(), |el| {
                            el.child(sides::empty_label(&theme, false))
                        }),
                )
                .child(av_divider(
                    &theme,
                    outline,
                    (!one_sided).then_some(stack_h),
                    cx,
                ))
                .child(
                    sides::frame(audio_viewport_h)
                        .child(audio_group)
                        .when(audio_side.is_empty(), |el| {
                            el.child(sides::empty_label(&theme, true))
                        }),
                )
                .child(drop::edge_strip(
                    &theme, false, scroll_x, outline, fps, pps, cx,
                ))
                .when(no_clips, |el| {
                    el.child(
                        div()
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .left(px(outline))
                            .right_0()
                            .flex()
                            .child(crate::components::empty_state(
                                "timeline",
                                "Drag media here to start.",
                                &theme,
                            )),
                    )
                }),
        )
        .when_some(playhead_at.filter(|_| graph_open), |el, playhead_at| {
            el.child(graph::render_graph_editor(
                &theme,
                state,
                pps,
                fps,
                scroll_x,
                playhead_at,
                graph_ruled,
                cx,
            ))
        })
        .child(overview(
            &theme,
            &store,
            scroll_x,
            visible_width,
            add_name,
            zoom,
            state,
            cx,
        ))
        .child(outline_edge(outline, cx))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, event: &MouseDownEvent, _, cx| {
                let tool = this.state.read(cx).tool;
                if tool.pans() {
                    this.start_pan(f32::from(event.position.x), MouseButton::Left, cx);
                } else if tool.zooms() {
                    let factor = if event.modifiers.alt {
                        1.0 / crate::transport::ZOOM_STEP
                    } else {
                        crate::transport::ZOOM_STEP
                    };
                    this.zoom_at(f32::from(event.position.x), factor, cx);
                }
                this.state
                    .update(cx, |state, _| state.active_panel = ActivePanel::Timeline);
            }),
        )
}

/// The grab band on the outline column's right border, inside the column. It
/// spans the timeline's full height, so the ruler, the lanes, and the graph
/// editor resize together.
fn outline_edge(outline: f32, cx: &mut gpui::Context<crate::shell::Shell>) -> impl IntoElement {
    div()
        .id("timeline-outline-edge")
        .absolute()
        .top_0()
        .bottom_0()
        .left(px(outline - OUTLINE_EDGE_W))
        .w(px(OUTLINE_EDGE_W))
        .cursor(CursorStyle::ResizeLeftRight)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                this.start_outline_resize(f32::from(event.position.x), outline);
            }),
        )
}

/// The bar between the video and audio sides. While both sides have lanes it
/// drags the split of `stack_h`, the height the sides share; with one side
/// empty there is no split and it is a rule. Its fill is its edge: the lane
/// above ends in its own rule, and a second one here would double it.
fn av_divider(
    theme: &Theme,
    outline: f32,
    stack_h: Option<f32>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    div()
        .id("timeline-av-divider")
        .debug_selector(|| "timeline-av-divider".into())
        .w_full()
        .h(px(AV_DIVIDER_H))
        .flex_none()
        .bg(theme.surface_raised)
        .when(stack_h.is_some(), |el| el.cursor(CursorStyle::ResizeUpDown))
        .flex()
        .items_center()
        // The bar is 6 px high, too short for a label; the rule marks the
        // outline column's edge.
        .child(
            div()
                .w(px(outline))
                .h_full()
                .border_r_1()
                .border_color(theme.border),
        )
        .when_some(stack_h, |el, stack_h| {
            el.on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    let split = this.state.read(cx).timeline_av_split;
                    this.start_av_split(f32::from(event.position.y), split, stack_h);
                }),
            )
        })
}

/// The zoom factor a wheel turn asks for: [`crate::transport::ZOOM_WHEEL_STEP`]
/// for each notch turned up or away, its inverse for each notch turned down or
/// toward, as [`crate::transport::wheel_notches`] counts them. `None` for a
/// turn that travels nowhere.
pub(crate) fn wheel_zoom_factor(event: &ScrollWheelEvent) -> Option<f32> {
    let notches = crate::transport::wheel_notches(event);
    (notches != 0.0).then(|| crate::transport::ZOOM_WHEEL_STEP.powf(notches))
}

fn handle_scroll(
    this: &mut crate::shell::Shell,
    event: &ScrollWheelEvent,
    cx: &mut gpui::Context<crate::shell::Shell>,
) {
    if event.modifiers.control || event.modifiers.platform || event.modifiers.alt {
        if let Some(factor) = wheel_zoom_factor(event) {
            this.zoom_at(f32::from(event.position.x), factor, cx);
        }
        return;
    }
    let delta = event.delta.pixel_delta(px(crate::transport::WHEEL_LINE_PX));
    let dx = f32::from(delta.x);
    let dy = f32::from(delta.y);
    let trackpad = matches!(event.delta, ScrollDelta::Pixels(_));
    let shift = event.modifiers.shift;
    let scroll_dx = if shift {
        if dx.abs() > dy.abs() { dx } else { dy }
    } else if trackpad && dx.abs() > 0.0 {
        dx
    } else {
        return;
    };
    // The content moves with the wheel as in every scrolled list: a turn down
    // or a swipe to the left shows later time.
    this.state.update(cx, |state, _| {
        let next = state.scroll_x - scroll_dx;
        state.scroll_timeline_to(next, false);
    });
    this.redraw(crate::shell::Moved::View, cx);
}

fn measure(state: &Entity<AppState>) -> impl IntoElement {
    let state = state.clone();
    gpui::canvas(
        move |bounds, _, cx| {
            let frame = [
                f32::from(bounds.origin.x),
                f32::from(bounds.origin.y),
                f32::from(bounds.size.width),
                f32::from(bounds.size.height),
            ];
            if state.update(cx, |state, _| state.record_timeline_frame(frame)) {
                crate::shell::notify_after_draw(&state, cx);
            }
        },
        |_, _, _, _| {},
    )
    .absolute()
    .size_full()
}

pub(crate) fn pointer_ticks(
    x: gpui::Pixels,
    content_left: f32,
    scroll_x: f32,
    fps: f32,
    pps: f32,
) -> i64 {
    px_to_ticks(f32::from(x) - content_left + scroll_x, fps, pps)
}

pub(super) fn ticks_to_px(ticks: i64, fps: f32, pps: f32) -> f32 {
    crate::transport::ticks_to_px_f(ticks as f32, fps, pps)
}

pub(crate) fn px_to_ticks(x: f32, fps: f32, pps: f32) -> i64 {
    if pps <= 0.0 {
        return 0;
    }
    ((x.max(0.0) / pps) * fps.max(1.0)).floor() as i64
}

/// The ruler's tick spacing at one zoom, in ticks: a labelled tick every
/// `major`, an unlabelled one every `minor`. `minor` divides `major`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RulerScale {
    pub major: i64,
    pub minor: i64,
}

/// The closest two unlabelled ticks are drawn, in pixels.
pub(crate) const RULER_MINOR_MIN_PX: f32 = 8.0;
/// Clear space after a label before the next labelled tick, in pixels.
pub(crate) const RULER_LABEL_GAP_PX: f32 = 12.0;
/// Frame counts under one timecode second a timecode ruler steps by, where
/// they divide the second.
const TIMECODE_SUBSECOND: [i64; 13] = [1, 2, 3, 4, 5, 6, 8, 10, 12, 15, 20, 25, 30];
/// Timecode steps from one second to half an hour, in seconds. Past that the
/// ruler steps in hours.
const TIMECODE_CLOCK: [i64; 12] = [1, 2, 5, 10, 15, 30, 60, 120, 300, 600, 900, 1800];
/// Frame counts under one foot a feet-and-frames ruler steps by.
const FEET_SUBFOOT: [i64; 4] = [1, 2, 4, 8];

/// Every spacing `display` counts in at `rate`, in ticks, ascending: steps
/// that divide its unit (a timecode second, a foot), then whole units on the
/// 1-2-5 series (seconds, minutes and hours for timecode).
fn ruler_steps(
    rate: aria_models::Rational,
    display: aria_models::TimeDisplay,
) -> impl Iterator<Item = i64> {
    use aria_models::TimeDisplay;
    let (unit, below, clock, decade_unit): (i64, &'static [i64], &'static [i64], i64) =
        match display {
            TimeDisplay::Timecode => {
                let second = rate.nominal_fps();
                (second, &TIMECODE_SUBSECOND, &TIMECODE_CLOCK, second * 3600)
            }
            TimeDisplay::Frames => (1, &[], &[], 1),
            TimeDisplay::FeetFrames => (
                TimeDisplay::FRAMES_PER_FOOT,
                &FEET_SUBFOOT,
                &[],
                TimeDisplay::FRAMES_PER_FOOT,
            ),
        };
    below
        .iter()
        .copied()
        .filter(move |step| *step < unit && unit % step == 0)
        .chain(clock.iter().map(move |seconds| seconds * unit))
        .chain(
            (0..10_u32)
                .flat_map(|power| [1_i64, 2, 5].map(|n| n * 10_i64.pow(power)))
                .map(move |n| n * decade_unit),
        )
}

/// The spacing that keeps every label clear of the next at `px_per_tick`.
///
/// `label_px` is the widest label's drawn width. The labelled step is the
/// smallest one whose gap holds that label plus `RULER_LABEL_GAP_PX`, so two
/// labels never overlap at any zoom; the unlabelled step is the smallest one
/// that divides it and stays `RULER_MINOR_MIN_PX` apart.
pub fn ruler_scale(
    rate: aria_models::Rational,
    display: aria_models::TimeDisplay,
    px_per_tick: f32,
    label_px: f32,
) -> RulerScale {
    let fits = |step: i64, min_px: f32| step as f32 * px_per_tick >= min_px;
    let largest = ruler_steps(rate, display).last().unwrap_or(1);
    let major = ruler_steps(rate, display)
        .find(|step| fits(*step, label_px + RULER_LABEL_GAP_PX))
        .unwrap_or(largest);
    let minor = ruler_steps(rate, display)
        .take_while(|step| *step < major)
        .find(|step| major % step == 0 && fits(*step, RULER_MINOR_MIN_PX))
        .unwrap_or(major);
    RulerScale { major, minor }
}

/// Characters in the widest label a ruler over `[0, last_tick]` draws.
pub fn ruler_label_chars(
    rate: aria_models::Rational,
    display: aria_models::TimeDisplay,
    last_tick: i64,
) -> usize {
    struct Count(usize);
    impl std::fmt::Write for Count {
        fn write_str(&mut self, text: &str) -> std::fmt::Result {
            self.0 += text.chars().count();
            Ok(())
        }
    }
    let mut count = Count(0);
    // Infallible: `Count` only ever returns `Ok`.
    let _ = std::fmt::Write::write_fmt(
        &mut count,
        format_args!(
            "{}",
            crate::timecode::display_time(last_tick.max(0), rate, display)
        ),
    );
    count.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pointer_tick_is_measured_from_where_the_panel_was_drawn() {
        let (fps, pps) = (24.0, 80.0);
        let outline = crate::settings::OUTLINE_DEFAULT;
        let left = 264.0 + outline;

        assert_eq!(
            pointer_ticks(px(left), left, 0.0, fps, pps),
            0,
            "the first pixel of content is tick zero, wherever the panel sits"
        );
        assert_eq!(
            pointer_ticks(px(left + 80.0), left, 0.0, fps, pps),
            24,
            "one second along the content is one second into the sequence"
        );
        assert_eq!(
            pointer_ticks(px(left - 40.0), left, 0.0, fps, pps),
            0,
            "a pointer over the track headers is clamped to the start"
        );
        assert_eq!(
            pointer_ticks(px(left + 80.0), left, 160.0, fps, pps),
            72,
            "a scrolled view adds its offset: 160px is two more seconds"
        );
        assert_eq!(
            px_to_ticks(left + 80.0 - outline, fps, pps),
            103,
            "the old conversion put a click on the first second at 103 ticks"
        );
    }

    /// An eased zoom around the playhead keeps the playhead where it is drawn
    /// on every frame of the travel, not only at the two ends.
    ///
    /// The class: the scale and the offset riding two specs, so a zoom that
    /// lands on the right frame drifts the anchored tick sideways on the way
    /// there. Sampled from the glide store across the whole span, so a spec or
    /// curve that disagrees with the zoom's at any instant is red. It does not
    /// cover a release, whose overshoot is the point of the fling spec.
    #[test]
    fn an_eased_zoom_holds_its_anchor_on_every_frame() {
        use crate::motion::{MotionKey, TestFrames};

        let frames = TestFrames::new();
        let (zoom_key, scroll_key) = (
            MotionKey::new("anchor-test", "zoom"),
            MotionKey::new("anchor-test", "scroll"),
        );
        let (anchor_s, from_zoom, to_zoom) = (5.0_f32, 1.0_f32, 2.0_f32);
        let from_scroll = 120.0_f32;
        let anchor_px = anchor_s * BASE_PPS * from_zoom - from_scroll;
        let to_scroll = anchor_s * BASE_PPS * to_zoom - anchor_px;
        glide::glide(zoom_key, from_zoom, glide::ZOOM, false);
        glide::glide(scroll_key, from_scroll, scroll_spec(false), false);

        let steps = 20;
        let step = glide::ZOOM.total().max(scroll_spec(false).total()) / steps;
        let mut moved = false;
        for _ in 0..=steps + 1 {
            let zoom = glide::glide(zoom_key, to_zoom, glide::ZOOM, false);
            let scroll = glide::glide(scroll_key, to_scroll, scroll_spec(false), false);
            moved |= zoom != from_zoom && zoom != to_zoom;
            let drawn = anchor_s * BASE_PPS * zoom - scroll;
            assert!(
                (drawn - anchor_px).abs() < 0.5,
                "the anchor drew at {drawn} px instead of {anchor_px} px (zoom {zoom}, scroll {scroll})"
            );
            frames.advance(step);
        }
        assert!(
            moved,
            "the zoom never traveled, so the anchor was never tested"
        );
    }

    #[test]
    fn ticks_to_px_uses_fps_not_seconds_as_pixels() {
        assert!((ticks_to_px(24, 24.0, 80.0) - 80.0).abs() < f32::EPSILON);
        assert!((ticks_to_px(12, 24.0, 80.0) - 40.0).abs() < f32::EPSILON);
    }

    /// Labels never overlap at any zoom, in any time display, at any rate the
    /// settings dialog offers, and they are as dense as that allows.
    ///
    /// The class: a ruler whose spacing came from fixed zoom bands, so a label
    /// wider than its band (full timecode, a long frame count) ran into the
    /// next, and a band chosen by the rate's numerator put 23.976 on a grid of
    /// 24000 ticks. The sweep covers the whole zoom range; the rates are the
    /// dialog's, and the displays are the model's own list.
    #[test]
    fn ruler_labels_never_overlap_and_stay_as_dense_as_they_fit() {
        use aria_models::TimeDisplay;
        let digit_px = 6.6_f32;
        for rate in crate::surfaces::settings_pick::RATES {
            for display in TimeDisplay::ALL {
                let steps: Vec<i64> = ruler_steps(rate, display).collect();
                assert!(
                    steps.windows(2).all(|pair| pair[0] < pair[1]),
                    "{display:?} at {rate:?} steps out of order: {steps:?}"
                );
                let label_px = ruler_label_chars(rate, display, 3 * 3600 * 60) as f32 * digit_px;
                let mut previous_major = i64::MAX;
                let mut px_per_tick = 0.0005_f32;
                while px_per_tick < 400.0 {
                    let scale = ruler_scale(rate, display, px_per_tick, label_px);
                    let need = label_px + RULER_LABEL_GAP_PX;
                    assert!(
                        scale.major as f32 * px_per_tick >= need
                            || scale.major == *steps.last().unwrap(),
                        "{display:?} at {rate:?}, {px_per_tick} px a tick: labels {} ticks \
                         apart overlap",
                        scale.major
                    );
                    if let Some(smaller) = steps.iter().rev().find(|step| **step < scale.major) {
                        assert!(
                            (*smaller as f32) * px_per_tick < need,
                            "{display:?} at {rate:?}: {smaller} ticks would have fit but the \
                             ruler labeled every {}",
                            scale.major
                        );
                    }
                    assert_eq!(scale.major % scale.minor, 0, "{scale:?}");
                    assert!(
                        scale.minor == scale.major
                            || scale.minor as f32 * px_per_tick >= RULER_MINOR_MIN_PX,
                        "{scale:?} draws unlabelled ticks closer than they read"
                    );
                    assert!(
                        scale.major <= previous_major,
                        "zooming in widened the label spacing to {scale:?}"
                    );
                    previous_major = scale.major;
                    px_per_tick *= 1.07;
                }
            }
        }
    }

    /// A timecode label lands on a whole second, or on a frame that divides
    /// the second evenly, so its frame field reads a round value.
    #[test]
    fn timecode_labels_land_on_round_frames() {
        use aria_models::{Rational, TimeDisplay};
        for rate in crate::surfaces::settings_pick::RATES {
            let second = rate.nominal_fps();
            for step in ruler_steps(rate, TimeDisplay::Timecode) {
                assert!(
                    second % step == 0 || step % second == 0,
                    "{rate:?}: a label every {step} ticks drifts off the second"
                );
            }
        }
        let scale = ruler_scale(Rational::FPS_23_976, TimeDisplay::Timecode, 4.0, 72.6);
        assert_eq!(
            scale,
            RulerScale {
                major: 24,
                minor: 2
            }
        );
    }

    #[test]
    fn the_widest_label_is_measured_in_the_display() {
        use aria_models::{Rational, TimeDisplay};
        let rate = Rational::FPS_24;
        assert_eq!(ruler_label_chars(rate, TimeDisplay::Timecode, 292), 11);
        assert_eq!(ruler_label_chars(rate, TimeDisplay::Frames, 292), 3);
        assert_eq!(ruler_label_chars(rate, TimeDisplay::Frames, 120_000), 6);
        assert_eq!(ruler_label_chars(rate, TimeDisplay::FeetFrames, 292), 7);
    }
}

#[cfg(test)]
#[path = "layout_tests.rs"]
mod layout_tests;
