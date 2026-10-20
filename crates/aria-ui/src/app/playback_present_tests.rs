//! Playback driven from the display's vsync.
//!
//! The class closed here: a frame clock that rendered the frame due now on a
//! thread started for it, kept every texture it ever showed in the window
//! atlas, and had no count of the frames it failed to show. These drive the
//! real loop one vsync at a time against a clock the test sets, and pin that a
//! vsync shows the newest frame rendered ahead, counts every due frame it did
//! not show by cause, releases the texture it replaced, asks for no still
//! frame, and that the loop ends on a stop.
//!
//! The run plays at 2x against a monotonic clock the test sets, which is the
//! master while no audio stream reports a tick.
//!
//! Not covered: the log lines written with Settings > Playback > Log dropped
//! frames on, and the drawn indicator.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use aria_render::DropCause;
use gpui::{TestAppContext, VisualTestContext};

use super::AppState;
use crate::test_support::drawn_shell;

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/texture.png")
}

/// Put the monotonic clock half a frame past `periods` frames into the run.
fn clock_at(state: &mut AppState, periods: u32) {
    let period = state.frame_period();
    state.playback_started = Instant::now().checked_sub(period * periods + period / 2);
}

/// Presenting loops with a vsync callback pending.
fn armed(state: &gpui::Entity<AppState>, visual: &mut VisualTestContext) -> usize {
    visual.read(|app| std::rc::Rc::strong_count(&state.read(app).playback_vsyncs) - 1)
}

/// Run one vsync with the clock at `periods`; returns the loops still armed.
pub(crate) fn vsync(
    state: &gpui::Entity<AppState>,
    visual: &mut VisualTestContext,
    periods: u32,
) -> usize {
    visual.update(|window, app| {
        state.update(app, |state, _| clock_at(state, periods));
        window.simulate_next_frame(app);
    });
    armed(state, visual)
}

/// Wait until the worker has nothing left to render ahead.
pub(crate) async fn settle(state: &gpui::Entity<AppState>, visual: &mut VisualTestContext) {
    let executor = visual.executor();
    let barrier = visual.read(|app| state.read(app).preview_worker.run(&executor, || ()));
    assert_eq!(barrier.await, Some(()), "the preview worker stopped");
}

#[gpui::test]
async fn each_vsync_shows_the_newest_frame_and_counts_what_it_did_not(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, "vsync");
    visual.update(|_, app| {
        state.update(app, |state, _| {
            let media = state.import_media(fixture()).expect("import the fixture");
            state
                .drop_media_on_timeline(media, None, 0)
                .expect("place the clip");
            state.set_shuttle(1);
            state.set_shuttle(1);
            assert_eq!(state.shuttle, 2);
            assert!(
                state.audio_follows(),
                "2x is within the 4x audible shuttle range"
            );
        })
    });
    visual.update(|window, app| crate::app::start_playback(state.clone(), window, app));
    settle(&state, visual).await;
    visual.run_until_parked();
    let run = visual.read(|app| {
        state
            .read(app)
            .playback_cadence
            .expect("a run is presenting")
    });
    assert_eq!(run.tick(1) - run.anchor, 2);
    assert_eq!(armed(&state, visual), 1);
    let asked = visual.read(|app| state.read(app).preview_generation);

    // Slot 1 is due and was rendered ahead.
    assert_eq!(vsync(&state, visual, 1), 1);
    let first = visual.read(|app| {
        let state = state.read(app);
        assert_eq!(state.shown_tick, Some(run.tick(1)));
        assert!(
            state.dropped_frames().is_empty(),
            "{:?}",
            state.dropped_frames()
        );
        assert_eq!(state.playback_report().presented, 1);
        state.last_render.clone().expect("the frame has a texture")
    });
    visual.run_until_parked();
    // The playhead moved, and playback shows its own frames: the shell's draw
    // asks for no still frame.
    assert_eq!(
        visual.read(|app| state.read(app).preview_generation),
        asked,
        "a vsync's playhead move asked for a still frame"
    );
    assert!(
        visual.update(|window, _| window.has_image_atlas_entry(&first)),
        "the shown frame was not drawn"
    );
    settle(&state, visual).await;

    // Slots 2 and 3 came due on one vsync: 3 is shown, 2 is present-late.
    assert_eq!(vsync(&state, visual, 3), 1);
    visual.read(|app| {
        let state = state.read(app);
        assert_eq!(state.shown_tick, Some(run.tick(3)));
        assert_eq!(state.dropped_frames().count(DropCause::PresentLate), 1);
        assert_eq!(state.dropped_frames().total(), 1);
    });
    assert!(
        !visual.update(|window, _| window.has_image_atlas_entry(&first)),
        "the replaced frame stayed in the window atlas"
    );

    // Slot 4 came due with nothing rendered: a not-ready drop, and the frame
    // on screen stays.
    visual.update(|_, app| state.update(app, |state, _| state.preview_worker.clear()));
    settle(&state, visual).await;
    visual.update(|_, app| state.update(app, |state, _| state.preview_worker.clear()));
    assert_eq!(vsync(&state, visual, 4), 1);
    visual.read(|app| {
        let state = state.read(app);
        let dropped = state.dropped_frames();
        assert_eq!(
            state.shown_tick,
            Some(run.tick(3)),
            "a missing frame moved the picture"
        );
        assert_eq!(dropped.total(), 2);
        assert_eq!(
            dropped.count(DropCause::DecodeLate) + dropped.count(DropCause::RenderLate),
            1,
            "{dropped:?}"
        );
    });

    // The indicator resets; the run's own report keeps its count.
    visual.update(|_, app| state.update(app, |state, _| state.reset_dropped_frames()));
    visual.read(|app| {
        let state = state.read(app);
        assert!(state.dropped_frames().is_empty());
        assert_eq!(state.playback_report().dropped.total(), 2);
    });

    // A stop ends the loop on its next vsync and drops the run's frames.
    visual.update(|_, app| state.update(app, |state, _| state.toggle_playback()));
    assert_eq!(
        visual.read(|app| state.read(app).preview_worker.cached()),
        0,
        "a stop kept the frames rendered ahead"
    );
    assert_eq!(
        armed(&state, visual),
        1,
        "the stop dropped the pending vsync"
    );
    assert_eq!(vsync(&state, visual, 5), 0, "the loop outlived the stop");
    assert_eq!(vsync(&state, visual, 6), 0);
}

/// Starting playback again while a run presents leaves one loop: the earlier
/// one sees a generation that is not its own and ends.
#[gpui::test]
async fn a_second_start_leaves_one_loop(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, "vsync-twice");
    visual.update(|_, app| {
        state.update(app, |state, _| {
            let media = state.import_media(fixture()).expect("import the fixture");
            state
                .drop_media_on_timeline(media, None, 0)
                .expect("place the clip");
            state.set_shuttle(1);
            state.set_shuttle(1);
        })
    });
    visual.update(|window, app| crate::app::start_playback(state.clone(), window, app));
    visual.update(|window, app| crate::app::start_playback(state.clone(), window, app));
    settle(&state, visual).await;
    visual.run_until_parked();
    assert_eq!(armed(&state, visual), 2, "both loops were pending");
    assert_eq!(
        vsync(&state, visual, 1),
        1,
        "the superseded loop kept running"
    );
    assert_eq!(vsync(&state, visual, 2), 1);
    visual.update(|_, app| state.update(app, |state, _| state.toggle_playback()));
    assert_eq!(vsync(&state, visual, 3), 0, "the loop outlived the stop");
}

/// A playback vsync moves the playhead while another thread holds the engine,
/// as the preview worker does for each frame it renders, and the stop polls
/// what the vsync skipped.
///
/// The class closed here is a poll on the UI thread that waits for the engine
/// on each move of a run, so every vsync waits out the frame being rendered
/// ahead. The holder edits the engine while it holds it: a stop that skipped
/// the poll leaves the edit out of the store. Not caught: a wait on another
/// lock than the engine's.
#[gpui::test]
async fn a_vsync_moves_the_playhead_while_the_engine_is_held(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, "vsync-held");
    visual.update(|_, app| {
        state.update(app, |state, _| {
            let media = state.import_media(fixture()).expect("import the fixture");
            state
                .drop_media_on_timeline(media, None, 0)
                .expect("place the clip");
            state.set_shuttle(1);
            state.set_shuttle(1);
        })
    });
    visual.update(|window, app| crate::app::start_playback(state.clone(), window, app));
    settle(&state, visual).await;
    visual.run_until_parked();
    let (engine, before) = visual.read(|app| {
        let state = state.read(app);
        (state.engine().clone(), state.playhead.value)
    });

    // The holder takes the engine and edits it, then lets go when told, or
    // after five seconds.
    let (held, holding) = std::sync::mpsc::channel();
    let (release, released) = std::sync::mpsc::channel::<()>();
    let holder = std::thread::spawn(move || {
        let mut engine = engine.lock();
        engine.mark_unsaved();
        let _ = held.send(engine.revision());
        let _ = released.recv_timeout(Duration::from_secs(5));
    });
    let edited = holding.recv().expect("the holder took the engine");
    let started = Instant::now();
    assert_eq!(vsync(&state, visual, 1), 1, "the run ended");
    let waited = started.elapsed();
    let _ = release.send(());
    holder.join().expect("the holder ran to the end");
    assert!(
        waited < Duration::from_secs(2),
        "the vsync waited {waited:?} for the engine"
    );
    visual.read(|app| {
        let state = state.read(app);
        assert_ne!(state.playhead.value, before, "the vsync left the playhead");
        assert_ne!(state.revision, edited, "the vsync polled a held engine");
    });

    visual.update(|_, app| state.update(app, |state, _| state.toggle_playback()));
    assert_eq!(
        visual.read(|app| state.read(app).revision),
        edited,
        "the stop left the edit made under the run out of the store"
    );
}
