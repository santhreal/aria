//! Pointer gesture dispatch for a virtualized lane-group canvas.

use std::cell::Cell;
use std::rc::Rc;

use aria_models::{ClipId, ClipParam, TrackId};
use gpui::{
    Bounds, Context, CursorStyle, InteractiveElement, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, Point, Stateful,
};

use super::VisibleLane;
use super::bar::BarRows;
use super::hit::{HitTarget, cursor_for, hit_test};
use crate::app::clip_line::LinePress;
use crate::app::property_rows::{KeyframeContextMenu, MaskPathKeyMenu};
use crate::panels::timeline::px_to_ticks;
use crate::shell::Shell;
use crate::surfaces::timeline_gesture::{self as gesture, Drag, Gesture, Span};
use crate::types::{ContextTarget, EditorTool, RenameTarget, TrimSide};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DragKind {
    Move,
    TrimStart,
    TrimEnd,
    TransitionLeft,
    TransitionRight,
    FadeIn,
    FadeOut,
}

/// Active pointer gesture started on a lane-group canvas.
#[derive(Clone, Copy, Debug)]
enum CanvasGesture {
    Clip {
        kind: DragKind,
        clip_id: u64,
        origin_track_id: u64,
        target_track_id: u64,
        origin_x: f32,
        start_ticks: i64,
        end_ticks: i64,
    },
    Marquee {
        origin_x: f32,
        moved: bool,
    },
    KeyframeDrag {
        origin_x: f32,
    },
    MaskPathKeyDrag {
        origin_x: f32,
    },
    KeyframeMarquee {
        clip: ClipId,
        param: ClipParam,
        origin_tick: i64,
        additive: bool,
    },
    /// A clip bar's line held; the travel is measured from `origin_y`.
    Line {
        origin_y: f32,
    },
}

thread_local! {
    static ACTIVE_GESTURE: Cell<Option<CanvasGesture>> = const { Cell::new(None) };
    /// The cursor the hovered target asked for at the last pointer move. The
    /// canvas sets the cursor while it paints, so a change repaints it.
    static HOVER_CURSOR: Cell<CursorStyle> = const { Cell::new(CursorStyle::Arrow) };
}

/// Attach left/right mouse-down, mouse-move, and mouse-up handlers to a
/// lane-group container `div`.
pub(crate) fn attach_group_mouse_listeners(
    el: Stateful<gpui::Div>,
    canvas_bounds: Rc<Cell<Bounds<Pixels>>>,
    badges: super::FxBadges,
    visible_lanes: Rc<Vec<VisibleLane>>,
    fps: f32,
    pps: f32,
    scroll_x: f32,
    tool: EditorTool,
    cx: &mut Context<Shell>,
) -> Stateful<gpui::Div> {
    let bounds_down = canvas_bounds.clone();
    let lanes_down = visible_lanes.clone();
    let badges_down = badges.clone();
    let bounds_right = canvas_bounds.clone();
    let lanes_right = visible_lanes.clone();
    let badges_right = badges;
    let bounds_up = canvas_bounds.clone();
    let lanes_up = visible_lanes.clone();
    let bounds_move = canvas_bounds;
    let lanes_move = visible_lanes;

    el.on_mouse_down(
        MouseButton::Left,
        cx.listener(move |this, event: &MouseDownEvent, _, cx| {
            if open_badge_menu(this, event, &badges_down, &lanes_down, cx) {
                return;
            }
            on_left_down(
                this,
                event,
                bounds_down.get(),
                &lanes_down,
                fps,
                pps,
                scroll_x,
                tool,
                cx,
            );
        }),
    )
    .on_mouse_down(
        MouseButton::Right,
        cx.listener(move |this, event: &MouseDownEvent, _, cx| {
            if open_badge_menu(this, event, &badges_right, &lanes_right, cx) {
                return;
            }
            on_right_down(
                this,
                event,
                bounds_right.get(),
                &lanes_right,
                fps,
                pps,
                scroll_x,
                cx,
            );
        }),
    )
    .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
        on_move(
            this,
            event,
            bounds_move.get(),
            &lanes_move,
            fps,
            pps,
            scroll_x,
            tool,
            cx,
        );
    }))
    .on_mouse_up(
        MouseButton::Left,
        cx.listener(move |this, event: &MouseUpEvent, _, cx| {
            // A Parent pick whip dragged out of the inspector lands on the clip
            // it is released over. A gesture this canvas started ends first.
            if ACTIVE_GESTURE.get().is_none() {
                let (local_x, local_y) = local_coords(event.position, bounds_up.get());
                if let Some(HitTarget::ClipBody { clip_id, .. }) =
                    hit_test(&lanes_up, local_x, local_y, fps, pps, scroll_x)
                    && land_parent_whip(this, clip_id, event.modifiers.shift, cx)
                {
                    return;
                }
            }
            on_left_up(this, event, fps, pps, tool, cx);
        }),
    )
}

/// Open the menu of the fx badge a press landed on. Returns whether it landed
/// on one, so the press does nothing else.
///
/// The badges are the ones the last paint drew, in window coordinates, so a
/// press reaches exactly the badge on screen. An armed Parent pick whip takes
/// the press instead, as it does anywhere on a clip.
fn open_badge_menu(
    this: &mut Shell,
    event: &MouseDownEvent,
    badges: &super::FxBadges,
    lanes: &[VisibleLane],
    cx: &mut Context<Shell>,
) -> bool {
    let Some(clip) = badges
        .borrow()
        .iter()
        .find(|(_, badge)| badge.contains(&event.position))
        .map(|(clip, _)| *clip)
    else {
        return false;
    };
    let Some(track) = lanes
        .iter()
        .find(|lane| lane.clips.iter().any(|c| c.id == clip))
        .map(|lane| lane.track_id)
    else {
        return false;
    };
    if this.state.read(cx).parent_whip().is_some() {
        return false;
    }
    cx.stop_propagation();
    let (x, y) = (f32::from(event.position.x), f32::from(event.position.y));
    this.state.update(cx, |state, _| {
        state.close_keyframe_context_menu();
        state.close_mask_path_key_menu();
        state.open_context(ContextTarget::ClipBadge { clip, track }, x, y);
    });
    cx.notify();
    true
}

/// Land an armed Parent pick whip on `clip_id`, snapping with `shift`.
/// Returns whether one was armed, so the press or release that landed it does
/// nothing else.
fn land_parent_whip(this: &mut Shell, clip_id: u64, shift: bool, cx: &mut Context<Shell>) -> bool {
    if this.state.read(cx).parent_whip().is_none() {
        return false;
    }
    this.state.update(cx, |state, _| {
        if let Err(err) = state.land_parent_whip(ClipId::from_raw(clip_id), shift) {
            state.report_error(&err);
        }
    });
    cx.notify();
    true
}

#[allow(clippy::too_many_arguments)]
fn on_left_down(
    this: &mut Shell,
    event: &MouseDownEvent,
    bounds: Bounds<Pixels>,
    lanes: &[VisibleLane],
    fps: f32,
    pps: f32,
    scroll_x: f32,
    tool: EditorTool,
    cx: &mut Context<Shell>,
) {
    let (local_x, local_y) = local_coords(event.position, bounds);
    let Some(hit) = hit_test(lanes, local_x, local_y, fps, pps, scroll_x) else {
        return;
    };
    let origin_x = f32::from(event.position.x);
    // A press anywhere on the canvas dismisses an open keyframe menu.
    this.state.update(cx, |state, _| {
        state.close_keyframe_context_menu();
        state.close_mask_path_key_menu();
    });

    // An armed Parent pick whip takes the press on a clip, whatever the tool.
    if let HitTarget::ClipBody { clip_id, .. } = hit
        && land_parent_whip(this, clip_id, event.modifiers.shift, cx)
    {
        cx.stop_propagation();
        return;
    }

    if tool.pans() {
        this.start_pan(origin_x, cx);
        return;
    }
    if tool.zooms() {
        let factor = if event.modifiers.alt {
            1.0 / crate::transport::ZOOM_STEP
        } else {
            crate::transport::ZOOM_STEP
        };
        this.zoom_at(origin_x, factor, cx);
        return;
    }
    if let Some(dir) = tool.track_select() {
        let (track_id, tick) = match hit {
            HitTarget::ClipEdge {
                track_id,
                start_ticks,
                ..
            }
            | HitTarget::ClipBody {
                track_id,
                start_ticks,
                ..
            } => (track_id, start_ticks),
            HitTarget::EmptyLane { track_id, tick, .. } => (track_id, tick),
            _ => {
                let first_track = lanes.first().map(|l| l.track_id).unwrap_or(0);
                (
                    first_track,
                    px_to_ticks((local_x + scroll_x).max(0.0), fps, pps),
                )
            }
        };
        this.state.update(cx, |state, _| {
            state.select_track_clips(
                TrackId::from_raw(track_id),
                tick,
                dir,
                event.modifiers.shift,
            );
        });
        cx.stop_propagation();
        cx.notify();
        return;
    }
    if tool.cuts() {
        let tick = px_to_ticks((local_x + scroll_x).max(0.0), fps, pps);
        let track_id = match hit {
            HitTarget::ClipEdge { track_id, .. }
            | HitTarget::ClipBody { track_id, .. }
            | HitTarget::EmptyLane { track_id, .. } => track_id,
            _ => lanes.first().map(|l| l.track_id).unwrap_or(0),
        };
        this.state.update(cx, |state, _| {
            if let Err(err) =
                state.split_at_tick(TrackId::from_raw(track_id), tick, event.modifiers.shift)
            {
                state.report_error(&err);
            }
        });
        cx.stop_propagation();
        cx.notify();
        return;
    }
    if tool.pens() {
        let tick = px_to_ticks((local_x + scroll_x).max(0.0), fps, pps);
        match hit {
            HitTarget::ClipBody {
                clip_id,
                locked: false,
                ..
            }
            | HitTarget::ClipEdge { clip_id, .. }
            | HitTarget::ClipLine { clip_id, .. } => {
                this.state.update(cx, |state, _| {
                    if let Err(err) = state.add_clip_line_keyframe(ClipId::from_raw(clip_id), tick)
                    {
                        state.report_error(&err);
                    }
                });
                cx.stop_propagation();
                cx.notify();
                return;
            }
            HitTarget::PropertyRow { clip, param, tick } => {
                this.state.update(cx, |state, _| {
                    if let Err(err) = state.add_param_keyframe_at(clip, param, tick) {
                        state.report_error(&err);
                    }
                });
                cx.stop_propagation();
                cx.notify();
                return;
            }
            _ => {}
        }
    }

    match hit {
        HitTarget::TransitionEdge {
            clip_id,
            right,
            duration_ticks,
        } => {
            cx.stop_propagation();
            ACTIVE_GESTURE.set(Some(CanvasGesture::Clip {
                kind: if right {
                    DragKind::TransitionRight
                } else {
                    DragKind::TransitionLeft
                },
                clip_id,
                origin_track_id: 0,
                target_track_id: 0,
                origin_x,
                start_ticks: duration_ticks,
                end_ticks: 0,
            }));
        }
        HitTarget::TransitionBody { clip_id } => {
            cx.stop_propagation();
            this.state.update(cx, |state, _| {
                state.select_clip(ClipId::from_raw(clip_id));
            });
            cx.notify();
        }
        HitTarget::Fade {
            clip_id,
            left,
            fade_ticks,
            ..
        } => {
            cx.stop_propagation();
            ACTIVE_GESTURE.set(Some(CanvasGesture::Clip {
                kind: if left {
                    DragKind::FadeIn
                } else {
                    DragKind::FadeOut
                },
                clip_id,
                origin_track_id: 0,
                target_track_id: 0,
                origin_x,
                start_ticks: fade_ticks,
                end_ticks: 0,
            }));
        }
        HitTarget::ClipEdge {
            clip_id,
            track_id,
            left,
            start_ticks,
            end_ticks,
        } => {
            cx.stop_propagation();
            let side = if left { TrimSide::In } else { TrimSide::Out };
            let span = Span {
                start: start_ticks,
                end: end_ticks,
                threshold: crate::snap::threshold_ticks(pps, fps),
                magnet: !event.modifiers.alt,
            };
            this.state.update(cx, |state, _| {
                state.select_edge(ClipId::from_raw(clip_id), side);
                let g = resolve_edge_gesture(
                    tool,
                    event.modifiers.control,
                    event.modifiers.alt,
                    left,
                    span,
                );
                let d = Drag {
                    clip: ClipId::from_raw(clip_id),
                    delta: 0,
                };
                state.trim_feedback = state.compute_trim_feedback(g, d);
            });
            ACTIVE_GESTURE.set(Some(CanvasGesture::Clip {
                kind: if left {
                    DragKind::TrimStart
                } else {
                    DragKind::TrimEnd
                },
                clip_id,
                origin_track_id: track_id,
                target_track_id: track_id,
                origin_x,
                start_ticks,
                end_ticks,
            }));
            cx.notify();
        }
        HitTarget::ClipBody {
            clip_id,
            track_id,
            start_ticks,
            end_ticks,
            locked,
            in_name_band,
        } => {
            cx.stop_propagation();
            if event.click_count >= 2 {
                this.state.update(cx, |state, _| {
                    if in_name_band {
                        state.begin_rename(RenameTarget::Clip(clip_id));
                    } else {
                        state.double_click_clip(clip_id);
                    }
                });
                cx.notify();
                return;
            }
            this.state.update(cx, |state, _| {
                if event.modifiers.shift || event.modifiers.control || event.modifiers.platform {
                    state.toggle_clip_in_selection(ClipId::from_raw(clip_id));
                } else {
                    state.select_clip_linked(ClipId::from_raw(clip_id), !event.modifiers.alt);
                }
            });
            if !locked {
                ACTIVE_GESTURE.set(Some(CanvasGesture::Clip {
                    kind: DragKind::Move,
                    clip_id,
                    origin_track_id: track_id,
                    target_track_id: track_id,
                    origin_x,
                    start_ticks,
                    end_ticks,
                }));
            }
            cx.notify();
        }
        HitTarget::ClipLine {
            clip_id,
            track_id,
            key,
            tick,
        } => {
            cx.stop_propagation();
            let clip = ClipId::from_raw(clip_id);
            if event.modifiers.control || event.modifiers.platform {
                this.state.update(cx, |state, _| {
                    if let Err(err) = state.add_clip_line_keyframe(clip, tick) {
                        state.report_error(&err);
                    }
                });
                crate::app::spawn_preview(this.state.clone(), cx);
                cx.notify();
                return;
            }
            let band = lanes
                .iter()
                .find(|lane| lane.track_id == track_id)
                .map_or(0.0, |lane| {
                    BarRows::of((lane.clip_y_bottom - lane.y_top).max(1.0)).band()
                });
            let at = key.map_or(LinePress::Line(tick), LinePress::Key);
            let held = this.state.update(cx, |state, _| {
                if !state.selection.contains(&clip) {
                    state.select_clip_linked(clip, !event.modifiers.alt);
                }
                state.press_clip_line(clip, at, band)
            });
            if held {
                ACTIVE_GESTURE.set(Some(CanvasGesture::Line {
                    origin_y: f32::from(event.position.y),
                }));
            }
            cx.notify();
        }
        HitTarget::Keyframe {
            clip, param, tick, ..
        } => {
            cx.stop_propagation();
            if event.click_count >= 2 {
                this.state.update(cx, |state, _| {
                    state.select_clip(clip);
                    state.select_timeline_keyframe(clip, param, tick, false);
                    state.open_keyframe_velocity();
                });
                cx.notify();
                return;
            }
            this.state.update(cx, |state, _| {
                state.select_clip(clip);
                state.begin_timeline_keyframe_drag(clip, param, tick, event.modifiers.shift);
            });
            ACTIVE_GESTURE.set(Some(CanvasGesture::KeyframeDrag { origin_x }));
            cx.notify();
        }
        HitTarget::PropertyRow { clip, param, tick } => {
            cx.stop_propagation();
            this.state.update(cx, |state, _| {
                state.select_clip(clip);
                state.select_timeline_property(clip, param, event.modifiers.shift);
                if !event.modifiers.shift {
                    state.clear_timeline_keyframe_selection();
                }
            });
            ACTIVE_GESTURE.set(Some(CanvasGesture::KeyframeMarquee {
                clip,
                param,
                origin_tick: tick,
                additive: event.modifiers.shift,
            }));
            cx.notify();
        }
        HitTarget::MaskPathKey {
            clip, mask, tick, ..
        } => {
            cx.stop_propagation();
            this.state.update(cx, |state, _| {
                state.begin_mask_path_key_drag(clip, mask, tick);
            });
            ACTIVE_GESTURE.set(Some(CanvasGesture::MaskPathKeyDrag { origin_x }));
            cx.notify();
        }
        HitTarget::MaskPathRow { clip, mask } => {
            cx.stop_propagation();
            this.state
                .update(cx, |state, _| state.focus_mask_path(clip, mask));
            cx.notify();
        }
        HitTarget::EmptyLane { track_id, tick, .. } => {
            this.state.update(cx, |state, _| {
                state.select_gap_at(TrackId::from_raw(track_id), tick);
                state.begin_marquee(track_id, tick);
            });
            ACTIVE_GESTURE.set(Some(CanvasGesture::Marquee {
                origin_x,
                moved: false,
            }));
            cx.notify();
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn on_right_down(
    this: &mut Shell,
    event: &MouseDownEvent,
    bounds: Bounds<Pixels>,
    lanes: &[VisibleLane],
    fps: f32,
    pps: f32,
    scroll_x: f32,
    cx: &mut Context<Shell>,
) {
    let (local_x, local_y) = local_coords(event.position, bounds);
    let Some(hit) = hit_test(lanes, local_x, local_y, fps, pps, scroll_x) else {
        return;
    };
    let (x, y) = (f32::from(event.position.x), f32::from(event.position.y));
    match hit {
        HitTarget::TransitionEdge { clip_id, .. } | HitTarget::TransitionBody { clip_id } => {
            cx.stop_propagation();
            this.state.update(cx, |state, _| {
                state.open_context(ContextTarget::Transition { clip: clip_id }, x, y);
            });
            cx.notify();
        }
        HitTarget::Fade {
            clip_id, track_id, ..
        }
        | HitTarget::ClipEdge {
            clip_id, track_id, ..
        }
        | HitTarget::ClipBody {
            clip_id, track_id, ..
        }
        | HitTarget::ClipLine {
            clip_id, track_id, ..
        } => {
            cx.stop_propagation();
            this.state.update(cx, |state, _| {
                if !state.selection.contains(&ClipId::from_raw(clip_id)) {
                    state.select_clip_linked(ClipId::from_raw(clip_id), !event.modifiers.alt);
                }
                state.open_context(
                    ContextTarget::Clip {
                        clip: clip_id,
                        track: track_id,
                    },
                    x,
                    y,
                );
            });
            cx.notify();
        }
        HitTarget::Keyframe {
            clip,
            param,
            tick,
            interior,
            hold,
            roving,
        } => {
            cx.stop_propagation();
            this.state.update(cx, |state, _| {
                state.close_mask_path_key_menu();
                state.select_clip(clip);
                if !state.is_timeline_keyframe_selected(clip, param, tick) {
                    state.select_timeline_keyframe(clip, param, tick, false);
                }
                state.open_keyframe_context_menu(KeyframeContextMenu {
                    clip,
                    param,
                    tick,
                    interior,
                    hold,
                    roving,
                    x,
                    y,
                });
            });
            cx.notify();
        }
        HitTarget::MaskPathKey {
            clip,
            mask,
            tick,
            hold,
        } => {
            cx.stop_propagation();
            this.state.update(cx, |state, _| {
                state.close_keyframe_context_menu();
                state.open_mask_path_key_menu(MaskPathKeyMenu {
                    clip,
                    mask,
                    tick,
                    hold,
                    x,
                    y,
                });
            });
            cx.notify();
        }
        HitTarget::PropertyRow { .. } | HitTarget::MaskPathRow { .. } => {}
        HitTarget::EmptyLane { .. } => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn on_move(
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
    let Some(active) = ACTIVE_GESTURE.get() else {
        return;
    };
    if !event.dragging() {
        ACTIVE_GESTURE.set(None);
        // The button came up where this canvas did not see it.
        if matches!(active, CanvasGesture::Line { .. }) {
            this.state.update(cx, |state, _| state.release_clip_line());
            cx.notify();
        }
        return;
    }

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
            if matches!(kind, DragKind::Move)
                && let Some(hovered_lane) = lanes
                    .iter()
                    .find(|l| local_y >= l.y_top && local_y <= l.y_bottom && !l.locked)
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
            let delta_ticks = ((delta_px / pps.max(1.0)) * fps.max(1.0)).round() as i64;
            let threshold = crate::snap::threshold_ticks(pps, fps);
            let span = Span {
                start: start_ticks,
                end: end_ticks,
                threshold,
                magnet: !event.modifiers.alt,
            };
            this.state.update(cx, |state, _| {
                let candidate = match kind {
                    DragKind::Move => Some((start_ticks + delta_ticks).max(0)),
                    DragKind::TrimStart => {
                        Some((start_ticks + delta_ticks).clamp(0, end_ticks - 1))
                    }
                    DragKind::TrimEnd => Some((end_ticks + delta_ticks).max(start_ticks + 1)),
                    _ => None,
                };
                state.snap_guide = candidate.and_then(|t| {
                    state
                        .snap_at(t, Some(clip_id), threshold, event.modifiers.alt)
                        .1
                });
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
                state.trim_feedback = state.compute_trim_feedback(g, d);
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
        CanvasGesture::KeyframeMarquee {
            clip,
            param,
            origin_tick,
            additive,
        } => {
            let tick = px_to_ticks((local_x + scroll_x).max(0.0), fps, pps);
            this.state.update(cx, |state, _| {
                state.marquee_select_keyframes(clip, param, origin_tick, tick, additive);
            });
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
    }
}

fn on_left_up(
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
            target_track_id,
            origin_x,
            start_ticks,
            end_ticks,
            ..
        } => {
            let delta_px = f32::from(event.position.x) - origin_x;
            let delta_ticks = ((delta_px / pps.max(1.0)) * fps.max(1.0)).round() as i64;
            let threshold_px = match kind {
                DragKind::Move => 2.0,
                _ => 1.0,
            };
            let span = Span {
                start: start_ticks,
                end: end_ticks,
                threshold: crate::snap::threshold_ticks(pps, fps),
                magnet: !event.modifiers.alt,
            };
            this.state.update(cx, |state, _| {
                state.snap_guide = None;
                state.trim_feedback = None;
                if delta_px.abs() < threshold_px {
                    return;
                }
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
                        state.status_message = Some(format!("Selected {count} clip(s)"));
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
        CanvasGesture::KeyframeMarquee { .. } => {
            cx.notify();
        }
        CanvasGesture::Line { .. } => {
            this.state.update(cx, |state, _| state.release_clip_line());
            crate::app::spawn_preview(this.state.clone(), cx);
            cx.notify();
        }
    }
}

fn resolve_edge_gesture(
    tool: EditorTool,
    ctrl: bool,
    alt: bool,
    left: bool,
    span: Span,
) -> Gesture {
    let drag = tool.edge_tool(ctrl, alt).edge_drag();
    Gesture::Edge { drag, left, span }
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

fn local_coords(pos: Point<Pixels>, bounds: Bounds<Pixels>) -> (f32, f32) {
    (
        f32::from(pos.x - bounds.origin.x).max(0.0),
        f32::from(pos.y - bounds.origin.y).max(0.0),
    )
}
