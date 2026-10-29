//! A filmstrip cell not rendered yet draws a stand-in from its own clip.
//!
//! Closes the class "a strip goes blank while it refills": after a zoom in,
//! the cells of the finer grid between the ones already rendered drew
//! nothing until each rendered, so most of a lane stayed empty for as long as
//! the refill took. Each now draws the picture the strip showed at its left
//! edge before the zoom, and asks for its own. The other half is the bound:
//! a stand-in never comes from outside its clip, so no frame is drawn across
//! the clip's cut, and never from farther than [`stand_in_reach`]. Every
//! draw checks what it asks for: each cell without its exact frame, a
//! snapped one included, and, as empty, each cell it drew no picture in. Not
//! covered: the paint, which the test platform does not record.

use std::sync::Arc;

use super::{FilmCell, cell_pictures, stand_in_reach, strip_cells};
use crate::filmstrips::{Cell, FilmstripStore, test_cell};
use crate::timeline_display::{StripCell, Thumbnails};

/// The cells a continuous strip shows of `clip` at `px_per_tick`, the whole
/// clip on screen.
fn strip(clip: (i64, i64), px_per_tick: f32) -> Vec<StripCell> {
    let width = (clip.1 - clip.0) as f32 * px_per_tick;
    strip_cells(
        Thumbnails::Continuous,
        clip.0,
        clip.1,
        width,
        px_per_tick,
        clip,
    )
    .collect()
}

/// What `clip` draws at `px_per_tick` from `store`: each drawn cell's tick
/// and picture, and the ticks it asks for.
fn draw(
    store: &FilmstripStore,
    clip: (i64, i64),
    px_per_tick: f32,
) -> (Vec<(i64, Arc<gpui::RenderImage>)>, Vec<i64>) {
    let cells = strip(clip, px_per_tick);
    // A cell's left edge is unique in a strip, so a picture is matched back to
    // its cell by where it is drawn.
    let heads: Vec<(f32, i64)> = cells.iter().map(|cell| (cell.from, cell.tick)).collect();
    let (mut missing, mut blank) = (Vec::new(), Vec::new());
    let pictures = cell_pictures(
        store,
        cells.into_iter(),
        clip,
        stand_in_reach(px_per_tick),
        &mut missing,
        &mut blank,
    );
    let drawn: Vec<(i64, Arc<gpui::RenderImage>)> = pictures
        .into_iter()
        .map(|picture: FilmCell| {
            let (_, tick) = heads
                .iter()
                .find(|(from, _)| *from == picture.from)
                .expect("a picture drawn where a cell is");
            (*tick, picture.image)
        })
        .collect();
    let inexact: Vec<i64> = heads
        .iter()
        .map(|(_, tick)| *tick)
        .filter(|tick| store.get(*tick).is_none_or(|cell| cell.snapped))
        .collect();
    assert_eq!(
        missing, inexact,
        "the draw asks for every cell without its exact frame, in draw order"
    );
    let empty: Vec<i64> = heads
        .iter()
        .map(|(_, tick)| *tick)
        .filter(|tick| drawn.iter().all(|(drawn, _)| drawn != tick))
        .collect();
    assert_eq!(
        blank, empty,
        "the draw names every cell it drew empty, and no other, in draw order"
    );
    (drawn, missing)
}

/// A store holding a distinct picture at each of `ticks`.
fn holding(ticks: impl IntoIterator<Item = i64>) -> FilmstripStore {
    let mut store = FilmstripStore::default();
    for tick in ticks {
        store.hold(tick, test_cell());
    }
    store
}

#[test]
fn a_zoom_in_draws_each_new_cell_with_the_picture_shown_there_before() {
    let clip = (0, 480);
    for zoom in [2.0_f32, 4.0] {
        let before = 1.0 / zoom;
        let shown = strip(clip, before);
        let store = holding(shown.iter().map(|cell| cell.tick));
        let (drawn, missing) = draw(&store, clip, 1.0);
        let after = strip(clip, 1.0);
        assert!(
            !missing.is_empty(),
            "{zoom}x: the zoom in asked for no new cells"
        );
        assert_eq!(
            drawn.len(),
            after.len(),
            "{zoom}x: a cell of the zoomed strip drew nothing"
        );
        for (tick, picture) in drawn {
            // The picture the strip drew at this cell's left edge before the
            // zoom: the last cell there that starts at or before it.
            let x = (tick - clip.0) as f32 * before;
            let was = shown
                .iter()
                .rev()
                .find(|cell| cell.from <= x)
                .expect("a cell at the clip's head");
            let was = &store.get(was.tick).expect("held").image;
            assert!(
                Arc::ptr_eq(&picture, was),
                "{zoom}x: the cell at tick {tick} drew a picture the strip did not show there"
            );
        }
    }
}

#[test]
fn a_stand_in_never_comes_from_across_the_clips_cut() {
    let (a, b) = ((0, 240), (240, 480));
    // Only the clip before the cut is rendered.
    let store = holding(strip(a, 1.0).into_iter().map(|cell| cell.tick));
    let (drawn, missing) = draw(&store, b, 1.0);
    assert!(
        drawn.is_empty(),
        "the clip after the cut drew a frame of the clip before it: {:?}",
        drawn.iter().map(|(tick, _)| tick).collect::<Vec<_>>()
    );
    assert_eq!(missing.len(), strip(b, 1.0).len());
    // Only the clip after the cut is rendered.
    let store = holding(strip(b, 1.0).into_iter().map(|cell| cell.tick));
    let (drawn, _) = draw(&store, a, 1.0);
    assert!(
        drawn.is_empty(),
        "the clip before the cut drew a frame of the clip after it: {:?}",
        drawn.iter().map(|(tick, _)| tick).collect::<Vec<_>>()
    );
}

#[test]
fn a_stand_in_comes_from_no_farther_than_the_reach() {
    let clip = (0, 4000);
    let reach = stand_in_reach(1.0);
    assert!(reach > 0 && reach < clip.1);
    let store = holding([0]);
    let (drawn, _) = draw(&store, clip, 1.0);
    let near: Vec<i64> = strip(clip, 1.0)
        .iter()
        .map(|cell| cell.tick)
        .filter(|tick| *tick <= reach)
        .collect();
    assert_eq!(
        drawn.iter().map(|(tick, _)| *tick).collect::<Vec<_>>(),
        near,
        "cells within {reach} ticks of the one rendered draw it, and no others"
    );
    // A clip whose only rendered cell is past its head draws that one there.
    let store = holding([32]);
    let (drawn, _) = draw(&store, clip, 1.0);
    assert_eq!(drawn.first().map(|(tick, _)| *tick), Some(0));
}

/// WHY: a snapped cell shows the sync frame before its tick. Drawn as a
/// missing cell it would draw a neighbor in its place, and left out of the
/// asks it would show the wrong frame for as long as it stays on screen. A
/// snapped cell draws its own picture and is asked for again.
#[test]
fn a_snapped_cell_draws_itself_and_is_asked_for_again() {
    let clip = (0, 480);
    let shown = strip(clip, 1.0);
    let snapped: Vec<i64> = shown.iter().map(|cell| cell.tick).step_by(2).collect();
    let mut store = FilmstripStore::default();
    for cell in &shown {
        let held = Cell {
            snapped: snapped.contains(&cell.tick),
            ..test_cell()
        };
        store.hold(cell.tick, held);
    }
    let (drawn, missing) = draw(&store, clip, 1.0);
    assert_eq!(
        missing, snapped,
        "the draw did not ask for the snapped cells"
    );
    assert_eq!(drawn.len(), shown.len(), "a cell drew nothing");
    for (tick, picture) in drawn {
        let own = &store.get(tick).expect("held").image;
        assert!(
            Arc::ptr_eq(&picture, own),
            "the cell at tick {tick} drew another cell's picture"
        );
    }
}
