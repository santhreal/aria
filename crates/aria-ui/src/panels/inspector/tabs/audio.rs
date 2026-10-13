//! Audio tab: volume, pan, fades, and the measured loudness contour.

use gpui::{
    Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    SharedString, Styled, div, px,
};

use crate::app::AppState;
use crate::components;
use crate::fluent::FluentBuilder;
use crate::panels::insert_chain::{self, Chain};
use crate::surfaces::clip_pick::{self, Pick};
use crate::theme::Theme;
use crate::timecode::format_ticks;
use crate::types::UiClip;

use crate::panels::inspector::blocks::widgets::{audio_row, param_rows, section_label};

pub fn audio_tab(
    theme: &Theme,
    clip: &UiClip,
    fps_num: i32,
    fps_den: i32,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let fade_in = clip.fade_in_ticks;
    let fade_out = clip.fade_out_ticks;
    let reverse_label = if clip.reversed { "On" } else { "Off" };
    // Volume and pan are catalog properties, so the block names its group and
    // takes what the clip owns in it; the fades are not, so they are drawn here.
    let mut rows = param_rows(theme, "audio", state, cx);
    rows.push(
        audio_row(
            theme,
            crate::motion::MotionKey::new("insp.row", "aud-fi"),
            "Fade in",
            format_ticks(fade_in, fps_num, fps_den),
            1.0,
            move |state, delta| {
                clip_pick::apply(state, Pick::FadeIn((fade_in as f32 + delta).round() as i64))
            },
            cx,
        )
        .into_any_element(),
    );
    rows.push(
        audio_row(
            theme,
            crate::motion::MotionKey::new("insp.row", "aud-fo"),
            "Fade out",
            format_ticks(fade_out, fps_num, fps_den),
            1.0,
            move |state, delta| {
                clip_pick::apply(
                    state,
                    Pick::FadeOut((fade_out as f32 + delta).round() as i64),
                )
            },
            cx,
        )
        .into_any_element(),
    );
    let reverse = components::ghost_button("inspector-reverse", reverse_label, theme)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                this.state.update(cx, |state, _| {
                    if let Err(err) = clip_pick::apply(state, Pick::ToggleReverse) {
                        state.report_error(&err);
                    }
                });
                cx.notify();
            }),
        );
    // The clip's insert chain is drawn only where the model accepts one: a clip
    // with sound.
    let inserts = clip.can_hold_inserts.then(|| {
        insert_chain::render(
            theme,
            Chain {
                target: aria_models::AudioInsertTarget::Clip(aria_models::ClipId::from_raw(
                    clip.id,
                )),
                inserts: &clip.audio_inserts,
                keys: &[],
                expanded: true,
            },
            state,
            cx,
        )
    });
    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .child(section_label(theme, "Audio"))
        .children(rows)
        .child(
            div()
                .flex()
                .flex_row()
                .justify_between()
                .items_center()
                .px(px(Theme::SPACE_XS))
                .py(px(Theme::SPACE_XS))
                .child(
                    div()
                        .text_size(px(13.))
                        .text_color(theme.text_muted)
                        .child("Reverse"),
                )
                .child(reverse),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .justify_between()
                .items_center()
                .px(px(Theme::SPACE_XS))
                .py(px(Theme::SPACE_XS))
                .child(
                    div()
                        .text_size(px(13.))
                        .text_color(theme.text_muted)
                        .child("Pitch lock"),
                )
                .child(
                    components::ghost_button(
                        "inspector-pitch",
                        if clip.preserve_pitch { "On" } else { "Off" },
                        theme,
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                            this.state.update(cx, |state, _| {
                                if let Err(err) = clip_pick::apply(state, Pick::TogglePitchLock) {
                                    state.report_error(&err);
                                }
                            });
                            cx.notify();
                        }),
                    ),
                ),
        )
        .when_some(inserts, |el, chain| {
            el.child(section_label(theme, "Audio effects")).child(chain)
        })
        .child(section_label(theme, "Role"))
        .child(
            div()
                .text_size(px(11.))
                .text_color(theme.text_faint)
                .child(match clip.audio_role {
                    Some(role) => role.label(),
                    None => "Untagged",
                }),
        )
        .children(aria_models::AudioRole::ALL.into_iter().map(|role| {
            components::ghost_button(role.id(), role.label(), theme).on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    this.state.update(cx, |state, _| {
                        if let Err(err) = clip_pick::apply(state, Pick::Role(Some(role))) {
                            state.report_error(&err);
                        }
                    });
                    cx.notify();
                }),
            )
        }))
        .child(
            components::ghost_button("aud-role-clear", "Untag", theme).on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, _, cx| {
                    this.state.update(cx, |state, _| {
                        if let Err(err) = clip_pick::apply(state, Pick::Role(None)) {
                            state.report_error(&err);
                        }
                    });
                    cx.notify();
                }),
            ),
        )
        .child(section_label(theme, "Loudness"))
        .children(loudness_rows(theme, state, cx))
        .child(section_label(theme, "Duck under voice"))
        .children(duck_rows(theme, state, cx))
        .child(
            div()
                .pt(px(Theme::SPACE_SM))
                .text_size(px(11.))
                .text_color(theme.text_faint)
                .child("Left click raises, right click lowers."),
        )
}

/// The dip's shape, the lane it listens to, and the button that measures it.
///
/// The button carries its own refusal: a duck needs a selection, a flagged voice
/// lane, and audio on it, and a greyed button with no sentence is a feature the
/// operator cannot find their way into. The precheck that would refuse the run is
/// the same one that writes this line.
fn duck_rows(
    theme: &Theme,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Vec<gpui::AnyElement> {
    let form = state.read(cx).duck;
    let refusal = state.read(cx).duck_precheck().err();
    let lane = state.read(cx).duck_source_name();
    let mut rows: Vec<gpui::AnyElement> = Vec::new();
    let choices: [(&'static str, &'static str, String, fn(&mut AppState)); 3] = [
        (
            "duck-amount",
            "Dip to",
            form.amount_label(),
            AppState::cycle_duck_amount,
        ),
        (
            "duck-attack",
            "Attack",
            form.attack_label(),
            AppState::cycle_duck_attack,
        ),
        (
            "duck-release",
            "Release",
            form.release_label(),
            AppState::cycle_duck_release,
        ),
    ];
    for (id, label, value, cycle) in choices {
        rows.push(
            div()
                .flex()
                .flex_row()
                .justify_between()
                .items_center()
                .px(px(Theme::SPACE_XS))
                .py(px(Theme::SPACE_XS))
                .child(
                    div()
                        .text_size(px(13.))
                        .text_color(theme.text_muted)
                        .child(SharedString::from(label)),
                )
                .child(components::ghost_button(id, &value, theme).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        this.state.update(cx, |state, _| cycle(state));
                        cx.notify();
                    }),
                ))
                .into_any_element(),
        );
    }
    rows.push(
        div()
            .px(px(Theme::SPACE_XS))
            .text_size(px(11.))
            .text_color(theme.text_faint)
            .child(match &lane {
                Some(name) => format!("Listening to {name}"),
                None => "No lane is the duck source yet".to_string(),
            })
            .into_any_element(),
    );
    rows.push(
        components::ghost_button("duck-run", "Duck under voice", theme)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    this.state.update(cx, |state, cx| {
                        if let Err(err) = state.duck_selected(cx) {
                            state.status_message = Some(err);
                        }
                    });
                    cx.notify();
                }),
            )
            .into_any_element(),
    );
    if let Some(reason) = refusal {
        rows.push(
            div()
                .px(px(Theme::SPACE_XS))
                .text_size(px(11.))
                .text_color(theme.warning)
                .child(reason)
                .into_any_element(),
        );
    }
    rows
}

/// The measured contour as bars, plus a clipping line.
///
/// Drawn from what the Analysis tab measured rather than measuring here: a decode per
/// inspector frame would make selecting a clip cost a file read. With nothing measured
/// the section says where to measure it instead of drawing an empty axis.
fn loudness_rows(
    theme: &Theme,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Vec<gpui::AnyElement> {
    // The handle, not the contour: a measured minute is over a thousand windows
    // and this reduces them to forty-eight columns without owning any of them.
    let contour = state.read(cx).analysis.loudness.clone();
    if contour.is_empty() {
        return vec![
            div()
                .text_size(px(11.))
                .text_color(theme.text_faint)
                .child("Measure it in the Analysis tab")
                .into_any_element(),
        ];
    }
    let clipping = state.read(cx).loudness_clips();
    let peak = state.read(cx).loudness_peak_dbfs().unwrap_or(0.0);
    // One row per bar would be one element per 50 ms window: a minute of audio is 1200
    // elements nothing can read. The contour is reduced to a fixed number of columns,
    // each the loudest window it covers, because a peak that averages away is the one
    // the operator was looking for.
    const COLUMNS: usize = 48;
    let per_column = contour.len().div_ceil(COLUMNS).max(1);
    let bars: Vec<gpui::AnyElement> = contour
        .chunks(per_column)
        .map(|window| {
            let loudest = window
                .iter()
                .map(|point| point.dbfs)
                .fold(f32::NEG_INFINITY, f32::max);
            // -60 dBFS is the floor of the drawing, not of the measurement.
            let height = ((loudest + 60.0) / 60.0).clamp(0.02, 1.0) * 40.0;
            div()
                .w(px(3.))
                .h(px(height))
                .bg(if loudest >= -0.1 {
                    theme.danger
                } else {
                    theme.accent
                })
                .into_any_element()
        })
        .collect();
    vec![
        div()
            .flex()
            .flex_row()
            .items_end()
            .h(px(44.))
            .gap(px(1.))
            .children(bars)
            .into_any_element(),
        div()
            .text_size(px(11.))
            .text_color(if clipping {
                theme.danger
            } else {
                theme.text_muted
            })
            .child(SharedString::from(if clipping {
                format!("Peak {peak:.1} dBFS — clipping")
            } else {
                format!("Peak {peak:.1} dBFS")
            }))
            .into_any_element(),
    ]
}
