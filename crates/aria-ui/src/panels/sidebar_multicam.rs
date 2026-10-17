//! Multicam UI in the pool sidebar: create a multicam source, switch active
//! angles, and place each camera on the group's clock against its sound.

use gpui::{
    Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    SharedString, Styled, div, px,
};

use crate::app::AppState;
use crate::components;
use crate::fluent::FluentBuilder;
use crate::surfaces::sidebar_pick::{self, Pick};
use crate::theme::Theme;

/// Press one row of the registry, reporting a refusal where the operator is
/// looking rather than swallowing it. The button used to discard the result, so
/// a group the model refused looked like one it built.
fn press(state: &mut AppState, pick: Pick) {
    if let Err(err) = sidebar_pick::apply(state, pick) {
        state.report_error(&err);
    }
}

/// One angle's strip: wide enough for a shape to be read, short enough that a
/// group of six cameras still fits a sidebar.
const STRIP_W: f32 = 168.;
const STRIP_H: f32 = 22.;
/// Pixels per drawn bar. Two is the narrowest that still leaves a gap between
/// bars at this height.
const BAR_W: f32 = 2.;

/// What one angle row draws, read while the state is readable.
struct Row {
    label: String,
    offset: i64,
    /// Bar heights over the group's whole span, or empty when this camera's
    /// sound has not been read.
    bars: Vec<f32>,
}

/// Every angle of `group` as a row, each placed on the group's common clock.
///
/// The window is the group's whole span: the earliest camera's start to the
/// latest camera's end. Two strips drawn one above the other are therefore two
/// pictures of the same stretch of time, and an offset that is wrong reads as
/// sound that does not line up. Cutting each strip to its own file instead
/// would draw every camera as if it started on time, which is the thing the
/// operator is trying to see.
fn rows(state: &AppState, group: &crate::types::UiMulticamGroup, fps: f64) -> Vec<Row> {
    let count = (STRIP_W / BAR_W) as usize;
    let placed: Vec<(f64, Option<f64>)> = group
        .angles
        .iter()
        .map(|angle| {
            let start = angle.sync_offset as f64 / fps;
            let seconds = state.waveforms.get(angle.media).and_then(|waveform| {
                (waveform.per_second > 0.0)
                    .then(|| waveform.peaks.len() as f64 / waveform.per_second)
            });
            (start, seconds)
        })
        .collect();
    let from = placed
        .iter()
        .map(|(start, _)| *start)
        .fold(f64::INFINITY, f64::min)
        .min(0.0);
    let to = placed
        .iter()
        .map(|(start, seconds)| start + seconds.unwrap_or(0.0))
        .fold(f64::NEG_INFINITY, f64::max)
        .max(from);
    group
        .angles
        .iter()
        .enumerate()
        .zip(&placed)
        .map(|((index, angle), (start, _))| Row {
            label: match &angle.name {
                Some(name) => name.to_string(),
                None => format!("Cam {}", index + 1),
            },
            offset: angle.sync_offset,
            bars: state
                .waveforms
                .get(angle.media)
                .map(|waveform| crate::waveforms::bars(waveform, from - start, to - start, count))
                .unwrap_or_default(),
        })
        .collect()
}

/// An offset as the operator nudges it: frames of the sequence rate, signed, so
/// zero reads as sync rather than as an empty field.
fn offset_label(offset: i64) -> String {
    match offset {
        0 => "0f".to_string(),
        other => format!("{other:+}f"),
    }
}

/// Render the multicam section in the pool sidebar: button to create from selected items,
/// list of multicam groups with their angle count, and angle switch buttons.
pub fn multicam_section(
    theme: &Theme,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    // The projection carries the groups and their cuts, so this panel never
    // reaches past it into the engine. Taken by value: the listeners below
    // need `cx` mutably, so no borrow of the entity may outlive this line.
    // The playhead comes along because the live angle is a read of the cut
    // list at a tick, and the tick changes without the project changing.
    let (groups, playhead, fps) = {
        let read = state.read(cx);
        (
            read.multicam_groups(),
            read.store.playhead_ticks,
            read.store.sequence.frame_rate.as_f64().max(1.0),
        )
    };
    // A camera's sound is what an offset is aimed against, so this asks for the
    // peaks for the same reason the timeline asks for a visible clip's: the
    // angle is on screen. The request is idempotent per source, and a file with
    // no sound refuses once rather than being retried per frame.
    let sources: Vec<(u64, std::path::PathBuf)> = {
        let read = state.read(cx);
        groups
            .iter()
            .flat_map(|group| group.angles.iter())
            .filter_map(|angle| {
                read.store
                    .media
                    .iter()
                    .find(|item| item.id == angle.media)
                    .map(|item| (angle.media, std::path::PathBuf::from(&*item.path)))
            })
            .collect()
    };
    for (media, path) in sources {
        crate::waveforms::request(state, cx, media, path);
    }
    let strips: Vec<Vec<Row>> = {
        let read = state.read(cx);
        groups.iter().map(|group| rows(read, group, fps)).collect()
    };

    let create_button =
        components::ghost_button("create-multicam-group", "+ Multicam Group", theme).on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                this.state
                    .update(cx, |state, _| press(state, Pick::MulticamGroup));
                cx.notify();
            }),
        );

    let mut group_elements = Vec::new();
    for (group, angles) in groups.iter().zip(strips) {
        let gid = aria_models::MulticamGroupId::from_raw(group.id);
        let angle_count = group.angles.len();
        let count_label = format!(
            "{angle_count} angle{}",
            if angle_count == 1 { "" } else { "s" }
        );
        let active_angle = group.active_angle_at(playhead);

        let mut angle_buttons = Vec::new();
        for (idx, angle) in angles.into_iter().enumerate() {
            let is_active = idx == active_angle;
            let chip = components::chip(
                crate::motion::MotionKey::new("switch-multicam-angle", group.id).with(idx),
                &angle.label,
                is_active,
                components::ChipSize::Compact,
                theme,
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    this.state.update(cx, |state, _| {
                        press(
                            state,
                            Pick::MulticamAngle {
                                group: gid,
                                angle: idx,
                            },
                        )
                    });
                    cx.notify();
                }),
            );
            // An empty strip is a camera whose sound has not been read yet, so
            // the frame it leaves is the strip's own outline rather than a bar
            // of silence that would read as a quiet take.
            let strip = div()
                .w(px(STRIP_W))
                .h(px(STRIP_H))
                .rounded(px(Theme::CONTROL_RADIUS))
                .bg(theme.surface)
                .when(!angle.bars.is_empty(), |el| {
                    el.child(crate::waveforms::wave(angle.bars, theme))
                });
            let back = components::icon_button(
                (
                    "nudge-multicam-back",
                    crate::motion::MotionKey::new("nudge-multicam-back", group.id).with(idx),
                ),
                crate::icons::CHEVRON_LEFT,
                theme,
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    this.state.update(cx, |state, _| {
                        press(
                            state,
                            Pick::MulticamNudge {
                                group: gid,
                                angle: idx,
                                frames: -1,
                            },
                        )
                    });
                    cx.notify();
                }),
            );
            let forward = components::icon_button(
                (
                    "nudge-multicam-forward",
                    crate::motion::MotionKey::new("nudge-multicam-forward", group.id).with(idx),
                ),
                crate::icons::CHEVRON_RIGHT,
                theme,
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    this.state.update(cx, |state, _| {
                        press(
                            state,
                            Pick::MulticamNudge {
                                group: gid,
                                angle: idx,
                                frames: 1,
                            },
                        )
                    });
                    cx.notify();
                }),
            );

            angle_buttons.push(
                div()
                    .id(crate::motion::MotionKey::new("multicam-angle", group.id)
                        .with(idx)
                        .element_id("multicam-angle"))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(Theme::SPACE_XS))
                    .child(chip)
                    .child(strip)
                    .child(back)
                    .child(
                        div()
                            .text_size(Theme::TEXT_SMALL)
                            .text_color(theme.text_muted)
                            .child(SharedString::from(offset_label(angle.offset))),
                    )
                    .child(forward)
                    .into_any_element(),
            );
        }

        // A group is only a set of cameras until it is on a lane, and the length
        // it lands with is the model's answer, not this panel's.
        let place_button = components::ghost_button(
            crate::motion::MotionKey::new("place-multicam-group", group.id),
            "Place",
            theme,
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                this.state.update(cx, |state, _| {
                    press(state, Pick::MulticamPlace { group: gid })
                });
                cx.notify();
            }),
        );

        // Measured from the sound the cameras recorded, which is why it sits
        // beside the strips rather than in a menu: the press and its result are
        // in the same place.
        let sync_button = components::icon_button(
            (
                "sync-multicam-group",
                crate::motion::MotionKey::new("sync-multicam-group", group.id),
            ),
            crate::icons::AUDIO_LINES,
            theme,
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                this.state.update(cx, |state, _| {
                    press(state, Pick::MulticamSync { group: gid })
                });
                cx.notify();
            }),
        );

        group_elements.push(
            div()
                .id(crate::motion::MotionKey::new("multicam-group", group.id)
                    .element_id("multicam-group"))
                .flex()
                .flex_col()
                .gap(px(Theme::SPACE_XXS))
                .p(px(Theme::SPACE_XS))
                .rounded(px(Theme::CONTROL_RADIUS))
                .bg(theme.surface_raised)
                .border_1()
                .border_color(theme.border)
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .justify_between()
                        .items_center()
                        .child(
                            div()
                                .text_size(Theme::TEXT_TITLE)
                                .text_color(theme.text)
                                .child(group.name.to_string()),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(Theme::SPACE_XS))
                                .child(
                                    div()
                                        .text_size(Theme::TEXT_SMALL)
                                        .text_color(theme.text_muted)
                                        .child(SharedString::from(count_label)),
                                )
                                .child(sync_button)
                                .child(place_button),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(Theme::SPACE_XS))
                        .children(angle_buttons),
                )
                .into_any_element(),
        );
    }

    div()
        .id("sidebar-multicam")
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .child(
            div()
                .flex()
                .flex_row()
                .justify_between()
                .items_center()
                .child(create_button),
        )
        .when(!group_elements.is_empty(), |el| {
            el.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(Theme::SPACE_XS))
                    .children(group_elements),
            )
        })
        .into_any_element()
}
