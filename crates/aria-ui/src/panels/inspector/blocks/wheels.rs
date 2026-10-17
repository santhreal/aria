//! Lift, Gamma and Gain colour wheels, drawn wherever the grade is.
//!
//! A wheel paints a hue ring, crosshairs, and a puck at the offset its
//! channels read as. A press anywhere on the wheel puts the puck under the
//! pointer and hands the drag to the state (`crate::app::grade_controls`), and the
//! shell routes the pointer there until the release. Under each wheel the
//! master is a scrubbable value, and the reset button returns the channels and
//! the master to neutral as one undo step.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    Bounds, CursorStyle, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    ParentElement, Pixels, Styled, canvas, div, hsla, point, px, size,
};

use aria_models::ClipParam;

use crate::app::grade_controls::WheelControl;
use crate::app::value_scrub::{ScrubTarget, ValueKind};
use crate::app::{AppState, spawn_preview};
use crate::components;
use crate::motion::MotionKey;
use crate::panels::value_readout::{self, Look};
use crate::theme::Theme;

use super::widgets::section_label;

/// Radius of a drawn wheel in pixels.
pub const WHEEL_RADIUS: f32 = 36.0;

/// Dots in the hue ring.
const RING_DOTS: usize = 36;

/// Diameter of one ring dot.
const RING_DOT: f32 = 4.0;

/// Diameter of the puck.
const PUCK: f32 = 9.0;

/// One wheel with its title, reset button, and master value. `place` is the
/// surface drawing it, which the wheel's selector starts with.
fn single_wheel_widget(
    control: WheelControl,
    place: &'static str,
    theme: &Theme,
    state: &gpui::Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let (offset, master, held) = {
        let read = state.read(cx);
        (
            control.current_offset(read),
            control.current_master(read),
            read.wheel_held() == Some(control),
        )
    };
    let diameter = WHEEL_RADIUS * 2.0;
    let travel = WHEEL_RADIUS - PUCK * 0.5;
    let puck = [
        WHEEL_RADIUS + offset[0] * travel - PUCK * 0.5,
        WHEEL_RADIUS + offset[1] * travel - PUCK * 0.5,
    ];
    let key = MotionKey::new(place, control.label());

    // Where the wheel painted, in window pixels, for the press to read the
    // pointer against. The ring canvas covers the whole wheel: the outline is
    // an overlay rather than a border, so no inset shifts the canvas, the
    // crosshairs, or the puck off the wheel's centre.
    let frame = Rc::new(Cell::new([0.0_f32; 4]));
    let pressed = Rc::clone(&frame);
    let ring = canvas(
        move |bounds: Bounds<Pixels>, _, _| {
            frame.set([
                f32::from(bounds.origin.x),
                f32::from(bounds.origin.y),
                f32::from(bounds.size.width),
                f32::from(bounds.size.height),
            ]);
        },
        |bounds: Bounds<Pixels>, _, window, _| paint_ring(bounds, window),
    )
    .absolute()
    .size_full();

    let master_param = control.master_param();
    let target = ScrubTarget {
        key: key.with("master"),
        value: master,
        unit: 0.01,
        kind: ValueKind::Number { decimals: 2 },
        write: Rc::new(move |state: &mut AppState, delta| {
            state.nudge_shared_param(
                ClipParam::Look {
                    param: master_param,
                },
                None,
                delta,
            )
        }),
    };

    div()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(Theme::SPACE_XS))
        .child(
            div()
                .flex()
                .w_full()
                .justify_between()
                .items_center()
                .child(
                    div()
                        .text_size(Theme::TEXT_TITLE)
                        .text_color(theme.text)
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .child(control.label()),
                )
                .child(
                    components::icon_button(
                        ("inspector-wheel-reset", key.with("reset")),
                        crate::icons::ROTATE_CCW,
                        theme,
                    )
                    .debug_selector(move || format!("{place}-wheel-{}-reset", control.id()))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            this.state.update(cx, |state, _| {
                                if let Err(err) = control.reset(state) {
                                    state.report_error(&err);
                                }
                            });
                            spawn_preview(this.state.clone(), cx);
                            cx.notify();
                        }),
                    ),
                ),
        )
        .child(
            div()
                .id(key.element_id("wheel"))
                .debug_selector(move || format!("{place}-wheel-{}", control.id()))
                .relative()
                .w(px(diameter))
                .h(px(diameter))
                .rounded(px(WHEEL_RADIUS))
                .bg(theme.surface_raised)
                .cursor(CursorStyle::Crosshair)
                .child(
                    div()
                        .absolute()
                        .size_full()
                        .rounded(px(WHEEL_RADIUS))
                        .border_1()
                        .border_color(if held { theme.accent } else { theme.border }),
                )
                .child(ring)
                .child(
                    div()
                        .absolute()
                        .left(px(WHEEL_RADIUS - 0.5))
                        .top(px(RING_DOT * 2.5))
                        .w(px(1.0))
                        .h(px(diameter - RING_DOT * 5.0))
                        .bg(theme.border),
                )
                .child(
                    div()
                        .absolute()
                        .left(px(RING_DOT * 2.5))
                        .top(px(WHEEL_RADIUS - 0.5))
                        .w(px(diameter - RING_DOT * 5.0))
                        .h(px(1.0))
                        .bg(theme.border),
                )
                .child(
                    div()
                        .absolute()
                        .left(px(puck[0]))
                        .top(px(puck[1]))
                        .w(px(PUCK))
                        .h(px(PUCK))
                        .rounded(px(PUCK * 0.5))
                        .bg(if held { theme.accent } else { theme.text })
                        .border_1()
                        .border_color(theme.surface_raised),
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        let at = [f32::from(event.position.x), f32::from(event.position.y)];
                        let frame = pressed.get();
                        let changed = this
                            .state
                            .update(cx, |state, _| state.press_wheel(control, frame, at));
                        if changed {
                            spawn_preview(this.state.clone(), cx);
                        }
                        cx.notify();
                    }),
                ),
        )
        .child(
            div()
                .flex()
                .w_full()
                .justify_between()
                .items_center()
                .text_size(Theme::TEXT_UI)
                .text_color(theme.text_faint)
                .child("Master")
                .child(value_readout::readout(
                    theme,
                    state,
                    target,
                    format!("{master:.2}"),
                    Look {
                        size: Theme::TEXT_UI,
                        ink: theme.value,
                    },
                    cx,
                )),
        )
}

/// The hue ring: one dot per step around the rim, in the colour a press at
/// that angle pushes the picture toward. Red is at 0°, green at 120° and blue
/// at 240°, turning clockwise on screen.
fn paint_ring(bounds: Bounds<Pixels>, window: &mut gpui::Window) {
    let radius = f32::from(bounds.size.width).min(f32::from(bounds.size.height)) * 0.5;
    let center = bounds.center();
    let orbit = radius - RING_DOT;
    for dot in 0..RING_DOTS {
        let turn = dot as f32 / RING_DOTS as f32;
        let angle = turn * std::f32::consts::TAU;
        let x = f32::from(center.x) + orbit * angle.cos() - RING_DOT * 0.5;
        let y = f32::from(center.y) + orbit * angle.sin() - RING_DOT * 0.5;
        window.paint_quad(
            gpui::fill(
                Bounds {
                    origin: point(px(x), px(y)),
                    size: size(px(RING_DOT), px(RING_DOT)),
                },
                hsla(turn, 0.75, 0.55, 1.0),
            )
            .corner_radii(px(RING_DOT * 0.5)),
        );
    }
}

/// The Lift, Gamma, and Gain wheels under a section title. `place` is the
/// surface drawing them: `color` or `inspector`.
pub(crate) fn wheels_block(
    theme: &Theme,
    place: &'static str,
    state: &gpui::Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let row = WheelControl::ALL.into_iter().fold(
        div()
            .flex()
            .flex_row()
            .justify_between()
            .gap(px(Theme::SPACE_SM)),
        |row, control| row.child(single_wheel_widget(control, place, theme, state, cx)),
    );
    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .child(section_label(theme, "Color Wheels"))
        .child(row)
}
