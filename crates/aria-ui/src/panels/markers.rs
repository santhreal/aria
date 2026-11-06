//! The Markers dock panel: every ruler marker as a row.
//!
//! A row states the time, the label, the color and the note of one marker, so
//! a marker off screen on the ruler is still found here. Every control on a
//! row is an existing marker command: click seeks, the swatch cycles the
//! palette, Edit… opens the marker's dialog, Rename edits the label and Note
//! the comment in a text box on the row, and Remove deletes it. The four
//! commands after the swatch are drawn on the row under the pointer. The
//! color filter above the rows shows the markers of one color.
//!
//! The rows are the projected markers in projection order (by tick), which is
//! the order the ruler draws them in; nothing here is computed beyond what the
//! state holds.

use crate::components::Wraps as _;
use aria_core::count::counted;
use gpui::{
    CursorStyle, Entity, Hsla, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    ParentElement, SharedString, StatefulInteractiveElement, Styled, div, px,
};

use crate::app::AppState;
use crate::components;
use crate::motion;
use crate::surfaces::markers_pick::{self, Pick};
use crate::theme::Theme;
use crate::timecode;
use crate::tooltip::WithTip as _;
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
    // The project's own count, not the row count. They agree unless a color
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

    // The draft of a Rename or Note begun on a row, which that row draws.
    let draft = {
        let app = state.read(cx);
        app.open_draft().filter(|_| app.rename_in_row)
    };
    let mut rows = Vec::with_capacity(markers.len());
    for (index, marker) in markers.iter().enumerate() {
        rows.push(row(
            theme,
            marker,
            index,
            playhead,
            rate,
            draft.as_ref(),
            cx,
        ));
    }

    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        // The count and the panel's one action share the head line, as the
        // Mixer's head row holds its action.
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(Theme::SPACE_SM))
                .child(count_line(theme, rows.len(), held))
                .child(add_button(theme, cx)),
        )
        .child(filter_bar(theme, active_filter, cx))
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
    // The colors are the swatches the Edit Marker dialog picks from, so a color
    // reads the same in the filter as where it is set; the one in force is ringed.
    let swatches = aria_models::MarkerColor::ALL
        .into_iter()
        .enumerate()
        .map(|(index, color)| {
            let id = ("marker-filter-color", index);
            crate::swatch::swatch(id, theme.marker(color), active == Some(color), theme)
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
                .with_tip(crate::tooltip::text_builder(color.label()))
        });
    div()
        .flex()
        .wraps()
        .items_center()
        .gap(px(Theme::SPACE_XS))
        .child(all_chip)
        .children(swatches)
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

/// One marker: one line of what it is, and its note under it. The whole row
/// seeks; the controls stop propagation so a click on Remove is not also a
/// seek. The controls stand over the end of the first line while the pointer
/// is on the row, so a list of markers reads as a list rather than as a grid
/// of buttons, and no row's name is covered while nothing points at it.
///
/// A Rename or Note begun on the row draws its `draft` in a text box in
/// place of the name or under the first line, and the row holds no controls
/// while it does.
fn row(
    theme: &Theme,
    marker: &UiMarker,
    index: usize,
    playhead: i64,
    rate: aria_models::Rational,
    draft: Option<&crate::app::rename::OpenDraft>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let id = marker.id;
    let tick = marker.tick;
    let (rate_num, rate_den) = (rate.num, rate.den);
    let at_playhead = tick == playhead;
    let key = motion::MotionKey::new("markers.row", id);
    let name = if marker.name.is_empty() {
        "Unnamed".into()
    } else {
        marker.name.clone()
    };
    let comment = marker.comment.clone();
    let editing = draft.filter(|(target, ..)| {
        matches!(target, RenameTarget::Marker(m) | RenameTarget::MarkerComment(m) if *m == id)
    });
    let (name_draft, note_draft) = match editing {
        Some((RenameTarget::Marker(_), line, selection)) => (Some((line, *selection)), None),
        Some((_, line, selection)) => (None, Some((line, *selection))),
        None => (None, None),
    };
    let lit = motion::hover_t(key);
    let (fill, plate) = row_fills(theme, at_playhead, index, lit);
    // Drawn only while seen: a control at no opacity would still take a click.
    let actions = (lit > 0.0 && editing.is_none()).then(|| {
        div()
            .absolute()
            .top(px(Theme::SPACE_XS))
            .right(px(Theme::SPACE_SM))
            .h(px(Theme::CONTROL_HEIGHT))
            .flex()
            .items_center()
            .gap(px(Theme::SPACE_XS))
            .pl(px(Theme::SPACE_SM))
            .bg(plate)
            .opacity(lit)
            .debug_selector(move || format!("marker-actions-{id}"))
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
            .child(remove(theme, id, cx))
    });

    let card = div()
        .id(gpui::ElementId::named_usize("marker-row", id as usize))
        .debug_selector(move || format!("marker-row-{id}"))
        .relative()
        .flex()
        .flex_col()
        .px(px(Theme::SPACE_SM))
        .py(px(Theme::SPACE_XS))
        .rounded(px(Theme::CONTROL_RADIUS))
        .bg(fill)
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
                .h(px(Theme::CONTROL_HEIGHT))
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
                .child(match name_draft {
                    Some(draft) => {
                        draft_box(theme, "marker-name-draft", id, draft, "Marker name", cx)
                            .flex_1()
                            .into_any_element()
                    }
                    None => div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(Theme::TEXT_UI)
                        .text_color(theme.text)
                        .child(name)
                        .into_any_element(),
                })
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
                            .text_color(theme.text_muted)
                            .child("Clip"),
                    )
                }),
        )
        .map(|el| match note_draft {
            Some(draft) => el.child(draft_box(theme, "marker-note-draft", id, draft, "Note", cx)),
            None => el.when(!comment.is_empty(), |el| {
                el.child(
                    div()
                        .text_size(Theme::TEXT_UI)
                        .text_color(theme.text_muted)
                        .child(comment),
                )
            }),
        })
        .when_some(actions, |el, actions| el.child(actions));
    // The rows arrive in their own order rather than all at once: a list that
    // blinks into place says nothing about how long it is.
    crate::pending::arriving(crate::motion::MotionKey::new("marker-in", id), index, card)
}

/// The fill of a marker row at hover `lit`, and the plate its controls stand
/// on. The plate is the row's fill over the panel ground, opaque: it hides the
/// end of the name under the controls and reads as the row itself, where the
/// row's translucent fill drawn a second time reads as a darker box.
fn row_fills(theme: &Theme, at_playhead: bool, index: usize, lit: f32) -> (Hsla, Hsla) {
    let rest = if at_playhead {
        theme.wash(0.11)
    } else {
        components::band_fill(theme, index)
    };
    let hover = theme.wash(if at_playhead { 0.17 } else { 0.06 });
    let fill = motion::mix(rest, hover, lit);
    (fill, crate::theme::flatten(fill, theme.bg))
}

/// The color chip. Clicking it advances the palette, which is the only edit a
/// marker's color has.
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
            this.state
                .update(cx, |state, _| state.begin_rename_in_row(target));
            cx.notify();
        }),
    )
    .into_any_element()
}

/// The text box a Rename or Note begun on a row draws its draft in. The keys
/// reach the draft through the rename route, as they reach the rename bar; a
/// press in the box is not a seek, and a press anywhere else cancels the
/// draft, as one off the rename bar does.
fn draft_box(
    theme: &Theme,
    id_prefix: &'static str,
    marker: u64,
    (line, selection): (&String, (usize, usize)),
    placeholder: &'static str,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::Stateful<gpui::Div> {
    let key = motion::MotionKey::new(id_prefix, marker);
    components::text_box(key, line, placeholder, Some(selection), theme)
        .debug_selector(move || format!("{id_prefix}-{marker}"))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, _, cx| {
            this.state.update(cx, |state, _| state.cancel_rename());
            cx.notify();
        }))
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

#[cfg(test)]
#[path = "markers_tests.rs"]
mod tests;
