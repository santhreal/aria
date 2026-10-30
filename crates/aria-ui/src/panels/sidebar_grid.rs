//! The sidebar's header and bodies: the group chips and the tab row, the
//! entry lists of the catalog tabs, the Stock form, and the cloud cards.
//!
//! Items called by `sidebar.rs` are `pub(super)`.

use super::*;
use crate::panels::inspector::blocks::widgets::picked;

/// The sidebar's header: the groups as chips, then the open group's tabs as an
/// underlined tab row when it lists more than one and they are not the pool's.
/// The pool's tabs are one list read two ways, so the Media tab's filter row
/// switches between them. The groups stand on one row down to the panel's
/// minimum width. When their labels at the regular padding are wider than the
/// row, the chips take the compact padding, and only when that does not make
/// room either do the groups other than the active one shrink with an
/// ellipsis.
///
/// Both rows start on the panel's inset, the edge the filter field, the items
/// and the footer start on. The tabs take the groups' padding and gap, so a
/// tab's label stands on the edge a group's label stands on at either
/// padding. The tab row stands on the header's bottom border, so the open
/// tab's underline is drawn on the line that ends the header.
pub(super) fn header(
    theme: &Theme,
    tab: SidebarTab,
    width: Option<f32>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let open = tab.group();
    let tabs = open.tabs();
    let listed = tabs.len() > 1 && !tabs.iter().all(|tab| tab.shows_pool());
    // The row is the panel less its inset on each side.
    let narrow = width.is_some_and(|w| w - 2.0 * Theme::SPACE_MD < groups_width(theme, cx));
    let gap = px(if narrow { 0.0 } else { Theme::SPACE_XXS });
    div()
        .px(px(Theme::SPACE_MD))
        .pt(px(Theme::SPACE_XS))
        .when(!listed, |el| el.pb(px(Theme::SPACE_XS)))
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .border_b_1()
        .border_color(theme.border)
        .child(
            div()
                .id("sidebar-groups")
                .debug_selector(|| "sidebar-groups".into())
                .flex()
                .flex_row()
                .items_center()
                .gap(gap)
                .children(
                    SidebarGroup::ALL
                        .iter()
                        .map(|group| group_chip(theme, *group, open, narrow, cx)),
                ),
        )
        .when(listed, |el| {
            el.child(
                div()
                    .debug_selector(|| "sidebar-tabs".into())
                    .h(px(Theme::TAB_ROW_HEIGHT))
                    .flex()
                    .flex_row()
                    .gap(gap)
                    .children(
                        tabs.iter()
                            .map(|target| group_tab(theme, *target, tab, narrow, cx)),
                    ),
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
    .when(narrow, |el| el.px(px(Theme::SPACE_XS)))
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

/// One tab of the open group, as an underlined tab at the group chips'
/// padding. The look belongs to [`components::underline_tab`]; this adds the
/// padding and the click.
fn group_tab(
    theme: &Theme,
    target: SidebarTab,
    current: SidebarTab,
    narrow: bool,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let active = target == current;
    components::underline_tab(
        ("sidebar-tab", target.label()),
        target.label(),
        active,
        theme,
    )
    .when(narrow, |el| el.px(px(Theme::SPACE_XS)))
    .when(active, |el| el.flex_none())
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

/// A list of entries a click places or applies: the line that states what a
/// click does, once, then one row per entry. The rows hang one ghost inset
/// past the column, so each name starts on the column's edge, and the list
/// scrolls in the height the panel has left. Drawn as `name`, with its usage
/// line as `{name}-usage`.
pub(super) fn entry_list(
    theme: &Theme,
    name: &'static str,
    usage: &'static str,
    rows: Vec<gpui::AnyElement>,
) -> gpui::AnyElement {
    div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .child(
            div()
                .debug_selector(move || format!("{name}-usage"))
                .text_size(Theme::TEXT_UI)
                .text_color(theme.text_faint)
                .child(usage),
        )
        .child(crate::edge_fade::scroll_y(
            div()
                .id(motion::ControlId::from(name).element("entry-list"))
                .debug_selector(move || name.into())
                .flex_1()
                .min_h_0()
                .mx(px(-components::GHOST_INSET.0))
                .flex()
                .flex_col()
                .children(rows),
        ))
        .into_any_element()
}

/// The frame of one row of an [`entry_list`], under the row's `key`: at least
/// one list row tall, its contents one ghost inset in, a wash that fades in
/// under the pointer, and a standing wash while `chosen` holds. The caller
/// adds the contents and the listeners.
pub(super) fn entry_frame(
    theme: &Theme,
    key: motion::MotionKey,
    chosen: bool,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(key.element_id("entry"))
        .min_h(px(Theme::ROW_HEIGHT))
        .flex()
        .items_center()
        .gap(px(Theme::SPACE_SM))
        .px(px(components::GHOST_INSET.0))
        .py(px(Theme::SPACE_XXS))
        .rounded(px(Theme::CONTROL_RADIUS))
        .cursor(CursorStyle::PointingHand)
        .active(|style| style.opacity(0.92))
        .bg(motion::hover_blend(
            key,
            theme.wash(if chosen { 0.11 } else { 0.0 }),
            theme.wash(0.06),
        ))
        .text_size(Theme::TEXT_UI)
}

/// One row per generator in `items`; a click places it at the playhead.
pub(super) fn generator_rows(
    theme: &Theme,
    items: &[(&'static str, Generator)],
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Vec<gpui::AnyElement> {
    items
        .iter()
        .cloned()
        .enumerate()
        .map(|(index, (label, generator))| generator_row(theme, label, generator, index, cx))
        .collect()
}

/// One row per entry of a browsable catalog: clicking applies it to the
/// current selection through `apply`, dragging carries a [`CatalogDrag`] the
/// timeline lands. The rows are built from whatever list the caller passes, so
/// the sidebar never keeps a second copy of a model catalog. `apply` returns
/// the engine's verdict, and a refusal is reported rather than swallowed — a
/// row with no selection to act on has to say so.
///
/// `is_applied` reads what the selection already carries from this catalog and
/// always returns false for a catalog that places rather than chooses. A
/// predicate avoids rebuilding a parallel string vector on every repaint.
pub(super) fn catalog_rows(
    theme: &Theme,
    entries: &[crate::catalog_text::Choice],
    kind: CatalogDragKind,
    apply: fn(&mut AppState, &str) -> Result<(), aria_engine::EngineError>,
    is_applied: impl Fn(&str) -> bool,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Vec<gpui::AnyElement> {
    entries
        .iter()
        .enumerate()
        .map(|(index, choice)| {
            catalog_row(
                theme,
                choice,
                kind,
                apply,
                is_applied(choice.id.as_str()),
                index,
                cx,
            )
        })
        .collect()
}

/// One stock kind, as a chip.
fn kind_chip(
    theme: &Theme,
    kind: &StockKindRow,
    current: StockKind,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let (target, id) = (kind.kind, kind.id);
    components::chip(
        ("stock-kind", id),
        kind.label,
        target == current,
        components::ChipSize::Regular,
        theme,
    )
    .debug_selector(move || id.into())
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

/// A cloud feed that fills the tab: Stock's search form over its results, or
/// the SFX catalog with the button that loads it.
///
/// The feed is never fetched from inside a render: a render that started network
/// traffic would refetch on every repaint. Loading is an explicit action.
pub(super) fn cloud_grid(
    theme: &Theme,
    feed: CloudFeed,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let list = if feed.is_search() {
        let route = match state.read(cx).stock_route() {
            crate::cloud::StockRoute::Direct => {
                "Searches go to the providers whose keys are set in Settings."
            }
            crate::cloud::StockRoute::Backend => {
                "Searches go through the Aria service with no account. A Pexels or \
                 Pixabay key in Settings sends them to the provider instead."
            }
        };
        let note = div()
            .max_w_full()
            .text_size(Theme::TEXT_UI)
            .text_color(theme.text_faint)
            .child(route)
            .into_any_element();
        crate::panels::cloud::cloud_feed_list(
            theme,
            feed,
            state,
            "Search free stock media, then click a result to import it.",
            Some(note),
            cx,
        )
    } else {
        let load = components::button("cloud-feed-load", "Load catalog", theme)
            .debug_selector(|| "cloud-feed-load".into())
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    this.state
                        .update(cx, |state, cx| state.fetch_feed(feed, cx));
                    cx.notify();
                }),
            )
            .into_any_element();
        crate::panels::cloud::cloud_feed_list(
            theme,
            feed,
            state,
            "Browse free sound effects, then click one to import it.",
            Some(load),
            cx,
        )
    };
    div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_MD))
        .when(feed.is_search(), |el| {
            el.child(stock_form(theme, state, cx))
        })
        .child(list)
        .into_any_element()
}

/// Stock's search form: the query box with the Search button beside it, one
/// control tall, then the kinds to search for.
fn stock_form(
    theme: &Theme,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let app = state.read(cx);
    let query = app.stock_query.clone();
    let focused = app.stock_search_focus;
    let kind = app.stock_kind;
    let placeholder = STOCK_KINDS
        .iter()
        .find(|row| row.kind == kind)
        .map_or("Search stock media", |row| row.placeholder);
    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_SM))
        .child(
            div()
                .flex()
                .gap(px(Theme::SPACE_XS))
                .child(
                    components::text_box("stock-query", query, placeholder, focused, theme)
                        .flex_1()
                        .debug_selector(|| "stock-query".into())
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                this.state
                                    .update(cx, |state, _| state.focus_stock_search(true));
                                cx.notify();
                            }),
                        ),
                )
                .child(
                    components::button("stock-search", "Search", theme)
                        .flex_none()
                        .debug_selector(|| "stock-search".into())
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                this.state
                                    .update(cx, |state, cx| state.run_stock_search(cx));
                                cx.notify();
                            }),
                        ),
                ),
        )
        .child(
            div().flex().gap(px(Theme::SPACE_XXS)).children(
                STOCK_KINDS
                    .iter()
                    .map(|row| kind_chip(theme, row, kind, cx)),
            ),
        )
}

/// One catalog entry: its name, marked while the selection carries it.
fn catalog_row(
    theme: &Theme,
    choice: &crate::catalog_text::Choice,
    kind: CatalogDragKind,
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
    // Animation is the one catalog whose entry is worth seeing before it is chosen:
    // hovering renders the preset as an engine override, so the frame changes with
    // nothing on the undo stack, and leaving the row restores the clip. That preview
    // and the row's own fade share one hook, because a second `on_hover` on one
    // element is a panic rather than a second handler.
    let previews = kind == CatalogDragKind::Animation;
    let hover_key = key;
    let hover_id = id.clone();
    let click_id = id.clone();
    let row = entry_frame(theme, key, applied)
        .child(
            div()
                .min_w_0()
                .truncate()
                .text_color(text)
                .child(picked(choice.label.clone(), applied)),
        )
        .on_hover(cx.listener(move |this, hovered: &bool, window, cx| {
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
        }))
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

/// One generator: its name. A click places it at the playhead.
fn generator_row(
    theme: &Theme,
    label: &'static str,
    generator: Generator,
    index: usize,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let key = motion::MotionKey::new("sidebar.generator", label);
    let lit = motion::hover_t(key);
    let row = entry_frame(theme, key, false)
        .on_hover(motion::hover_listener(key))
        .child(
            div()
                .min_w_0()
                .truncate()
                .text_color(motion::mix(theme.text_muted, theme.text, lit))
                .child(label),
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
    crate::pending::arriving(motion::MotionKey::new("generator-in", label), index, row)
}
