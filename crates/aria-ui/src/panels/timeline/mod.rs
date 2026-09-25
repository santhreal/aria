//! Timeline: ruler, track headers, clip bodies.
//!
//! This module owns the panel's frame — geometry constants, the tick/pixel
//! conversions, and the scroll handler — and composes the five surfaces that
//! draw it.

mod clip;
mod drop;
mod headers;
mod lanes;
mod media;
mod overview;
mod ruler;

#[cfg(test)]
mod empty_timeline_tests;

use crate::fluent::FluentBuilder;
use gpui::{
    Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    ScrollDelta, ScrollHandle, ScrollWheelEvent, StatefulInteractiveElement, Styled, div, px,
};

use crate::app::AppState;
use crate::motion::{self, glide};
use crate::theme::Theme;
use crate::types::{ActivePanel, UiRulerTick};

use lanes::lane;
use media::{clip_bars, clip_cells};
use overview::overview;
use ruler::ruler;

thread_local! {
    /// The track stack's scroll offset. Held across renders because that is what
    /// the edge fade reads: it softens the top edge only once there is something
    /// above it, and the bottom only while there is more below.
    static STACK_SCROLL: ScrollHandle = ScrollHandle::new();
}

pub(super) const HEADER_W: f32 = crate::transport::TIMELINE_HEADER_W;
pub(super) const RULER_H: f32 = 28.0;
pub(super) const BASE_PPS: f32 = crate::transport::TIMELINE_BASE_PPS;
/// Pixels per waveform bar. Wide enough that a long clip draws hundreds of
/// bars rather than thousands, narrow enough to read as a wave.
pub(super) const BAR_STEP: f32 = 3.0;
/// Width of a filmstrip cell: a 16:9 frame at the cell height.
pub(super) const CELL_W: f32 = 60.0;
/// Height of a collapsed lane: enough to read the cut pattern, not enough for
/// a clip label.
pub(super) const COLLAPSED_H: f32 = 20.0;

pub fn render(
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let theme = Theme::of(cx);
    // A handle on the projection, not a copy of it: the panel needs the store
    // for the whole draw and cannot hold a borrow of the state across the
    // listeners it attaches below.
    let store = state.read(cx).store.clone();
    let playhead = store.playhead_ticks;
    let reduced = motion::reduced_motion(cx);
    // A playhead is a whole tick in the model and a fraction of one on screen.
    // Stepping it frame to frame reads as a stutter, so it follows its target
    // instead — and a scrub stays continuous because the pointer never lands on
    // a tick boundary either.
    let playhead_at = glide::glide(
        crate::motion::MotionKey::new("playhead", ""),
        playhead as f32,
        glide::PLAYHEAD,
        reduced,
    );
    let stack_scroll = STACK_SCROLL.with(ScrollHandle::clone);
    // No clamp: `AppState::set_zoom` is the only writer and it clamps, so a
    // second one here could only hide a writer that had bypassed it. The scale
    // the editor chose travels there; the one a pointer anchored is already
    // where the pointer put it.
    let zoom = glide::glide(
        crate::motion::MotionKey::new("timeline-zoom", ""),
        store.zoom,
        glide::ZOOM,
        reduced || !state.read(cx).zoom_eased,
    );
    // The offset the operator moves by hand is theirs on the frame they move it;
    // one the editor chose for them travels there instead. A pan that was thrown
    // is a third case: it travels on the spring, so it carries the weight of the
    // release past the target and back.
    let scroll_x = glide::glide(
        crate::motion::MotionKey::new("timeline-scroll", ""),
        store.scroll_x.max(0.0),
        if state.read(cx).scroll_fling {
            glide::FLING
        } else {
            glide::SCROLL
        },
        reduced || !state.read(cx).scroll_eased,
    );
    let pps = BASE_PPS * zoom;
    let rate = store.sequence.frame_rate;
    let fps = rate.num.max(1) as f32;
    let snap_guide = state.read(cx).snap_guide;
    let expanded = state.read(cx).expanded_clips.clone();
    let ticks = ruler_ticks(store.sequence.duration_ticks, rate.num.max(1) as i64, zoom);
    // The width the panel actually has, from the same chrome the shell laid it
    // out with. Culling and the edge fades read it, so a narrow window stops
    // building rows it has no room for instead of building 2000px worth.
    let visible_width = state.read(cx).timeline_viewport_px();
    let view_start = ((scroll_x / pps) * fps).floor() as i64;
    let view_end = (((scroll_x + visible_width) / pps) * fps).ceil() as i64;
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
    let tool = state.read(cx).tool;
    let cursor = tool.cursor();
    let track_dragging = state.read(cx).track_dragging();
    let drop_row = state.read(cx).track_drop_row;
    // A resize in flight is not an edit yet, so the lane draws the previewed
    // height and the command lands once, on mouse-up. The state answers which
    // lane is being dragged, so the panel keeps no copy of that rule.
    // Asked once per draw rather than per card: the answer is the same for all
    // of them, and it expires on its own.
    let landing = state.read(cx).landing_clip();
    let waves = clip_bars(state, &store, pps, fps, view_start, view_end, cx);
    let strips = clip_cells(state, &store, pps, fps, view_start, view_end, cx);
    let lanes: Vec<_> = store
        .sequence
        .tracks
        .iter()
        .enumerate()
        .map(|(row, track)| {
            let previewed = state.read(cx).lane_preview(track.id);
            let band = state
                .read(cx)
                .marquee
                .filter(|_| state.read(cx).marquee_covers_row(row))
                .map(|band| band.ticks());
            lane(
                &theme,
                track,
                row,
                track_dragging,
                drop_row,
                pps,
                fps,
                scroll_x,
                view_start,
                view_end,
                visible_width,
                playhead_at,
                snap_guide,
                band,
                &expanded,
                &waves,
                &strips,
                previewed,
                reduced,
                landing,
                cx,
            )
            .into_any_element()
        })
        .collect();

    div()
        .id("timeline")
        .size_full()
        .relative()
        .flex()
        .flex_col()
        .bg(theme.surface)
        .border_t_1()
        .border_color(theme.border)
        .cursor(cursor)
        .on_scroll_wheel(cx.listener(move |this, event: &ScrollWheelEvent, _, cx| {
            handle_scroll(this, event, cx);
        }))
        .child(measure(state))
        .child(ruler(
            &theme,
            &ticks,
            playhead_at,
            pps,
            fps,
            scroll_x,
            visible_width,
            snap_guide,
            add_name,
            zoom,
            state,
            cx,
        ))
        .child(overview(
            &theme,
            &store,
            pps,
            fps,
            scroll_x,
            visible_width,
            state,
            cx,
        ))
        .child(
            // The stack scrolls under a fixed ruler, and its top and bottom
            // edges fade rather than cut: a lane sliding out of view says so.
            crate::edge_fade::edge_faded(
                Theme::SPACE_LG,
                true,
                true,
                div()
                    .id("timeline-stack")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&stack_scroll)
                    .flex()
                    .flex_col()
                    .when(no_clips, |el| {
                        el.child(
                            div()
                                .flex_1()
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(px(13.))
                                .text_color(theme.text_faint)
                                .child("Drag media here to start."),
                        )
                    })
                    // The strips above and below the stack are the only media
                    // drop that adds a lane.
                    .child(drop::edge_strip(&theme, true, scroll_x, fps, pps, cx))
                    .children(lanes)
                    .child(drop::edge_strip(&theme, false, scroll_x, fps, pps, cx)),
            )
            .fade_overflow_y(&stack_scroll),
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, event: &MouseDownEvent, _, cx| {
                let tool = this.state.read(cx).tool;
                // Asked as capabilities, not as variant names: a tool that pans
                // says so in its row. There is no fallthrough arm, so a new
                // tool cannot silently inherit "does nothing on lane space".
                if tool.pans() {
                    this.start_pan(f32::from(event.position.x), cx);
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

fn handle_scroll(
    this: &mut crate::shell::Shell,
    event: &ScrollWheelEvent,
    cx: &mut gpui::Context<crate::shell::Shell>,
) {
    let delta = event.delta.pixel_delta(px(24.0));
    let dx = f32::from(delta.x);
    let dy = f32::from(delta.y);
    let ctrl = event.modifiers.control || event.modifiers.platform;
    if ctrl {
        let notches = if dy.abs() > 0.0 { -dy } else { -dx };
        let factor = if notches > 0.0 {
            crate::transport::ZOOM_WHEEL_STEP
        } else {
            1.0 / crate::transport::ZOOM_WHEEL_STEP
        };
        this.state.update(cx, |state, _| {
            state.set_zoom(state.zoom * factor, false);
        });
        cx.notify();
        return;
    }
    let trackpad = matches!(event.delta, ScrollDelta::Pixels(_));
    let shift = event.modifiers.shift;
    let scroll_dx = if shift {
        if dx.abs() > dy.abs() { dx } else { dy }
    } else if trackpad && dx.abs() > 0.0 {
        dx
    } else {
        return;
    };
    this.state.update(cx, |state, _| {
        let next = state.scroll_x + scroll_dx;
        state.scroll_timeline_to(next, false);
    });
    cx.notify();
}

/// Records where the panel is drawn every paint, so a pointer position can be
/// turned into a tick. Draws nothing and takes no pointer.
///
/// The panel is not at the window's left edge and does not stay put: the media
/// rail beside it is resizable and collapsible, and the window itself resizes.
/// Measuring is the only way to know, which is what the ramp graph and the
/// source monitor's scrub bar already do.
///
/// Repaints only when the rectangle moved; anything else is a repaint loop.
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
            state.update(cx, |state, cx| {
                if state.record_timeline_frame(frame) {
                    cx.notify();
                }
            });
        },
        |_, _, _, _| {},
    )
    .absolute()
    .size_full()
}

/// The tick under a pointer, from the position the platform reported.
///
/// `content_left` is where the scrolling content begins in window coordinates
/// ([`AppState::timeline_content_left`]). Every gesture that reads a tick out of
/// a pointer — a click on the ruler, a scrub, a rubber band — goes through here,
/// so they cannot disagree about where tick zero is.
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
    ticks_to_px_f(ticks as f32, fps, pps)
}

/// The same conversion for a position between two frames, which is where a
/// followed playhead spends most of its time.
pub(super) fn ticks_to_px_f(ticks: f32, fps: f32, pps: f32) -> f32 {
    (ticks / fps.max(1.0)) * pps
}

/// The tick a lane-relative x lands on, never negative. Rounds down, so a drop
/// lands on the frame the pointer is inside rather than the next one.
pub(crate) fn px_to_ticks(x: f32, fps: f32, pps: f32) -> i64 {
    if pps <= 0.0 {
        return 0;
    }
    ((x.max(0.0) / pps) * fps.max(1.0)).floor() as i64
}

pub fn ruler_ticks(duration_ticks: i64, fps: i64, zoom: f32) -> Vec<UiRulerTick> {
    let fps = fps.max(1);
    let step = if zoom >= 4.0 {
        1
    } else if zoom >= 1.5 {
        fps / 4
    } else if zoom >= 0.6 {
        fps
    } else {
        fps * 5
    }
    .max(1);
    let mut ticks = Vec::new();
    let mut t = 0;
    while t <= duration_ticks.max(0) {
        let seconds = t / fps;
        let frames = t % fps;
        let major = t % (step * 4).max(step) == 0;
        ticks.push(UiRulerTick {
            position_ticks: t,
            label: if major {
                format!("{seconds}:{frames:02}").into()
            } else {
                gpui::SharedString::default()
            },
            major,
        });
        t += step;
    }
    ticks
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A pointer is in window coordinates; a tick is not.**
    ///
    /// The conversion used to subtract the header column alone, which is only
    /// right when the panel starts at x=0. It never does: the media rail and its
    /// splitter are to its left, so a click on the first frame landed the whole
    /// rail's width into the sequence — three seconds at the default zoom. The
    /// panel measures where it is drawn and every gesture reads that.
    #[test]
    fn a_pointer_tick_is_measured_from_where_the_panel_was_drawn() {
        // 24fps, 80px per second: one second of sequence is 80px and 24 ticks.
        let (fps, pps) = (24.0, 80.0);
        // The panel drawn at x=264 (a 260px rail plus its 4px splitter), so its
        // content starts at 264 + 132.
        let left = 264.0 + HEADER_W;

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
        // The bug this closes, stated as the number it produced: subtracting the
        // header width alone from the same pointer.
        assert_eq!(
            px_to_ticks(left + 80.0 - HEADER_W, fps, pps),
            103,
            "the old conversion put a click on the first second at 103 ticks"
        );
    }

    #[test]
    fn ruler_ticks_at_unit_zoom_steps_by_fps() {
        let ticks = ruler_ticks(48, 24, 1.0);
        assert_eq!(
            ticks.iter().map(|t| t.position_ticks).collect::<Vec<_>>(),
            vec![0, 24, 48]
        );
        assert!(ticks[0].major);
        assert_eq!(ticks[0].label, "0:00");
        assert!(!ticks[1].major);
        assert_eq!(ticks[1].label, "");
        assert!(!ticks[2].major);
        assert_eq!(ticks[2].label, "");
    }

    #[test]
    fn ruler_ticks_at_high_zoom_are_per_frame() {
        let ticks = ruler_ticks(3, 24, 4.0);
        assert_eq!(ticks.len(), 4);
        assert_eq!(ticks[1].position_ticks, 1);
    }

    #[test]
    fn ticks_to_px_uses_fps_not_seconds_as_pixels() {
        assert!((ticks_to_px(24, 24.0, 80.0) - 80.0).abs() < f32::EPSILON);
        assert!((ticks_to_px(12, 24.0, 80.0) - 40.0).abs() < f32::EPSILON);
    }
}
