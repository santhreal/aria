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
use crate::tooltip::WithTip as _;
use gpui::deferred;

/// Height of the order button, which its menu opens above.
const SORT_H: f32 = 22.0;

/// The filter row: the field, the Audio only switch, and Import.
pub(super) fn header(
    theme: &Theme,
    state: &Entity<AppState>,
    tab: SidebarTab,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let query = state.read(cx).media_query.clone();
    let focused = state.read(cx).media_search_focus;
    let audio = tab == SidebarTab::Audio;
    div()
        .id("pool-header")
        .debug_selector(|| "pool-header".into())
        .flex_none()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(Theme::SPACE_XS))
        .child(
            components::text_box("media-query", &query, "Search", focused, theme)
                .flex_1()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        this.state
                            .update(cx, |state, _| state.focus_media_search(true));
                        cx.notify();
                    }),
                ),
        )
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
    let (open, sort) = {
        let read = state.read(cx);
        (read.media_sort_list_open, read.media_sort)
    };
    let list = open.then(|| sort_list(sort, theme, cx));
    // The outside press is the wrapper's: a press on the button toggles the
    // menu rather than closing and reopening it.
    div()
        .relative()
        .when(open, |el| {
            el.on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, _, cx| {
                this.state
                    .update(cx, |state, _| state.media_sort_list_open = false);
                cx.notify();
            }))
        })
        .child(
            components::list_chip("pool-sort", sort.label(), open, theme)
                .h(px(SORT_H))
                .debug_selector(|| "pool-sort".into())
                .when_some(crate::tooltip::builder("pool-sort"), |el, tip| {
                    el.with_tip(tip)
                })
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        this.state.update(cx, |state, _| {
                            state.media_sort_list_open = !state.media_sort_list_open;
                        });
                        cx.notify();
                    }),
                ),
        )
        .when_some(list, |el, list| el.child(list))
}

/// The open menu of orders, the current one washed.
fn sort_list(
    sort: MediaSort,
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement + use<> {
    let mut rows: Vec<gpui::AnyElement> = Vec::with_capacity(MediaSort::ALL.len());
    for (index, order) in MediaSort::ALL.iter().copied().enumerate() {
        let selected = order == sort;
        let id = gpui::ElementId::named_usize("pool-sort-row", index);
        let key = motion::MotionKey::new("pool-sort-row", &id);
        let (rest, hot) = if selected {
            (theme.glass_selected_bg(), theme.element_active)
        } else {
            (theme.wash(0.0), theme.element_hover)
        };
        rows.push(
            div()
                .id(id)
                .debug_selector(move || format!("pool-sort-{}", order.label()))
                .px(px(Theme::SPACE_SM))
                .py(px(Theme::SPACE_XXS))
                .rounded(px(Theme::CONTROL_RADIUS))
                .text_size(Theme::TEXT_TITLE)
                .text_color(if selected {
                    theme.text
                } else {
                    theme.text_muted
                })
                .bg(motion::hover_blend(key, rest, hot))
                .cursor(gpui::CursorStyle::PointingHand)
                .active(|style| style.opacity(0.92))
                .on_hover(motion::hover_listener(key))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        this.state.update(cx, |state, _| {
                            state.media_sort = order;
                            state.media_sort_list_open = false;
                        });
                        cx.notify();
                    }),
                )
                .child(order.label())
                .into_any_element(),
        );
    }
    let menu = div()
        .id("pool-sort-list")
        .min_w(px(120.))
        .rounded(px(Theme::PANEL_RADIUS))
        .bg(theme.surface_overlay)
        .border_1()
        .border_color(theme.border)
        .p(px(Theme::SPACE_XS))
        .flex()
        .flex_col()
        // A press on the menu is the menu's, not the items' drawn under it.
        .occlude()
        .children(rows);
    // Deferred so the menu paints over the items above the footer.
    deferred(
        div()
            .absolute()
            .bottom(px(SORT_H + Theme::SPACE_XS))
            .right(px(0.))
            .child(crate::frost::frost(motion::menu_in(
                motion::MotionKey::new("menu", "pool-sort"),
                menu,
            ))),
    )
}
