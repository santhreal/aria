//! Finish: 7 helpers from [`super`].
//!
//! Split out of `clip.rs` to stay under the file ceiling. A pure
//! move. Each item is `pub(super)` so the parent and its other children can
//! reach it by name; nothing else changed.

use super::*;
use crate::surfaces::timeline_gesture as gesture;

/// A fade handle: a small square that sits where the fade currently ends, so
/// its position is the fade's length and dragging it along the card sets that
/// length. A gesture is one edit, committed on mouse-up.
pub(super) fn fade_handle(
    left: bool,
    theme: &Theme,
    id: u64,
    fade_ticks: i64,
    pps: f32,
    fps: f32,
    drag: Rc<Cell<Option<DragOrigin>>>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let side_id =
        motion::MotionKey::new(if left { "fade-l" } else { "fade-r" }, id).element_id("fade-grip");
    let kind = if left {
        DragKind::FadeIn
    } else {
        DragKind::FadeOut
    };
    let inset = ticks_to_px(fade_ticks, fps, pps).max(0.0);
    let drag_down = drag.clone();
    // One fade per instance, so the key carries the clip and the side. A handle
    // is nearly invisible until the pointer is on the card, and it strengthens
    // rather than switching: an 8px square that blinks reads as a glitch.
    let grip = motion::MotionKey::new(if left { "fade-grip-l" } else { "fade-grip-r" }, id);
    let handle = div()
        .id(side_id)
        .absolute()
        .top(px(2.))
        .size(px(FADE_GRIP))
        .rounded(px(2.))
        .bg(motion::hover_blend(
            grip,
            theme.wash(0.22),
            theme.wash(0.40),
        ))
        .border_1()
        .border_color(motion::hover_blend(
            grip,
            theme.border_strong.opacity(0.35),
            theme.border_strong,
        ))
        .on_hover(motion::hover_listener(grip))
        .cursor(CursorStyle::ResizeLeftRight)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |_, event: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                drag_down.set(Some(DragOrigin {
                    kind,
                    x: f32::from(event.position.x),
                }));
            }),
        )
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseUpEvent, _, cx| {
                let Some(origin) = drag.take() else {
                    return;
                };
                let mine = matches!(
                    (origin.kind, left),
                    (DragKind::FadeIn, true) | (DragKind::FadeOut, false)
                );
                if !mine {
                    drag.set(Some(origin));
                    return;
                }
                finish_fade(this, event, origin.x, id, fade_ticks, left, pps, fps, cx);
            }),
        );
    if left {
        handle.left(px(inset + 2.0))
    } else {
        handle.right(px(inset + 2.0))
    }
}

/// A fade handle drag. Which side it is and what the fade currently measures is
/// all the registry needs: how a pointer direction maps onto a fade's length is
/// its row, not this module's.
#[allow(clippy::too_many_arguments)]
pub(super) fn finish_fade(
    this: &mut crate::shell::Shell,
    event: &MouseUpEvent,
    origin_x: f32,
    id: u64,
    fade_ticks: i64,
    left: bool,
    pps: f32,
    fps: f32,
    cx: &mut gpui::Context<crate::shell::Shell>,
) {
    let delta_px = f32::from(event.position.x) - origin_x;
    if delta_px.abs() < 1.0 {
        return;
    }
    let gesture = gesture::Gesture::Fade {
        left,
        current: fade_ticks,
    };
    run(this, gesture, id, ticks(delta_px, pps, fps), cx);
}

pub(super) fn trim_handle(
    left: bool,
    theme: &Theme,
    id: u64,
    start_ticks: i64,
    end_ticks: i64,
    pps: f32,
    fps: f32,
    drag: Rc<Cell<Option<DragOrigin>>>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let side_id =
        motion::MotionKey::new(if left { "trim-l" } else { "trim-r" }, id).element_id("trim-grip");
    let drag_down = drag.clone();
    let kind = if left {
        DragKind::TrimStart
    } else {
        DragKind::TrimEnd
    };
    // A trim handle has no resting appearance: it is the card's edge until the
    // pointer is over it, and then it becomes a grip. Per clip and per side, so
    // one card's left edge is not the other's.
    let trim = motion::MotionKey::new(if left { "trim-grip-l" } else { "trim-grip-r" }, id);
    let handle = div()
        .id(side_id)
        .absolute()
        .top_0()
        .bottom_0()
        .w(px(TRIM))
        .bg(motion::hover_blend(
            trim,
            theme.wash(0.0),
            theme.element_active,
        ))
        .on_hover(motion::hover_listener(trim))
        .cursor(CursorStyle::ResizeLeftRight)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |_, event: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                drag_down.set(Some(DragOrigin {
                    kind,
                    x: f32::from(event.position.x),
                }));
            }),
        )
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseUpEvent, _, cx| {
                let Some(origin) = drag.take() else {
                    return;
                };
                let expected = if left {
                    DragKind::TrimStart
                } else {
                    DragKind::TrimEnd
                };
                if !matches!(
                    (origin.kind, expected),
                    (DragKind::TrimStart, DragKind::TrimStart)
                        | (DragKind::TrimEnd, DragKind::TrimEnd)
                ) {
                    drag.set(Some(origin));
                    return;
                }
                finish_trim(
                    this,
                    event,
                    origin.x,
                    id,
                    start_ticks,
                    end_ticks,
                    left,
                    pps,
                    fps,
                    cx,
                );
            }),
        );
    if left {
        handle.left_0()
    } else {
        handle.right_0()
    }
}

/// A drag across a clip card. What it means is the active tool's decision, and
/// what each decision edits is [`crate::surfaces::timeline_gesture`]: this reads
/// the pointer, converts pixels to ticks, and hands the gesture over.
pub(super) fn finish_body_drag(
    this: &mut crate::shell::Shell,
    event: &MouseUpEvent,
    origin_x: f32,
    id: u64,
    track_id: u64,
    origin_start: i64,
    origin_end: i64,
    pps: f32,
    fps: f32,
    cx: &mut gpui::Context<crate::shell::Shell>,
) {
    let delta_px = f32::from(event.position.x) - origin_x;
    if delta_px.abs() < 2.0 {
        return;
    }
    let body_drag = this.state.read(cx).tool.body_drag();
    let span = gesture::Span {
        start: origin_start,
        end: origin_end,
        threshold: threshold_ticks(pps, fps),
        magnet: !event.modifiers.alt,
    };
    let track = TrackId::from_raw(track_id);
    // Alt still drops the magnet. Shift on a pointer-tool body drag ripples:
    // the hole closes. Slip/slide keep Shift free; Shift-click never reaches
    // here because a press that moved less than two pixels is not a drag.
    let gesture = if body_drag.is_none() && event.modifiers.shift {
        gesture::Gesture::RippleBody { track, span }
    } else {
        gesture::Gesture::Body {
            drag: body_drag,
            track,
            span,
        }
    };
    run(this, gesture, id, ticks(delta_px, pps, fps), cx);
}

/// An edge drag. Which edge and which tool is all the registry needs: the trim,
/// the roll, the rate stretch and the ripple are its rows, not this module's.
#[allow(clippy::too_many_arguments)]
pub(super) fn finish_trim(
    this: &mut crate::shell::Shell,
    event: &MouseUpEvent,
    origin_x: f32,
    id: u64,
    start_ticks: i64,
    end_ticks: i64,
    left: bool,
    pps: f32,
    fps: f32,
    cx: &mut gpui::Context<crate::shell::Shell>,
) {
    let delta_px = f32::from(event.position.x) - origin_x;
    if delta_px.abs() < 1.0 {
        return;
    }
    let gesture = gesture::Gesture::Edge {
        drag: this.state.read(cx).tool.edge_drag(),
        left,
        span: gesture::Span {
            start: start_ticks,
            end: end_ticks,
            threshold: threshold_ticks(pps, fps),
            magnet: !event.modifiers.alt,
        },
    };
    run(this, gesture, id, ticks(delta_px, pps, fps), cx);
}

/// A pointer distance in the timeline's own units.
fn ticks(delta_px: f32, pps: f32, fps: f32) -> i64 {
    ((delta_px / pps.max(1.0)) * fps.max(1.0)).round() as i64
}

/// Hand one gesture to the registry and report a refusal.
fn run(
    this: &mut crate::shell::Shell,
    gesture: gesture::Gesture,
    id: u64,
    delta: i64,
    cx: &mut gpui::Context<crate::shell::Shell>,
) {
    let drag = gesture::Drag {
        clip: ClipId::from_raw(id),
        delta,
    };
    this.state.update(cx, |state, _| {
        if let Err(err) = gesture::apply(state, gesture, drag) {
            state.report_error(&err);
        }
    });
    cx.notify();
}

/// The magnet, as a drag sees it.
///
/// `suspended` is the drag-time modifier: holding Alt places the clip where the
/// pointer is even with the magnet on in the toolbar.
pub(super) fn snap_desired(
    this: &crate::shell::Shell,
    desired: i64,
    moving: u64,
    pps: f32,
    fps: f32,
    suspended: bool,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> (i64, Option<crate::snap::SnapCandidate>) {
    let threshold = threshold_ticks(pps, fps);
    this.state
        .read(cx)
        .snap_at(desired, Some(moving), threshold, suspended)
}
