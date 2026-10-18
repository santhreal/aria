//! Speed block: flat speed, the ramp presets, and the ramp curve editor.
//!
//! Presets come from `aria_models::speed_preset_catalog` at render time, and the
//! active one is resolved by shape rather than by a stored id, so a hand-edited
//! ramp shows as no preset. The whole block is hidden on a clip the model refuses
//! a ramp on.
//!
//! The graph is the ramp's real keyframes, not samples of it: a point is drawn
//! where it sits. The rectangle it occupies is measured during paint (`gpui`
//! canvas), because that is the only place the drawn geometry is known, and the
//! pointer is read against that rectangle. Values run logarithmically so 1x is
//! near the middle instead of the bottom two percent.
//!
//! A drag is one edit: pointer moves update a draft on `AppState`, the release
//! commits, Esc abandons. What each pointer event means is a row of
//! [`ramp_gesture`] rather than a rule inside a
//! listener, so the graph's behaviour can be enumerated and pressed.

use gpui::{
    InteractiveElement, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    ParentElement, Styled, canvas, div, px, relative,
};

use crate::app::clip_speed::{RAMP_POINTS, ramp_tick_to_frac, ramp_value_to_frac, speed_presets};
use crate::components;
use crate::fluent::FluentBuilder;
use crate::surfaces::clip_pick::{self, Pick};
use crate::surfaces::ramp_gesture::{self, Gesture};
use crate::theme::Theme;
use crate::timecode::format_ticks;
use crate::types::UiClip;

use crate::panels::inspector::blocks::widgets::{Readout, audio_row, section_label};

/// Height of the curve graph. Tall enough for a log axis to read, short enough to
/// leave the rest of the tab on screen.
const GRAPH_HEIGHT: f32 = 120.0;

/// Side of a draggable point.
const DOT: f32 = 9.0;

/// How many columns the curve is drawn with. The graph has no path primitive, so
/// the curve is a row of thin bars; 60 is smooth at any inspector width.
const COLUMNS: usize = 60;

pub fn speed_block(
    theme: &Theme,
    clip: &UiClip,
    fps_num: i32,
    fps_den: i32,
    state: &gpui::Entity<crate::app::AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let state = gpui::Entity::clone(state);
    let read = state.read(cx);
    let active = read.selected_speed_preset();
    let points = read.selected_ramp_values();
    let has_ramp = read.selected_has_ramp();
    let curve = read.selected_ramp_curve();
    let dragging = read.ramp_dragging().map(|drag| drag.origin);
    let length = read.selected_ramp_length_ticks();

    let flat = if clip.speed_den == 0 {
        1.0
    } else {
        clip.speed_num as f32 / clip.speed_den as f32
    };

    let chips: Vec<_> = speed_presets()
        .iter()
        .map(|spec| {
            let id = spec.id.as_str();
            let selected = active == Some(id);
            components::ghost_button(
                id,
                if selected {
                    format!("• {}", spec.label)
                } else {
                    spec.label.to_string()
                },
                theme,
            )
            .id(crate::motion::MotionKey::new("speed", &id).element_id("speed"))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    this.state.update(cx, |state, _| {
                        if let Err(err) = clip_pick::apply(state, Pick::SpeedPreset(id.to_string()))
                        {
                            state.report_error(&err);
                        }
                    });
                    cx.notify();
                }),
            )
            .into_any_element()
        })
        .collect();

    let rows: Vec<_> = points
        .into_iter()
        .map(|(id, label, value)| {
            let tick = RAMP_POINTS
                .iter()
                .find(|(point_id, ..)| *point_id == id)
                .map(|(.., tick)| *tick)
                .unwrap_or(0);
            audio_row(
                theme,
                &state,
                crate::motion::MotionKey::new("insp.row", id),
                label,
                Readout::printed(format!("{value:.2}x"), value, 2, 0.05),
                move |state, by| state.nudge_selected_ramp_point(tick, by),
                cx,
            )
            .into_any_element()
        })
        .collect();

    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .child(section_label(theme, "Speed"))
        .child(
            div()
                .text_size(Theme::TEXT_UI)
                .text_color(theme.text_faint)
                .child(if has_ramp {
                    format!("Ramp on a {flat:.2}x base")
                } else {
                    format!("Flat {flat:.2}x")
                }),
        )
        .children(chips)
        .child(graph(theme, &curve, dragging, &state, cx))
        .when_some(length, |el, ticks| {
            el.child(
                div()
                    .text_size(Theme::TEXT_UI)
                    .text_color(theme.text_faint)
                    .child(format!("Length {}", format_ticks(ticks, fps_num, fps_den))),
            )
        })
        .children(rows)
        .when(has_ramp, |el| {
            el.child(
                components::ghost_button("speed-clear", "Clear ramp", theme).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        this.state.update(cx, |state, _| {
                            if let Err(err) = clip_pick::apply(state, Pick::ClearSpeedCurve) {
                                state.report_error(&err);
                            }
                        });
                        cx.notify();
                    }),
                ),
            )
        })
}

/// Records the graph rectangle every paint, so a pointer position can be turned
/// into a ramp point. Draws nothing.
///
/// Repaint only when the rectangle moved; anything else is a repaint loop.
fn measure(state: &gpui::Entity<crate::app::AppState>) -> impl IntoElement {
    let state = gpui::Entity::clone(state);
    canvas(
        move |bounds, _, cx| {
            let frame = [
                f32::from(bounds.origin.x),
                f32::from(bounds.origin.y),
                f32::from(bounds.size.width),
                f32::from(bounds.size.height),
            ];
            if state.update(cx, |state, _| state.record_ramp_frame(frame)) {
                crate::shell::notify_after_draw(&state, cx);
            }
        },
        |_, _, _, _| {},
    )
    .absolute()
    .size_full()
}

/// The curve graph: a measured box, a 1x guide, the curve as columns, and one
/// draggable dot per keyframe.
///
/// Every pointer gesture is routed on the box rather than on the dots, because a
/// press has to be able to grab a nearby point *or* add one where it landed, and
/// only the box knows where the pointer is.
fn graph(
    theme: &Theme,
    curve: &[(i64, f32)],
    dragging: Option<i64>,
    state: &gpui::Entity<crate::app::AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let columns: Vec<_> = (0..COLUMNS)
        .map(|column| {
            let frac = column as f32 / (COLUMNS - 1) as f32;
            let value = sample_points(curve, frac);
            let height = (ramp_value_to_frac(value) * GRAPH_HEIGHT).max(1.0);
            div()
                .absolute()
                .left(relative(frac))
                .bottom(px(0.))
                .w(px(2.))
                .h(px(height))
                .bg(theme.accent.opacity(0.35))
                .into_any_element()
        })
        .collect();

    let dots: Vec<_> = curve
        .iter()
        .map(|(tick, value)| {
            let held = dragging == Some(*tick);
            div()
                .absolute()
                .left(relative(ramp_tick_to_frac(*tick)))
                .bottom(px(ramp_value_to_frac(*value) * GRAPH_HEIGHT - DOT / 2.0))
                .w(px(DOT))
                .h(px(DOT))
                .rounded(px(DOT))
                .bg(if held { theme.accent } else { theme.text })
                .into_any_element()
        })
        .collect();

    div()
        .id("speed-graph")
        .relative()
        .h(px(GRAPH_HEIGHT))
        .w_full()
        .rounded(px(Theme::CONTROL_RADIUS))
        .bg(theme.surface)
        .border_1()
        .border_color(theme.border)
        .child(
            // The 1x guide, so a point above or below unit speed is obvious.
            div()
                .absolute()
                .left(px(0.))
                .bottom(px(ramp_value_to_frac(1.0) * GRAPH_HEIGHT))
                .w_full()
                .h(px(1.))
                .bg(theme.border),
        )
        .children(columns)
        .children(dots)
        .child(measure(state))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, event: &MouseDownEvent, _, cx| {
                run(
                    this,
                    Gesture::Press {
                        at: point(event.position),
                    },
                    cx,
                );
            }),
        )
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(|this, event: &MouseDownEvent, _, cx| {
                run(
                    this,
                    Gesture::Drop {
                        at: point(event.position),
                    },
                    cx,
                );
            }),
        )
        .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
            run(
                this,
                Gesture::Drag {
                    at: point(event.position),
                },
                cx,
            );
        }))
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(|this, _: &MouseUpEvent, _, cx| {
                run(this, Gesture::Release, cx);
            }),
        )
}

/// A pointer position in the graph's own units.
fn point(at: gpui::Point<gpui::Pixels>) -> [f32; 2] {
    [f32::from(at.x), f32::from(at.y)]
}

/// Run one gesture, repainting only when the graph changed. A refusal is
/// reported where the operator is looking rather than swallowed.
fn run(
    shell: &mut crate::shell::Shell,
    gesture: Gesture,
    cx: &mut gpui::Context<crate::shell::Shell>,
) {
    let drew = shell
        .state
        .update(cx, |state, _| match ramp_gesture::apply(state, gesture) {
            Ok(drew) => drew,
            Err(err) => {
                state.report_error(&err);
                true
            }
        });
    if drew {
        cx.notify();
    }
}

/// The curve's value at a graph x fraction, interpolated between the keyframes
/// either side. Empty curve reads as flat 1x.
///
/// Linear, matching the easing every ramp point is written with.
fn sample_points(points: &[(i64, f32)], frac: f32) -> f32 {
    if points.is_empty() {
        return 1.0;
    }
    let tick = crate::app::clip_speed::ramp_frac_to_tick(frac);
    let mut previous: Option<(i64, f32)> = None;
    for (point, value) in points {
        if *point >= tick {
            let Some((prev_tick, prev_value)) = previous else {
                return *value;
            };
            let span = (point - prev_tick) as f32;
            if span <= 0.0 {
                return *value;
            }
            let t = (tick - prev_tick) as f32 / span;
            return prev_value + (value - prev_value) * t;
        }
        previous = Some((*point, *value));
    }
    points[points.len() - 1].1
}
