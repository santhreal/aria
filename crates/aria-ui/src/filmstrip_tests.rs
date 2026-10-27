//! Contracts for filmstrip cells.
//!
//! The class closed here is "a cell outlives the state it was rendered from":
//! a cached thumbnail drawn after an edit shows a frame the project no longer
//! produces, which is worse than showing nothing. The other half is bounding
//! the work: a tick that renders nothing is not retried on every frame.
//!
//! Cells are asked for from the timeline's draw, so asking must never wait on
//! the engine lock. A cell draws nothing on the session's renderer: its
//! thread holds the engine only to take the cell's source, and decodes on one
//! of the store's own renderers.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use aria_models::RationalTime;
use gpui::AppContext as _;

use super::{CELL_H, CELL_MAX_W, FilmstripStore, ask, cell_spans, cell_step, test_cell as cell};

/// Every tick a clip from `start` to `end` draws, as cell heads.
fn heads(start: i64, end: i64, width: f32) -> Vec<i64> {
    cell_spans(start, end, width, 60.0, (start, end))
        .into_iter()
        .map(|(tick, _)| tick)
        .collect()
}

#[test]
fn cells_sit_on_one_grid_so_neighbours_share_them() {
    // 600px of clip over 240 ticks is 24 ticks per 60px cell, rounded down
    // to 16 so a cell is never wider than its picture.
    let step = cell_step(240, 600.0, 60.0);
    assert_eq!(step, 16, "the step is the power of two under the ask");

    let a = heads(0, 240, 600.0);
    assert!(
        a.iter().all(|tick| tick % step == 0),
        "every cell is on the grid: {a:?}"
    );

    // A clip starting mid-step opens on its own first frame, then lands on
    // the same grid, so a cell a neighbour already rendered is reused rather
    // than rendered again.
    let b = heads(12, 252, 600.0);
    assert_eq!(b[0], 12, "the head cell is the clip's first frame: {b:?}");
    assert!(
        b[1..].iter().all(|tick| tick % step == 0),
        "an off-grid start still snaps after its head: {b:?}"
    );
    assert!(
        b.contains(&32) && a.contains(&32),
        "the two clips share the cells they overlap: {a:?} / {b:?}"
    );
}

#[test]
fn the_step_only_changes_at_a_power_of_two_boundary() {
    // 300px over 240 ticks is 48 ticks per cell, the middle of the 32 band,
    // so a zoom nudge either way keeps the cells already rendered.
    let base = cell_step(240, 300.0, 60.0);
    assert_eq!(base, 32);
    for width in [230.0, 260.0, 300.0, 380.0, 440.0] {
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
fn a_long_clip_builds_the_cells_on_screen_and_no_others() {
    // An hour at 30 fps drawn 20px a tick is 3 ticks per cell, a step of 2:
    // 54000 cells, of which a 600 tick view shows 300.
    let (start, end) = (0, 108_000);
    let width = 108_000.0 * 20.0;
    let spans = cell_spans(start, end, width, 60.0, (50_000, 50_600));
    assert_eq!(
        spans.len(),
        300,
        "{} cells for a 600 tick view",
        spans.len()
    );
    assert_eq!(spans[0], (50_000, 50_002));
    assert_eq!(spans[299], (50_598, 50_600));
    // A view that opens mid-cell starts at the cell it opens in, and one that
    // ends mid-cell keeps the cell it ends in.
    let spans = cell_spans(start, end, width, 60.0, (50_001, 50_003));
    assert_eq!(spans, vec![(50_000, 50_002), (50_002, 50_004)]);
    // 195 ticks over 1950px is 6 ticks per cell, a step of 4. A view that
    // opens inside an off-grid clip's head cell starts with the head.
    assert_eq!(
        cell_spans(5, 200, 1950.0, 60.0, (6, 9)),
        vec![(5, 8), (8, 12)]
    );
}

#[test]
fn a_degenerate_span_asks_for_no_work() {
    assert_eq!(cell_step(0, 600.0, 60.0), 0);
    assert!(heads(0, 0, 600.0).is_empty());
    assert!(heads(100, 50, 600.0).is_empty());
    assert!(heads(0, 240, 0.0).is_empty());
    assert!(
        cell_spans(0, 240, 600.0, 60.0, (300, 400)).is_empty(),
        "a clip off screen asks for nothing"
    );
    assert_eq!(
        cell_spans(0, 5, 10.0, 60.0, (0, 5)),
        vec![(0, 5)],
        "a clip narrower than one cell still shows one frame across itself"
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

    store.advance(4, 5, vec![(16, 32)]);

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

    store.advance(7, 8, Vec::new());
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

/// How long the lock holder below keeps the engine locked at most.
const HELD: Duration = Duration::from_secs(3);

/// Cells asked for while another thread holds the engine lock return at once
/// and mark their tick in flight. Closes the class where the timeline's
/// draw read the revision under the engine lock and the window stood still
/// behind a cell decode. The holder releases after [`HELD`] whatever happens,
/// so an ask that waits ends the test red instead of hanging it. Does not
/// cover the render thread's read of the cell's source, which waits by design.
#[test]
fn asking_for_cells_does_not_wait_on_the_engine_lock() {
    let cx = gpui::TestAppContext::single();
    let state = cx.update(|app| app.new(|_| crate::app::premiere_tests::with_sequence(1, 10)));
    let engine = cx.update(|app| state.read(app).engine().clone());
    let (held_tx, held_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let holder = std::thread::spawn(move || {
        let _guard = engine.lock();
        let _ = held_tx.send(());
        let _ = release_rx.recv_timeout(HELD);
    });
    held_rx
        .recv_timeout(HELD)
        .expect("the holder took the engine lock");
    let started = Instant::now();
    cx.update(|app| ask(&state, app, vec![0]));
    let waited = started.elapsed();
    drop(release_tx);
    holder.join().expect("the holder released the engine lock");
    assert!(
        waited < Duration::from_millis(500),
        "the ask waited {waited:?} on the engine lock"
    );
    assert!(
        cx.update(|app| !state.read(app).filmstrips.wants(0)),
        "the ask did not mark its tick in flight"
    );
    // The cell task holds the state until its render lands; a task left
    // pending at the end is a leaked handle.
    cx.run_until_parked();
}

/// The session's last frame stats, bit for bit.
fn stats(engine: &parking_lot::Mutex<aria_engine::Engine>) -> [u64; 5] {
    let stats = engine.lock().last_frame_stats();
    [
        stats.decode_ms.to_bits(),
        stats.raster_ms.to_bits(),
        stats.composite_ms.to_bits(),
        u64::from(stats.effect_passes),
        u64::from(stats.effect_pass_bind_groups),
    ]
}

/// WHY: a cell rendered on the session's renderer held the engine through its
/// decode, 81 to 90 ms at the median on Server, and moved the decoder the
/// Viewer reads from, so the Viewer's next frame took 21 ms instead of 1.7 ms.
/// Closes the class of cell work reaching the session's renderer: a cell
/// requested far from the Viewer's frame renders nothing there, which leaves
/// the session's last frame stats as the Viewer's frame left them, and lands
/// the frame the session itself draws at the cell's tick. Does not cover the
/// engine read that takes the cell's source.
#[test]
fn a_cell_renders_nothing_on_the_sessions_renderer() {
    let cx = gpui::TestAppContext::single();
    let state = cx.update(|app| {
        app.new(|_| {
            crate::app::premiere_tests::with_media(crate::app::fixtures::asset(
                "fixture-640x360-24fps-3s.mp4",
            ))
            .0
        })
    });
    let (engine, rate) = cx.update(|app| {
        let state = state.read(app);
        (state.engine().clone(), state.playhead.rate)
    });
    let far = 48;
    engine
        .lock()
        .get_frame_fit(RationalTime::new(0, rate), 320, 180)
        .expect("the Viewer's frame");
    let viewer = stats(&engine);

    cx.update(|app| ask(&state, app, vec![far]));
    cx.run_until_parked();

    assert_eq!(
        stats(&engine),
        viewer,
        "the cell rendered on the session's renderer"
    );
    let landed = cx
        .update(|app| {
            state
                .read(app)
                .filmstrips
                .get(far)
                .map(|cell| cell.image.clone())
        })
        .expect("the cell landed");
    let drawn = engine
        .lock()
        .get_frame_fit(RationalTime::new(far, rate), CELL_MAX_W, CELL_H)
        .expect("the session's frame at the cell's tick");
    let drawn = crate::frame::rgba_to_render(&drawn).expect("a drawable frame");
    assert_eq!(landed.size(0), drawn.size(0), "the cell's size");
    assert!(
        landed.as_bytes(0) == drawn.as_bytes(0),
        "the cell differs from the frame the session draws at its tick"
    );
}
