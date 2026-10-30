//! A mask path over the picture.
//!
//! The path comes from [`AppState::drawn_mask_path`]. The outline, the tangent
//! levers and the rubber band to the pointer are painted; the vertices and the
//! tangent handles are elements above them, because each takes the pointer.
//! With the Pen, a press anywhere else on the picture is the Pen's: a vertex
//! of the path being drawn, a vertex inserted on a segment, or a new path.

use std::sync::Arc;

use gpui::{
    ContentMask, Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    MouseMoveEvent, ParentElement, PathBuilder, StatefulInteractiveElement, Styled, canvas, div,
    point, px,
};

use crate::app::AppState;
use crate::app::canvas_overlay::motion_path::TangentSide;
use crate::app::mask_path::{DrawnMaskPath, DrawnVertex};
use crate::motion::MotionKey;
use crate::surfaces::overlay_gesture::{self, Gesture, Step};
use crate::theme::Theme;
use gpui::prelude::FluentBuilder;

/// A vertex square's side, in pixels.
const VERTEX: f32 = 8.0;
/// A tangent handle's diameter, in pixels.
const TANGENT: f32 = 7.0;

fn at(point_xy: [f32; 2]) -> gpui::Point<gpui::Pixels> {
    point(px(point_xy[0]), px(point_xy[1]))
}

/// The cubic from vertex `a` to vertex `b`.
fn segment(path: &mut PathBuilder, a: &DrawnVertex, b: &DrawnVertex) {
    path.cubic_bezier_to(at(b.at), at(a.handle_out), at(b.handle_in));
}

/// Paint the outline, the tangent levers, and the rubber band.
fn paint(drawn: &DrawnMaskPath, colour: gpui::Hsla, window: &mut gpui::Window) {
    let mut lines = PathBuilder::stroke(px(1.0));
    let Some(first) = drawn.vertices.first() else {
        return;
    };
    lines.move_to(at(first.at));
    for pair in drawn.vertices.windows(2) {
        segment(&mut lines, &pair[0], &pair[1]);
    }
    if drawn.closed
        && let Some(last) = drawn.vertices.last()
    {
        segment(&mut lines, last, first);
    }
    for vertex in &drawn.vertices {
        for side in TangentSide::ALL {
            if let Some(handle) = vertex.handle(side) {
                lines.move_to(at(vertex.at));
                lines.line_to(at(handle));
            }
        }
    }
    if let Some([from, to]) = drawn.rubber {
        lines.move_to(at(from));
        lines.line_to(at(to));
    }
    if let Ok(lines) = lines.build() {
        window.paint_path(lines, colour);
    }
}

/// The selected clip's mask path and the Pen's surface, or nothing when
/// neither the Pen nor the Mask handles are live on a framed clip.
pub(super) fn overlay(
    state: &Entity<AppState>,
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Option<gpui::AnyElement> {
    let read = state.read(cx);
    if !read.mask_path_live() {
        return None;
    }
    let pen = read.tool.pens();
    let holding = read.mask_path_holding();
    let drawing = read.mask_path_drawing();
    let frame = read.overlay_frame;
    let drawn = read.drawn_mask_path().map(Arc::new);
    let colour = theme.playhead;

    let mut root = div()
        .absolute()
        .size_full()
        .when(pen, |el| el.cursor(gpui::CursorStyle::Crosshair))
        // A Ctrl+click on a motion path is the motion path's; every other Pen
        // press on the picture is the mask path's.
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, event: &MouseDownEvent, _, cx| {
                let at = [f32::from(event.position.x), f32::from(event.position.y)];
                if !this
                    .state
                    .read(cx)
                    .pen_takes_press(at, event.modifiers.secondary())
                {
                    return;
                }
                cx.stop_propagation();
                this.state.update(cx, |state, _| {
                    overlay_gesture::run(state, Step::Grab(Gesture::MaskPathPen, at))
                });
                cx.notify();
            }),
        );

    if let Some(drawn) = drawn {
        let painted = Arc::clone(&drawn);
        root = root.child(
            canvas(
                |_, _, _| (),
                move |bounds, (), window, _| {
                    window.with_content_mask(Some(ContentMask::from(bounds)), |window| {
                        paint(&painted, colour, window);
                    });
                },
            )
            .absolute()
            .size_full(),
        );
        for (index, vertex) in drawn.vertices.iter().enumerate() {
            for side in TangentSide::ALL {
                let Some(handle) = vertex.handle(side) else {
                    continue;
                };
                root = root.child(tangent_dot(
                    index,
                    side,
                    frame.local(handle),
                    colour,
                    theme,
                    cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                        let at = [f32::from(event.position.x), f32::from(event.position.y)];
                        this.state.update(cx, |state, _| {
                            overlay_gesture::run(
                                state,
                                Step::Grab(Gesture::MaskTangent(index, side), at),
                            )
                        });
                        cx.notify();
                    }),
                ));
            }
            // The first vertex of a path being drawn is filled: a click on it
            // closes the path.
            let fill = if drawing && index == 0 {
                colour
            } else {
                theme.on_solid
            };
            root = root.child(vertex_square(
                index,
                frame.local(vertex.at),
                fill,
                colour,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    let at = [f32::from(event.position.x), f32::from(event.position.y)];
                    let gesture = this.state.read(cx).mask_vertex_gesture(
                        index,
                        event.modifiers.secondary(),
                        event.modifiers.alt,
                    );
                    this.state.update(cx, |state, _| {
                        overlay_gesture::run(state, Step::Grab(gesture, at))
                    });
                    cx.notify();
                }),
            ));
        }
    }

    // A held vertex or tangent is carried and released by the shell
    // (`Shell::carry_gesture`). Between the clicks that draw, the rubber band
    // follows the pointer over the picture, which the Viewer draws.
    let root = root.when(drawing && !holding, |el| {
        el.on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
            let at = [f32::from(event.position.x), f32::from(event.position.y)];
            if this.state.update(cx, |state, _| state.hover_mask_path(at)) {
                this.redraw(crate::shell::Moved::Frame, cx);
            }
        }))
    });
    Some(root.into_any_element())
}

/// A vertex's square, centred on `point`. The press is the square's alone: no
/// Pen press on the picture under it, no move, and no pick of the clip.
fn vertex_square(
    index: usize,
    point: [f32; 2],
    fill: gpui::Hsla,
    border: gpui::Hsla,
    on_down: impl Fn(&MouseDownEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> impl IntoElement {
    div()
        .id(MotionKey::new("mask-vertex", index).element_id("handle"))
        .absolute()
        .left(px(point[0] - VERTEX * 0.5))
        .top(px(point[1] - VERTEX * 0.5))
        .w(px(VERTEX))
        .h(px(VERTEX))
        .bg(fill)
        .border_1()
        .border_color(border)
        .cursor(gpui::CursorStyle::PointingHand)
        .active(|style| style.opacity(0.92))
        .on_mouse_down(MouseButton::Left, move |event, window, cx| {
            cx.stop_propagation();
            on_down(event, window, cx);
        })
}

/// One tangent handle of a smooth vertex, centred on `point`.
fn tangent_dot(
    index: usize,
    side: TangentSide,
    point: [f32; 2],
    colour: gpui::Hsla,
    theme: &Theme,
    on_down: impl Fn(&MouseDownEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> impl IntoElement {
    div()
        .id(MotionKey::new("mask-vertex", index)
            .with(side.id())
            .element_id("tangent"))
        .absolute()
        .left(px(point[0] - TANGENT * 0.5))
        .top(px(point[1] - TANGENT * 0.5))
        .w(px(TANGENT))
        .h(px(TANGENT))
        .rounded(px(TANGENT * 0.5))
        .bg(colour)
        .border_1()
        .border_color(theme.on_solid)
        .cursor(gpui::CursorStyle::PointingHand)
        .active(|style| style.opacity(0.92))
        .on_mouse_down(MouseButton::Left, move |event, window, cx| {
            cx.stop_propagation();
            on_down(event, window, cx);
        })
}
