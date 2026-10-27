//! Motion paths over the picture.
//!
//! The paths come from [`AppState::motion_paths`]. The dots and the lines to
//! the tangent handles are painted; the keyframe squares and the handles are
//! elements above them, because each takes the pointer. A Ctrl+click on a
//! painted dot adds a keyframe there, so the picture area takes that press and
//! lets every other press through to the transform handles and the picture.

use std::sync::Arc;

use gpui::{
    Bounds, ContentMask, Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    ParentElement, PathBuilder, StatefulInteractiveElement, Styled, canvas, div, fill, point, px,
    size,
};

use crate::app::AppState;
use crate::app::canvas_overlay::motion_path::{DrawnPath, PathKey, PathParam, TangentSide};
use crate::motion::MotionKey;
use crate::surfaces::overlay_gesture::{self, Gesture, Step};
use crate::theme::Theme;
use crate::types::OverlayMode;

/// A keyframe square's side, in pixels.
const KEY: f32 = 9.0;
/// A tangent handle's diameter, in pixels.
const TANGENT: f32 = 8.0;
/// A painted dot's side, in pixels.
const DOT: f32 = 2.0;

/// The colour a path is drawn in.
fn colour(theme: &Theme, param: PathParam) -> gpui::Hsla {
    match param {
        PathParam::Position => theme.accent,
        PathParam::AnchorPoint => theme.playhead,
    }
}

/// The selected clip's motion paths, or nothing when no keyframed point has a
/// path to draw or another handle set is live.
pub(super) fn overlay(
    state: &Entity<AppState>,
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Option<gpui::AnyElement> {
    let read = state.read(cx);
    if read.overlay_mode() != OverlayMode::Transform {
        return None;
    }
    let paths: Arc<[DrawnPath]> = read.motion_paths().into();
    if paths.is_empty() {
        return None;
    }
    let frame = read.overlay_frame;

    let painted = Arc::clone(&paths);
    let (position, anchor) = (
        colour(theme, PathParam::Position),
        colour(theme, PathParam::AnchorPoint),
    );
    let lines = canvas(
        |_, _, _| (),
        move |bounds, (), window, _| {
            window.with_content_mask(Some(ContentMask::from(bounds)), |window| {
                for path in painted.iter() {
                    let colour = match path.param {
                        PathParam::Position => position,
                        PathParam::AnchorPoint => anchor,
                    };
                    let mut levers = PathBuilder::stroke(px(1.0));
                    let mut any = false;
                    for key in &path.keys {
                        for side in TangentSide::ALL {
                            if let Some(handle) = key.handle(side) {
                                levers.move_to(point(px(key.at[0]), px(key.at[1])));
                                levers.line_to(point(px(handle[0]), px(handle[1])));
                                any = true;
                            }
                        }
                    }
                    if any && let Ok(levers) = levers.build() {
                        window.paint_path(levers, colour);
                    }
                    for dot in &path.dots {
                        window.paint_quad(fill(
                            Bounds {
                                origin: point(px(dot[0] - DOT * 0.5), px(dot[1] - DOT * 0.5)),
                                size: size(px(DOT), px(DOT)),
                            },
                            colour,
                        ));
                    }
                }
            });
        },
    )
    .absolute()
    .size_full();

    let mut root = div()
        .absolute()
        .size_full()
        .child(lines)
        // A Ctrl+click on the path with the Pen tool adds a keyframe. Every
        // other press goes on to the handles and the picture below.
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, event: &MouseDownEvent, _, cx| {
                let at = [f32::from(event.position.x), f32::from(event.position.y)];
                if !this
                    .state
                    .read(cx)
                    .can_add_key_on_path(event.modifiers.secondary(), at)
                {
                    return;
                }
                cx.stop_propagation();
                this.state.update(cx, |state, _| {
                    overlay_gesture::run(state, Step::Grab(Gesture::PathAdd, at))
                });
                cx.notify();
            }),
        );
    for path in paths.iter() {
        let colour = colour(theme, path.param);
        for drawn in &path.keys {
            let key = drawn.key;
            for side in TangentSide::ALL {
                let Some(handle) = drawn.handle(side) else {
                    continue;
                };
                root = root.child(tangent_dot(
                    key,
                    side,
                    frame.local(handle),
                    colour,
                    theme,
                    cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                        let at = [f32::from(event.position.x), f32::from(event.position.y)];
                        this.state.update(cx, |state, _| {
                            overlay_gesture::run(
                                state,
                                Step::Grab(Gesture::PathTangent(key, side), at),
                            )
                        });
                        cx.notify();
                    }),
                ));
            }
            root = root.child(key_square(
                key,
                frame.local(drawn.at),
                if drawn.current {
                    colour
                } else {
                    theme.on_solid
                },
                colour,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    let at = [f32::from(event.position.x), f32::from(event.position.y)];
                    let gesture = if event.modifiers.alt {
                        Gesture::PathCorner(key)
                    } else {
                        Gesture::PathKey(key)
                    };
                    this.state.update(cx, |state, _| {
                        overlay_gesture::run(state, Step::Grab(gesture, at))
                    });
                    cx.notify();
                }),
            ));
        }
    }

    // A path drag is carried and released by the shell
    // (`Shell::carry_gesture`), so it follows the pointer off the picture.
    Some(root.into_any_element())
}

/// A keyframe's square, centred on `point`. The press is the square's alone:
/// no move of the picture under it, and no pick of the clip.
fn key_square(
    key: PathKey,
    point: [f32; 2],
    fill: gpui::Hsla,
    border: gpui::Hsla,
    on_down: impl Fn(&MouseDownEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> impl IntoElement {
    div()
        .id(MotionKey::new(key.param.id(), key.index).element_id("path-key"))
        .absolute()
        .left(px(point[0] - KEY * 0.5))
        .top(px(point[1] - KEY * 0.5))
        .w(px(KEY))
        .h(px(KEY))
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

/// One tangent handle of a smooth keyframe, centred on `point`.
fn tangent_dot(
    key: PathKey,
    side: TangentSide,
    point: [f32; 2],
    colour: gpui::Hsla,
    theme: &Theme,
    on_down: impl Fn(&MouseDownEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> impl IntoElement {
    div()
        .id(MotionKey::new(key.param.id(), key.index)
            .with(side.id())
            .element_id("path-tangent"))
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
