//! The Audio Gain dialog (`G`).

use crate::components::Wraps as _;
use gpui::{
    Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement, Styled,
    div, px,
};

use crate::app::clip_ops::AudioGainMode;
use crate::app::{AppState, DialogKind};
use crate::chrome::dialog::{DialogBody, dialog_surface};
use crate::components::{self, ChipSize};
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

const DB_STEPS: &[(f32, &str, &str)] = &[
    (-3.0, "gain-step-m3", "-3 dB"),
    (-1.0, "gain-step-m1", "-1 dB"),
    (-0.5, "gain-step-m05", "-0.5 dB"),
    (0.5, "gain-step-p05", "+0.5 dB"),
    (1.0, "gain-step-p1", "+1 dB"),
    (3.0, "gain-step-p3", "+3 dB"),
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

    let (active_db, unit) = match form.mode {
        AudioGainMode::SetGain => (form.set_db, "dB"),
        AudioGainMode::AdjustGain => (form.adjust_db, "dB"),
        AudioGainMode::NormalizeMaxPeak | AudioGainMode::NormalizeAllPeaks => {
            (form.target_db, "dBFS")
        }
    };

    let presets = match form.mode {
        AudioGainMode::SetGain | AudioGainMode::AdjustGain => DB_PRESETS,
        AudioGainMode::NormalizeMaxPeak | AudioGainMode::NormalizeAllPeaks => NORMALIZE_PRESETS,
    };

    let mode = form.mode;
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
                    this.state.update(cx, |state, _| match mode {
                        AudioGainMode::SetGain => {
                            let _ = state.set_audio_gain_set_db(val);
                        }
                        AudioGainMode::AdjustGain => {
                            let _ = state.set_audio_gain_adjust_db(val);
                        }
                        AudioGainMode::NormalizeMaxPeak | AudioGainMode::NormalizeAllPeaks => {
                            let _ = state.set_audio_gain_target_db(val);
                        }
                    });
                    cx.notify();
                }),
            )
        }));

    let step_row = div()
        .flex()
        .wraps()
        .gap(px(Theme::SPACE_XS))
        .children(DB_STEPS.iter().map(|&(delta, id, label)| {
            let next = active_db + delta;
            components::ghost_button(("audio-gain-step", id), label, &theme).on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    this.state.update(cx, |state, _| match mode {
                        AudioGainMode::SetGain => {
                            let _ = state.set_audio_gain_set_db(next);
                        }
                        AudioGainMode::AdjustGain => {
                            let _ = state.set_audio_gain_adjust_db(next);
                        }
                        AudioGainMode::NormalizeMaxPeak | AudioGainMode::NormalizeAllPeaks => {
                            let _ = state.set_audio_gain_target_db(next);
                        }
                    });
                    cx.notify();
                }),
            )
        }));

    let peak_text = match form.max_peak_dbfs {
        Some(peak) => format!(
            "Peak Amplitude: {peak:.1} dBFS ({} audio clip{})",
            form.clip_count,
            if form.clip_count == 1 { "" } else { "s" }
        ),
        None => format!(
            "Peak Amplitude: silence ({} audio clip{})",
            form.clip_count,
            if form.clip_count == 1 { "" } else { "s" }
        ),
    };

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
                                .child(form.mode.label()),
                        )
                        .child(
                            div()
                                .text_size(Theme::TEXT_UI)
                                .text_color(theme.text)
                                .child(format!("{active_db:+.1} {unit}")),
                        ),
                )
                .child(preset_row)
                .child(step_row),
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
