//! Media: the pool as the panel lists it, and what the pool has on the picked
//! item.
//!
//! Split out of `sidebar.rs` to stay under the file ceiling; the wall of cards
//! it draws lives in `sidebar_media_grid.rs` for the same reason. Each item is
//! `pub(super)` so the parent and its other children can reach it by name.

use aria_core::count::counted;

use super::*;
use crate::app::pool_rows::PoolRows;
use crate::theme::Theme;

#[path = "sidebar_pool_bars.rs"]
mod bars;
#[path = "sidebar_media_grid.rs"]
mod grid;

/// The pool as the panel lists it: the filter row, the items, and the
/// footer with the count, the views and the order, and what the pool knows
/// about the one they picked.
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
    let (shown, total) = (rows.len(), rows.total());
    let view = state.read(cx).pool_view();
    let properties = state.read(cx).media_properties();
    let count = if query.trim().is_empty() {
        counted(total, "item", "items").to_string()
    } else {
        format!("{shown} of {total}")
    };

    div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_SM))
        .child(bars::header(theme, state, tab, cx))
        .child(crate::edge_fade::scroll_y(
            div()
                .id("pool-items")
                .debug_selector(|| "pool-items".into())
                .flex_1()
                .min_h_0()
                // The frame clips its content to its own width, so it stands
                // a tile's inset past the column and pads back in; the tiles
                // hang into that margin and are drawn whole.
                .mx(px(-grid::TILE_INSET))
                .px(px(grid::TILE_INSET))
                .flex()
                .flex_col()
                .gap(px(Theme::SPACE_SM))
                .children(bins::strip(theme, state, cx))
                .children(multicam::multicam_section(theme, state, cx))
                // The source monitor's in/out marks are only worth setting if
                // something consumes them; the button appears exactly when a
                // range exists and lands the subclip in the same pool the rows
                // below come from.
                .children(subclip::subclip_button(theme, state, cx))
                .child(grid::media_grid(theme, state, tab, rows, view, cx))
                .when(!properties.is_empty(), |el| {
                    el.child(properties_block(theme, &properties))
                }),
        ))
        .child(bars::footer(theme, state, count, cx))
        .into_any_element()
}

/// What the pool knows about the selected item, one row per fact.
pub(super) fn properties_block(
    theme: &Theme,
    rows: &[(&'static str, SharedString)],
) -> impl IntoElement {
    div()
        .id("pool-properties")
        .debug_selector(|| "pool-properties".into())
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XXS))
        .pt(px(Theme::SPACE_SM))
        .border_t_1()
        .border_color(theme.border)
        .children(
            rows.iter()
                .map(|(label, value)| crate::components::readout(theme, *label, value.clone())),
        )
}
