//! The Speed/Duration dialog (`Ctrl+R`).

use crate::components::Wraps as _;
use gpui::{
    Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement, Styled,
    div, px,
};

use crate::app::{AppState, DialogKind};
use crate::chrome::dialog::{DialogBody, dialog_surface};
use crate::components::{self, ChipSize};
use crate::theme::Theme;

const SPEED_PRESETS: &[(f32, &str, &str)] = &[
    (25.0, "speed-preset-25", "25%"),
    (50.0, "speed-preset-50", "50%"),
    (75.0, "speed-preset-75", "75%"),
    (100.0, "speed-preset-100", "100%"),
    (125.0, "speed-preset-125", "125%"),
    (150.0, "speed-preset-150", "150%"),
    (200.0, "speed-preset-200", "200%"),
    (400.0, "speed-preset-400", "400%"),
];

const SPEED_STEPS: &[(f32, &str, &str)] = &[
    (-25.0, "speed-step-m25", "-25%"),
    (-5.0, "speed-step-m5", "-5%"),
    (-1.0, "speed-step-m1", "-1%"),
    (1.0, "speed-step-p1", "+1%"),
    (5.0, "speed-step-p5", "+5%"),
    (25.0, "speed-step-p25", "+25%"),
];

const DURATION_STEPS: &[(i64, &str, &str)] = &[
    (-24, "dur-step-m24", "-24f"),
    (-5, "dur-step-m5", "-5f"),
    (-1, "dur-step-m1", "-1f"),
    (1, "dur-step-p1", "+1f"),
    (5, "dur-step-p5", "+5f"),
    (24, "dur-step-p24", "+24f"),
];

pub fn render(
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let theme = Theme::of(cx);
    let (form, selected_count) = {
        let s = state.read(cx);
        (s.speed_duration.clone(), s.selection.len())
    };

    let cancel = components::ghost_button("speed-duration-cancel", "Cancel", &theme).on_mouse_down(
        MouseButton::Left,
        cx.listener(|this, _: &MouseDownEvent, _, cx| {
            this.state
                .update(cx, |state, _| state.open_dialog(DialogKind::None));
            cx.notify();
        }),
    );
    let apply = components::button("speed-duration-apply", "Apply", &theme).on_mouse_down(
        MouseButton::Left,
        cx.listener(|this, _: &MouseDownEvent, _, cx| {
            this.state
                .update(cx, |state, _| state.apply_speed_duration());
            cx.notify();
        }),
    );

    let speed_presets =
        div()
            .flex()
            .wraps()
            .gap(px(Theme::SPACE_XS))
            .children(SPEED_PRESETS.iter().map(|&(pct, id, label)| {
                let active = (form.speed_percent - pct).abs() < 0.05;
                components::chip(
                    ("speed-duration-preset", id),
                    label,
                    active,
                    ChipSize::Compact,
                    &theme,
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        this.state.update(cx, |state, _| {
                            let _ = state.set_speed_duration_percent(pct);
                        });
                        cx.notify();
                    }),
                )
            }));

    let speed_steppers =
        div()
            .flex()
            .wraps()
            .gap(px(Theme::SPACE_XS))
            .children(SPEED_STEPS.iter().map(|&(delta, id, label)| {
                let current = form.speed_percent;
                components::ghost_button(("speed-duration-speed-step", id), label, &theme)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                            this.state.update(cx, |state, _| {
                                let _ = state.set_speed_duration_percent(current + delta);
                            });
                            cx.notify();
                        }),
                    )
            }));

    let duration_steppers =
        div()
            .flex()
            .wraps()
            .gap(px(Theme::SPACE_XS))
            .children(DURATION_STEPS.iter().map(|&(delta, id, label)| {
                let current = form.duration_ticks;
                components::ghost_button(("speed-duration-length-step", id), label, &theme)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                            this.state.update(cx, |state, _| {
                                let _ = state.set_speed_duration_ticks(current + delta);
                            });
                            cx.notify();
                        }),
                    )
            }));

    let reverse_chip = components::chip(
        "speed-duration-reverse",
        if form.reverse {
            "Reverse Speed: On"
        } else {
            "Reverse Speed: Off"
        },
        form.reverse,
        ChipSize::Regular,
        &theme,
    )
    .on_mouse_down(
        MouseButton::Left,
        cx.listener(|this, _: &MouseDownEvent, _, cx| {
            this.state
                .update(cx, |state, _| state.toggle_speed_duration_reverse());
            cx.notify();
        }),
    );

    let pitch_chip = components::chip(
        "speed-duration-pitch",
        if form.preserve_pitch {
            "Maintain Audio Pitch: On"
        } else {
            "Maintain Audio Pitch: Off"
        },
        form.preserve_pitch,
        ChipSize::Regular,
        &theme,
    )
    .on_mouse_down(
        MouseButton::Left,
        cx.listener(|this, _: &MouseDownEvent, _, cx| {
            this.state
                .update(cx, |state, _| state.toggle_speed_duration_preserve_pitch());
            cx.notify();
        }),
    );

    let mut content = div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_MD))
        .child(
            div()
                .text_size(Theme::TEXT_TITLE)
                .text_color(theme.text_muted)
                .child(format!(
                    "{} selected clip{}",
                    selected_count,
                    if selected_count == 1 { "" } else { "s" }
                )),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(Theme::SPACE_XS))
                .child(
                    div()
                        .flex()
                        .justify_between()
                        .child(
                            div()
                                .text_size(Theme::TEXT_TITLE)
                                .text_color(theme.text_muted)
                                .child("Speed"),
                        )
                        .child(
                            div()
                                .text_size(Theme::TEXT_UI)
                                .text_color(theme.text)
                                .child(format!("{:.1}%", form.speed_percent)),
                        ),
                )
                .child(speed_presets)
                .child(speed_steppers),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(Theme::SPACE_XS))
                .child(
                    div()
                        .flex()
                        .justify_between()
                        .child(
                            div()
                                .text_size(Theme::TEXT_TITLE)
                                .text_color(theme.text_muted)
                                .child("Duration"),
                        )
                        .child(
                            div()
                                .text_size(Theme::TEXT_UI)
                                .text_color(theme.text)
                                .child(format!(
                                    "{} ({} frames)",
                                    form.duration_label, form.duration_ticks
                                )),
                        ),
                )
                .child(duration_steppers),
        )
        .child(
            div()
                .flex()
                .gap(px(Theme::SPACE_SM))
                .child(reverse_chip)
                .child(pitch_chip),
        );

    if let Some(err) = form.error {
        content = content.child(
            div()
                .text_size(Theme::TEXT_TITLE)
                .text_color(theme.danger)
                .child(err),
        );
    }

    let body = DialogBody::new(content).action(cancel).action(apply);
    dialog_surface(
        "speed-duration",
        DialogKind::SpeedDuration,
        "Speed/Duration",
        state,
        cx,
        body,
    )
}
