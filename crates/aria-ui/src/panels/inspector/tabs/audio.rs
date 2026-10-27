//! Audio tab: volume, pan, fades, and the measured loudness contour.

use gpui::{
    Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    SharedString, Styled, div, px,
};

use crate::app::AppState;
use crate::components;
use crate::motion::MotionKey;
use crate::panels::insert_chain::{self, Chain};
use crate::surfaces::clip_pick::{self, Pick};
use crate::surfaces::inspector_pick;
use crate::theme::Theme;
use crate::types::UiClip;
use aria_models::AudioRole;
use gpui::prelude::FluentBuilder;

use crate::panels::inspector::blocks::widgets::{
    Chip, Columns, Readout, chip_row, control_line, ghosts_under_labels, label_note, param_rows,
    pick_button, switch_line, under_labels, unkeyed_row,
};

use crate::components::section_label;

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
    // Volume and pan are catalog properties, so the block names its group and
    // takes what the clip has in it; the fades, the switches and the role are
    // not, so they are drawn here, in the columns the keyed rows stand in.
    let mut rows = param_rows(theme, "audio", state, cx);
    rows.push(
        unkeyed_row(
            theme,
            state,
            MotionKey::new("insp.row", "aud-fi"),
            "Fade in",
            Readout::frames(fade_in, fps_num, fps_den),
            Columns::Keyed,
            move |state, delta| {
                clip_pick::apply(state, Pick::FadeIn((fade_in as f32 + delta).round() as i64))
            },
            cx,
        )
        .into_any_element(),
    );
    rows.push(
        unkeyed_row(
            theme,
            state,
            MotionKey::new("insp.row", "aud-fo"),
            "Fade out",
            Readout::frames(fade_out, fps_num, fps_den),
            Columns::Keyed,
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
    rows.push(switch_line(
        theme,
        ("aud-rev", "Reverse"),
        "inspector-reverse",
        clip.reversed,
        cx,
        |state| clip_pick::apply(state, Pick::ToggleReverse),
    ));
    rows.push(switch_line(
        theme,
        ("aud-pitch", "Pitch lock"),
        "inspector-pitch",
        clip.preserve_pitch,
        cx,
        |state| clip_pick::apply(state, Pick::TogglePitchLock),
    ));
    rows.push(switch_line(
        theme,
        ("aud-denoise", "Denoise"),
        "denoise-toggle",
        clip.denoise,
        cx,
        |state| inspector_pick::apply(state, inspector_pick::Pick::ToggleDenoise),
    ));
    rows.push(role_row(theme, clip.audio_role, cx));
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
                columns: Columns::Keyed,
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
        .when_some(inserts, |el, chain| {
            el.child(section_label(theme, "Audio effects")).child(chain)
        })
        .when(
            clip.transition_media == Some(aria_models::TransitionMedia::Audio),
            |el| {
                el.child(
                    crate::panels::inspector::blocks::transition::transition_block(
                        theme, clip, state, cx,
                    ),
                )
            },
        )
        .child(section_label(theme, "Loudness"))
        .children(loudness_rows(theme, state, cx))
        .child(section_label(theme, "Duck under voice"))
        .children(duck_rows(theme, state, cx))
}

/// The clip's audio role, with a chip for each role and, on a tagged clip, one
/// that clears the tag.
fn role_row(
    theme: &Theme,
    tagged: Option<AudioRole>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let mut chips: Vec<Chip> = AudioRole::ALL
        .into_iter()
        .map(|role| {
            Chip::choice(role.label(), tagged == Some(role), |label| {
                role_chip(theme, ("aud-role", role.id()), label, Some(role), cx)
            })
        })
        .collect();
    if tagged.is_some() {
        chips.push(Chip::other(role_chip(
            theme,
            "aud-role-clear",
            "Untag".into(),
            None,
            cx,
        )));
    }
    chip_row(
        theme,
        MotionKey::new("insp.row", "aud-role"),
        "Role",
        Columns::Keyed,
        tagged.map_or("Untagged", AudioRole::label),
        chips,
    )
    .into_any_element()
}

/// A chip that tags the selected clip with `role`, or clears its tag.
fn role_chip(
    theme: &Theme,
    id: impl Into<crate::motion::ControlId>,
    label: SharedString,
    role: Option<AudioRole>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    pick_button(id, label, theme, cx, move |state| {
        clip_pick::apply(state, Pick::Role(role))
    })
    .into_any_element()
}

/// The duck settings a row steps, by the row's id and label: the depth of the
/// dip, then its attack and its release.
pub(crate) const DUCK_SETTINGS: [(&str, &str); 3] = [
    ("duck-amount", "Dip to"),
    ("duck-attack", "Attack"),
    ("duck-release", "Release"),
];

/// The dip's shape, the lane it follows, and the button that measures it.
///
/// A duck needs a selection, a flagged voice lane, and audio on it. While one
/// is missing, the line under the button states which, from the precheck that
/// rejects the run.
fn duck_rows(
    theme: &Theme,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Vec<gpui::AnyElement> {
    let form = state.read(cx).duck;
    let refusal = state.read(cx).duck_precheck().err();
    let lane = state.read(cx).duck_source_name();
    let mut rows: Vec<gpui::AnyElement> = Vec::new();
    let steps: [(String, fn(&mut AppState)); 3] = [
        (form.amount_label(), AppState::cycle_duck_amount),
        (form.attack_label(), AppState::cycle_duck_attack),
        (form.release_label(), AppState::cycle_duck_release),
    ];
    for ((id, label), (value, cycle)) in DUCK_SETTINGS.into_iter().zip(steps) {
        rows.push(
            control_line(
                theme,
                MotionKey::new("insp.row", id),
                label,
                Columns::Keyed,
                pick_button(("duck-setting", id), value, theme, cx, move |state| {
                    cycle(state);
                    Ok(())
                }),
            )
            .into_any_element(),
        );
    }
    rows.push(
        label_note(
            Columns::Keyed,
            theme.text_faint,
            match &lane {
                Some(name) => format!("Listening to {name}"),
                None => "No lane is the duck source yet".to_string(),
            },
        )
        .into_any_element(),
    );
    let run = components::ghost_button("duck-run", "Duck under voice", theme).on_mouse_down(
        MouseButton::Left,
        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
            this.state.update(cx, |state, cx| {
                if let Err(err) = state.duck_selected(cx) {
                    state.status_message = Some(err);
                }
            });
            cx.notify();
        }),
    );
    rows.push(ghosts_under_labels(Columns::Keyed, [run.into_any_element()]).into_any_element());
    if let Some(reason) = refusal {
        rows.push(label_note(Columns::Keyed, theme.warning, reason).into_any_element());
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
            label_note(
                Columns::Keyed,
                theme.text_faint,
                "Measure it in the Analysis tab",
            )
            .into_any_element(),
        ];
    }
    let clipping = state.read(cx).loudness_clips();
    let peak = state.read(cx).loudness_peak_dbfs().unwrap_or(0.0);
    // One row per bar would be one element per 50 ms window: a minute of audio is 1200
    // elements nothing can read. The contour is reduced to a fixed number of columns,
    // each the loudest window it covers, because a peak that averages away is the one
    // the meter exists to show.
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
        under_labels(
            Columns::Keyed,
            [div()
                .flex()
                .flex_row()
                .items_end()
                .h(px(44.))
                .gap(px(1.))
                .children(bars)
                .into_any_element()],
        )
        .into_any_element(),
        label_note(
            Columns::Keyed,
            if clipping {
                theme.danger
            } else {
                theme.text_muted
            },
            crate::app::analysis::peak_line(peak, clipping),
        )
        .into_any_element(),
    ]
}
