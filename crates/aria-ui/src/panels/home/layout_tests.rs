//! Home's rows of cards form one set of columns.
//!
//! The class closed here: two card rows on one pane drawn at different widths.
//! Recent cards 280 px wide under 320 px Start tiles ended 40 px short of the
//! tile above, and each later column started 40 px further left of its tile.
//! Every recent card takes the left edge and the width of the
//! Start tile in its column, at the window size the editor opens at and at the
//! smallest window. Only the Recents rail stacks two rows of cards.
//!
//! The second class: a pane that lays its tiles out in a column. Samples drew
//! Use Template under Sample Project; every pane's tiles now share one row.
//!
//! Not covered: the inset of the text inside a card, which no selector reaches.

use gpui::TestAppContext;

use super::scroll_tests::{bounds, draw, home_with_recents};
use crate::settings::{MIN_WINDOW_HEIGHT, MIN_WINDOW_WIDTH, UiSettings};

/// Layout is in floats; an edge two boxes share lands within this.
const EPS: f32 = 0.51;

#[gpui::test]
fn every_recent_card_sits_in_the_column_of_the_start_tile_above_it(cx: &mut TestAppContext) {
    let opening = UiSettings::default().resolve_window_placement(&[[0.0, 0.0, 1920.0, 1080.0]]);
    for (tag, window) in [
        ("home-columns-open", (opening.width, opening.height)),
        ("home-columns-min", (MIN_WINDOW_WIDTH, MIN_WINDOW_HEIGHT)),
    ] {
        let (_state, mut visual) = home_with_recents(cx, tag, window, 3);
        let visual = &mut visual;
        draw(visual);
        for index in 0..3 {
            let tile = bounds(visual, format!("hero-tile-{index}").leak());
            let card = bounds(visual, format!("recent-card-{index}").leak());
            let (tile_x, card_x) = (f32::from(tile.left()), f32::from(card.left()));
            let (tile_w, card_w) = (f32::from(tile.size.width), f32::from(card.size.width));
            assert!(
                (tile_x - card_x).abs() <= EPS && (tile_w - card_w).abs() <= EPS,
                "{tag}: recent card {index} spans x {card_x}..{} and Start tile {index} \
                 spans x {tile_x}..{}; a column of Home cards shares one left edge and one \
                 width",
                card_x + card_w,
                tile_x + tile_w
            );
        }
    }
}

/// WHY: the Samples pane stacked Use Template under Sample Project, where the
/// Recents pane draws its Start tiles across the width. Closes "a pane lays its
/// tiles out in a column" on every pane in `HomeRail::ALL`: at the window the
/// editor opens at, each tile shares the top of the tile before it and starts
/// right of it. Does not cover a window too narrow for the row, where the
/// tiles wrap onto a second line by design.
#[gpui::test]
fn every_pane_draws_its_tiles_in_one_row(cx: &mut TestAppContext) {
    let opening = UiSettings::default().resolve_window_placement(&[[0.0, 0.0, 1920.0, 1080.0]]);
    let window = (opening.width, opening.height);
    let (state, mut visual) = home_with_recents(cx, "home-tile-row", window, 0);
    let visual = &mut visual;
    let mut rows = 0;
    for rail in crate::types::HomeRail::ALL {
        visual.update(|_, app| {
            state.update(app, |state, cx| {
                state.set_home_rail(rail);
                cx.notify();
            })
        });
        draw(visual);
        let tiles: Vec<_> = (0..)
            .map_while(|index| visual.debug_bounds(format!("hero-tile-{index}").leak()))
            .collect();
        for (index, pair) in tiles.windows(2).enumerate() {
            let (before, after) = (pair[0], pair[1]);
            let (top, next_top) = (f32::from(before.top()), f32::from(after.top()));
            assert!(
                (top - next_top).abs() <= EPS && after.left() > before.right(),
                "{rail:?}: tile {} starts at x {}, y {next_top}; tile {index} spans x {}..{} \
                 at y {top}, and a pane's tiles stand in one row",
                index + 1,
                f32::from(after.left()),
                f32::from(before.left()),
                f32::from(before.right())
            );
        }
        if tiles.len() > 1 {
            rows += 1;
        }
    }
    assert!(
        rows >= 2,
        "only {rows} panes drew more than one tile; Recents and Samples each draw a row"
    );
}
