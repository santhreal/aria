//! Media: the pool as the operator reads it, and what the pool knows about the
//! item they picked.
//!
//! Split out of `sidebar.rs` to stay under the file ceiling; the wall of cards
//! it draws lives in `sidebar_media_grid.rs` for the same reason. Each item is
//! `pub(super)` so the parent and its other children can reach it by name.

use super::*;
use crate::app::pool_rows::PoolRows;
use crate::theme::Theme;

#[path = "sidebar_media_grid.rs"]
mod grid;

/// The pool as the operator reads it: the filter, the order, the rows, and what
/// the pool knows about the one they picked.
///
/// The list and the count both come from one [`PoolRows`] view, so "3 of 40" is
/// never a number computed beside the rows it describes, and neither costs a
/// pool-sized copy per frame.
pub(super) fn bin(
    theme: &Theme,
    state: &Entity<AppState>,
    tab: SidebarTab,
    rows: &PoolRows,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let query = state.read(cx).media_query.clone();
    let focused = state.read(cx).media_search_focus;
    let (shown, total) = (rows.len(), rows.total());
    let sort = state.read(cx).media_sort;
    let view = state.read(cx).pool_view();
    let properties = state.read(cx).media_properties();

    // Both rows are generated from their registry, so a fifth order or a fourth
    // view is a variant and its answers, not an edit here. They share
    // `components::chip`, which owns the look; only the verb differs.
    let mut sorts = Vec::with_capacity(MediaSort::ALL.len());
    for (index, order) in MediaSort::ALL.iter().enumerate() {
        let order = *order;
        sorts.push(
            components::chip(
                crate::motion::MotionKey::new("sidebar-sort", index),
                order.label(),
                order == sort,
                components::ChipSize::Compact,
                theme,
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    this.state.update(cx, |state, _| state.media_sort = order);
                    cx.notify();
                }),
            )
            .into_any_element(),
        );
    }
    let mut views = Vec::with_capacity(PoolView::ALL.len());
    for (index, target) in PoolView::ALL.iter().enumerate() {
        let target = *target;
        views.push(
            components::chip(
                crate::motion::MotionKey::new("sidebar-view", index),
                target.label(),
                target == view,
                components::ChipSize::Compact,
                theme,
            )
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

    let count = if query.trim().is_empty() {
        format!("{total} item(s)")
    } else {
        format!("{shown} of {total}")
    };

    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_SM))
        .child(
            components::text_box(
                "media-query",
                &query,
                "Click, then type to filter",
                focused,
                theme,
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, _, cx| {
                    this.state
                        .update(cx, |state, _| state.focus_media_search(true));
                    cx.notify();
                }),
            ),
        )
        .child(bins::strip(theme, state, cx))
        .child(multicam::multicam_section(theme, state, cx))
        // The source monitor's in/out marks are only worth setting if something
        // consumes them; the button appears exactly when a range exists and
        // lands the subclip in the same pool the rows below come from.
        .child(subclip::subclip_button(theme, state, cx))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(Theme::SPACE_XS))
                .child(
                    div()
                        .text_size(Theme::TEXT_UI)
                        .text_color(theme.text_muted)
                        .child(SharedString::from(count)),
                )
                .children(sorts)
                .children(views),
        )
        .child(grid::media_grid(theme, state, tab, rows, view, cx))
        .when(!properties.is_empty(), |el| {
            el.child(properties_block(theme, &properties))
        })
        .into_any_element()
}

/// What the pool knows about the selected item, one row per fact.
pub(super) fn properties_block(
    theme: &Theme,
    rows: &[(&'static str, SharedString)],
) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XXS))
        .pt(px(Theme::SPACE_SM))
        .border_t_1()
        .border_color(theme.border)
        .children(rows.iter().map(|(label, value)| {
            div()
                .flex()
                .justify_between()
                .gap(px(Theme::SPACE_SM))
                .text_size(Theme::TEXT_UI)
                .child(
                    div()
                        .text_color(theme.text_faint)
                        .child(SharedString::from(*label)),
                )
                .child(div().text_color(theme.text_muted).child(value.clone()))
        }))
}
