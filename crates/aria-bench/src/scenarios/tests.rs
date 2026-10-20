//! A timed sample draws the editor at the instant the sample is taken.
//!
//! Closes the class where the clock gpui animations sample, the test
//! executor's, stood at the instant the window opened: every one-shot entrance
//! stayed on its first frame and requested the next one, so each simulated
//! vsync rebuilt the views holding it, and the timeline-frame samples measured
//! an editor that never finished opening. Every scenario's UI sample goes
//! through `timed_frame`, so the tests hold it there. Does not check which
//! animations a frame draws, or the playback clock, which reads the wall clock
//! directly.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use super::{open_editor, timed_frame};

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("aria-bench-scenarios-{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

/// Longer than every one-shot the editor plays when a project opens.
const SETTLE: Duration = Duration::from_millis(1500);

#[test]
fn a_sample_samples_motion_at_the_wall_clock() {
    let (app, _state, mut window) = open_editor(&scratch("clock"));
    std::thread::sleep(Duration::from_millis(20));
    let before = Instant::now();
    timed_frame(&app, &mut window, |_, _, _| {});
    let motion = app.background_executor().now();
    assert!(
        motion >= before,
        "the motion clock is {:?} behind the wall clock the sample started at",
        before.saturating_duration_since(motion)
    );
}

#[test]
fn an_opened_editor_at_rest_requests_no_frame() {
    let (mut app, state, mut window) = open_editor(&scratch("rest"));
    app.update(|cx| {
        state.update(cx, |s, cx| {
            s.new_project().expect("new project");
            cx.notify();
        });
    });
    timed_frame(&app, &mut window, |_, _, _| {});
    std::thread::sleep(SETTLE);
    // The first vsync after the opening delivers the frames its entrances
    // requested while they ran; each lands on it.
    timed_frame(&app, &mut window, |_, window, cx| {
        window.simulate_next_frame(cx);
    });
    let mut requested = usize::MAX;
    timed_frame(&app, &mut window, |_, window, cx| {
        requested = window.simulate_next_frame(cx);
    });
    assert_eq!(
        requested, 0,
        "{requested} frame requests are still pending {SETTLE:?} after the editor opened"
    );
}
