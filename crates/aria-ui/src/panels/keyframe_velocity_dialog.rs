//! The Keyframe Velocity dialog: incoming and outgoing speed and influence of
//! the selected keyframes, with Continuous to keep one speed on both sides.

use gpui::{
    Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement, Styled,
    div, px,
};

use crate::app::animation_verbs::{KeyframeVelocityForm, VelocitySide};
use crate::app::{AppState, DialogKind};
use crate::chrome::dialog::{DialogBody, dialog_surface};
use crate::components::{self, ChipSize};
use crate::motion::MotionKey;
use crate::theme::Theme;

/// Speed steps in property units per second.
const SPEED_STEPS: &[(f32, &str)] = &[
    (-100.0, "-100"),
    (-10.0, "-10"),
    (-1.0, "-1"),
    (1.0, "+1"),
    (10.0, "+10"),
    (100.0, "+100"),
];

/// Influence presets in per cent.
const INFLUENCE_PRESETS: &[(f32, &str)] = &[
    (10.0, "10%"),
    (33.33, "33.33%"),
    (50.0, "50%"),
    (75.0, "75%"),
    (100.0, "100%"),
];

/// Influence steps in per cent.
const INFLUENCE_STEPS: &[(f32, &str)] =
    &[(-10.0, "-10%"), (-1.0, "-1%"), (1.0, "+1%"), (10.0, "+10%")];

fn side_id(side: VelocitySide) -> &'static str {
    match side {
        VelocitySide::Incoming => "in",
        VelocitySide::Outgoing => "out",
    }
}

fn readout(theme: &Theme, label: &'static str, value: String) -> impl IntoElement {
    div()
        .flex()
        .justify_between()
        .child(
            div()
                .text_size(Theme::TEXT_TITLE)
                .text_color(theme.text_muted)
                .child(label),
        )
        .child(
            div()
                .text_size(Theme::TEXT_UI)
                .font_family(theme.font_mono.clone())
                .text_color(theme.text)
                .child(value),
        )
}

fn side_block(
    side: VelocitySide,
    form: &KeyframeVelocityForm,
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let tag = side_id(side);
    let speed = form.speed_per_second(side);
    let influence = form.influence_percent(side);

    let zero = components::chip(
        MotionKey::new("velocity-speed-zero", tag),
        "0",
        speed == 0.0,
        ChipSize::Compact,
        theme,
    )
    .on_mouse_down(
        MouseButton::Left,
        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
            this.state.update(cx, |state, _| {
                let _ = state.set_keyframe_velocity_speed(side, 0.0);
            });
            cx.notify();
        }),
    );
    let speed_steps = div()
        .flex()
        .flex_wrap()
        .gap(px(Theme::SPACE_XS))
        .child(zero)
        .children(SPEED_STEPS.iter().enumerate().map(|(i, &(delta, label))| {
            components::ghost_button(
                MotionKey::new("velocity-speed-step", tag).with(i),
                label,
                theme,
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    this.state.update(cx, |state, _| {
                        let _ = state.set_keyframe_velocity_speed(side, speed + delta);
                    });
                    cx.notify();
                }),
            )
        }));
    let influence_presets = div()
        .flex()
        .flex_wrap()
        .gap(px(Theme::SPACE_XS))
        .children(
            INFLUENCE_PRESETS
                .iter()
                .enumerate()
                .map(|(i, &(percent, label))| {
                    let active = (influence - percent).abs() < 0.01;
                    components::chip(
                        MotionKey::new("velocity-influence-preset", tag).with(i),
                        label,
                        active,
                        ChipSize::Compact,
                        theme,
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                            this.state.update(cx, |state, _| {
                                let _ = state.set_keyframe_velocity_influence(side, percent);
                            });
                            cx.notify();
                        }),
                    )
                }),
        )
        .children(
            INFLUENCE_STEPS
                .iter()
                .enumerate()
                .map(|(i, &(delta, label))| {
                    components::ghost_button(
                        MotionKey::new("velocity-influence-step", tag).with(i),
                        label,
                        theme,
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                            this.state.update(cx, |state, _| {
                                let next = (influence + delta).clamp(0.1, 100.0);
                                let _ = state.set_keyframe_velocity_influence(side, next);
                            });
                            cx.notify();
                        }),
                    )
                }),
        );

    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .child(
            div()
                .text_size(Theme::TEXT_UI)
                .text_color(theme.text_muted)
                .child(match side {
                    VelocitySide::Incoming => "Incoming Velocity",
                    VelocitySide::Outgoing => "Outgoing Velocity",
                }),
        )
        .child(readout(theme, "Speed", format!("{speed:.2} {}", form.unit)))
        .child(speed_steps)
        .child(readout(theme, "Influence", format!("{influence:.2}%")))
        .child(influence_presets)
}

pub fn render(
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let theme = Theme::of(cx);
    let form = state.read(cx).keyframe_velocity.clone();

    let cancel = components::ghost_button("keyframe-velocity-cancel", "Cancel", &theme)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                this.state
                    .update(cx, |state, _| state.open_dialog(DialogKind::None));
                cx.notify();
            }),
        );
    let apply = components::button("keyframe-velocity-apply", "OK", &theme).on_mouse_down(
        MouseButton::Left,
        cx.listener(|this, _: &MouseDownEvent, _, cx| {
            this.state
                .update(cx, |state, _| state.apply_keyframe_velocity());
            cx.notify();
        }),
    );
    let continuous = components::chip(
        "keyframe-velocity-continuous",
        if form.continuous {
            "Continuous: On"
        } else {
            "Continuous: Off"
        },
        form.continuous,
        ChipSize::Regular,
        &theme,
    )
    .on_mouse_down(
        MouseButton::Left,
        cx.listener(|this, _: &MouseDownEvent, _, cx| {
            this.state
                .update(cx, |state, _| state.toggle_keyframe_velocity_continuous());
            cx.notify();
        }),
    );

    let count = form.targets.len();
    let mut content = div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_MD))
        .child(
            div()
                .text_size(Theme::TEXT_TITLE)
                .text_color(theme.text_muted)
                .child(format!(
                    "{count} keyframe{}",
                    if count == 1 { "" } else { "s" }
                )),
        )
        .child(side_block(VelocitySide::Incoming, &form, &theme, cx))
        .child(side_block(VelocitySide::Outgoing, &form, &theme, cx))
        .child(continuous);
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
        "keyframe-velocity",
        DialogKind::KeyframeVelocity,
        "Keyframe Velocity",
        state,
        cx,
        body,
    )
}
