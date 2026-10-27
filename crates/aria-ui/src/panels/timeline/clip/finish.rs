//! Pointer gesture dispatch for a virtualized lane-group canvas.

use std::cell::Cell;
use std::rc::Rc;

use aria_models::{ClipId, TrackId};
use gpui::{
    Bounds, Context, CursorStyle, InteractiveElement, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, Point, Stateful,
};

use super::VisibleLane;
use super::bar::BarRows;
use super::hit::{HitTarget, cursor_for, hit_test};
use crate::app::clip_line::LinePress;
use crate::panels::timeline::px_to_ticks;
use crate::shell::{Hold, Shell};
use crate::surfaces::timeline_gesture::{self as gesture, Drag, Gesture, Span};
use crate::types::{ContextTarget, EditorTool, RenameTarget, TrimSide};

#[path = "finish_drag.rs"]
mod drag;
#[path = "finish_press.rs"]
mod press;

use drag::{cancel, carry, hover, on_left_up};
use press::on_left_down;

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
    /// A band on a property row; the state holds where it began.
    KeyBand,
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
            // The shell carries the gesture the press began, so a clip, a
            // trim or a band follows the pointer off this lane group and the
            // release lands wherever the button comes up.
            if ACTIVE_GESTURE.get().is_some() {
                let (bounds, lanes) = (bounds_down.clone(), lanes_down.clone());
                this.hold(Hold::new(
                    move |this, event, cx| {
                        carry(
                            this,
                            event,
                            bounds.get(),
                            &lanes,
                            fps,
                            pps,
                            scroll_x,
                            tool,
                            cx,
                        );
                    },
                    move |this, event, cx| on_left_up(this, event, fps, pps, tool, cx),
                    cancel,
                ));
            }
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
        hover(
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
            // it is released over. A gesture this canvas began ends at the
            // shell's release.
            if ACTIVE_GESTURE.get().is_none() {
                let (local_x, local_y) = local_coords(event.position, bounds_up.get());
                if let Some(HitTarget::ClipBody { clip_id, .. }) =
                    hit_test(&lanes_up, local_x, local_y, fps, pps, scroll_x)
                {
                    land_parent_whip(this, clip_id, event.modifiers.shift, cx);
                }
            }
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
        HitTarget::Keyframe { clip, param, tick } => {
            cx.stop_propagation();
            this.state.update(cx, |state, _| {
                state.select_clip(clip);
                let target = ContextTarget::Keyframe {
                    clip: clip.raw(),
                    param,
                    tick,
                };
                state.open_context(target, x, y);
            });
            cx.notify();
        }
        HitTarget::MaskPathKey { clip, mask, tick } => {
            cx.stop_propagation();
            this.state.update(cx, |state, _| {
                let target = ContextTarget::MaskPathKey {
                    clip: clip.raw(),
                    mask,
                    tick,
                };
                state.open_context(target, x, y);
            });
            cx.notify();
        }
        HitTarget::PropertyRow { .. } | HitTarget::MaskPathRow { .. } => {}
        HitTarget::EmptyLane { .. } => {}
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

fn local_coords(pos: Point<Pixels>, bounds: Bounds<Pixels>) -> (f32, f32) {
    (
        f32::from(pos.x - bounds.origin.x).max(0.0),
        f32::from(pos.y - bounds.origin.y).max(0.0),
    )
}
