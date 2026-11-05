//! The Keyframe Velocity dialog: incoming and outgoing speed and influence of
//! the selected keyframes, with Continuous to keep one speed on both sides.

use std::rc::Rc;

use crate::components::Wraps as _;
use aria_core::count::counted;
use gpui::{
    Div, Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    SharedString, Styled, div, px,
};

use crate::app::animation_verbs::{INFLUENCE_PERCENT_SPAN, KeyframeVelocityForm, VelocitySide};
use crate::app::param_units::Units;
use crate::app::value_scrub::{ScrubTarget, ValueKind, Write};
use crate::app::{AppState, DialogKind};
use crate::chrome::dialog::{DialogBody, dialog_surface, toggle_row};
use crate::components::{self, ChipSize};
use crate::motion::MotionKey;
use crate::panels::value_readout::{self, Look};
use crate::theme::Theme;

/// Influence presets in per cent.
const INFLUENCE_PRESETS: &[(f32, &str)] = &[
    (10.0, "10%"),
    (33.33, "33.33%"),
    (50.0, "50%"),
    (75.0, "75%"),
    (100.0, "100%"),
];

fn side_id(side: VelocitySide) -> &'static str {
    match side {
        VelocitySide::Incoming => "in",
        VelocitySide::Outgoing => "out",
    }
}

/// A label and a readout in one row, laid out as
/// [`components::readout`] lays out a fixed value: the label at the left in
/// `text_muted`, the number at the right edge.
fn number_row(
    theme: &Theme,
    state: &Entity<AppState>,
    label: &'static str,
    target: ScrubTarget,
    text: impl Into<SharedString>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Div {
    let look = Look {
        size: Theme::TEXT_UI,
        ink: theme.value,
    };
    div()
        .flex()
        .flex_row()
        .flex_wrap()
        .gap_x(px(Theme::SPACE_SM))
        .text_size(Theme::TEXT_UI)
        .child(div().max_w_full().text_color(theme.text_muted).child(label))
        .child(
            div()
                .ml_auto()
                .max_w_full()
                .child(value_readout::readout(theme, state, target, text, look, cx)),
        )
}

fn side_block(
    side: VelocitySide,
    form: &KeyframeVelocityForm,
    theme: &Theme,
    state: &Entity<AppState>,
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
    let speed_presets = div().flex().child(zero);
    let influence_presets = div().flex().wraps().gap(px(Theme::SPACE_XS)).children(
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
    );

    let key = MotionKey::new("keyframe-velocity", tag);
    let speed_row = number_row(
        theme,
        state,
        "Speed",
        ScrubTarget {
            key: key.with("speed"),
            value: speed,
            unit: 1.0,
            kind: ValueKind::Number { decimals: 2 },
            write: Write::Draft {
                set: Rc::new(move |state: &mut AppState, per_second| {
                    state.set_keyframe_velocity_speed(side, per_second)
                }),
                span: f32::MIN..=f32::MAX,
            },
        },
        format!("{speed:.2} {}", form.unit),
        cx,
    );
    let units = Units::INFLUENCE;
    let influence_row = number_row(
        theme,
        state,
        "Influence",
        ScrubTarget {
            key: key.with("influence"),
            value: influence,
            unit: units.number.drag,
            kind: units.number.kind(),
            write: Write::Draft {
                set: Rc::new(move |state: &mut AppState, percent| {
                    state.set_keyframe_velocity_influence(side, percent)
                }),
                span: INFLUENCE_PERCENT_SPAN,
            },
        },
        form.influence_label(side),
        cx,
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
        .child(speed_row)
        .child(speed_presets)
        .child(influence_row)
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
    let continuous = toggle_row(
        &theme,
        "keyframe-velocity-continuous",
        "Continuous",
        form.continuous,
        AppState::toggle_keyframe_velocity_continuous,
        cx,
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
                .child(counted(count, "keyframe", "keyframes").to_string()),
        )
        .child(side_block(VelocitySide::Incoming, &form, &theme, state, cx))
        .child(side_block(VelocitySide::Outgoing, &form, &theme, state, cx))
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
