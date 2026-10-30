//! The Markers dock panel: every ruler marker as a row.
//!
//! Markers used to exist only as flags on the ruler, so a marker off screen was
//! a marker nothing could find, and its colour and note were data
//! nothing showed. A row here names the time, the label, the colour, and the
//! note, and every control on it is an existing marker command: click seeks,
//! the swatch cycles the palette, Rename edits the label, Note edits the
//! comment, and Remove deletes it.
//!
//! Nothing is computed here that the state does not already own: the rows are
//! the projected markers in projection order (by tick), which is the order the
//! ruler draws them in.

use crate::components::Wraps as _;
use aria_core::count::counted;
use gpui::{
    CursorStyle, Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    ParentElement, SharedString, StatefulInteractiveElement, Styled, div, px,
};

use crate::app::AppState;
use crate::components;
use crate::motion;
use crate::surfaces::markers_pick::{self, Pick};
use crate::theme::Theme;
use crate::timecode;
use crate::types::{RenameTarget, UiMarker};
use gpui::prelude::FluentBuilder;

/// The surface name of the panel.
const SURFACE: &str = "markers-panel";

/// Window > Markers: the marker list, scrolled inside its frame.
pub fn render(
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let theme = Theme::of(cx);
    crate::edge_fade::scroll_y(
        div()
            .id(SURFACE)
            .debug_selector(|| SURFACE.into())
            .size_full()
            .p(px(Theme::SPACE_SM))
            .flex()
            .flex_col()
            .child(markers_panel(&theme, state, cx)),
    )
    .into_any_element()
}

fn markers_panel(
    theme: &Theme,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let is_empty = state.read(cx).store.sequence.markers.is_empty();
    let active_filter = state.read(cx).marker_color_filter();
    let markers = state.read(cx).visible_markers();
    let rate = state.read(cx).store.sequence.frame_rate;
    let playhead = state.read(cx).playhead.value;
    // The project's own count, not the row count. They agree unless a colour
    // filter is active or a projection dropped one.
    let held = state.read(cx).marker_count();

    if is_empty {
        return components::empty_state("markers", "No markers.", theme)
            // Not reachable from an ordinary sequence: no rows with markers on
            // the project means the projection lost them, and saying nothing
            // would present that as an empty ruler.
            .when(held > 0, |el| el.child(count_line(theme, 0, held)))
            .child(add_button(theme, cx))
            .into_any_element();
    }

    let mut rows = Vec::with_capacity(markers.len());
    for (index, marker) in markers.iter().enumerate() {
        rows.push(row(theme, marker, index, playhead, rate.num, rate.den, cx));
    }

    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .child(count_line(theme, rows.len(), held))
        .child(filter_bar(theme, active_filter, cx))
        .child(add_button(theme, cx))
        .children(rows)
        .into_any_element()
}

fn filter_bar(
    theme: &Theme,
    active: Option<aria_models::MarkerColor>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let all_chip = components::chip(
        "marker-filter-all",
        "All",
        active.is_none(),
        components::ChipSize::Compact,
        theme,
    )
    .on_mouse_down(
        MouseButton::Left,
        cx.listener(|this, _: &MouseDownEvent, _, cx| {
            this.state
                .update(cx, |state, _| state.set_marker_color_filter(None));
            cx.notify();
        }),
    );
    div()
        .flex()
        .wraps()
        .items_center()
        .gap(px(Theme::SPACE_XXS))
        .child(all_chip)
        .children(aria_models::MarkerColor::ALL.into_iter().map(|color| {
            let is_active = active == Some(color);
            let id = match color {
                aria_models::MarkerColor::Teal => "marker-filter-teal",
                aria_models::MarkerColor::Blue => "marker-filter-blue",
                aria_models::MarkerColor::Green => "marker-filter-green",
                aria_models::MarkerColor::Yellow => "marker-filter-yellow",
                aria_models::MarkerColor::Orange => "marker-filter-orange",
                aria_models::MarkerColor::Red => "marker-filter-red",
                aria_models::MarkerColor::Pink => "marker-filter-pink",
                aria_models::MarkerColor::Purple => "marker-filter-purple",
            };
            div()
                .id(id)
                .w(px(14.))
                .h(px(14.))
                .rounded(px(Theme::CONTROL_RADIUS))
                .border_1()
                .border_color(if is_active {
                    theme.accent
                } else {
                    theme.border
                })
                .bg(theme.marker(color))
                .cursor(CursorStyle::PointingHand)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        this.state.update(cx, |state, _| {
                            let next = if state.marker_color_filter() == Some(color) {
                                None
                            } else {
                                Some(color)
                            };
                            state.set_marker_color_filter(next);
                        });
                        cx.notify();
                    }),
                )
        }))
        .into_any_element()
}

/// How many markers the sequence holds, and whether the rows account for all of
/// them.
fn count_line(theme: &Theme, shown: usize, held: usize) -> gpui::AnyElement {
    let text = if shown == held {
        counted(held, "marker", "markers").to_string()
    } else {
        format!("{shown} of {held} markers shown")
    };
    div()
        .text_size(Theme::TEXT_UI)
        .text_color(if shown == held {
            theme.text_muted
        } else {
            theme.text
        })
        .child(SharedString::from(text))
        .into_any_element()
}

fn add_button(theme: &Theme, cx: &mut gpui::Context<crate::shell::Shell>) -> gpui::AnyElement {
    components::ghost_button("marker-add", "Add at playhead", theme)
        .debug_selector(|| "marker-add".into())
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                this.state
                    .update(cx, |state, _| press(state, Pick::AddAtPlayhead));
                cx.notify();
            }),
        )
        .into_any_element()
}

/// Press one row of the registry, reporting a refusal in the status line rather
/// than swallowing it.
fn press(state: &mut AppState, pick: Pick) {
    if let Err(err) = markers_pick::apply(state, pick) {
        state.report_error(&err);
    }
}

/// One marker. The whole row seeks; the controls stop propagation so a click on
/// Remove is not also a seek.
fn row(
    theme: &Theme,
    marker: &UiMarker,
    index: usize,
    playhead: i64,
    rate_num: i32,
    rate_den: i32,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let id = marker.id;
    let tick = marker.tick;
    let at_playhead = tick == playhead;
    let key = motion::MotionKey::new("markers.row", id);
    let name = if marker.name.is_empty() {
        "Unnamed".into()
    } else {
        marker.name.clone()
    };
    let comment = marker.comment.clone();

    let card = div()
        .id(gpui::ElementId::named_usize("marker-row", id as usize))
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XXS))
        .px(px(Theme::SPACE_SM))
        .py(px(Theme::SPACE_XS))
        .rounded(px(Theme::CONTROL_RADIUS))
        .bg(motion::hover_blend(
            key,
            if at_playhead {
                theme.wash(0.11)
            } else {
                crate::components::band_fill(theme, index)
            },
            theme.wash(if at_playhead { 0.17 } else { 0.06 }),
        ))
        .cursor(CursorStyle::PointingHand)
        .active(|style| style.opacity(0.92))
        .on_hover(motion::hover_listener(key))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                this.state.update(cx, |state, _| state.set_playhead(tick));
                crate::app::spawn_preview(this.state.clone(), cx);
                cx.notify();
            }),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(Theme::SPACE_XS))
                .child(swatch(theme, marker, cx))
                .child(
                    div()
                        .text_size(Theme::TEXT_UI)
                        .text_color(theme.text_muted)
                        .child(SharedString::from(timecode::format_ticks(
                            tick, rate_num, rate_den,
                        ))),
                )
                .when(marker.duration_ticks > 0, |el| {
                    let dur = timecode::format_ticks(marker.duration_ticks, rate_num, rate_den);
                    el.child(
                        div()
                            .text_size(Theme::TEXT_UI)
                            .text_color(theme.text_muted)
                            .child(SharedString::from(format!("+{dur}"))),
                    )
                })
                .child(
                    div()
                        .flex_1()
                        .text_size(Theme::TEXT_UI)
                        .text_color(theme.text)
                        .child(name),
                )
                .when(marker.kind != aria_models::MarkerKind::Comment, |el| {
                    el.child(
                        div()
                            .text_size(Theme::TEXT_SMALL)
                            .text_color(theme.text_muted)
                            .child(marker.kind.label()),
                    )
                })
                .when(marker.clip_id.is_some(), |el| {
                    el.child(
                        div()
                            .text_size(Theme::TEXT_SMALL)
                            .text_color(theme.text_faint)
                            .child("Clip"),
                    )
                }),
        )
        .when(!comment.is_empty(), |el| {
            el.child(
                div()
                    .text_size(Theme::TEXT_UI)
                    .text_color(theme.text_faint)
                    .child(comment),
            )
        })
        .child(
            div()
                .flex()
                .gap(px(Theme::SPACE_XS))
                .child(edit_dialog_action(theme, id, cx))
                .child(marker_action(
                    theme,
                    "marker-rename",
                    "Rename",
                    id,
                    RenameTarget::Marker(id),
                    cx,
                ))
                .child(marker_action(
                    theme,
                    "marker-note",
                    "Note",
                    id,
                    RenameTarget::MarkerComment(id),
                    cx,
                ))
                .child(remove(theme, id, cx)),
        );
    // The rows arrive in their own order rather than all at once: a list that
    // blinks into place says nothing about how long it is.
    crate::pending::arriving(crate::motion::MotionKey::new("marker-in", id), index, card)
}

/// The colour chip. Clicking it advances the palette, which is the only edit a
/// marker's colour has.
fn swatch(
    theme: &Theme,
    marker: &UiMarker,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let id = marker.id;
    div()
        .id(gpui::ElementId::named_usize("marker-color", id as usize))
        .w(px(10.))
        .h(px(10.))
        .rounded(px(Theme::CONTROL_RADIUS))
        .bg(theme.marker(marker.color))
        .cursor(CursorStyle::PointingHand)
        .active(|style| style.opacity(0.92))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                this.state
                    .update(cx, |state, _| press(state, Pick::CycleColour(id)));
                cx.notify();
            }),
        )
        .into_any_element()
}

fn remove(theme: &Theme, id: u64, cx: &mut gpui::Context<crate::shell::Shell>) -> gpui::AnyElement {
    components::ghost_button(
        crate::motion::MotionKey::new("marker-remove", id),
        "Remove",
        theme,
    )
    .on_mouse_down(
        MouseButton::Left,
        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
            cx.stop_propagation();
            this.state
                .update(cx, |state, _| press(state, Pick::Remove(id)));
            cx.notify();
        }),
    )
    .into_any_element()
}

/// A row control that opens a draft on this marker.
fn marker_action(
    theme: &Theme,
    id_prefix: &'static str,
    label: &'static str,
    marker: u64,
    target: RenameTarget,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    components::ghost_button(
        crate::motion::MotionKey::new(id_prefix, marker),
        label,
        theme,
    )
    .on_mouse_down(
        MouseButton::Left,
        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
            cx.stop_propagation();
            this.state.update(cx, |state, _| state.begin_rename(target));
            cx.notify();
        }),
    )
    .into_any_element()
}

fn edit_dialog_action(
    theme: &Theme,
    marker: u64,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    components::ghost_button(
        crate::motion::MotionKey::new("marker-edit", marker),
        "Edit…",
        theme,
    )
    .on_mouse_down(
        MouseButton::Left,
        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
            cx.stop_propagation();
            this.state.update(cx, |state, _| {
                state.open_marker_edit(marker);
            });
            cx.notify();
        }),
    )
    .into_any_element()
}
