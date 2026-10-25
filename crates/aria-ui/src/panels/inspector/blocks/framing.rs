//! Framing block of the Transform tab: the crop insets, mirroring, and motion
//! blur.
//!
//! Crop rows are generated from `app::clip_frame::CROP_EDGES`, so the panel
//! cannot drift from the rect the model validates.

use gpui::{
    InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement, Styled, div, px,
};

use crate::app::clip_frame::{CROP_EDGES, crop_inset};
use crate::components;
use crate::surfaces::inspector_pick::{self, Pick};
use crate::theme::Theme;
use crate::types::UiClip;

use super::widgets::{Columns, Readout, control_line, unkeyed_row};

use crate::components::section_label;

pub fn framing_block(
    theme: &Theme,
    clip: &UiClip,
    state: &gpui::Entity<crate::app::AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let crops: Vec<_> = CROP_EDGES
        .iter()
        .map(|(id, label, ..)| {
            let edge = *id;
            let current = crop_inset(&clip.crop, edge);
            let percent = current * 100.0;
            unkeyed_row(
                theme,
                state,
                crate::motion::MotionKey::new("insp.row", edge),
                *label,
                Readout::printed(format!("{percent:.0}%"), percent, 0, 1.0),
                Columns::Keyed,
                move |state, by| state.nudge_selected_crop_inset(edge, by / 100.0),
                cx,
            )
            .into_any_element()
        })
        .collect();

    let blur = clip.motion_blur;

    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .child(section_label(theme, "Crop"))
        .children(crops)
        .child(
            div()
                .flex()
                .flex_row()
                .gap(px(Theme::SPACE_XS))
                .child(
                    components::ghost_button(
                        "flip-h",
                        if clip.flip_h { "Flip H on" } else { "Flip H" },
                        theme,
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseDownEvent, _, cx| {
                            this.state.update(cx, |state, _| {
                                if let Err(err) = inspector_pick::apply(state, Pick::FlipHorizontal)
                                {
                                    state.report_error(&err);
                                }
                            });
                            cx.notify();
                        }),
                    ),
                )
                .child(
                    components::ghost_button(
                        "flip-v",
                        if clip.flip_v { "Flip V on" } else { "Flip V" },
                        theme,
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseDownEvent, _, cx| {
                            this.state.update(cx, |state, _| {
                                if let Err(err) = inspector_pick::apply(state, Pick::FlipVertical) {
                                    state.report_error(&err);
                                }
                            });
                            cx.notify();
                        }),
                    ),
                )
                .child(
                    components::ghost_button("crop-clear", "Reset framing", theme).on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseDownEvent, _, cx| {
                            this.state.update(cx, |state, _| {
                                if let Err(err) = inspector_pick::apply(state, Pick::ClearCrop) {
                                    state.report_error(&err);
                                }
                            });
                            cx.notify();
                        }),
                    ),
                ),
        )
        .child(section_label(theme, "Motion blur"))
        .child(control_line(
            theme,
            crate::motion::MotionKey::new("insp.row", "blur-on"),
            "Enabled",
            Columns::Keyed,
            components::ghost_button(
                "blur-toggle",
                if blur.enabled { "On" } else { "Off" },
                theme,
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, _, cx| {
                    this.state.update(cx, |state, _| {
                        if let Err(err) = inspector_pick::apply(state, Pick::ToggleMotionBlur) {
                            state.report_error(&err);
                        }
                    });
                    cx.notify();
                }),
            ),
        ))
        .child(unkeyed_row(
            theme,
            state,
            crate::motion::MotionKey::new("insp.row", "blur-shutter"),
            "Shutter",
            Readout::printed(
                format!("{:.0}°", blur.shutter_deg),
                blur.shutter_deg,
                0,
                1.0,
            ),
            Columns::Keyed,
            |state, by| state.nudge_selected_shutter(by),
            cx,
        ))
        .child(unkeyed_row(
            theme,
            state,
            crate::motion::MotionKey::new("insp.row", "blur-samples"),
            "Samples",
            Readout::number(blur.samples as f32, 0, 1.0),
            Columns::Keyed,
            |state, by| state.nudge_selected_blur_samples(by.round() as i32),
            cx,
        ))
}
