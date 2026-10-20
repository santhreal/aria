//! A playback vsync builds the panel bodies that draw the playhead or the
//! frame it shows, and nothing else.
//!
//! The class closed here is a vsync that redraws the whole window, so each
//! frame of a run builds every panel body and chrome part again, and its
//! converse, a vsync that leaves a body drawing the playhead, the picture or
//! the dropped-frame count replaying its last drawing. The run is driven
//! through the real loop, one vsync at a time against a clock the test sets.
//! A vsync that shows a frame must build the bodies a playhead move and a
//! landed frame build; one that drops the frame due must build the bodies a
//! playhead move builds. Neither may build a chrome part or notify the state's
//! observers, and the window each draws must equal a full redraw. A vsync that
//! wraps a loop restarts the audio stream, which the shell starts on a notify
//! of the state, so it must notify the state's observers and build what a
//! notification of the state builds.
//!
//! Not caught: a panel outside the launch layout that draws a drop, a body
//! stale only in an image, and a stream the shell starts from the notify.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{Entity, TestAppContext, VisualTestContext};

use crate::app::AppState;
use crate::app::playback_present_tests::{settle, vsync};
use crate::dock::PanelId;
use crate::shell::Moved;

use super::builds::{self, Part};
use super::tests::{drawing, opened, rebuilt, stamps};
use super::unmoved;

/// The panels the layout docks, sorted.
fn docked(state: &Entity<AppState>, visual: &mut VisualTestContext) -> Vec<PanelId> {
    let mut panels: Vec<_> = visual.read(|app| {
        state
            .read(app)
            .dock_tree
            .frames()
            .into_iter()
            .map(|frame| frame.active_panel())
            .collect()
    });
    panels.sort_unstable();
    panels.dedup();
    panels
}

/// The docked panels a change that moved any of `moved` redraws, sorted.
fn redrawn(
    state: &Entity<AppState>,
    visual: &mut VisualTestContext,
    moved: &[Moved],
) -> Vec<PanelId> {
    let mut panels = docked(state, visual);
    panels.retain(|&panel| moved.iter().any(|&kind| !unmoved(panel, kind)));
    panels
}

/// Runs one vsync with the clock at `periods` and checks what it built and
/// drew. `shows` is whether a frame was rendered for the slot due.
fn presents(
    state: &Entity<AppState>,
    visual: &mut VisualTestContext,
    notified: &Cell<u32>,
    periods: u32,
    shows: bool,
) {
    visual.run_until_parked();
    builds::take();
    builds::take_parts();
    notified.set(0);
    let (tick, shown, dropped) = visual.read(|app| {
        let state = state.read(app);
        (
            state.playhead.value,
            state.shown_tick,
            state.dropped_frames().total(),
        )
    });

    assert_eq!(vsync(state, visual, periods), 1, "the run ended");
    visual.run_until_parked();
    visual.read(|app| {
        let state = state.read(app);
        assert_ne!(
            state.playhead.value, tick,
            "vsync {periods} left the playhead"
        );
        assert_eq!(
            state.shown_tick != shown,
            shows,
            "vsync {periods} showed tick {:?} after {shown:?}",
            state.shown_tick
        );
        assert_eq!(
            state.dropped_frames().total() != dropped,
            !shows,
            "vsync {periods} dropped {:?}",
            state.dropped_frames()
        );
    });
    let moved: &[Moved] = if shows {
        &[Moved::Playhead, Moved::Frame]
    } else {
        &[Moved::Playhead]
    };
    let mut built = builds::take();
    built.sort_unstable();
    built.dedup();
    assert_eq!(
        built,
        redrawn(state, visual, moved),
        "the bodies vsync {periods} built"
    );
    assert_eq!(
        builds::take_parts(),
        Vec::<Part>::new(),
        "the chrome parts vsync {periods} built"
    );
    assert_eq!(
        notified.get(),
        0,
        "vsync {periods} notified the state's observers"
    );

    let cached = drawing(visual);
    visual.update(|window, _| window.refresh());
    visual.run_until_parked();
    let full = drawing(visual);
    assert!(
        cached == full,
        "vsync {periods} drew a window that differs from a full redraw\n{}",
        cached.differences(&full)
    );
}

#[gpui::test]
async fn a_playback_vsync_builds_the_bodies_that_draw_the_playhead_or_the_frame(
    cx: &mut TestAppContext,
) {
    cx.executor().allow_parking();
    let (state, visual) = opened(cx, "panel-view-playback");
    assert!(
        redrawn(&state, visual, &[Moved::Playhead, Moved::Frame]).len()
            < docked(&state, visual).len(),
        "the launch layout docks no panel a playback vsync leaves alone"
    );

    // Counts the notifications of the state, which the shell observes.
    let notified = Rc::new(Cell::new(0u32));
    let _observing = visual.update(|_, app| {
        let notified = notified.clone();
        app.observe(&state, move |_, _| notified.set(notified.get() + 1))
    });

    // Each frame that drew the boot cover asked for the next one, and the
    // shell redraws every body on it. Those frames run first, so the vsyncs
    // below build only what playback moves.
    for frame in 0.. {
        assert!(frame < 8, "the open never came to rest");
        if visual.update(|window, app| window.simulate_next_frame(app)) == 0 {
            break;
        }
        visual.run_until_parked();
    }

    // 2x, started the way a command starts a run: the transport changes, the
    // presenting loop starts, and the window redraws. The test sets the
    // monotonic clock, which is the master while no audio stream reports a
    // tick.
    visual.update(|window, app| {
        state.update(app, |state, _| {
            state.set_shuttle(1);
            state.set_shuttle(1);
        });
        crate::app::start_playback(state.clone(), window, app);
        state.update(app, |_, cx| cx.notify());
    });
    for periods in 1..=3 {
        settle(&state, visual).await;
        presents(&state, visual, &notified, periods, true);
    }

    // Slot 4 comes due with nothing rendered for it: a drop, and the frame on
    // screen stays.
    visual.update(|_, app| state.update(app, |state, _| state.preview_worker.clear()));
    settle(&state, visual).await;
    visual.update(|_, app| state.update(app, |state, _| state.preview_worker.clear()));
    presents(&state, visual, &notified, 4, false);

    // Slot 5 is past an Out point two ticks ahead, with loop on: the run wraps
    // to the In point and restarts its clock and its audio stream.
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.mark_in = Some(0);
            state.mark_out = Some(state.playhead.value + 2);
            state.toggle_loop();
            cx.notify();
        })
    });
    visual.run_until_parked();
    builds::take();
    notified.set(0);
    let panels = docked(&state, visual);
    let before = stamps(&state, visual, &panels);
    assert_eq!(vsync(&state, visual, 5), 1, "the run ended at the wrap");
    visual.run_until_parked();
    assert_eq!(visual.read(|app| state.read(app).playhead.value), 0);
    assert_ne!(
        notified.get(),
        0,
        "the wrap notified none of the state's observers"
    );
    let mut built = builds::take();
    built.sort_unstable();
    built.dedup();
    assert_eq!(
        built,
        rebuilt(&state, visual, &panels, &before),
        "the bodies the wrap built"
    );

    visual.update(|_, app| state.update(app, |state, _| state.toggle_playback()));
}
