//! The sidebar's chip rows and its grids: the group and tab chips, the
//! generator tiles, the catalog rows, and the cloud cards.
//!
//! Split out of `sidebar.rs` to stay under the file ceiling. The items the
//! parent draws are `pub(super)`.

use super::*;
use crate::motion::RiseOnHover as _;

/// The sidebar's chip rows: the groups, then the open group's tabs when it
/// lists more than one and they are not the pool's. The pool's tabs are one
/// list read two ways, so the Media tab's filter row switches between them.
/// The groups stand on one row down to the panel's minimum width. When their
/// labels at the regular padding are wider than the row, the chips take the
/// compact padding, and only when that does not make room either do the
/// groups other than the active one shrink with an ellipsis. Each row steps
/// out by its chips' padding, so the first label stands on the panel's text
/// edge.
pub(super) fn filter_chips(
    theme: &Theme,
    tab: SidebarTab,
    width: Option<f32>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let open = tab.group();
    let tabs = open.tabs();
    let listed = tabs.len() > 1 && !tabs.iter().all(|tab| tab.shows_pool());
    // The row is the panel less its right border and its text inset, stepped
    // out by a chip's padding on each side.
    let narrow = width.is_some_and(|w| {
        w - 1.0 - 2.0 * (Theme::SPACE_MD - Theme::SPACE_SM) < groups_width(theme, cx)
    });
    div()
        .px(px(Theme::SPACE_MD))
        .py(px(Theme::SPACE_XS))
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XXS))
        .border_b_1()
        .border_color(theme.border)
        .child(
            div()
                .id("sidebar-groups")
                .debug_selector(|| "sidebar-groups".into())
                .mx(px(-Theme::SPACE_SM))
                .flex()
                .flex_row()
                .items_center()
                .gap(px(if narrow { 0.0 } else { Theme::SPACE_XXS }))
                .children(
                    SidebarGroup::ALL.iter().enumerate().map(|(index, group)| {
                        group_chip(theme, *group, open, index == 0, narrow, cx)
                    }),
                ),
        )
        .when(listed, |el| {
            el.child(
                div()
                    .min_h(px(Theme::ICON_HIT))
                    .ml(px(-Theme::SPACE_XS))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(Theme::SPACE_XXS))
                    .children(tabs.iter().map(|target| tab_chip(theme, *target, tab, cx))),
            )
        })
}

/// The group row's width at the regular chip padding: every group's label in
/// the face its chip draws, the padding on both sides of each, and the gaps
/// between them. Each label's advance is a hit in the text system's cache
/// after the first frame.
fn groups_width(theme: &Theme, cx: &gpui::App) -> f32 {
    let system = cx.text_system();
    let font = system.resolve_font(&theme.ui_font(gpui::FontWeight::NORMAL));
    let labels: f32 = SidebarGroup::ALL
        .iter()
        .map(|group| {
            f32::from(system.advance_for_text_with_font_id(group.label(), font, Theme::TEXT_UI))
        })
        .sum();
    let count = SidebarGroup::ALL.len() as f32;
    labels + count * 2.0 * Theme::SPACE_SM + (count - 1.0) * Theme::SPACE_XXS
}

/// One group, as a chip. Choosing another group opens its first tab; the open
/// group's chip keeps the tab in front.
fn group_chip(
    theme: &Theme,
    group: SidebarGroup,
    open: SidebarGroup,
    first: bool,
    narrow: bool,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let active = group == open;
    components::chip(
        motion::MotionKey::new("sidebar-group", group.label()),
        group.label(),
        active,
        components::ChipSize::Regular,
        theme,
    )
    .when(narrow, |el| {
        el.px(px(Theme::SPACE_XS))
            .when(first, |el| el.pl(px(Theme::SPACE_SM)))
    })
    .when(active, |el| el.flex_none())
    .when(!active, |el| el.min_w_0())
    .debug_selector(move || format!("sidebar-group-{}", group.label()))
    .on_mouse_down(
        MouseButton::Left,
        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
            this.state.update(cx, |state, _| {
                if state.sidebar_tab.group() != group
                    && let Some(first) = group.tabs().first()
                {
                    state.sidebar_tab = *first;
                }
            });
            cx.notify();
        }),
    )
    .into_any_element()
}

/// One tab of the open group, as a compact chip. The look belongs to
/// [`components::chip`]; this adds the click.
fn tab_chip(
    theme: &Theme,
    target: SidebarTab,
    current: SidebarTab,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let active = target == current;
    components::chip(
        target.label(),
        target.label(),
        active,
        components::ChipSize::Compact,
        theme,
    )
    .when(active, |el| el.flex_none())
    .when(!active, |el| el.min_w_0())
    .debug_selector(move || format!("sidebar-tab-{}", target.label()))
    .on_mouse_down(
        MouseButton::Left,
        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
            this.state.update(cx, |state, _| state.sidebar_tab = target);
            cx.notify();
        }),
    )
    .into_any_element()
}

pub(super) fn generator_grid(
    theme: &Theme,
    items: &[(&'static str, Generator)],
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    div().flex().flex_col().gap(px(Theme::SPACE_XS)).children(
        items
            .iter()
            .cloned()
            .enumerate()
            .map(|(index, (label, generator))| generator_tile(theme, label, generator, index, cx)),
    )
}

/// A browsable catalog: one row per entry, clicking applies it to the current
/// selection through `apply`, dragging carries a [`CatalogDrag`] the timeline lands.
/// The rows are built from whatever list the caller passes, so the sidebar never
/// keeps a second copy of a model catalog. `apply` returns the engine's verdict, and
/// a refusal is reported rather than swallowed — a row with no selection to act on
/// has to say so.
///
/// `is_applied` reads what the selection already carries from this catalog and
/// always returns false for a catalog that places rather than chooses. A
/// predicate avoids rebuilding a parallel string vector on every repaint.
pub(super) fn catalog_grid(
    theme: &Theme,
    entries: &[crate::catalog_text::Choice],
    kind: CatalogDragKind,
    caption: &'static str,
    apply: fn(&mut AppState, &str) -> Result<(), aria_engine::EngineError>,
    is_applied: impl Fn(&str) -> bool,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .children(entries.iter().enumerate().map(|(index, choice)| {
            catalog_row(
                theme,
                choice,
                kind,
                caption,
                apply,
                is_applied(choice.id.as_str()),
                index,
                cx,
            )
        }))
}

pub(super) fn kind_chip(
    theme: &Theme,
    id: &'static str,
    label: &'static str,
    target: StockKind,
    current: StockKind,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    components::chip(
        id,
        label,
        target == current,
        components::ChipSize::Regular,
        theme,
    )
    .on_mouse_down(
        MouseButton::Left,
        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
            this.state
                .update(cx, |state, _| state.set_stock_kind(target));
            cx.notify();
        }),
    )
    .into_any_element()
}

/// A cloud feed: the search box or a Refresh button, then one row per card.
///
/// The feed is never fetched from inside a render: a render that started network
/// traffic would refetch on every repaint. Loading is an explicit action.
pub(super) fn cloud_grid(
    theme: &Theme,
    feed: CloudFeed,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let load = state.read(cx).feed(feed);
    let route = state.read(cx).stock_route();
    let query = state.read(cx).stock_query.clone();
    let focused = state.read(cx).stock_search_focus;
    let kind = state.read(cx).stock_kind;

    let control = if feed.is_search() {
        let caption = match route {
            crate::cloud::StockRoute::Direct => "Your provider keys",
            crate::cloud::StockRoute::Backend => "Anonymous search",
        };
        let box_border = if focused { theme.accent } else { theme.border };
        div()
            .flex()
            .flex_col()
            .gap(px(Theme::SPACE_XS))
            .child(
                div()
                    .text_size(Theme::TEXT_UI)
                    .text_color(theme.text_muted)
                    .child(caption),
            )
            .child(
                div()
                    .id("stock-query")
                    .px(px(Theme::SPACE_SM))
                    .py(px(Theme::SPACE_XS))
                    .rounded(px(Theme::CONTROL_RADIUS))
                    .bg(theme.surface_raised)
                    .border_1()
                    .border_color(box_border)
                    .cursor(CursorStyle::IBeam)
                    .text_size(Theme::TEXT_TITLE)
                    .text_color(if query.is_empty() {
                        theme.text_muted
                    } else {
                        theme.text
                    })
                    .child(SharedString::from(if query.is_empty() {
                        "Click, then type a query".to_string()
                    } else if focused {
                        format!("{query}|")
                    } else {
                        query
                    }))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseDownEvent, _, cx| {
                            this.state
                                .update(cx, |state, _| state.focus_stock_search(true));
                            cx.notify();
                        }),
                    ),
            )
            .child(div().flex().gap(px(Theme::SPACE_XS)).children(
                STOCK_KINDS.iter().map(|(id, label, this_kind)| {
                    kind_chip(theme, *id, *label, *this_kind, kind, cx)
                }),
            ))
            .child(
                div()
                    .text_size(Theme::TEXT_UI)
                    .text_color(theme.text_muted)
                    .child(SharedString::from(if focused {
                        "Enter searches, Esc leaves the box".to_string()
                    } else {
                        String::new()
                    })),
            )
            .child(
                components::ghost_button("stock-search", "Search", theme).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        this.state
                            .update(cx, |state, cx| state.run_stock_search(cx));
                        cx.notify();
                    }),
                ),
            )
            .into_any_element()
    } else {
        return crate::panels::cloud::cloud_catalog_block(theme, feed, state, cx)
            .into_any_element();
    };

    let rows: Vec<_> = load
        .cards()
        .iter()
        .cloned()
        .map(|card| crate::panels::cloud::cloud_card_row(theme, feed, card, cx))
        .collect();

    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_SM))
        .child(control)
        .when_some(load.placeholder(), |el, line| {
            el.child(
                div()
                    .text_size(Theme::TEXT_UI)
                    .text_color(theme.text_muted)
                    .child(SharedString::from(line)),
            )
        })
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(Theme::SPACE_XS))
                .children(rows),
        )
        .into_any_element()
}

#[allow(clippy::too_many_arguments)]
pub(super) fn catalog_row(
    theme: &Theme,
    choice: &crate::catalog_text::Choice,
    kind: CatalogDragKind,
    caption: &'static str,
    apply: fn(&mut AppState, &str) -> Result<(), aria_engine::EngineError>,
    applied: bool,
    index: usize,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    // The row's id and label are shared text, so the drag payload, the hover
    // preview, and the click each hold a refcount rather than a copy.
    let id = choice.id.clone();
    let key = motion::MotionKey::new("sidebar.catalog", &id);
    let lit = motion::hover_t(key);
    // The applied entry reads as chosen without a hover: full-strength text and a
    // standing wash, so the list says what the clip carries.
    let text = motion::mix(
        if applied {
            theme.text
        } else {
            theme.text_muted
        },
        theme.text,
        lit,
    );
    let muted = motion::mix(theme.text_faint, theme.text_muted, lit);
    let rest = theme.wash(if applied { 0.11 } else { 0.0 });
    // Animation is the one catalog whose entry is worth seeing before it is chosen:
    // hovering renders the preset as an engine override, so the frame changes with
    // nothing on the undo stack, and leaving the row restores the clip. That preview
    // and the row's own fade share one hook, because a second `on_hover` on one
    // element is a panic rather than a second handler.
    let previews = kind == CatalogDragKind::Animation;
    let hover_key = key;
    let hover_id = id.clone();
    let click_id = id.clone();
    let row = div()
        .id(id.clone())
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .px(px(Theme::SPACE_SM))
        .py(px(Theme::SPACE_XS))
        .rounded(px(Theme::CONTROL_RADIUS))
        .cursor(CursorStyle::PointingHand)
        .bg(motion::hover_blend(key, rest, theme.wash(0.06)))
        .child(
            div()
                .text_size(Theme::TEXT_UI)
                .text_color(text)
                .child(if applied {
                    gpui::SharedString::from(format!("• {}", choice.label))
                } else {
                    choice.label.clone()
                }),
        )
        .child(
            div()
                .text_size(Theme::TEXT_UI)
                .text_color(muted)
                .child(caption),
        )
        // The row rises as it lights. In a list of forty rows that differ only
        // by their text, the lift is what says which one the click will hit.
        .rise_on_hover(
            2.0,
            lit,
            cx.listener(move |this, hovered: &bool, window, cx| {
                let hovered = *hovered;
                motion::set_hover(hover_key, hovered, cx.motion_policy(), cx.frame_instant());
                window.refresh();
                if !previews {
                    return;
                }
                let changed = this.state.update(cx, |state, _| {
                    if hovered {
                        state.preview_animation(&hover_id)
                    } else {
                        state.clear_animation_preview()
                    }
                });
                if changed {
                    spawn_preview(this.state.clone(), cx);
                    cx.notify();
                }
            }),
        )
        .on_drag(
            CatalogDrag {
                kind,
                id: id.clone(),
                label: choice.label.clone(),
            },
            move |drag, _offset, _window, cx| {
                let label = drag.label.clone();
                cx.new(|_| DragGhost { label })
            },
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                this.state.update(cx, |state, _| {
                    if let Err(err) = apply(state, &click_id) {
                        state.report_error(&err);
                    }
                });
                spawn_preview(this.state.clone(), cx);
                cx.notify();
            }),
        );
    crate::pending::arriving(motion::MotionKey::new("catalog-in", &id), index, row)
}

pub(super) fn generator_tile(
    theme: &Theme,
    label: &'static str,
    generator: Generator,
    index: usize,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let key = motion::MotionKey::new("sidebar.generator", label);
    let lit = motion::hover_t(key);
    let text = motion::mix(theme.text_muted, theme.text, lit);
    let muted = motion::mix(theme.text_faint, theme.text_muted, lit);
    let tile = div()
        .id(label)
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .px(px(Theme::SPACE_SM))
        .py(px(Theme::SPACE_XS))
        .rounded(px(Theme::CONTROL_RADIUS))
        .cursor(CursorStyle::PointingHand)
        .active(|style| style.opacity(0.92))
        .bg(motion::hover_blend(key, theme.wash(0.0), theme.wash(0.06)))
        .rise_on_hover(2.0, lit, motion::hover_listener(key))
        .child(
            div()
                .text_size(Theme::TEXT_UI)
                .text_color(text)
                .child(label),
        )
        .child(
            div()
                .text_size(Theme::TEXT_UI)
                .text_color(muted)
                .child("Click to place at the playhead"),
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                this.state.update(cx, |state, _| {
                    if let Err(err) = state.add_generated(generator.clone()) {
                        state.report_error(&err);
                    }
                });
                spawn_preview(this.state.clone(), cx);
                cx.notify();
            }),
        );
    crate::pending::arriving(motion::MotionKey::new("generator-in", label), index, tile)
}
