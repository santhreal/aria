//! Pointer travel and release on a lane-group canvas: the drag a press started,
//! the hover cursor, and the edit a release commits.

use aria_core::count::counted;

use crate::app::value_scrub::CLICK_SLOP_PX;

use super::*;

/// The pointer over the canvas: the cursor and the edge modifiers its target
/// asks for.
#[allow(clippy::too_many_arguments)]
pub(super) fn hover(
    this: &mut Shell,
    event: &MouseMoveEvent,
    bounds: Bounds<Pixels>,
    lanes: &[VisibleLane],
    fps: f32,
    pps: f32,
    scroll_x: f32,
    tool: EditorTool,
    cx: &mut Context<Shell>,
) {
    let (local_x, local_y) = local_coords(event.position, bounds);
    let hit = hit_test(lanes, local_x, local_y, fps, pps, scroll_x);
    let over_edge = matches!(hit, Some(HitTarget::ClipEdge { .. }));
    let ctrl = event.modifiers.control || event.modifiers.platform;
    let edge_changed = this.state.update(cx, |state, _| {
        state.update_edge_modifiers(over_edge, ctrl, event.modifiers.alt)
    });
    if edge_changed {
        cx.notify();
    }
    let cursor = cursor_for(tool, hit);
    if HOVER_CURSOR.replace(cursor) != cursor {
        cx.notify();
    }
}

/// The pointer anywhere with the button held on a gesture this canvas began.
/// `bounds`, `lanes` and the view are the canvas's as the press found them.
#[allow(clippy::too_many_arguments)]
pub(super) fn carry(
    this: &mut Shell,
    event: &MouseMoveEvent,
    bounds: Bounds<Pixels>,
    lanes: &[VisibleLane],
    fps: f32,
    pps: f32,
    scroll_x: f32,
    tool: EditorTool,
    cx: &mut Context<Shell>,
) {
    let Some(active) = ACTIVE_GESTURE.get() else {
        return;
    };
    let (local_x, local_y) = local_coords(event.position, bounds);
    match active {
        CanvasGesture::Clip {
            kind,
            clip_id,
            origin_track_id,
            mut target_track_id,
            origin_x,
            start_ticks,
            end_ticks,
        } => {
            // A move takes the lane under the pointer when that lane's kind
            // holds the clip; over any other lane it keeps the last one.
            if matches!(kind, DragKind::Move)
                && let Some(hovered_lane) = lanes
                    .iter()
                    .find(|l| local_y >= l.y_top && local_y <= l.y_bottom && !l.locked)
                && hovered_lane.track_id != target_track_id
                && this.state.update(cx, |state, _| {
                    state.drag_lands_on(
                        ClipId::from_raw(clip_id),
                        TrackId::from_raw(hovered_lane.track_id),
                    )
                })
            {
                target_track_id = hovered_lane.track_id;
                ACTIVE_GESTURE.set(Some(CanvasGesture::Clip {
                    kind,
                    clip_id,
                    origin_track_id,
                    target_track_id,
                    origin_x,
                    start_ticks,
                    end_ticks,
                }));
            }
            let delta_px = f32::from(event.position.x) - origin_x;
            let delta_ticks = travel(kind, delta_px, pps, fps).unwrap_or(0);
            let threshold = crate::snap::threshold_ticks(pps, fps);
            let span = Span {
                start: start_ticks,
                end: end_ticks,
                threshold,
                magnet: !event.modifiers.alt,
            };
            this.state.update(cx, |state, _| {
                let g = gesture_for_drag(
                    kind,
                    tool,
                    event.modifiers.control,
                    event.modifiers.shift,
                    event.modifiers.alt,
                    target_track_id,
                    span,
                );
                let d = Drag {
                    clip: ClipId::from_raw(clip_id),
                    delta: delta_ticks,
                };
                state.hold_drag(g, d);
            });
            cx.notify();
        }
        CanvasGesture::Marquee {
            origin_x,
            mut moved,
        } => {
            let delta_px = (f32::from(event.position.x) - origin_x).abs();
            if delta_px >= 2.0 || moved {
                moved = true;
                ACTIVE_GESTURE.set(Some(CanvasGesture::Marquee { origin_x, moved }));
                let tick = px_to_ticks((local_x + scroll_x).max(0.0), fps, pps);
                let track_id = lanes
                    .iter()
                    .find(|l| local_y >= l.y_top && local_y <= l.y_bottom)
                    .or_else(|| lanes.last())
                    .map(|l| l.track_id)
                    .unwrap_or(0);
                this.state.update(cx, |state, _| {
                    state.selected_gap = None;
                    state.drag_marquee(track_id, tick);
                });
                cx.notify();
            }
        }
        CanvasGesture::KeyframeDrag { origin_x } => {
            let delta_px = f32::from(event.position.x) - origin_x;
            let raw_delta = ((delta_px / pps.max(1.0)) * fps.max(1.0)).round() as i64;
            let threshold = crate::snap::threshold_ticks(pps, fps);
            this.state.update(cx, |state, _| {
                state.update_timeline_keyframe_drag(raw_delta, threshold, event.modifiers.alt);
            });
            cx.notify();
        }
        CanvasGesture::MaskPathKeyDrag { origin_x } => {
            let delta_px = f32::from(event.position.x) - origin_x;
            let raw_delta = ((delta_px / pps.max(1.0)) * fps.max(1.0)).round() as i64;
            let threshold = crate::snap::threshold_ticks(pps, fps);
            this.state.update(cx, |state, _| {
                state.update_mask_path_key_drag(raw_delta, threshold, event.modifiers.alt);
            });
            cx.notify();
        }
        CanvasGesture::KeyBand => {
            let tick = px_to_ticks((local_x + scroll_x).max(0.0), fps, pps);
            this.state.update(cx, |state, _| state.drag_key_band(tick));
            cx.notify();
        }
        CanvasGesture::Line { origin_y } => {
            let travel = f32::from(event.position.y) - origin_y;
            if this
                .state
                .update(cx, |state, _| state.drag_clip_line(travel))
            {
                crate::app::spawn_preview(this.state.clone(), cx);
                cx.notify();
            }
        }
        CanvasGesture::LineOrMove {
            clip_id,
            track_id,
            origin_x,
            origin_y,
            tick,
            band,
            start_ticks,
            end_ticks,
        } => {
            let dx = f32::from(event.position.x) - origin_x;
            let dy = f32::from(event.position.y) - origin_y;
            if dx.abs().max(dy.abs()) < CLICK_SLOP_PX {
                return;
            }
            // A line the press cannot hold, its clip or its value gone since
            // the press, leaves the clip to move whichever way the pointer went.
            let line = dy.abs() >= dx.abs()
                && this.state.update(cx, |state, _| {
                    state.press_clip_line(ClipId::from_raw(clip_id), LinePress::Line(tick), band)
                });
            ACTIVE_GESTURE.set(Some(if line {
                CanvasGesture::Line { origin_y }
            } else {
                CanvasGesture::Clip {
                    kind: DragKind::Move,
                    clip_id,
                    origin_track_id: track_id,
                    target_track_id: track_id,
                    origin_x,
                    start_ticks,
                    end_ticks,
                }
            }));
            carry(this, event, bounds, lanes, fps, pps, scroll_x, tool, cx);
        }
    }
}

pub(super) fn on_left_up(
    this: &mut Shell,
    event: &MouseUpEvent,
    fps: f32,
    pps: f32,
    tool: EditorTool,
    cx: &mut Context<Shell>,
) {
    let Some(active) = ACTIVE_GESTURE.take() else {
        return;
    };
    match active {
        CanvasGesture::Clip {
            kind,
            clip_id,
            origin_track_id,
            target_track_id,
            origin_x,
            start_ticks,
            end_ticks,
        } => {
            let delta_px = f32::from(event.position.x) - origin_x;
            let moved = travel(kind, delta_px, pps, fps);
            let still = moved.is_none();
            let delta_ticks = moved.unwrap_or(0);
            let span = Span {
                start: start_ticks,
                end: end_ticks,
                threshold: crate::snap::threshold_ticks(pps, fps),
                magnet: !event.modifiers.alt,
            };
            let g = gesture_for_drag(
                kind,
                tool,
                event.modifiers.control,
                event.modifiers.shift,
                event.modifiers.alt,
                target_track_id,
                span,
            );
            // A move carried to another lane lands there with no travel along
            // the lane, and travel under the threshold is a hand's jitter.
            let relaned = g.lane().is_some_and(|lane| lane.raw() != origin_track_id);
            this.state.update(cx, |state, _| {
                state.end_drag();
                if still && !relaned {
                    return;
                }
                let d = Drag {
                    clip: ClipId::from_raw(clip_id),
                    delta: delta_ticks,
                };
                if let Err(err) = gesture::apply(state, g, d) {
                    state.report_error(&err);
                }
            });
            cx.notify();
        }
        CanvasGesture::Marquee { moved, .. } => {
            this.state.update(cx, |state, _| {
                if moved {
                    let count = state.commit_marquee();
                    if count > 0 {
                        state.status_message =
                            Some(format!("Selected {}", counted(count, "clip", "clips")));
                    }
                } else {
                    state.marquee = None;
                }
            });
            cx.notify();
        }
        CanvasGesture::KeyframeDrag { .. } => {
            this.state.update(cx, |state, _| {
                if let Err(err) = state.commit_timeline_keyframe_drag() {
                    state.report_error(&err);
                }
            });
            crate::app::spawn_preview(this.state.clone(), cx);
            cx.notify();
        }
        CanvasGesture::MaskPathKeyDrag { .. } => {
            this.state.update(cx, |state, _| {
                if let Err(err) = state.commit_mask_path_key_drag() {
                    state.report_error(&err);
                }
            });
            crate::app::spawn_preview(this.state.clone(), cx);
            cx.notify();
        }
        CanvasGesture::KeyBand => {
            this.state.update(cx, |state, _| state.release_key_band());
            cx.notify();
        }
        CanvasGesture::Line { .. } => {
            this.state.update(cx, |state, _| state.release_clip_line());
            crate::app::spawn_preview(this.state.clone(), cx);
            cx.notify();
        }
        // The press selected the clip, and nothing travelled.
        CanvasGesture::LineOrMove { .. } => {}
    }
}

/// Escape on a gesture this canvas began: drop it, putting back what its
/// press found. A clip drag, a band and a key drag land nothing before their
/// release, so dropping what they hold puts them back; a key band puts the
/// selection back, and a clip's line its value.
pub(super) fn cancel(this: &mut Shell, cx: &mut Context<Shell>) {
    let Some(active) = ACTIVE_GESTURE.take() else {
        return;
    };
    let line = this.state.update(cx, |state, _| match active {
        CanvasGesture::Clip { .. } => {
            state.end_drag();
            false
        }
        CanvasGesture::Marquee { .. } => {
            state.cancel_marquee();
            false
        }
        CanvasGesture::KeyframeDrag { .. } => {
            state.cancel_timeline_keyframe_drag();
            false
        }
        CanvasGesture::MaskPathKeyDrag { .. } => {
            state.cancel_mask_path_key_drag();
            false
        }
        CanvasGesture::KeyBand => {
            state.cancel_key_band();
            false
        }
        CanvasGesture::Line { .. } => state.cancel_clip_line(),
        CanvasGesture::LineOrMove { .. } => false,
    });
    if line {
        crate::app::spawn_preview(this.state.clone(), cx);
    }
    cx.notify();
}

/// The ticks a drag has travelled, or `None` under the threshold, where travel
/// is a hand's jitter. The held frame and the release read it alike, so a
/// drag draws where it lands.
fn travel(kind: DragKind, delta_px: f32, pps: f32, fps: f32) -> Option<i64> {
    let threshold_px = match kind {
        DragKind::Move => 2.0,
        _ => 1.0,
    };
    (delta_px.abs() >= threshold_px)
        .then(|| ((delta_px / pps.max(1.0)) * fps.max(1.0)).round() as i64)
}

fn gesture_for_drag(
    kind: DragKind,
    tool: EditorTool,
    ctrl: bool,
    shift: bool,
    alt: bool,
    target_track_id: u64,
    span: Span,
) -> Gesture {
    let track = TrackId::from_raw(target_track_id);
    match kind {
        DragKind::Move if ctrl || shift => Gesture::RippleBody { track, span },
        DragKind::Move => Gesture::Body {
            drag: tool.body_drag(),
            track,
            span,
        },
        DragKind::TrimStart => resolve_edge_gesture(tool, ctrl, alt, true, span),
        DragKind::TrimEnd => resolve_edge_gesture(tool, ctrl, alt, false, span),
        DragKind::TransitionLeft => Gesture::Transition {
            right: false,
            duration: span.start,
        },
        DragKind::TransitionRight => Gesture::Transition {
            right: true,
            duration: span.start,
        },
        DragKind::FadeIn => Gesture::Fade {
            left: true,
            current: span.start,
        },
        DragKind::FadeOut => Gesture::Fade {
            left: false,
            current: span.start,
        },
    }
}
