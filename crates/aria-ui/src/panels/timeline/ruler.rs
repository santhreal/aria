//! Ruler, playhead, snap guide, and the transition wedge at a cut.

use crate::fluent::FluentBuilder;
use gpui::{
    CursorStyle, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent,
    ParentElement, SharedString, StatefulInteractiveElement, Styled, div, px,
};

use aria_models::TrackKind;

use crate::components;
use crate::motion;
use crate::surfaces::timeline_gesture as gesture;
use crate::theme::Theme;
use crate::types::{ContextTarget, UiRulerTick};

use super::{HEADER_W, RULER_H, ticks_to_px, ticks_to_px_f};

/// The magnet's confirmation, as a 0..1 breath.
///
/// Only worth asking for while a guide is on screen: the clock it leases
/// repaints this view while it runs. Flat zero under reduced motion, so a held
/// snap is then a still line.
pub(super) fn snap_glow(cx: &mut gpui::Context<crate::shell::Shell>) -> f32 {
    let view = cx.entity_id();
    motion::pulse_wave(motion::pulse_delta(&motion::ZERON_PULSE, view, cx))
}

pub(super) fn ruler(
    theme: &Theme,
    ticks: &[UiRulerTick],
    playhead: f32,
    pps: f32,
    fps: f32,
    scroll_x: f32,
    // The panel's measured width, so a tick, a marker, or a beat is built only
    // when it lands inside the body that is on screen.
    viewport: f32,
    snap_guide: Option<crate::snap::SnapCandidate>,
    add_name: String,
    zoom: f32,
    state: &gpui::Entity<crate::app::AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let add_label = add_name;
    // The magnet breathes only while it holds something.
    let glow = snap_guide.map(|_| snap_glow(cx)).unwrap_or_default();
    // Content runs off both ends of the body: the left edge is soft once the
    // view has been scrolled, the right while the sequence still has length
    // beyond the width the body was given.
    let sequence_px = ticks_to_px(ticks.last().map_or(0, |tick| tick.position_ticks), fps, pps);
    let fade_left = scroll_x > 0.0;
    let fade_right = sequence_px > scroll_x + viewport;
    div()
        .id("ruler")
        .h(px(RULER_H))
        .w_full()
        .flex()
        .flex_row()
        .border_b_1()
        .border_color(theme.border)
        .child(
            div()
                .w(px(HEADER_W))
                .h_full()
                .border_r_1()
                .border_color(theme.border)
                .flex()
                .items_center()
                .justify_between()
                .px(px(Theme::SPACE_SM))
                .child(
                    div()
                        .text_size(px(11.))
                        .font_family(theme.font_mono.clone())
                        .text_color(theme.text_muted)
                        .child(format!("{:.0}%", zoom * 100.0)),
                )
                .child(
                    components::icon_button("add-track", crate::icons::PLUS, theme).on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            let name = add_label.clone();
                            this.state.update(cx, |state, _| {
                                if let Err(err) = state.add_track(TrackKind::Video, name) {
                                    state.report_error(&err);
                                }
                            });
                            cx.notify();
                        }),
                    ),
                ),
        )
        .child(
            // The body scrolls under the fixed header column, so its own edges
            // are where content is cut: soft on the left once the view has
            // scrolled, soft on the right while the sequence runs past it.
            crate::edge_fade::edge_faded(
                Theme::SPACE_LG,
                false,
                false,
                ruler_body(
                    theme, ticks, playhead, pps, fps, scroll_x, viewport, snap_guide, glow, state,
                    cx,
                ),
            )
            .fade_left(fade_left)
            .fade_right(fade_right),
        )
}

/// The ruler's scrolling half: the ticks, the flags, the playhead, and the
/// guide the magnet draws. Split out so the frame that scrolls can be wrapped
/// in an edge fade without indenting the whole ruler.
#[allow(clippy::too_many_arguments)]
fn ruler_body(
    theme: &Theme,
    ticks: &[UiRulerTick],
    playhead: f32,
    pps: f32,
    fps: f32,
    scroll_x: f32,
    viewport: f32,
    snap_guide: Option<crate::snap::SnapCandidate>,
    glow: f32,
    state: &gpui::Entity<crate::app::AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    div()
        .id("ruler-body")
        .flex_1()
        .h_full()
        .relative()
        .overflow_hidden()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                // No measurement means the panel has never been painted, and a
                // pointer cannot be over an unpainted panel: nothing to convert
                // rather than a conversion against a guessed origin.
                let Some(left) = this.state.read(cx).timeline_content_left() else {
                    return;
                };
                let ticks = super::pointer_ticks(event.position.x, left, scroll_x, fps, pps);
                this.state.update(cx, |state, _| state.set_playhead(ticks));
                crate::app::spawn_preview(this.state.clone(), cx);
                cx.notify();
            }),
        )
        // Holding the button and moving keeps scrubbing. The pointer's own
        // position is the only state a scrub needs, so there is none to keep:
        // whichever tick it is over is the one the playhead follows to.
        .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
            if event.pressed_button != Some(MouseButton::Left) {
                return;
            }
            let Some(left) = this.state.read(cx).timeline_content_left() else {
                return;
            };
            let ticks = super::pointer_ticks(event.position.x, left, scroll_x, fps, pps);
            if this.state.read(cx).playhead.value == ticks {
                return;
            }
            this.state.update(cx, |state, _| state.set_playhead(ticks));
            crate::app::spawn_preview(this.state.clone(), cx);
            cx.notify();
        }))
        .children(ticks.iter().filter_map(|tick| {
            let x = ticks_to_px(tick.position_ticks, fps, pps) - scroll_x;
            if x < -8.0 || x > viewport + 8.0 {
                return None;
            }
            Some(
                div()
                    .absolute()
                    .left(px(x))
                    .h_full()
                    .w(px(1.))
                    .bg(if tick.major {
                        theme.border_strong
                    } else {
                        theme.border
                    })
                    .when(tick.major, |el| {
                        el.child(
                            div()
                                .ml(px(Theme::SPACE_XS))
                                .text_size(px(11.))
                                .font_family(theme.font_mono.clone())
                                .text_color(theme.text_faint)
                                .child(tick.label.clone()),
                        )
                    }),
            )
        }))
        .children({
            // The borrow ends before the listeners are built, which is why this
            // reads values out rather than holding `app`. It reads only what a
            // flag draws: the whole marker list used to be copied here, names
            // and notes and all, once per frame of every scrub and every
            // playback tick, to draw six pixels of colour.
            let (visible, mark_in, mark_out, beats) = {
                let app = state.read(cx);
                let visible: Vec<_> = app
                    .store
                    .sequence
                    .markers
                    .iter()
                    .filter_map(|marker| {
                        let x = ticks_to_px(marker.tick, fps, pps) - scroll_x;
                        (x >= -8.0 && x <= viewport + 8.0).then_some((
                            x,
                            marker.tick,
                            marker.id,
                            marker.color,
                        ))
                    })
                    .collect();
                // Only the beats on screen: a project with beat grids on every
                // clip holds tens of thousands of onsets, and sorting all of
                // them to draw the dozen inside the viewport is work the frame
                // pays and the operator never sees. The sort still runs, on
                // what is visible, because two clips can share an onset.
                let mut beats: Vec<i64> = app
                    .store
                    .sequence
                    .tracks
                    .iter()
                    .flat_map(|track| track.clips.iter())
                    .flat_map(|clip| clip.beat_ticks.iter().copied())
                    .filter(|tick| {
                        let x = ticks_to_px(*tick, fps, pps) - scroll_x;
                        x >= -8.0 && x <= viewport + 8.0
                    })
                    .collect();
                beats.sort_unstable();
                beats.dedup();
                (visible, app.mark_in, app.mark_out, beats)
            };
            let mut flags = Vec::with_capacity(visible.len() + beats.len() + 2);
            for (x, tick, id, color) in visible {
                flags.push(
                    div()
                        .id(gpui::ElementId::named_usize("marker", id as usize))
                        .absolute()
                        .left(px(x - 3.0))
                        .top_0()
                        .w(px(6.))
                        .h(px(10.))
                        .bg(theme.marker(color))
                        .cursor(CursorStyle::PointingHand)
                        .active(|style| style.opacity(0.92))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                this.state.update(cx, |state, _| state.set_playhead(tick));
                                crate::app::spawn_preview(this.state.clone(), cx);
                                cx.notify();
                            }),
                        )
                        .into_any_element(),
                );
            }
            if let Some(tick) = mark_in {
                let x = ticks_to_px(tick, fps, pps) - scroll_x;
                flags.push(
                    div()
                        .absolute()
                        .left(px(x))
                        .bottom_0()
                        .w(px(2.))
                        .h(px(12.))
                        .bg(theme.success)
                        .into_any_element(),
                );
            }
            if let Some(tick) = mark_out {
                let x = ticks_to_px(tick, fps, pps) - scroll_x;
                flags.push(
                    div()
                        .absolute()
                        .left(px(x))
                        .bottom_0()
                        .w(px(2.))
                        .h(px(12.))
                        .bg(theme.warning)
                        .into_any_element(),
                );
            }
            // Beats are drawn under the flags: a detected onset is a magnet and
            // a reference, never a thing to click, so it gets a hairline rather
            // than a hit target.
            for tick in beats {
                let x = ticks_to_px(tick, fps, pps) - scroll_x;
                flags.push(
                    div()
                        .absolute()
                        .left(px(x))
                        .bottom_0()
                        .w(px(1.))
                        .h(px(6.))
                        .bg(theme.text_faint)
                        .into_any_element(),
                );
            }
            flags
        })
        .child(playhead_line(theme, playhead, pps, fps, scroll_x))
        .when_some(snap_guide, |el, guide| {
            el.child(snap_guide_line(theme, guide, pps, fps, scroll_x, glow))
        })
}

/// The playhead, at a position that may sit between two frames.
///
/// The caller passes a followed value rather than the model's tick, so playback
/// and a scrub both read as travel instead of a step per frame.
pub(super) fn playhead_line(
    theme: &Theme,
    playhead: f32,
    pps: f32,
    fps: f32,
    scroll_x: f32,
) -> impl IntoElement {
    let x = ticks_to_px_f(playhead, fps, pps) - scroll_x;
    div()
        .absolute()
        .left(px(x - 5.0))
        .top_0()
        .bottom_0()
        .w(px(11.))
        .child(div().w(px(11.)).h(px(8.)).bg(theme.playhead))
        .child(
            div()
                .absolute()
                .left(px(5.))
                .top_0()
                .bottom_0()
                .w(px(1.))
                .bg(theme.playhead),
        )
}

/// The magnet's guide: the line plus the name of what fired it, so a snap onto
/// the wrong edge is visible instead of looking like a missed drag.
///
/// `glow` is the confirmation. The line fades in on the frame the magnet takes
/// hold, and then breathes while it holds, so a snap that fired reads apart from
/// a drag that merely passed over the same tick. Keyed by the candidate, so
/// moving from one magnet to the next replays the confirmation rather than
/// sliding a line that was already there.
pub(super) fn snap_guide_line(
    theme: &Theme,
    guide: crate::snap::SnapCandidate,
    pps: f32,
    fps: f32,
    scroll_x: f32,
    glow: f32,
) -> impl IntoElement {
    let x = ticks_to_px(guide.tick, fps, pps) - scroll_x;
    let line = div()
        .absolute()
        .left(px(x))
        .top_0()
        .bottom_0()
        .w(px(1.))
        .bg(theme.snap_guide)
        .child(
            // The confirmation itself: a wider, dimmer wash over the line whose
            // strength is the breath.
            div()
                .absolute()
                .left(px(-Theme::SPACE_XS))
                .top_0()
                .bottom_0()
                .w(px(Theme::SPACE_SM + 1.0))
                .bg(theme.snap_guide.opacity(glow * 0.35)),
        )
        .child(motion::menu_in(
            guide.guide_key().with("label"),
            div()
                .absolute()
                .top(px(2.))
                .left(px(3.))
                .px(px(3.))
                .rounded(px(2.))
                .bg(theme.surface_card)
                .text_size(px(10.))
                .text_color(theme.snap_guide)
                .child(guide.kind.label()),
        ));
    motion::fade_quick(guide.guide_key(), line)
}

/// The blend window at a cut, drawn centred on the junction so its width reads as
/// the transition's duration.
///
/// Right-clicking offers Remove and the blend catalog for that junction; dragging
/// either edge sets the window length. Both address the wedge's own clip, so
/// neither steals the selection.
pub(super) fn transition_wedge(
    theme: &Theme,
    clip_id: u64,
    center_ticks: i64,
    duration_ticks: i64,
    label: impl Into<SharedString>,
    pps: f32,
    fps: f32,
    scroll_x: f32,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let half = duration_ticks.max(1) / 2;
    let left = ticks_to_px(center_ticks - half, fps, pps) - scroll_x;
    let width = (ticks_to_px(center_ticks + half, fps, pps) - scroll_x - left).max(4.0);
    div()
        .id(gpui::ElementId::named_usize("transition", clip_id as usize))
        .absolute()
        .left(px(left))
        .top_0()
        .bottom_0()
        .w(px(width))
        .bg(theme.accent.opacity(0.28))
        .border_1()
        .border_color(theme.accent)
        .overflow_hidden()
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                this.state.update(cx, |state, _| {
                    state.open_context(
                        ContextTarget::Transition { clip: clip_id },
                        f32::from(event.position.x),
                        f32::from(event.position.y),
                    );
                });
                cx.notify();
            }),
        )
        .child(
            div()
                .text_size(px(9.))
                .text_color(theme.text_faint)
                .child(label.into()),
        )
        .child(wedge_edge(false, clip_id, duration_ticks, pps, fps, cx))
        .child(wedge_edge(true, clip_id, duration_ticks, pps, fps, cx))
}

/// Width of a wedge edge grab strip, in pixels.
pub(super) const WEDGE_EDGE: f32 = 6.0;

/// One draggable edge of a wedge. The window is committed once on release, so a
/// drag is one edit and a release that moved nothing writes nothing.
pub(super) fn wedge_edge(
    right: bool,
    clip_id: u64,
    duration_ticks: i64,
    pps: f32,
    fps: f32,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let origin: std::rc::Rc<std::cell::Cell<Option<f32>>> = Default::default();
    let down = origin.clone();
    let side = if right {
        "transition-r"
    } else {
        "transition-l"
    };
    let handle = div()
        .id(gpui::ElementId::named_usize(side, clip_id as usize))
        .absolute()
        .top_0()
        .bottom_0()
        .w(px(WEDGE_EDGE))
        .cursor(CursorStyle::ResizeLeftRight)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |_, event: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                down.set(Some(f32::from(event.position.x)));
            }),
        )
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(move |this, event: &gpui::MouseUpEvent, _, cx| {
                let Some(origin_x) = origin.take() else {
                    return;
                };
                let delta_px = f32::from(event.position.x) - origin_x;
                let gesture = gesture::Gesture::Transition {
                    right,
                    duration: duration_ticks,
                };
                let drag = gesture::Drag {
                    clip: aria_models::ClipId::from_raw(clip_id),
                    delta: ((delta_px / pps.max(1.0)) * fps.max(1.0)).round() as i64,
                };
                this.state.update(cx, |state, _| {
                    if let Err(err) = gesture::apply(state, gesture, drag) {
                        state.report_error(&err);
                    }
                });
                cx.notify();
            }),
        );
    if right {
        handle.right_0()
    } else {
        handle.left_0()
    }
}
