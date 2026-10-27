//! A continuous filmstrip covers the part of its clip on screen end to end,
//! with every picture drawn at its own width.
//!
//! Closes the class "a strip leaves part of a clip empty with every cell
//! rendered": a grid step wider than a cell, a clip that opens off the grid,
//! a frame zoomed wider than a cell, and a neighbor drawn into a cell that is
//! not ready. Each is a geometry fault between the grid
//! ([`crate::filmstrips::cell_spans`]), the placement
//! ([`crate::timeline_display::Thumbnails::cells`] through
//! [`crate::panels::timeline::media::strip_cells`]), and the tiling
//! ([`super::film_tiles`]), so the tests drive all three over a sweep of clip
//! starts, lengths, zooms, and views. Also bounds the work to two cells per
//! cell width on screen. Not covered: the paint call, which hands each tile's
//! bounds to `paint_image` and which the test platform does not record.

use super::{Tile, film_tiles};
use crate::panels::timeline::CELL_W;
use crate::panels::timeline::media::strip_cells;
use crate::timeline_display::Thumbnails;

/// The cells a continuous strip shows of a clip `len` ticks long from
/// `start`, drawn `px_per_tick` wide a tick, inside the visible ticks `view`:
/// their tiles left to right, how many cells there are, and the part of the
/// clip on screen in pixels from its head.
fn strip(
    start: i64,
    len: i64,
    px_per_tick: f32,
    view: (i64, i64),
) -> (Vec<Tile>, usize, (f32, f32)) {
    let width = len as f32 * px_per_tick;
    let shown = (
        (view.0 - start) as f32 * px_per_tick,
        (view.1 - start) as f32 * px_per_tick,
    );
    let cells: Vec<_> = strip_cells(
        Thumbnails::Continuous,
        start,
        start + len,
        width,
        px_per_tick,
        view,
    )
    .collect();
    let tiles = cells
        .iter()
        .flat_map(|cell| film_tiles(cell.from, cell.to, width, shown))
        .collect();
    (tiles, cells.len(), (shown.0.max(0.0), shown.1.min(width)))
}

/// Within the rounding of an `f32` at these magnitudes.
fn near(a: f32, b: f32) -> bool {
    (a - b).abs() <= (a.abs().max(b.abs()) * 4.0 * f32::EPSILON).max(1e-3)
}

#[test]
fn a_continuous_strip_covers_the_clip_on_screen_at_every_zoom() {
    let mut checked = 0;
    for start in [0i64, 1, 7, 1_001, 86_399] {
        for len in [1i64, 2, 5, 240, 3_600, 108_000] {
            let mut px_per_tick = 0.01f32;
            while px_per_tick < 400.0 {
                // The whole clip while it is short enough to tile in a test,
                // and a 1600px view a third of the way in.
                let reach = ((1600.0 / px_per_tick).ceil() as i64).max(1);
                let inside = start + len / 3;
                let whole = len as f32 * px_per_tick <= 100_000.0;
                let views = [(start, start + len), (inside, inside + reach)];
                for view in views.into_iter().skip(usize::from(!whole)) {
                    let (tiles, cells, (lo, hi)) = strip(start, len, px_per_tick, view);
                    if hi <= lo {
                        continue;
                    }
                    let at =
                        format!("start {start}, {len} ticks, {px_per_tick}px a tick, {view:?}");
                    let mut edge = lo;
                    for tile in &tiles {
                        assert!(
                            near(tile.from, edge),
                            "a gap or an overlap at {edge}px before {tile:?}: {at}"
                        );
                        assert!(
                            tile.to > tile.from || near(tile.to, tile.from),
                            "a tile ends before it starts, {tile:?}: {at}"
                        );
                        assert!(
                            (tile.image < tile.from || near(tile.image, tile.from))
                                && (tile.to < tile.image + CELL_W
                                    || near(tile.to, tile.image + CELL_W)),
                            "a tile shows past its picture, so the picture is \
                             squeezed or stretched, {tile:?}: {at}"
                        );
                        edge = tile.to;
                    }
                    assert!(
                        near(edge, hi),
                        "the strip stops at {edge}px of {hi}px: {at}"
                    );
                    let across = ((hi - lo) / CELL_W).ceil() as usize;
                    if px_per_tick <= CELL_W {
                        assert!(
                            cells <= 2 * across + 1,
                            "{cells} cells for {}px: {at}",
                            hi - lo
                        );
                    }
                    assert!(
                        tiles.len() <= cells + across + 1,
                        "{} tiles of {cells} cells for {}px: {at}",
                        tiles.len(),
                        hi - lo
                    );
                    checked += 1;
                }
                px_per_tick *= 1.09;
            }
        }
    }
    assert!(checked > 2_000, "the sweep checked {checked} strips");
}

#[test]
fn a_cell_not_yet_rendered_leaves_its_own_part_empty() {
    // 240 ticks over 600px is 24 ticks per cell, a step of 16: 40px cells,
    // each narrower than its 60px picture.
    let cells: Vec<_> = strip_cells(Thumbnails::Continuous, 0, 240, 600.0, 2.5, (0, 240)).collect();
    let missing = 3;
    let hole = (cells[missing].from, cells[missing].to);
    assert_eq!(hole, (120.0, 160.0));
    let drawn: Vec<Tile> = cells
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != missing)
        .flat_map(|(_, cell)| film_tiles(cell.from, cell.to, 600.0, (0.0, 600.0)))
        .collect();
    assert!(
        drawn
            .iter()
            .all(|tile| tile.to <= hole.0 || tile.from >= hole.1),
        "a neighbor drew its picture into the cell that is not ready: {drawn:?}"
    );
    let covered: f32 = drawn.iter().map(|tile| tile.to - tile.from).sum();
    assert_eq!(
        covered, 560.0,
        "every other cell is drawn across its own part"
    );
}

#[test]
fn a_head_or_tail_off_screen_is_not_asked_for() {
    // A 400 tick clip at 4px a tick, seen from tick 100 to 200: neither its
    // head at 0..60px nor its tail at 1540..1600px is on screen.
    for thumbnails in [Thumbnails::Head, Thumbnails::HeadAndTail] {
        assert_eq!(
            strip_cells(thumbnails, 0, 400, 1600.0, 4.0, (100, 200)).count(),
            0,
            "{thumbnails:?}"
        );
    }
    let tail: Vec<_> =
        strip_cells(Thumbnails::HeadAndTail, 0, 400, 1600.0, 4.0, (300, 400)).collect();
    assert_eq!(
        tail.iter().map(|cell| cell.tick).collect::<Vec<_>>(),
        vec![399],
        "only the tail is on screen"
    );
}
