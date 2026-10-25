//! Curve editor block: the property picker, the graph, and the easing row.
//!
//! Nothing here holds drag state. A press, a move and a release go straight to
//! [`crate::app::curve_drag`], which owns the draft between events and commits
//! it as one edit; the panel's job is to report where it drew the graph so a
//! window pointer can be read against it.

use gpui::{
    Bounds, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, ParentElement, Pixels, Styled, canvas, div, point, px, size,
};

use crate::app::AppState;
use crate::app::clip_keys::easing_presets;
use crate::app::curve_geometry::CurvePolyline;
use crate::app::curve_stroke::curve_columns;
use crate::components;
use crate::panels::inspector::blocks::widgets::{
    Columns, chips_under_labels, ghosts_under_labels, pick_button, under_labels,
};
use crate::surfaces::keyframe_rows::{self, Row};
use crate::theme::Theme;
use crate::types::UiClip;
use gpui::prelude::FluentBuilder;

/// Height of the curve editor canvas in the inspector.
pub const CURVE_EDITOR_HEIGHT: f32 = 140.0;

/// Diameter of keyframe dots in pixels.
pub const KEYFRAME_DOT_SIZE: f32 = 8.0;

/// Diameter of bezier control handles in pixels.
pub const HANDLE_DOT_SIZE: f32 = 6.0;

/// The axis labels a two-dimensional property is graphed on, in axis order.
pub const AXIS_LABELS: [&str; 2] = ["X", "Y"];

/// Renders the curve editor block for animatable properties of the selected clip.
pub fn curve_editor_block(
    theme: &Theme,
    clip: &UiClip,
    state: &gpui::Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let _ = clip;
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

    let Some(active_param) = active_param else {
        return div().id("curve-editor-empty").into_any_element();
    };

    // Every animatable property of the selection is offered, from the same rows
    // the keyframe strip draws, so a property the model can animate is a
    // property the graph can draw.
    let param_chips: Vec<_> = params
        .iter()
        .map(|row| {
            let param = row.param();
            components::chip(
                crate::motion::MotionKey::new("curve-param", &row.id),
                row.label.clone(),
                param == active_param,
                components::ChipSize::Compact,
                theme,
            )
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
                    crate::motion::MotionKey::new("curve-axis", label),
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

    let easing_chips: Vec<_> = easing_presets()
        .iter()
        .map(|preset| {
            let preset_id = preset.id.as_str();
            pick_button(
                crate::motion::MotionKey::new("ce-preset", &preset.id),
                gpui::SharedString::new_static(preset.label.as_str()),
                theme,
                cx,
                move |state| {
                    keyframe_rows::apply(
                        state,
                        Row::Easing {
                            param: active_param,
                            preset: preset_id,
                        },
                    )
                },
            )
            .into_any_element()
        })
        .collect();

    let graph_element = div()
        .id("curve-editor-graph")
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

    div()
        .id("curve-editor-container")
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
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
        .child(ghosts_under_labels(Columns::Keyed, easing_chips))
        .into_any_element()
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

/// Paint the curve graph background, grid, sampled polyline, keyframe dots, and bezier handles.
fn render_canvas(theme: &Theme, polyline: Option<CurvePolyline>) -> impl IntoElement {
    let theme_accent = theme.accent;
    let theme_border = theme.border;
    let theme_text = theme.text;

    canvas(
        |_, _, _| (),
        move |bounds: Bounds<Pixels>, _, window, _| {
            let width = f32::from(bounds.size.width);
            let height = f32::from(bounds.size.height);
            let ox = f32::from(bounds.origin.x);
            let oy = f32::from(bounds.origin.y);

            // Draw horizontal reference line (midpoint / 0.5 value)
            let mid_y = oy + height * 0.5;
            window.paint_quad(gpui::fill(
                Bounds {
                    origin: point(px(ox), px(mid_y)),
                    size: size(px(width), px(1.0)),
                },
                theme_border,
            ));

            let Some(poly) = &polyline else {
                return;
            };

            // The curve is drawn as pixel-wide columns spanning the y it
            // crosses, so a slope reads as a line rather than a staircase.
            for column in curve_columns(&poly.samples, height) {
                window.paint_quad(gpui::fill(
                    Bounds {
                        origin: point(px(ox + column.x), px(oy + column.y)),
                        size: size(px(column.w), px(column.h)),
                    },
                    theme_accent,
                ));
            }

            // Draw bezier tangent handle lines and tips
            for h in &poly.handles {
                // Line from keyframe anchor to handle tip
                let handle_px_x = ox + h.handle_x;
                let handle_px_y = oy + h.handle_y;

                window.paint_quad(gpui::fill(
                    Bounds {
                        origin: point(
                            px(handle_px_x - HANDLE_DOT_SIZE * 0.5),
                            px(handle_px_y - HANDLE_DOT_SIZE * 0.5),
                        ),
                        size: size(px(HANDLE_DOT_SIZE), px(HANDLE_DOT_SIZE)),
                    },
                    theme_accent,
                ));
            }

            // Draw keyframe diamonds / dots
            for kp in &poly.key_points {
                let dot_x = ox + kp.pixel_x - KEYFRAME_DOT_SIZE * 0.5;
                let dot_y = oy + kp.pixel_y - KEYFRAME_DOT_SIZE * 0.5;
                window.paint_quad(gpui::fill(
                    Bounds {
                        origin: point(px(dot_x), px(dot_y)),
                        size: size(px(KEYFRAME_DOT_SIZE), px(KEYFRAME_DOT_SIZE)),
                    },
                    theme_text,
                ));
            }
        },
    )
    .size_full()
}
