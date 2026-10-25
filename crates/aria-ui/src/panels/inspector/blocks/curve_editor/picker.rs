//! Graph block: the property picker, the axis picker, and the graph.
//!
//! Nothing here holds drag state. A press, a move and a release go straight to
//! [`crate::app::curve_drag`], which owns the draft between events and commits
//! it as one edit; the panel's job is to report where it drew the graph so a
//! window pointer can be read against it.

use gpui::{
    AnyElement, Bounds, ContentMask, Hsla, InteractiveElement, IntoElement, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, Styled, canvas, div,
    point, px, size,
};

use crate::app::AppState;
use crate::app::curve_geometry::CurvePolyline;
use crate::app::curve_stroke::{LEVER_STROKE, curve_columns, segment_columns};
use crate::components::{self, section_label};
use crate::motion::MotionKey;
use crate::panels::inspector::blocks::widgets::{Columns, chips_under_labels, under_labels};
use crate::theme::Theme;
use gpui::prelude::FluentBuilder;

/// Height of the curve editor canvas in the inspector.
pub const CURVE_EDITOR_HEIGHT: f32 = 140.0;

/// Diameter of keyframe dots in pixels.
pub const KEYFRAME_DOT_SIZE: f32 = 8.0;

/// Diameter of bezier control handles in pixels.
pub const HANDLE_DOT_SIZE: f32 = 6.0;

/// The axis labels a two-dimensional property is graphed on, in axis order.
pub const AXIS_LABELS: [&str; 2] = ["X", "Y"];

/// The Graph block: the curve of one keyframed property of the selected clip.
/// `None` while no property of the selection holds a keyframe, since a
/// constant property draws a flat line with no point to drag.
pub fn curve_editor_block(
    theme: &Theme,
    state: &gpui::Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Option<AnyElement> {
    let (params, active_param, component, polyline, dragging) = {
        let read = state.read(cx);
        let active = read.curve_param();
        let component = active.and_then(|param| read.curve_component(param));
        let polyline = active.and_then(|param| read.curve_polyline(param, component));
        (
            read.key_params(),
            active,
            component,
            polyline,
            read.curve_dragging().is_some(),
        )
    };

    if params.iter().all(|row| row.marks.is_empty()) {
        return None;
    }
    let active_param = active_param?;

    // The keyframed properties, and the graphed one when a property row chose
    // a constant property, so the lit chip always states what the graph draws.
    let param_chips: Vec<_> = params
        .iter()
        .filter(|row| !row.marks.is_empty() || row.param() == active_param)
        .map(|row| {
            let param = row.param();
            components::chip(
                MotionKey::new("curve-param", &row.id),
                row.label.clone(),
                param == active_param,
                components::ChipSize::Compact,
                theme,
            )
            .debug_selector(|| format!("curve-param-{}", row.id))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    this.state
                        .update(cx, |state, _| state.set_curve_param(param));
                    cx.notify();
                }),
            )
            .into_any_element()
        })
        .collect();

    // An axis row appears only for a property that has two: a scalar has one
    // curve, and offering a dead "Y" would be a control that does nothing.
    let axis_chips: Vec<_> = match component {
        None => Vec::new(),
        Some(active_axis) => AXIS_LABELS
            .iter()
            .enumerate()
            .map(|(index, label)| {
                let axis = index as u8;
                components::chip(
                    MotionKey::new("curve-axis", label),
                    *label,
                    axis == active_axis,
                    components::ChipSize::Compact,
                    theme,
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        this.state.update(cx, |state, _| state.set_curve_axis(axis));
                        cx.notify();
                    }),
                )
                .into_any_element()
            })
            .collect(),
    };

    let graph_element = div()
        .id("curve-editor-graph")
        .debug_selector(|| "curve-editor-graph".into())
        .relative()
        .w_full()
        .h(px(CURVE_EDITOR_HEIGHT))
        .rounded(px(Theme::CONTROL_RADIUS))
        .bg(theme.surface)
        .border_1()
        .border_color(theme.border)
        .child(render_canvas(theme, polyline))
        // Where the graph landed, in window pixels: a pointer arrives in those,
        // and the geometry is in the graph's own. Measured on every draw and
        // notified only when it moved, so a repaint is not a repaint loop.
        .child(measure(state))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                let x = f32::from(event.position.x);
                let y = f32::from(event.position.y);
                let grabbed = this.state.update(cx, |state, _| {
                    state.begin_curve_drag(active_param, component, x, y)
                });
                if grabbed {
                    cx.notify();
                }
            }),
        )
        .when(dragging, |el| {
            el.on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                let x = f32::from(event.position.x);
                let y = f32::from(event.position.y);
                let moved = this
                    .state
                    .update(cx, |state, _| state.update_curve_drag(x, y));
                if moved {
                    cx.notify();
                }
            }))
        })
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(|this, _: &MouseUpEvent, _, cx| {
                let dragging = this
                    .state
                    .update(cx, |state, _| state.curve_dragging().is_some());
                if !dragging {
                    return;
                }
                this.state.update(cx, |state, _| {
                    if let Err(err) = state.commit_curve_drag() {
                        state.report_error(&err);
                    }
                });
                cx.notify();
            }),
        );

    Some(
        div()
            .id("curve-editor-container")
            .flex()
            .flex_col()
            .gap(px(Theme::SPACE_XS))
            .child(section_label(theme, "Graph"))
            .child(chips_under_labels(
                Columns::Keyed,
                components::ChipSize::Compact,
                param_chips,
            ))
            .when(!axis_chips.is_empty(), |el| {
                el.child(chips_under_labels(
                    Columns::Keyed,
                    components::ChipSize::Compact,
                    axis_chips,
                ))
            })
            .child(under_labels(
                Columns::Keyed,
                [graph_element.into_any_element()],
            ))
            .into_any_element(),
    )
}

/// Report the graph's window rectangle to the state, so a pointer can be read
/// against it. Draws nothing.
fn measure(state: &gpui::Entity<AppState>) -> impl IntoElement {
    let state = gpui::Entity::clone(state);
    canvas(
        move |bounds: Bounds<Pixels>, _, cx| {
            let frame = [
                f32::from(bounds.origin.x),
                f32::from(bounds.origin.y),
                f32::from(bounds.size.width),
                f32::from(bounds.size.height),
            ];
            if state.update(cx, |state, _| state.record_curve_frame(frame)) {
                crate::shell::notify_after_draw(&state, cx);
            }
        },
        |_, _, _, _| (),
    )
    .absolute()
    .size_full()
}

/// The colors the graph paints its marks in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct GraphInk {
    /// The reference line at the graph's middle value.
    pub(super) rule: Hsla,
    /// The sampled curve.
    pub(super) curve: Hsla,
    /// A handle's lever and its tip, in the color the timeline graph draws a
    /// handle in.
    pub(super) handle: Hsla,
    /// A keyframe.
    pub(super) key: Hsla,
}

impl GraphInk {
    fn of(theme: &Theme) -> Self {
        Self {
            rule: theme.border,
            curve: theme.accent,
            handle: theme.warning,
            key: theme.text,
        }
    }
}

/// One filled rectangle the graph paints, `[x, y, w, h]` in graph pixels, and
/// its color.
pub(super) type GraphMark = ([f32; 4], Hsla);

/// What the graph paints, back to front: the reference line at its middle
/// value, the curve, each handle's lever from its keyframe to its tip and the
/// tip, then each keyframe over them. `width` and `height` are the graph's; a
/// lever or a tip past its edge is cut by the painter's content mask.
pub(super) fn graph_marks(
    polyline: Option<&CurvePolyline>,
    width: f32,
    height: f32,
    ink: GraphInk,
) -> impl Iterator<Item = GraphMark> + '_ {
    let rule = std::iter::once(([0.0, height * 0.5, width, 1.0], ink.rule));
    let drawn = polyline.into_iter().flat_map(move |poly| {
        let curve = curve_columns(&poly.samples, height)
            .map(move |column| ([column.x, column.y, column.w, column.h], ink.curve));
        let handles = poly.handles.iter().flat_map(move |handle| {
            let tip = (handle.handle_x, handle.handle_y);
            segment_columns((handle.anchor_x, handle.anchor_y), tip, LEVER_STROKE)
                .map(move |column| ([column.x, column.y, column.w, column.h], ink.handle))
                .chain(std::iter::once((dot(tip, HANDLE_DOT_SIZE), ink.handle)))
        });
        let keys = poly
            .key_points
            .iter()
            .map(move |key| (dot((key.pixel_x, key.pixel_y), KEYFRAME_DOT_SIZE), ink.key));
        curve.chain(handles).chain(keys)
    });
    rule.chain(drawn)
}

/// A square `side` pixels wide centered on `at`.
fn dot(at: (f32, f32), side: f32) -> [f32; 4] {
    [at.0 - side * 0.5, at.1 - side * 0.5, side, side]
}

/// Paint [`graph_marks`] at the canvas's origin, cut at its edges.
fn render_canvas(theme: &Theme, polyline: Option<CurvePolyline>) -> impl IntoElement {
    let ink = GraphInk::of(theme);
    canvas(
        |_, _, _| (),
        move |bounds: Bounds<Pixels>, _, window, _| {
            let (ox, oy) = (f32::from(bounds.origin.x), f32::from(bounds.origin.y));
            let (width, height) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
            window.with_content_mask(Some(ContentMask::from(bounds)), |window| {
                for ([x, y, w, h], color) in graph_marks(polyline.as_ref(), width, height, ink) {
                    window.paint_quad(gpui::fill(
                        Bounds {
                            origin: point(px(ox + x), px(oy + y)),
                            size: size(px(w), px(h)),
                        },
                        color,
                    ));
                }
            });
        },
    )
    .size_full()
}
