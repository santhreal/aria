//! How a filmstrip fills, and what its cells hold while it does.
//!
//! Closes three classes. A strip that fills one cell at a time and waits a
//! frame between cells: the store renders up to [`RENDERERS`] cells at once,
//! each on a renderer of its own, and starts the next queued cell as each
//! lands, one at a time during playback. A full store that dropped every
//! cell to make room for one, which blanked every strip on screen: it drops
//! the cell farthest from the one landing. And a dropped cell that stayed in
//! the window atlas for the life of the window: every path that takes a cell
//! out of the store hands its picture to the atlas release, and the window
//! no longer holds it once the release ran.
//!
//! Not covered: the order the renders finish in, which the threads decide.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use aria_commands::{Command, EditCommand};
use aria_models::{ClipId, ClipParam, ParamValue};
use aria_plugin::Reverted;
use gpui::{AppContext as _, RenderImage, TestAppContext};

use super::{FilmstripStore, MAX_CELLS, RENDERERS, ask, land, test_cell as cell};
use crate::test_support::drawn_shell;

/// Ticks `n` cells apart on an 8-tick grid.
fn tick(n: usize) -> i64 {
    i64::try_from(n * 8).expect("a small tick")
}

#[test]
fn no_more_cells_render_at_once_than_there_are_renderers() {
    let mut store = FilmstripStore::default();
    store.sync_revision(1);
    store.queue = (0..RENDERERS + 2).map(tick).collect();
    let mut started = Vec::new();
    while let Some(flight) = store.next_flight(RENDERERS) {
        started.push(flight);
    }
    assert_eq!(started.len(), RENDERERS, "cells in flight at once");
    for (index, (_, renderer)) in started.iter().enumerate() {
        assert!(
            started[index + 1..]
                .iter()
                .all(|(_, other)| !Arc::ptr_eq(renderer, other)),
            "two cells in flight on one renderer"
        );
    }
    store.finish(1, started[1].0, Some(cell()));
    let (next, renderer) = store
        .next_flight(RENDERERS)
        .expect("the freed renderer takes the next queued cell");
    assert_eq!(next, tick(RENDERERS), "the queue's order");
    assert!(
        Arc::ptr_eq(&renderer, &started[1].1),
        "the next cell went to a renderer that was busy"
    );
}

/// A state with a sequence to render cells of, outside any window.
fn sequence(cx: &TestAppContext) -> gpui::Entity<crate::app::AppState> {
    cx.update(|app| app.new(|_| crate::app::premiere_tests::with_sequence(1, 10)))
}

fn in_flight(cx: &TestAppContext, state: &gpui::Entity<crate::app::AppState>) -> Vec<i64> {
    cx.update(|app| {
        let mut ticks: Vec<i64> = state
            .read(app)
            .filmstrips
            .flights
            .iter()
            .map(|flight| flight.tick)
            .collect();
        ticks.sort_unstable();
        ticks
    })
}

#[test]
fn playback_renders_one_cell_at_a_time() {
    let cx = TestAppContext::single();
    let state = sequence(&cx);
    let asked: Vec<i64> = (0..RENDERERS + 2).map(tick).collect();
    cx.update(|app| state.update(app, |state, _| state.playing = true));
    cx.update(|app| ask(&state, app, asked.clone()));
    assert_eq!(
        in_flight(&cx, &state),
        [0],
        "a filmstrip took a second renderer during playback"
    );
    cx.update(|app| state.update(app, |state, _| state.playing = false));
    cx.update(|app| ask(&state, app, asked.clone()));
    assert_eq!(
        in_flight(&cx, &state),
        asked[..RENDERERS],
        "a paused filmstrip left renderers idle"
    );
    cx.run_until_parked();
}

#[test]
fn a_landing_starts_the_next_queued_cell_without_a_draw() {
    let cx = TestAppContext::single();
    let state = sequence(&cx);
    let asked: Vec<i64> = (0..=RENDERERS).map(tick).collect();
    cx.update(|app| ask(&state, app, asked.clone()));
    assert_eq!(in_flight(&cx, &state), asked[..RENDERERS]);
    cx.update(|app| {
        state.update(app, |state, cx| {
            let revision = state.revision;
            land(state, revision, asked[0], Some(cell()), cx);
        })
    });
    assert_eq!(
        in_flight(&cx, &state),
        asked[1..],
        "the landing left the next queued cell waiting for a draw"
    );
    cx.run_until_parked();
}

#[test]
fn a_full_store_drops_the_cell_farthest_from_the_one_landing() {
    let mut store = FilmstripStore::default();
    for n in 0..MAX_CELLS {
        store.hold(tick(n), cell());
    }
    // Past the last: the first is farthest.
    store.hold(tick(MAX_CELLS), cell());
    assert_eq!(store.cells.len(), MAX_CELLS);
    assert!(store.get(tick(0)).is_none(), "the farthest cell stayed");
    assert!(
        (1..=MAX_CELLS).all(|n| store.get(tick(n)).is_some()),
        "a full store dropped a cell other than the farthest"
    );
    // Before the first: the last is farthest.
    store.hold(-8, cell());
    assert!(
        store.get(tick(MAX_CELLS)).is_none(),
        "the farthest cell stayed"
    );
    assert!(store.get(tick(1)).is_some());
    // A cell landing again where one is held drops nothing else.
    store.hold(tick(2), cell());
    assert_eq!(store.cells.len(), MAX_CELLS);
    assert!(store.get(tick(MAX_CELLS - 1)).is_some() && store.get(-8).is_some());
}

/// Every picture the store was given is held or was handed to the release,
/// once and not both.
fn accounted(
    store: &mut FilmstripStore,
    given: &[Arc<RenderImage>],
    released: &mut Vec<Arc<RenderImage>>,
    step: &str,
) {
    store.retire_into(released);
    let mut seen: HashMap<*const RenderImage, usize> = HashMap::new();
    for image in store
        .cells
        .values()
        .map(|cell| &cell.image)
        .chain(released.iter())
    {
        *seen.entry(Arc::as_ptr(image)).or_default() += 1;
    }
    for (index, image) in given.iter().enumerate() {
        assert_eq!(
            seen.get(&Arc::as_ptr(image)).copied().unwrap_or(0),
            1,
            "{step}: picture {index} is not held or released exactly once"
        );
    }
}

/// Every path that takes a cell out of the store: a landing in a full store,
/// a landing over a held cell, an edit by span, a rollback by span, and a
/// change the store did not follow.
#[test]
fn every_cell_that_leaves_the_store_goes_to_the_atlas_release() {
    let mut store = FilmstripStore::default();
    store.sync_revision(4);
    let mut given = Vec::new();
    let mut released = Vec::new();
    let mut give = |store: &mut FilmstripStore, at: i64| {
        let cell = cell();
        given.push(Arc::clone(&cell.image));
        store.hold(at, cell);
    };
    for n in 0..=MAX_CELLS {
        give(&mut store, tick(n));
    }
    give(&mut store, tick(5));
    accounted(&mut store, &given, &mut released, "a landing");
    assert_eq!(released.len(), 2, "the evicted cell and the replaced one");

    store.advance(4, 5, vec![(tick(10), tick(20))]);
    accounted(&mut store, &given, &mut released, "an edit");
    store.advance(5, 6, vec![(tick(30), tick(40))]);
    store.revert(Reverted {
        opened: 5,
        found: 6,
        left: 7,
    });
    accounted(&mut store, &given, &mut released, "a rollback");
    assert!(store.get(tick(35)).is_none() && store.get(tick(45)).is_some());

    store.advance(9, 10, vec![(0, 1)]);
    accounted(&mut store, &given, &mut released, "an unfollowed change");
    assert!(store.cells.is_empty());
}

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/texture.png")
}

/// WHY: a dropped cell's picture stayed in the window atlas until the window
/// closed, so every zoom, scroll, and edit added pictures the timeline never
/// drew again. A cell drawn on the timeline and then dropped by an edit is
/// gone from the atlas once the work the edit started has settled.
#[gpui::test]
async fn a_cell_the_store_drops_leaves_the_window_atlas(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, "filmstrip-atlas");
    let clip = visual.update(|window, app| {
        let clip = state.update(app, |state, cx| {
            let media = state.import_media(fixture()).expect("import the fixture");
            state
                .drop_media_on_timeline(media, None, 0)
                .expect("place the clip");
            cx.notify();
            state
                .store
                .sequence
                .tracks
                .iter()
                .flat_map(|track| track.clips.iter())
                .map(|clip| ClipId::from_raw(clip.id))
                .next()
                .expect("the clip is on the timeline")
        });
        window.refresh();
        clip
    });
    visual.run_until_parked();
    let cells: Vec<Arc<RenderImage>> = visual.read(|app| {
        state
            .read(app)
            .filmstrips
            .cells
            .values()
            .map(|cell| Arc::clone(&cell.image))
            .collect()
    });
    let drawn = visual
        .update(|window, _| {
            cells
                .into_iter()
                .find(|image| window.has_image_atlas_entry(image))
        })
        .expect("the timeline drew a rendered cell");

    visual.update(|window, app| {
        state.update(app, |state, cx| {
            state
                .apply_command(Command::Edit(EditCommand::SetParamConstant {
                    clip,
                    param: ClipParam::Opacity,
                    value: ParamValue::Scalar(0.5),
                }))
                .expect("edit the clip's opacity");
            cx.notify();
        });
        window.refresh();
    });
    visual.run_until_parked();
    assert!(
        visual.read(|app| state
            .read(app)
            .filmstrips
            .cells
            .values()
            .all(|cell| !Arc::ptr_eq(&cell.image, &drawn))),
        "the edit kept the cell"
    );
    assert!(
        !visual.update(|window, _| window.has_image_atlas_entry(&drawn)),
        "the dropped cell stayed in the window atlas"
    );
}
