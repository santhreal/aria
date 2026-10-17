//! The Effects tab: every effect, transition, generator, clip type and
//! animation preset as one searchable tree.
//!
//! The rows are [`crate::app::effects_browser::tree`]; this file draws them. A
//! click on a folder opens or closes it, a click on an entry picks it, a
//! double-click applies it to the selection, and a drag carries it to a clip,
//! a cut or a lane. The star saves the entry under Favourites.

use crate::tooltip::WithTip as _;
use gpui::{
    AppContext, CursorStyle, Entity, FontWeight, InteractiveElement, IntoElement, MouseButton,
    MouseDownEvent, ParentElement, SharedString, StatefulInteractiveElement, Styled, div, px,
};

use crate::app::effects_browser::{self, Entry, EntryKind, Failed, FolderKey, Group, Node};
use crate::app::{AppState, spawn_preview};
use crate::components;
use crate::fluent::FluentBuilder;
use crate::motion;
use crate::panels::sidebar::DragGhost;
use crate::theme::Theme;
use crate::types::CatalogDrag;

/// Indent per tree level, in pixels.
const INDENT: f32 = 12.;

pub(crate) fn effects_panel(
    theme: &Theme,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let entries = crate::catalog_text::effect_entries();
    let (presets, failures, query, focused, picked, only) = {
        let read = state.read(cx);
        (
            read.effects_preset_entries(),
            read.effects_failures(),
            read.effects_browser.query.clone(),
            read.effects_search_focused(),
            read.effects_browser.picked.clone(),
            read.effects_browser.only,
        )
    };
    let searching = effects_browser::needle(&query).is_some();
    let nodes = {
        let read = state.read(cx);
        effects_browser::tree(
            &[&entries[..], &presets[..]],
            &failures,
            read.effect_favourites(),
            &query,
            &read.effects_browser.open,
            only,
        )
    };
    let reduced = cx.reduce_motion();
    let mut rows = Vec::with_capacity(nodes.len());
    for node in nodes {
        rows.push(match node {
            Node::Folder {
                key,
                label,
                depth,
                open,
                count,
            } => folder_row(
                theme,
                Folder {
                    key,
                    label,
                    depth,
                    open,
                    count,
                },
                searching,
                reduced,
                cx,
            ),
            Node::Entry {
                entry,
                group,
                depth,
                favourite,
            } => {
                let lit = picked
                    .as_ref()
                    .is_some_and(|(kind, id)| *kind == entry.kind && *id == entry.id);
                entry_row(
                    theme,
                    state,
                    entry,
                    Place {
                        group,
                        depth,
                        favourite,
                        picked: lit,
                    },
                    cx,
                )
            }
            Node::Failed { failed } => failed_row(theme, failed),
        });
    }
    let empty = rows.is_empty();

    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .gap(px(Theme::SPACE_SM))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(Theme::SPACE_XS))
                .child(
                    components::text_box(
                        "effects-query",
                        query,
                        "Click, then type to filter",
                        focused,
                        theme,
                    )
                    .flex_1()
                    .min_w_0()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseDownEvent, _, cx| {
                            this.state
                                .update(cx, |state, _| state.focus_effects_search(true));
                            cx.notify();
                        }),
                    ),
                )
                .when(searching, |el| {
                    el.child(
                        components::ghost_button("effects-query-clear", "Clear", theme)
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                    this.state
                                        .update(cx, |state, _| state.clear_effects_query());
                                    cx.notify();
                                }),
                            ),
                    )
                }),
        )
        .when_some(only, |el, group| {
            el.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap(px(Theme::SPACE_XS))
                    .child(
                        div()
                            .text_size(Theme::TEXT_UI)
                            .text_color(theme.text_muted)
                            .child(group.only_label()),
                    )
                    .child(
                        components::ghost_button("effects-filter-clear", "Show All", theme)
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                    this.state
                                        .update(cx, |state, _| state.set_effects_filter(None));
                                    cx.notify();
                                }),
                            ),
                    ),
            )
        })
        .child(
            div()
                .text_size(Theme::TEXT_UI)
                .text_color(theme.text_faint)
                .child(
                    "Double-click applies to the selection. Drag onto a clip, a cut, a lane \
                     or the Effects tab of Effect Controls.",
                ),
        )
        .child(
            div()
                .id("effects-tree")
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .when(empty, |el| {
                    el.child(
                        div()
                            .text_size(Theme::TEXT_UI)
                            .text_color(theme.text_faint)
                            .child("Nothing matches the search."),
                    )
                })
                .children(rows),
        )
        .into_any_element()
}

/// A folder row as [`effects_panel`] draws it.
struct Folder {
    key: FolderKey,
    label: SharedString,
    depth: u8,
    open: bool,
    count: usize,
}

/// Where an entry row sits and how it reads.
#[derive(Clone, Copy)]
struct Place {
    group: Group,
    depth: u8,
    favourite: bool,
    picked: bool,
}

/// The left padding of a row `depth` levels deep.
fn indent(depth: u8) -> f32 {
    Theme::SPACE_XS + INDENT * f32::from(depth)
}

fn folder_row(
    theme: &Theme,
    folder: Folder,
    searching: bool,
    reduced: bool,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let row = motion::MotionKey::new("effects.folder", folder.key.group.label())
        .with(&folder.key.category);
    let lit = motion::hover_t(row);
    let target = folder.key;
    div()
        .id(row.element_id("effects-folder"))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(Theme::SPACE_XS))
        .pl(px(indent(folder.depth)))
        .pr(px(Theme::SPACE_XS))
        .py(px(Theme::SPACE_XXS))
        .rounded(px(Theme::CONTROL_RADIUS))
        .bg(motion::hover_blend(row, theme.wash(0.0), theme.wash(0.06)))
        .on_hover(motion::hover_listener(row))
        // A search holds every folder with a match open, so a click there
        // would change a choice the operator cannot see.
        .when(!searching, |el| {
            el.cursor(CursorStyle::PointingHand).on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    let target = target.clone();
                    this.state
                        .update(cx, |state, _| state.toggle_effects_folder(target));
                    cx.notify();
                }),
            )
        })
        .child(components::chevron(
            row.with("chevron"),
            folder.open,
            motion::mix(theme.text_faint, theme.text_muted, lit),
            reduced,
        ))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(Theme::TEXT_TITLE)
                .font_weight(if folder.depth == 0 {
                    FontWeight::SEMIBOLD
                } else {
                    FontWeight::NORMAL
                })
                .text_color(theme.text)
                .child(folder.label),
        )
        .child(
            div()
                .flex_none()
                .text_size(Theme::TEXT_UI)
                .text_color(theme.text_faint)
                .child(SharedString::from(folder.count.to_string())),
        )
        .into_any_element()
}

fn entry_row(
    theme: &Theme,
    state: &Entity<AppState>,
    entry: &Entry,
    place: Place,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let kind = entry.kind;
    // The group is part of the identity: a favourite is drawn twice, once
    // under Favourites and once in its own folder.
    let row = motion::MotionKey::new("effects.entry", kind.tag())
        .with(&entry.id)
        .with(place.group.label());
    let lit = motion::hover_t(row);
    let text = motion::mix(
        if place.picked {
            theme.text
        } else {
            theme.text_muted
        },
        theme.text,
        lit,
    );
    let rest = theme.wash(if place.picked { 0.11 } else { 0.0 });
    let tip_state = state.clone();
    let tip_entry = entry.id.clone();
    let click_entry = entry.id.clone();
    div()
        .id(row.element_id("effects-entry"))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(Theme::SPACE_XS))
        .pl(px(indent(place.depth)))
        .pr(px(Theme::SPACE_XS))
        .py(px(Theme::SPACE_XXS))
        .rounded(px(Theme::CONTROL_RADIUS))
        .cursor(CursorStyle::PointingHand)
        .bg(motion::hover_blend(row, rest, theme.wash(0.06)))
        .on_hover(motion::hover_listener(row))
        // Resolved when the pointer rests, not per frame: the origin walks the
        // mounted plugins' registrations.
        .with_tip(move |window, cx| {
            let origin = tip_state.read(cx).effects_entry_origin(kind, &tip_entry);
            crate::tooltip::text_builder(origin)(window, cx)
        })
        .on_drag(
            CatalogDrag {
                kind: kind.drag(),
                id: entry.id.clone(),
                label: entry.label.clone(),
            },
            |drag, _offset, _window, cx| {
                let label = drag.label.clone();
                cx.new(|_| DragGhost { label })
            },
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                let double = event.click_count >= 2;
                let picked = click_entry.clone();
                this.state.update(cx, |state, _| {
                    if double {
                        if let Err(err) = state.apply_effects_entry(kind, &picked) {
                            state.report_error(&err);
                        }
                    } else {
                        state.pick_effects_entry(kind, picked);
                    }
                });
                if double {
                    spawn_preview(this.state.clone(), cx);
                }
                cx.notify();
            }),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(Theme::TEXT_TITLE)
                .text_color(text)
                .child(entry.label.clone()),
        )
        .child(star(theme, row, kind, entry, place.favourite, cx))
        .into_any_element()
}

/// The favourite toggle at the end of an entry row: lit when starred.
fn star(
    theme: &Theme,
    row: motion::MotionKey,
    kind: EntryKind,
    entry: &Entry,
    favourite: bool,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let hover = row.with("star");
    let lit = motion::hover_t(hover);
    let colour = if favourite {
        theme.accent
    } else {
        motion::mix(theme.text_faint, theme.text, lit)
    };
    let starred = entry.id.clone();
    div()
        .id(hover.element_id("effects-star"))
        .flex_none()
        .p(px(2.))
        .rounded(px(Theme::CONTROL_RADIUS))
        .cursor(CursorStyle::PointingHand)
        .on_hover(motion::hover_listener(hover))
        .child(
            crate::icons::icon(crate::icons::STAR)
                .size(px(12.))
                .text_color(colour),
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                this.state.update(cx, |state, cx| {
                    state.toggle_effect_favourite(kind, &starred);
                    state.persist_settings_later(cx);
                });
                cx.notify();
            }),
        )
}

/// A plugin that did not mount, or a preset file that did not read: its name,
/// and the reason under it.
fn failed_row(theme: &Theme, failed: &Failed) -> gpui::AnyElement {
    div()
        .flex()
        .flex_col()
        .pl(px(indent(1)))
        .pr(px(Theme::SPACE_XS))
        .py(px(Theme::SPACE_XXS))
        .child(
            div()
                .text_size(Theme::TEXT_TITLE)
                .text_color(theme.danger)
                .child(failed.source.clone()),
        )
        .child(
            div()
                .text_size(Theme::TEXT_UI)
                .text_color(theme.text_muted)
                .child(failed.reason.clone()),
        )
        .into_any_element()
}
