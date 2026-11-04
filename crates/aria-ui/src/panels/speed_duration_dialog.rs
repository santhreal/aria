//! The Speed/Duration dialog (`Ctrl+R`).

use std::rc::Rc;

use crate::components::Wraps as _;
use aria_core::count::counted;
use gpui::{
    Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement, Styled,
    div, px,
};

use crate::app::clip_ops::SPEED_PERCENT_SPAN;
use crate::app::value_scrub::{ScrubTarget, ValueKind, Write};
use crate::app::{AppState, DialogKind};
use crate::chrome::dialog::{DialogBody, dialog_surface, toggle_row};
use crate::components::{self, ChipSize};
use crate::motion::MotionKey;
use crate::panels::value_readout::{self, Look};
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

pub fn render(
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let theme = Theme::of(cx);
    let (form, selected_count, rate) = {
        let s = state.read(cx);
        (
            s.speed_duration.clone(),
            s.selection.len(),
            s.store.sequence.frame_rate,
        )
    };
    let duration_span = form.duration_span();

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

    let reverse = toggle_row(
        &theme,
        "speed-duration-reverse",
        "Reverse Speed",
        form.reverse,
        AppState::toggle_speed_duration_reverse,
        cx,
    );
    let pitch = toggle_row(
        &theme,
        "speed-duration-pitch",
        "Maintain Audio Pitch",
        form.preserve_pitch,
        AppState::toggle_speed_duration_preserve_pitch,
        cx,
    );

    let speed = value_readout::readout(
        &theme,
        state,
        ScrubTarget {
            key: MotionKey::new("speed-duration", "speed"),
            value: form.speed_percent,
            unit: 1.0,
            kind: ValueKind::Number { decimals: 1 },
            write: Write::Draft {
                set: Rc::new(|state: &mut AppState, percent| {
                    state.set_speed_duration_percent(percent)
                }),
                span: SPEED_PERCENT_SPAN,
            },
        },
        format!("{:.1}%", form.speed_percent),
        Look {
            size: Theme::TEXT_UI,
            ink: theme.value,
        },
        cx,
    );
    let duration = value_readout::readout(
        &theme,
        state,
        ScrubTarget {
            key: MotionKey::new("speed-duration", "duration"),
            value: form.duration_ticks as f32,
            unit: 1.0,
            kind: ValueKind::Frames {
                num: rate.num,
                den: rate.den,
            },
            write: Write::Draft {
                set: Rc::new(|state: &mut AppState, frames: f32| {
                    state.set_speed_duration_ticks(frames.round() as i64)
                }),
                span: duration_span,
            },
        },
        form.duration_label.clone(),
        Look {
            size: Theme::TEXT_UI,
            ink: theme.value,
        },
        cx,
    );

    let mut content = div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_MD))
        .child(
            div()
                .text_size(Theme::TEXT_TITLE)
                .text_color(theme.text_muted)
                .child(counted(selected_count, "selected clip", "selected clips").to_string()),
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
                        .child(speed),
                )
                .child(speed_presets),
        )
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
                        .flex()
                        .items_baseline()
                        .gap(px(Theme::SPACE_XS))
                        .child(duration)
                        .child(
                            div()
                                .text_size(Theme::TEXT_UI)
                                .text_color(theme.text_muted)
                                .font_features(crate::theme::tabular_figures())
                                .child(counted(form.duration_ticks, "frame", "frames").to_string()),
                        ),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(Theme::SPACE_XS))
                .child(reverse)
                .child(pitch),
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
