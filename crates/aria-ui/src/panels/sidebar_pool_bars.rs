//! The Media tab's two bars: the filter row above the items and the footer
//! below them.
//!
//! The filter row holds the filter field, the Audio only switch, and Import.
//! The footer holds the count at its left and, at its right, the views, the
//! order, and the buttons that make a bin or a multicam group. Every icon
//! control names itself through its tip.

use super::*;
use crate::components::lit_icon;
use crate::icons;

/// Height of the order button, which its menu opens above.
const SORT_H: f32 = 22.0;

/// The filter row: the field, the Audio only switch, and Import.
pub(super) fn header(
    theme: &Theme,
    state: &Entity<AppState>,
    tab: SidebarTab,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let query = {
        let read = state.read(cx);
        components::text_box(
            "media-query",
            &read.media_query,
            "Search",
            read.media_query.editing(read.media_search_focus),
            theme,
        )
    };
    let audio = tab == SidebarTab::Audio;
    div()
        .id("pool-header")
        .debug_selector(|| "pool-header".into())
        .flex_none()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(Theme::SPACE_XS))
        .child(query.flex_1().on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                this.state
                    .update(cx, |state, _| state.focus_media_search(true));
                cx.notify();
            }),
        ))
        .child(
            lit_icon("pool-audio-only", icons::AUDIO_LINES, audio, theme).on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    this.state.update(cx, |state, _| {
                        state.sidebar_tab = if audio {
                            SidebarTab::Media
                        } else {
                            SidebarTab::Audio
                        };
                    });
                    cx.notify();
                }),
            ),
        )
        .child(import_button(theme, cx))
}

/// Import: File > Import's own pick, so a file chosen here is read the same way
/// and heads File > Import Recent.
fn import_button(theme: &Theme, cx: &mut gpui::Context<crate::shell::Shell>) -> impl IntoElement {
    components::icon_button("media-import", icons::IMPORT, theme)
        .debug_selector(|| "media-import".into())
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                this.state.update(cx, |state, _| state.pick_and_import());
                cx.notify();
            }),
        )
}

/// The footer: one row holding the count at its left and the views, the order,
/// New bin and New multicam at its right. When the panel is narrower than all
/// four view buttons beside the count and the actions, the active view button
/// stands for the set and cycles through [`PoolView::ALL`] on click, so the
/// footer never wraps onto a second line.
pub(super) fn footer(
    theme: &Theme,
    state: &Entity<AppState>,
    count: String,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let (view, all_views) = {
        let read = state.read(cx);
        let width = read
            .dock_tree
            .find(crate::dock::PANEL_PROJECT)
            .and_then(|id| read.dock_frame_rect(id))
            .map(|[_, _, w, _]| w);
        (read.pool_view(), width.is_none_or(|w| w >= 248.0))
    };
    let mut controls: Vec<gpui::AnyElement> = Vec::with_capacity(PoolView::ALL.len() + 3);
    if all_views {
        for target in PoolView::ALL.iter().copied() {
            controls.push(
                view_icon(target, target == view, theme)
                    .size(px(Theme::ICON_HIT_ROW))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                            this.state.update(cx, |state, cx| {
                                state.set_pool_view(target);
                                state.persist_settings_later(cx);
                            });
                            cx.notify();
                        }),
                    )
                    .into_any_element(),
            );
        }
    } else {
        let next_idx = PoolView::ALL
            .iter()
            .position(|&v| v == view)
            .map_or(0, |idx| (idx + 1) % PoolView::ALL.len());
        let next = PoolView::ALL[next_idx];
        controls.push(
            view_icon(view, true, theme)
                .size(px(Theme::ICON_HIT_ROW))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        this.state.update(cx, |state, cx| {
                            state.set_pool_view(next);
                            state.persist_settings_later(cx);
                        });
                        cx.notify();
                    }),
                )
                .into_any_element(),
        );
    }
    controls.push(sort_menu(theme, state, cx).into_any_element());
    controls.extend(super::bins::footer_buttons(theme, state, cx));
    controls.push(super::multicam::create_button(theme, cx));
    div()
        .id("pool-footer")
        .debug_selector(|| "pool-footer".into())
        .flex_none()
        .min_h(px(Theme::ICON_HIT))
        // A bar across the panel's whole width and flush to its bottom, the
        // mirror of the group strip on top: the rule spans the panel and the
        // row stands an equal step under it and over the panel's edge.
        .mx(px(-Theme::SPACE_MD))
        .mb(px(-Theme::SPACE_MD))
        .px(px(Theme::SPACE_MD))
        .py(px(Theme::SPACE_XS))
        .border_t_1()
        .border_color(theme.border)
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(Theme::SPACE_XS))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(Theme::TEXT_UI)
                .text_color(theme.text_muted)
                .child(SharedString::from(count)),
        )
        .child(
            div()
                .id("pool-footer-controls")
                .flex_none()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(Theme::SPACE_XXS))
                .children(controls),
        )
}

/// A view's toggle, lit while `on`. One literal id per arm, so the controls
/// and tooltip censuses read each view's id at its call.
fn view_icon(view: PoolView, on: bool, theme: &Theme) -> gpui::Stateful<gpui::Div> {
    match view {
        PoolView::Tiles => lit_icon("pool-view-tiles", icons::IMAGE, on, theme),
        PoolView::List => lit_icon("pool-view-list", icons::LIST, on, theme),
        PoolView::Icons => lit_icon("pool-view-icons", icons::LAYOUT_GRID, on, theme),
        PoolView::Freeform => lit_icon("pool-view-freeform", icons::MOVE, on, theme),
    }
}

/// The order: a button reading the current order, and the menu of orders
/// above it while it is open.
fn sort_menu(
    theme: &Theme,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let open = crate::pick_list::is_open(state, crate::pick_list::ListId::PoolSort, cx);
    let sort = state.read(cx).media_sort;
    let chip = components::list_chip("pool-sort", sort.label(), open, theme);
    let rows = open.then(|| sort_list(sort, theme, cx));
    crate::pick_list::picker(
        "pool-sort",
        crate::pick_list::ListId::PoolSort,
        chip,
        SORT_H,
        crate::pick_list::Opens::AboveRight,
        rows,
        theme,
        cx,
    )
}

/// The rows of the order menu, the current order selected.
fn sort_list(
    sort: MediaSort,
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Vec<gpui::AnyElement> {
    MediaSort::ALL
        .iter()
        .copied()
        .enumerate()
        .map(|(index, order)| {
            crate::pick_list::row(
                ("pool-sort-row", index),
                order.label(),
                order == sort,
                theme,
                cx,
                move |state, _| state.media_sort = order,
            )
        })
        .collect()
}
