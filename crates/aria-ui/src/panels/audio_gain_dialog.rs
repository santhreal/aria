//! The Audio Gain dialog (`G`).

use std::rc::Rc;

use crate::components::Wraps as _;
use aria_core::count::counted;
use gpui::{
    Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement, Styled,
    div, px,
};

use crate::app::clip_ops::AudioGainMode;
use crate::app::value_scrub::{ScrubTarget, ValueKind, Write, write_number};
use crate::app::{AppState, DialogKind};
use crate::chrome::dialog::{DialogBody, dialog_surface};
use crate::components::{self, ChipSize};
use crate::motion::MotionKey;
use crate::panels::value_readout::{self, Look};
use crate::theme::Theme;

const DB_PRESETS: &[(f32, &str, &str)] = &[
    (-12.0, "gain-preset-m12", "-12 dB"),
    (-6.0, "gain-preset-m6", "-6 dB"),
    (-3.0, "gain-preset-m3", "-3 dB"),
    (-1.0, "gain-preset-m1", "-1 dB"),
    (0.0, "gain-preset-0", "0 dB"),
    (3.0, "gain-preset-p3", "+3 dB"),
    (6.0, "gain-preset-p6", "+6 dB"),
];

const NORMALIZE_PRESETS: &[(f32, &str, &str)] = &[
    (-12.0, "norm-preset-m12", "-12 dBFS"),
    (-6.0, "norm-preset-m6", "-6 dBFS"),
    (-3.0, "norm-preset-m3", "-3 dBFS"),
    (-1.0, "norm-preset-m1", "-1 dBFS"),
    (0.0, "norm-preset-0", "0 dBFS"),
];

pub fn render(
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let theme = Theme::of(cx);
    let form = state.read(cx).audio_gain.clone();

    let cancel = components::ghost_button("audio-gain-cancel", "Cancel", &theme).on_mouse_down(
        MouseButton::Left,
        cx.listener(|this, _: &MouseDownEvent, _, cx| {
            this.state
                .update(cx, |state, _| state.open_dialog(DialogKind::None));
            cx.notify();
        }),
    );
    let apply = components::button("audio-gain-apply", "Apply", &theme).on_mouse_down(
        MouseButton::Left,
        cx.listener(|this, _: &MouseDownEvent, _, cx| {
            this.state.update(cx, |state, _| state.apply_audio_gain());
            cx.notify();
        }),
    );

    let mode_row =
        div()
            .flex()
            .wraps()
            .gap(px(Theme::SPACE_XS))
            .children(AudioGainMode::ALL.into_iter().map(|mode| {
                let active = form.mode == mode;
                let id = match mode {
                    AudioGainMode::SetGain => "audio-gain-mode-set",
                    AudioGainMode::AdjustGain => "audio-gain-mode-adjust",
                    AudioGainMode::NormalizeMaxPeak => "audio-gain-mode-norm-max",
                    AudioGainMode::NormalizeAllPeaks => "audio-gain-mode-norm-all",
                };
                components::chip(
                    ("audio-gain-mode", id),
                    mode.label(),
                    active,
                    ChipSize::Regular,
                    &theme,
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        this.state
                            .update(cx, |state, _| state.set_audio_gain_mode(mode));
                        cx.notify();
                    }),
                )
            }));

    let active_db = form.db();
    let span = form.mode.span();
    let unit = match form.mode {
        AudioGainMode::SetGain | AudioGainMode::AdjustGain => "dB",
        AudioGainMode::NormalizeMaxPeak | AudioGainMode::NormalizeAllPeaks => "dBFS",
    };
    let mut level = String::new();
    write_number(&mut level, active_db, 1, true);
    level.push(' ');
    level.push_str(unit);

    let presets = match form.mode {
        AudioGainMode::SetGain | AudioGainMode::AdjustGain => DB_PRESETS,
        AudioGainMode::NormalizeMaxPeak | AudioGainMode::NormalizeAllPeaks => NORMALIZE_PRESETS,
    };

    let preset_row = div()
        .flex()
        .wraps()
        .gap(px(Theme::SPACE_XS))
        .children(presets.iter().map(|&(val, id, label)| {
            let active = (active_db - val).abs() < 0.05;
            components::chip(
                ("audio-gain-preset", id),
                label,
                active,
                ChipSize::Compact,
                &theme,
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    this.state.update(cx, |state, _| {
                        let _ = state.set_audio_gain_db(val);
                    });
                    cx.notify();
                }),
            )
        }));

    let peak_text = match form.max_peak_dbfs {
        Some(peak) => format!(
            "Peak Amplitude: {peak:.1} dBFS ({})",
            counted(form.clip_count, "audio clip", "audio clips")
        ),
        None => format!(
            "Peak Amplitude: silence ({})",
            counted(form.clip_count, "audio clip", "audio clips")
        ),
    };

    let value = value_readout::readout(
        &theme,
        state,
        ScrubTarget {
            key: MotionKey::new("audio-gain", "db"),
            value: active_db,
            unit: 0.1,
            kind: ValueKind::Number { decimals: 1 },
            write: Write::Draft {
                set: Rc::new(|state: &mut AppState, db| state.set_audio_gain_db(db)),
                span,
            },
        },
        level,
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
                .child(peak_text),
        )
        .child(mode_row)
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
                                .child(form.mode.quantity()),
                        )
                        .child(value),
                )
                .child(preset_row),
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
        "audio-gain",
        DialogKind::AudioGain,
        "Audio Gain",
        state,
        cx,
        body,
    )
}
