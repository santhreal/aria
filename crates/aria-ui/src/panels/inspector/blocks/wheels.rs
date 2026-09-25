//! 3-way color wheels for the inspector: Lift (shadows), Gamma (midtones), Gain (highlights).
//!
//! Each wheel provides 2D chromaticity balance (hue/saturation) via a draggable
//! handle on a unit circle, a master slider for luminance/density, and a per-control
//! reset button returning to documented neutral constants.

use gpui::{
    InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement, Styled, div, px,
};

use aria_engine::EngineError;
use aria_models::{ClipParam, GAIN_NEUTRAL, GAMMA_NEUTRAL, LIFT_NEUTRAL, LookParam};

use crate::app::AppState;
use crate::components;
use crate::theme::Theme;
use crate::types::UiClip;

use super::widgets::section_label;

/// Radius of the draggable color wheel in pixels.
pub const WHEEL_RADIUS: f32 = 36.0;

/// Map a pointer position in window coords to normalized unit-circle offset (-1.0..=1.0).
/// Clamps to the wheel radius when the pointer moves outside the circle.
pub fn pointer_to_wheel_offset(center: [f32; 2], radius: f32, pointer: [f32; 2]) -> [f32; 2] {
    let dx = pointer[0] - center[0];
    let dy = pointer[1] - center[1];
    let dist = (dx * dx + dy * dy).sqrt();
    if radius <= 0.0 {
        return [0.0, 0.0];
    }
    if dist > radius {
        [dx / dist, dy / dist]
    } else {
        [dx / radius, dy / radius]
    }
}

/// Convert normalized unit-circle offset (x, y) into RGB deltas with zero sum.
///
/// Places pure Red at 0°, Green at 120°, and Blue at 240°.
/// The sum (ΔR + ΔG + ΔB) is identically zero for all offsets, isolating
/// chromaticity from master luminance.
pub fn wheel_offset_to_rgb_deltas(offset: [f32; 2]) -> [f32; 3] {
    let sqrt3_over_2 = 0.8660254037844386;
    let dr = offset[0];
    let dg = -0.5 * offset[0] + sqrt3_over_2 * offset[1];
    let db = -0.5 * offset[0] - sqrt3_over_2 * offset[1];
    [dr, dg, db]
}

/// Convert RGB deltas back into normalized unit-circle offset (x, y).
pub fn rgb_deltas_to_wheel_offset(deltas: [f32; 3]) -> [f32; 2] {
    let sqrt3 = 1.7320508075688772;
    let ux = deltas[0];
    let uy = (deltas[1] - deltas[2]) / sqrt3;
    let dist = (ux * ux + uy * uy).sqrt();
    if dist > 1.0 {
        [ux / dist, uy / dist]
    } else {
        [ux, uy]
    }
}

/// The three primary grading wheel controls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WheelControl {
    Lift,
    Gamma,
    Gain,
}

impl WheelControl {
    pub const ALL: [Self; 3] = [Self::Lift, Self::Gamma, Self::Gain];

    pub fn label(self) -> &'static str {
        match self {
            Self::Lift => "Lift",
            Self::Gamma => "Gamma",
            Self::Gain => "Gain",
        }
    }

    pub fn neutral_master(self) -> f32 {
        match self {
            Self::Lift => LIFT_NEUTRAL,
            Self::Gamma => GAMMA_NEUTRAL,
            Self::Gain => GAIN_NEUTRAL,
        }
    }

    pub fn rgb_params(self) -> (LookParam, LookParam, LookParam) {
        match self {
            Self::Lift => (
                LookParam::AdjustLiftR,
                LookParam::AdjustLiftG,
                LookParam::AdjustLiftB,
            ),
            Self::Gamma => (
                LookParam::AdjustGammaR,
                LookParam::AdjustGammaG,
                LookParam::AdjustGammaB,
            ),
            Self::Gain => (
                LookParam::AdjustGainR,
                LookParam::AdjustGainG,
                LookParam::AdjustGainB,
            ),
        }
    }

    pub fn master_param(self) -> LookParam {
        match self {
            Self::Lift => LookParam::AdjustLiftMaster,
            Self::Gamma => LookParam::AdjustGammaMaster,
            Self::Gain => LookParam::AdjustGainMaster,
        }
    }

    pub fn reset(self, state: &mut AppState) -> Result<(), EngineError> {
        let (pr, pg, pb) = self.rgb_params();
        let pm = self.master_param();
        let neutral = self.neutral_master();
        state.set_param_scalar(ClipParam::Look { param: pr }, None, neutral)?;
        state.set_param_scalar(ClipParam::Look { param: pg }, None, neutral)?;
        state.set_param_scalar(ClipParam::Look { param: pb }, None, neutral)?;
        state.set_param_scalar(ClipParam::Look { param: pm }, None, neutral)?;
        Ok(())
    }

    pub fn apply_offset(self, state: &mut AppState, offset: [f32; 2]) -> Result<(), EngineError> {
        let [dr, dg, db] = wheel_offset_to_rgb_deltas(offset);
        let (pr, pg, pb) = self.rgb_params();
        let base = self.neutral_master();
        state.set_param_scalar(ClipParam::Look { param: pr }, None, base + dr)?;
        state.set_param_scalar(ClipParam::Look { param: pg }, None, base + dg)?;
        state.set_param_scalar(ClipParam::Look { param: pb }, None, base + db)?;
        Ok(())
    }

    pub fn current_offset(self, state: &AppState) -> [f32; 2] {
        let (pr, pg, pb) = self.rgb_params();
        let r = state
            .param_scalar(ClipParam::Look { param: pr }, None)
            .unwrap_or(self.neutral_master());
        let g = state
            .param_scalar(ClipParam::Look { param: pg }, None)
            .unwrap_or(self.neutral_master());
        let b = state
            .param_scalar(ClipParam::Look { param: pb }, None)
            .unwrap_or(self.neutral_master());
        let base = self.neutral_master();
        rgb_deltas_to_wheel_offset([r - base, g - base, b - base])
    }

    pub fn current_master(self, state: &AppState) -> f32 {
        state
            .param_scalar(
                ClipParam::Look {
                    param: self.master_param(),
                },
                None,
            )
            .unwrap_or(self.neutral_master())
    }
}

/// Render one color wheel control with its handle, reset button, and master control.
pub(super) fn single_wheel_widget(
    control: WheelControl,
    theme: &Theme,
    state: &gpui::Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let current_offset = state.read(cx).let_ref(|s| control.current_offset(s));
    let current_master = state.read(cx).let_ref(|s| control.current_master(s));

    let diameter = WHEEL_RADIUS * 2.0;
    let handle_size = 8.0;
    let handle_x = WHEEL_RADIUS + current_offset[0] * (WHEEL_RADIUS - handle_size * 0.5);
    let handle_y = WHEEL_RADIUS + current_offset[1] * (WHEEL_RADIUS - handle_size * 0.5);

    let reset_id = crate::motion::MotionKey::new("wheel-reset", control.label());
    let wheel_id = crate::motion::MotionKey::new("wheel", control.label()).element_id("wheel");

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
                        .text_size(px(12.0))
                        .text_color(theme.text)
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .child(control.label()),
                )
                .child(
                    components::icon_button(
                        ("inspector-wheel-reset", reset_id),
                        crate::icons::ROTATE_CCW,
                        theme,
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                            this.state.update(cx, |state, _| {
                                if let Err(err) = control.reset(state) {
                                    state.report_error(&err);
                                }
                            });
                            cx.notify();
                        }),
                    ),
                ),
        )
        .child(
            div()
                .id(wheel_id)
                .relative()
                .w(px(diameter))
                .h(px(diameter))
                .rounded(px(WHEEL_RADIUS))
                .bg(theme.surface_raised)
                .border_1()
                .border_color(theme.border)
                // Center crosshairs
                .child(
                    div()
                        .absolute()
                        .left(px(WHEEL_RADIUS - 0.5))
                        .top(px(4.0))
                        .w(px(1.0))
                        .h(px(diameter - 8.0))
                        .bg(theme.border),
                )
                .child(
                    div()
                        .absolute()
                        .left(px(4.0))
                        .top(px(WHEEL_RADIUS - 0.5))
                        .w(px(diameter - 8.0))
                        .h(px(1.0))
                        .bg(theme.border),
                )
                // Draggable handle
                .child(
                    div()
                        .absolute()
                        .left(px(handle_x - handle_size * 0.5))
                        .top(px(handle_y - handle_size * 0.5))
                        .w(px(handle_size))
                        .h(px(handle_size))
                        .rounded(px(handle_size * 0.5))
                        .bg(theme.accent)
                        .border_1()
                        .border_color(theme.text),
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                        let px_pos = [f32::from(event.position.x), f32::from(event.position.y)];
                        // In tests and headless rendering, position is window/element relative
                        let offset = pointer_to_wheel_offset(
                            [WHEEL_RADIUS, WHEEL_RADIUS],
                            WHEEL_RADIUS,
                            px_pos,
                        );
                        this.state.update(cx, |state, _| {
                            if let Err(err) = control.apply_offset(state, offset) {
                                state.report_error(&err);
                            }
                        });
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
                .text_size(px(11.0))
                .text_color(theme.text_faint)
                .child("Master")
                .child(format!("{:.2}", current_master)),
        )
}

/// Render the Lift, Gamma, and Gain color wheels section.
pub(super) fn wheels_block(
    theme: &Theme,
    _clip: &UiClip,
    state: &gpui::Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let row = WheelControl::ALL.into_iter().fold(
        div()
            .flex()
            .flex_row()
            .justify_between()
            .gap(px(Theme::SPACE_SM)),
        |row, control| row.child(single_wheel_widget(control, theme, state, cx)),
    );
    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .child(section_label(theme, "Color Wheels"))
        .child(row)
}

trait RefHelper {
    fn let_ref<R>(&self, f: impl FnOnce(&Self) -> R) -> R {
        f(self)
    }
}

impl<T> RefHelper for T {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wheel_pointer_to_offset_clamps_to_unit_circle() {
        let center = [100.0, 100.0];
        let radius = 50.0;

        // Center maps to [0, 0]
        let c = pointer_to_wheel_offset(center, radius, [100.0, 100.0]);
        assert!((c[0].abs() < 1e-4) && (c[1].abs() < 1e-4));

        // Right edge
        let r = pointer_to_wheel_offset(center, radius, [150.0, 100.0]);
        assert!((r[0] - 1.0).abs() < 1e-4);
        assert!(r[1].abs() < 1e-4);

        // Outside clamps to boundary with length 1.0
        let out = pointer_to_wheel_offset(center, radius, [200.0, 100.0]);
        assert!((out[0] - 1.0).abs() < 1e-4);
        assert!(out[1].abs() < 1e-4);

        // Diagonal
        let diag = pointer_to_wheel_offset(center, radius, [150.0, 150.0]);
        let len = (diag[0] * diag[0] + diag[1] * diag[1]).sqrt();
        assert!((len - 1.0).abs() < 1e-4);
    }

    #[test]
    fn wheel_offset_to_rgb_deltas_round_trips() {
        let offsets = [
            [0.0, 0.0],
            [1.0, 0.0],
            [0.0, 1.0],
            [-0.5, 0.866],
            [-0.5, -0.866],
            [0.3, -0.7],
        ];
        for off in offsets {
            let deltas = wheel_offset_to_rgb_deltas(off);
            let back = rgb_deltas_to_wheel_offset(deltas);
            assert!(
                (off[0] - back[0]).abs() < 1e-3,
                "offset x mismatch: {:?} vs {:?}",
                off,
                back
            );
            assert!(
                (off[1] - back[1]).abs() < 1e-3,
                "offset y mismatch: {:?} vs {:?}",
                off,
                back
            );
        }
    }

    #[test]
    fn wheel_offset_deltas_sum_to_zero() {
        let offsets = [
            [0.0, 0.0],
            [1.0, 0.0],
            [-0.5, 0.866],
            [0.45, -0.6],
            [-0.8, -0.3],
        ];
        for off in offsets {
            let [dr, dg, db] = wheel_offset_to_rgb_deltas(off);
            let sum = dr + dg + db;
            assert!(
                sum.abs() < 1e-5,
                "RGB deltas must sum to zero for chromaticity isolation: got {sum}"
            );
        }
    }
}
