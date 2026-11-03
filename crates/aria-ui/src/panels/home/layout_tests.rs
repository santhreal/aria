//! Home's rows of cards form one set of columns.
//!
//! The class closed here: two card rows on one pane drawn at different widths.
//! Recent cards 280 px wide under 320 px Start tiles ended 40 px short of the
//! tile above, and each later column started 40 px further left of its tile.
//! Every recent card takes the left edge and the width of the
//! Start tile in its column, at the window size the editor opens at and at the
//! smallest window. Only the Recents rail stacks two rows of cards.
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
