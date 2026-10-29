//! How a filmstrip fills, and what its cells hold while it does.
//!
//! Closes five classes. A strip that fills one cell at a time and waits a
//! frame between cells: a landing starts the next queued cell, and the
//! timeline's draw asks once for all its lane groups, as
//! `panels::timeline::strip_ask_tests` checks. A fill that decodes a frame
//! more than once, because adjacent cells went to different renderers or
//! because each run of cells seeked to a sync frame of its own: one renderer
//! renders the first cell at or after the one it started last, then wraps to
//! the first. A strip with nothing on it that stays empty through the whole
//! exact walk, which on a long-GOP source decodes every frame from each
//! cell's sync frame on: every blank cell snaps to its sync frame first, at
//! one decode each. A full store that dropped every cell to make room for
//! one, which blanked every strip on screen: it drops the cell farthest from
//! the one landing. And a dropped cell that stayed in the window atlas for
//! the life of the window: every path that takes a cell out of the store
//! hands its picture to the atlas release, and the window no longer holds it
//! once the release ran.
//!
//! Not covered: the frames a real decoder decodes, which the fill test
//! models, and the thread a render starts on.

use std::path::PathBuf;
use std::sync::Arc;

use aria_commands::{Command, EditCommand};
use aria_engine::SeekPolicy;
use aria_models::{ClipId, ClipParam, ParamValue};
use aria_plugin::Reverted;
use gpui::{AppContext as _, RenderImage, TestAppContext};

use super::{Cell, FilmstripStore, MAX_CELLS, ask, land, test_cell as cell};
use crate::test_support::atlas::held_or_released_once;
use crate::test_support::drawn_shell;

/// Ticks `n` cells apart on an 8-tick grid.
fn tick(n: usize) -> i64 {
    i64::try_from(n * 8).expect("a small tick")
}

/// The store started at `tick`, rendering it exactly: it is in flight, and
/// nothing else starts until it lands.
fn started(store: &mut FilmstripStore, tick: i64, why: &str) {
    assert_eq!(
        store.next_flight(),
        Some((tick, SeekPolicy::Exact)),
        "{why}"
    );
    assert_eq!(
        store.next_flight(),
        None,
        "two cells in flight on one renderer"
    );
}

#[test]
fn the_next_cell_is_the_first_at_or_after_the_last_one_started() {
    let mut store = FilmstripStore::default();
    store.sync_revision(1);
    store.queue = (0..6).map(tick).collect();
    started(
        &mut store,
        tick(0),
        "a cold renderer starts at the first cell",
    );
    store.finish(1, tick(0), Some(cell()));
    started(
        &mut store,
        tick(1),
        "the renderer left the cell after its own",
    );

    // A zoom out queues cells on both sides of where the decoders stand.
    store.queue = vec![-16, -8, tick(3), tick(4)];
    store.finish(1, tick(1), Some(cell()));
    started(&mut store, tick(3), "the decoders rolled back");
    store.finish(1, tick(3), Some(cell()));
    started(&mut store, tick(4), "the decoders skipped a cell ahead");
    store.finish(1, tick(4), Some(cell()));
    started(&mut store, -16, "the strip did not wrap to its first cell");
    store.finish(1, -16, None);
    started(&mut store, -8, "an empty landing stopped the walk");

    // An edit while a cell renders: the cell lands nothing, and the renderer
    // starts it again, where its decoders stand.
    store.sync_revision(2);
    store.queue = vec![-16, -8, tick(4)];
    store.finish(1, -8, Some(cell()));
    started(
        &mut store,
        -8,
        "the renderer left the cell that landed nothing",
    );
}

/// A state with a sequence to render cells of, outside any window.
fn sequence(cx: &TestAppContext) -> gpui::Entity<crate::app::AppState> {
    cx.update(|app| app.new(|_| crate::app::premiere_tests::with_sequence(1, 10)))
}

fn in_flight(cx: &TestAppContext, state: &gpui::Entity<crate::app::AppState>) -> Option<i64> {
    cx.update(|app| state.read(app).filmstrips.flight.map(|flight| flight.tick))
}

#[test]
fn a_landing_starts_the_next_queued_cell_without_a_draw() {
    let cx = TestAppContext::single();
    let state = sequence(&cx);
    cx.update(|app| ask(&state, app, (0..3).map(tick).collect(), Vec::new()));
    assert_eq!(in_flight(&cx, &state), Some(tick(0)));
    cx.update(|app| {
        state.update(app, |state, cx| {
            let revision = state.revision;
            land(state, revision, tick(0), Some(cell()), cx);
        })
    });
    assert_eq!(
        in_flight(&cx, &state),
        Some(tick(1)),
        "the landing left the next queued cell waiting for a draw"
    );
    cx.run_until_parked();
}

/// The draw asks lane by lane, so a tick comes twice and out of order; the
/// renderer still starts at the first.
#[test]
fn the_order_cells_are_drawn_in_does_not_change_the_walk() {
    let cx = TestAppContext::single();
    let ticks: Vec<i64> = (0..6).map(tick).collect();
    let drawn: Vec<i64> = ticks.iter().rev().chain(&ticks).copied().collect();
    let (in_order, as_drawn) = (sequence(&cx), sequence(&cx));
    cx.update(|app| ask(&in_order, app, ticks.clone(), Vec::new()));
    cx.update(|app| ask(&as_drawn, app, drawn, Vec::new()));
    assert_eq!(in_flight(&cx, &in_order), Some(tick(0)));
    assert_eq!(
        in_flight(&cx, &as_drawn),
        Some(tick(0)),
        "the order the cells were drawn in changed the walk"
    );
    let queue = |state: &gpui::Entity<crate::app::AppState>| {
        cx.update(|app| state.read(app).filmstrips.queue.clone())
    };
    assert_eq!(queue(&as_drawn), queue(&in_order), "a tick queued twice");
    cx.run_until_parked();
}

/// The draw names the cells it drew empty, in any order; the renderer snaps
/// the first of them before it renders the first cell the draw asked for.
#[test]
fn a_draw_with_empty_cells_snaps_the_first_of_them_first() {
    let cx = TestAppContext::single();
    let state = sequence(&cx);
    let blank = vec![tick(3), tick(2), tick(3)];
    cx.update(|app| ask(&state, app, (0..4).map(tick).collect(), blank));
    assert_eq!(
        in_flight(&cx, &state),
        Some(tick(2)),
        "the renderer did not start at the first cell drawn empty"
    );
    assert_eq!(
        cx.update(|app| state.read(app).filmstrips.blanks()),
        2,
        "the draw's empty cells were not each kept once"
    );
    cx.run_until_parked();
}

/// Ticks between sync frames in the modeled source.
const GOP: i64 = 60;

/// Frames a renderer decodes to render the cell at `tick` after the one at
/// `from`: the frames it rolls forward over, or the frames from the sync
/// frame at or before `tick` through `tick`, whichever are fewer.
fn decoded(from: Option<i64>, tick: i64) -> i64 {
    let seek = tick.rem_euclid(GOP) + 1;
    match from {
        Some(from) if from < tick => seek.min(tick - from),
        _ => seek,
    }
}

/// A store filled again and again, and where its renderer's decoders stand.
struct Filling {
    store: FilmstripStore,
    at: Option<i64>,
}

/// The cells of `queued` a fill renders exactly: all but the ones at `blank`
/// on a sync frame, whose snap lands the exact frame.
fn owed(queued: &[i64], blank: &[i64]) -> Vec<i64> {
    queued
        .iter()
        .copied()
        .filter(|tick| !blank.contains(tick) || tick.rem_euclid(GOP) != 0)
        .collect()
}

/// Fill `queued`, ascending cells `filling` holds none of or only a snapped
/// one, through its store, with the ones at `blank` drawn empty, landing each
/// cell before the next starts. The frames decoded rendering exactly.
///
/// A snap decodes the sync frame at or before its tick and leaves the
/// decoders there; a snap whose tick is that sync frame lands its exact frame.
fn filled(filling: &mut Filling, queued: &[i64], blank: &[i64]) -> i64 {
    let Filling { store, at } = filling;
    store.queue = queued.to_vec();
    store.blank = blank.to_vec();
    let (mut snapped, mut rendered) = (Vec::new(), Vec::new());
    let mut exact = 0;
    while let Some((tick, policy)) = store.next_flight() {
        let sync = tick - tick.rem_euclid(GOP);
        match policy {
            SeekPolicy::NearestSync => {
                assert!(
                    rendered.is_empty(),
                    "the blank cell at {tick} snapped after a cell rendered exactly"
                );
                *at = Some(sync);
                snapped.push(tick);
                let landed = Cell {
                    snapped: sync != tick,
                    ..cell()
                };
                store.finish(1, tick, Some(landed));
            }
            SeekPolicy::Exact => {
                exact += decoded(*at, tick);
                *at = Some(tick);
                rendered.push(tick);
                store.finish(1, tick, Some(cell()));
            }
        }
        assert!(
            snapped.len() <= blank.len() && rendered.len() <= queued.len(),
            "a cell rendered twice"
        );
    }
    assert_eq!(
        snapped, blank,
        "the blank cells did not snap, left to right"
    );
    rendered.sort_unstable();
    assert_eq!(
        rendered,
        owed(queued, blank),
        "a cell without its exact frame was never rendered exactly, or one with it was again"
    );
    exact
}

/// The frames a cold renderer decodes walking `queued` left to right.
fn walked(queued: &[i64]) -> i64 {
    let mut from = None;
    queued
        .iter()
        .map(|&tick| {
            let frames = decoded(from, tick);
            from = Some(tick);
            frames
        })
        .sum()
}

/// WHY: the store dealt adjacent cells to four renderers, so each renderer
/// rolled forward over every frame between its cells, and then dealt runs of
/// cells, so each run seeked to a sync frame of its own; a refill after a
/// zoom decoded four times the frames and took four times as long. And a
/// strip with nothing on it stayed empty until each cell's exact frame
/// decoded, every frame from its sync frame on, so a 4K long-GOP strip showed
/// nothing for seconds. A fill snaps every blank cell before it renders any
/// cell exactly, at one decode each, and then decodes no more frames than a
/// cold renderer walking the strip left to right: a clip's first fill, whose
/// cells are all blank, the refill after a zoom out, whose new cells lie on
/// both sides of where the decoders stand, and the refill after a zoom in,
/// whose new cells draw stand-ins and snap none.
///
/// Not covered: a real decoder, modeled here as one that snaps at one decode
/// and renders exactly by rolling forward or seeking to the sync frame,
/// whichever decodes fewer frames.
#[test]
fn a_fill_snaps_blank_cells_then_decodes_no_frame_a_walk_does_not() {
    for (cells, step) in [(64, 8), (48, 8), (32, 8), (24, 16)] {
        let span = cells * step;
        let placed: Vec<i64> = (0..cells).map(|n| n * step).collect();
        // A zoom out by two about the middle: the view doubles and the grid
        // coarsens, so the cells inside the placed ones are held.
        let coarse = usize::try_from(2 * step).expect("a small step");
        let zoomed_out: Vec<i64> = (-span / 2..span + span / 2)
            .step_by(coarse)
            .filter(|&tick| !(0..span).contains(&tick))
            .collect();
        // A zoom in by two: the finer grid's new cells sit between held ones.
        let fine = usize::try_from(step).expect("a small step");
        let zoomed_in: Vec<i64> = (step / 2..span).step_by(fine).collect();
        let mut filling = Filling {
            store: FilmstripStore::default(),
            at: None,
        };
        filling.store.sync_revision(1);
        for (when, queued, blank) in [
            ("placed", placed.clone(), placed),
            ("zoomed out", zoomed_out.clone(), zoomed_out),
            ("zoomed in", zoomed_in, Vec::new()),
        ] {
            let exact = filled(&mut filling, &queued, &blank);
            let walk = walked(&owed(&queued, &blank));
            assert!(
                exact <= walk,
                "{cells} cells {step} ticks apart, {when}: decoded {exact} frames exactly, a walk {walk}"
            );
        }
    }
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
    held_or_released_once(
        store.cells.values().map(|cell| &cell.image),
        released,
        given,
        step,
    );
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
