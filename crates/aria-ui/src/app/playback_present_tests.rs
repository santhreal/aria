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

/// The achieved rate is the frames shown over the time the run has played.
///
/// The class closed here: the rate divided by the clock that places the
/// playhead, which a seek, a loop wrap and the mixer's restart after a seek
/// each start again, while the shown count went on from the start of the run,
/// so one loop wrap read twice the sequence's rate. The test leaves the run
/// 10 s old and the placing clock a few frames old, which is what a seek late
/// in a run leaves, and then starts a new run, which counts from its own
/// start. Not caught: a frame the meter counts twice.
#[gpui::test]
async fn the_achieved_rate_counts_the_run_across_a_restarted_clock(cx: &mut TestAppContext) {
    const RUN: Duration = Duration::from_secs(10);
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, "achieved");
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
    let begun = Instant::now();
    visual.update(|_, app| {
        state.update(app, |state, _| {
            state.playback_run_started = begun.checked_sub(RUN).expect("a clock 10 s old");
        })
    });
    for periods in 1..=3 {
        assert_eq!(vsync(&state, visual, periods), 1);
        settle(&state, visual).await;
    }
    let report = visual.read(|app| state.read(app).playback_report());
    assert!(report.presented > 0, "the run showed no frame");
    let shown = f64::from(report.presented);
    let longest = (RUN + begun.elapsed()).as_secs_f64();
    assert!(
        report.achieved_fps <= shown / RUN.as_secs_f64() && report.achieved_fps >= shown / longest,
        "{:.2} fps for {} frames shown over a run of at least {RUN:?}",
        report.achieved_fps,
        report.presented
    );

    // A new run counts from its own start, not from the one before it.
    visual.update(|_, app| state.update(app, |state, _| state.toggle_playback()));
    assert_eq!(vsync(&state, visual, 4), 0, "the loop outlived the stop");
    let restarted = Instant::now();
    visual.update(|_, app| {
        state.update(app, |state, _| {
            state.set_shuttle(1);
            state.set_shuttle(1);
        })
    });
    visual.update(|window, app| crate::app::start_playback(state.clone(), window, app));
    settle(&state, visual).await;
    visual.run_until_parked();
    assert_eq!(vsync(&state, visual, 1), 1);
    let report = visual.read(|app| state.read(app).playback_report());
    assert!(report.presented > 0, "the new run showed no frame");
    let floor = f64::from(report.presented) / restarted.elapsed().as_secs_f64();
    assert!(
        report.achieved_fps >= floor,
        "{:.2} fps for {} frames shown since a restart {:?} ago: the old run's time was counted",
        report.achieved_fps,
        report.presented,
        restarted.elapsed()
    );
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

/// Starting a session from Home with media (`Pick::OpenSample` or
/// `drop_on_home`) places the media on a timeline lane so playback advances
/// both the sequence playhead and the published `EditorView` playhead.
///
/// The class closed here is a Home media entry point that imports into the
/// pool without placing a clip on a track, leaving `duration_ticks` at zero so
/// pressing Play immediately stops at `00:00:00:00` and plugins reading
/// `EditorView` stay at tick `0`. Not caught: an external audio device clock
/// that reports a non-advancing sample position.
#[gpui::test]
async fn sample_project_and_home_media_drop_place_a_clip_that_plays_past_zero(
    cx: &mut TestAppContext,
) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, "vsync-sample");
    for label in ["open_sample_project", "drop_on_home"] {
        visual.update(|window, app| {
            state.update(app, |state, cx| {
                state.go_home();
                // An untitled session has no path to save to, so it is saved to
                // one: a dirty session would stop the Home pick at a prompt.
                state
                    .save_project_to(
                        crate::scratch_pool::scratch("vsync-sample-save")
                            .join(format!("{label}.aria")),
                    )
                    .expect("clean session on Home");
                match label {
                    "open_sample_project" => {
                        crate::surfaces::home_pick::apply(
                            state,
                            crate::surfaces::home_pick::Pick::OpenSample,
                        )
                        .expect("open sample project");
                    }
                    "drop_on_home" => {
                        state
                            .drop_on_home(vec![fixture()])
                            .expect("drop media on Home");
                    }
                    _ => unreachable!(),
                }
                assert_eq!(state.route, crate::types::UiRoute::Editor, "{label}");
                state.toggle_viewer_playback();
                cx.notify();
            });
            crate::app::start_playback(state.clone(), window, app);
        });
        settle(&state, visual).await;
        visual.run_until_parked();

        assert_eq!(
            vsync(&state, visual, 1),
            1,
            "{label}: playback stopped on the first vsync (duration_ticks = {})",
            visual.read(|app| state.read(app).store.sequence.duration_ticks)
        );
        // The sample project has sound, so on a host with an output device the
        // run follows the audio clock, which holds the playhead at the start
        // for the device's output latency. The playhead gets a deadline to
        // move rather than a count of vsyncs.
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut periods = 2;
        loop {
            visual.run_until_parked();
            settle(&state, visual).await;
            assert_eq!(
                vsync(&state, visual, periods),
                1,
                "{label}: vsync {periods}"
            );
            if visual.read(|app| state.read(app).playhead.value) > 0 {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "{label}: playhead stayed at 0 for 5 s of vsyncs"
            );
            std::thread::sleep(Duration::from_millis(10));
            periods += 1;
        }
        // The vsync that moved the playhead shows the newest frame rendered
        // ahead, which under load can be one the playhead has passed. Once the
        // worker settles, a vsync shows a frame past the start and not past the
        // playhead; under the audio clock the playhead keeps moving, so the
        // frame is not required to be the playhead's own.
        settle(&state, visual).await;
        assert_eq!(vsync(&state, visual, periods), 1, "{label}: settled vsync");
        visual.run_until_parked();

        visual.read(|app| {
            let state = state.read(app);
            assert!(
                state.playhead.value > 0,
                "{label}: playhead stayed at {}",
                state.playhead.value
            );
            assert!(
                state
                    .shown_tick
                    .is_some_and(|shown| shown > 0 && shown <= state.playhead.value),
                "{label}: Viewer frame {:?} did not advance with playhead {}",
                state.shown_tick,
                state.playhead.value
            );
            let view = state
                .ctx
                .try_get::<aria_plugin::EditorView>()
                .expect("EditorView mounted")
                .current()
                .expect("EditorView published");
            assert_eq!(
                view.playhead, state.playhead,
                "{label}: plugin EditorView playhead disagreed with sequence playhead"
            );
            assert!(
                view.playing,
                "{label}: plugin EditorView not marked playing"
            );
        });
        visual.update(|_, app| state.update(app, |state, _| state.toggle_playback()));
        assert_eq!(vsync(&state, visual, 3), 0, "{label}: loop outlived stop");
    }
}
