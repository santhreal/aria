//! Contracts for filmstrip cells.
//!
//! The class closed here is "a cell outlives the state it was rendered from":
//! a cached thumbnail drawn after an edit shows a frame the project no longer
//! produces, which is worse than showing nothing. The other half is bounding
//! the work: one render in flight, and a tick that renders nothing is not
//! retried on every frame.
//!
//! Not covered: the render itself, which needs a GPU renderer and a project.

use std::sync::Arc;

use gpui::RenderImage;
use image::{ImageBuffer, Rgba};
use smallvec::SmallVec;

use super::{Cell, FilmstripStore, cell_step, cell_ticks};

fn cell() -> Cell {
    let buffer = ImageBuffer::<Rgba<u8>, _>::from_raw(1, 1, vec![0u8, 0, 0, 255]).expect("buffer");
    Cell {
        image: Arc::new(RenderImage::new(SmallVec::from_const([image::Frame::new(
            buffer,
        )]))),
    }
}

#[test]
fn cells_sit_on_one_grid_so_neighbours_share_them() {
    // 600px of clip over 240 ticks is 24 ticks per 60px cell, rounded to 32.
    let step = cell_step(240, 600.0, 60.0);
    assert_eq!(step, 32, "the step is a power of two near the ask");

    let a = cell_ticks(0, 240, 600.0, 60.0);
    assert!(
        a.iter().all(|tick| tick % step == 0),
        "every cell is on the grid: {a:?}"
    );

    // A clip starting mid-step lands on the same grid, so a cell a neighbour
    // already rendered is reused rather than rendered again.
    let b = cell_ticks(12, 252, 600.0, 60.0);
    assert!(
        b.iter().all(|tick| tick % step == 0),
        "an off-grid start still snaps: {b:?}"
    );
    assert!(
        b.contains(&32) && a.contains(&32),
        "the two clips share the cells they overlap: {a:?} / {b:?}"
    );
}

#[test]
fn the_step_only_changes_at_a_power_of_two_boundary() {
    // 450px over 240 ticks is exactly 32 ticks per cell — the middle of a band,
    // so a zoom nudge either way keeps the cells already rendered.
    let base = cell_step(240, 450.0, 60.0);
    assert_eq!(base, 32);
    for width in [340.0, 400.0, 450.0, 500.0, 630.0] {
        assert_eq!(
            cell_step(240, width, 60.0),
            base,
            "a zoom inside the band must not invalidate the strip at {width}px"
        );
    }
    // Every step is a power of two, and zooming in only ever asks for finer
    // cells — never a grid that shares nothing with the last one.
    let mut previous = i64::MAX;
    for width in [100.0, 200.0, 450.0, 900.0, 1800.0, 6000.0] {
        let step = cell_step(240, width, 60.0);
        assert!(step > 0 && step & (step - 1) == 0, "{step} at {width}px");
        assert!(
            step <= previous,
            "zooming in coarsened the grid at {width}px"
        );
        previous = step;
    }
}

#[test]
fn a_degenerate_span_asks_for_no_work() {
    assert_eq!(cell_step(0, 600.0, 60.0), 0);
    assert!(cell_ticks(0, 0, 600.0, 60.0).is_empty());
    assert!(cell_ticks(100, 50, 600.0, 60.0).is_empty());
    assert!(cell_ticks(0, 240, 0.0, 60.0).is_empty());
    assert_eq!(
        cell_ticks(0, 5, 10.0, 60.0).len(),
        1,
        "a clip narrower than one cell still shows one frame"
    );
}

#[test]
fn an_edit_drops_every_cell_rendered_before_it() {
    let mut store = FilmstripStore::default();
    store.sync_revision(4);
    store.start(24);
    store.finish(4, 24, Some(cell()));
    assert!(store.get(24).is_some());

    store.sync_revision(5);
    assert!(
        store.get(24).is_none(),
        "a frame from before the edit must not be shown after it"
    );
    assert!(store.wants(24), "and it is worth rendering again");
}

#[test]
fn a_picture_edit_invalidates_only_cells_inside_its_tick_spans() {
    let mut store = FilmstripStore::default();
    store.sync_revision(4);
    for tick in [8, 24, 40] {
        store.start(tick);
        store.finish(4, tick, Some(cell()));
    }

    store.sync_revision_selective(5, &[(16, 32)]);

    assert!(
        store.get(8).is_some(),
        "a frame before the edit was discarded"
    );
    assert!(store.get(24).is_none(), "the edited frame remained cached");
    assert!(
        store.get(40).is_some(),
        "a frame after the edit was discarded"
    );
    assert!(store.wants(24), "the edited cell is not scheduled again");
}

#[test]
fn an_audio_only_edit_keeps_every_cell_but_rejects_old_in_flight_work() {
    let mut store = FilmstripStore::default();
    store.sync_revision(7);
    store.start(8);
    store.finish(7, 8, Some(cell()));
    store.start(24);

    store.sync_revision_selective(8, &[]);
    store.finish(7, 24, Some(cell()));

    assert!(store.get(8).is_some(), "an audio edit discarded a picture");
    assert!(
        store.get(24).is_none(),
        "a render started before the edit was accepted afterward"
    );
}

#[test]
fn a_render_that_lands_late_against_an_old_revision_is_dropped() {
    let mut store = FilmstripStore::default();
    store.sync_revision(1);
    store.start(48);
    store.sync_revision(2);
    store.finish(1, 48, Some(cell()));
    assert!(
        store.get(48).is_none(),
        "the frame describes state that is gone"
    );
}

#[test]
fn only_one_cell_renders_at_a_time() {
    let mut store = FilmstripStore::default();
    store.sync_revision(1);
    assert!(store.wants(0));
    store.start(0);
    assert!(
        !store.wants(24),
        "a filmstrip never queues renders ahead of a scrub"
    );
    store.finish(1, 0, Some(cell()));
    assert!(store.wants(24), "the next cell follows the first");
}

#[test]
fn a_tick_that_renders_nothing_is_not_retried() {
    let mut store = FilmstripStore::default();
    store.sync_revision(1);
    store.start(96);
    store.finish(1, 96, None);
    assert!(
        !store.wants(96),
        "a gap in the sequence must not be re-rendered every frame"
    );
    assert!(store.get(96).is_none());
}
