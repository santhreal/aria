//! The right side of the status bar.
//!
//! The class closed here: a status bar that drew `0 dropped`, the playback
//! resolution, and the GPU device name on every frame at rest, three readouts
//! with nothing to report. These pin that at rest with no drop each readout
//! is absent, that a drop shows its count until a click resets it, and that
//! playback draws the resolution and the device beside the achieved rate and
//! takes them away again on a stop.
//!
//! Not covered: the text of the achieved rate (the playback report tests pin
//! the numbers), and the device name on a host with no adapter, where there is
//! nothing to draw.

use std::path::PathBuf;
use std::time::Instant;

use aria_render::DropCause;
use gpui::{TestAppContext, VisualTestContext};

use crate::test_support::drawn_shell;

/// Every readout the right side can draw.
const READOUTS: [&str; 3] = [
    "status-dropped-frames",
    "status-playback-resolution",
    "status-gpu",
];

fn drawn(visual: &mut VisualTestContext, id: &'static str) -> bool {
    visual.run_until_parked();
    visual.debug_bounds(id).is_some()
}

#[gpui::test]
fn at_rest_with_no_drop_the_right_side_draws_nothing(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (_state, visual) = drawn_shell(cx, "status-rest");
    for id in READOUTS {
        assert!(!drawn(visual, id), "{id} is drawn at rest");
    }
}

#[gpui::test]
fn a_drop_shows_its_count_until_a_click_resets_it(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, "status-drop");
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.dropped.record(DropCause::PresentLate, 3);
            cx.notify();
        })
    });
    assert!(
        drawn(visual, "status-dropped-frames"),
        "a drop is not drawn"
    );
    let chip = visual
        .debug_bounds("status-dropped-frames")
        .expect("the dropped chip has bounds");
    visual.simulate_click(chip.center(), gpui::Modifiers::none());
    assert!(
        visual.read(|app| state.read(app).dropped_frames().is_empty()),
        "the click did not reset the count"
    );
    assert!(
        !drawn(visual, "status-dropped-frames"),
        "the chip outlived the reset"
    );
}

#[gpui::test]
async fn playback_draws_the_resolution_and_a_stop_takes_it_away(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, "status-playing");
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/texture.png");
    visual.update(|_, app| {
        state.update(app, |state, _| {
            let media = state.import_media(fixture).expect("import the fixture");
            state
                .drop_media_on_timeline(media, None, 0)
                .expect("place the clip");
            // 2x: the audio path is silent, so no audio device takes part.
            state.set_shuttle(1);
            state.set_shuttle(1);
        })
    });
    visual.update(|window, app| crate::app::start_playback(state.clone(), window, app));
    assert!(
        drawn(visual, "status-playback-resolution"),
        "playback does not draw its resolution"
    );
    let device = visual.read(|app| state.read(app).preview_worker.device_name());
    assert_eq!(
        drawn(visual, "status-gpu"),
        device.is_some(),
        "the device readout does not follow the adapter {device:?}"
    );

    visual.update(|_, app| state.update(app, |state, _| state.toggle_playback()));
    // One vsync past the stop ends the loop.
    visual.update(|window, app| {
        state.update(app, |state, _| {
            let period = state.frame_period();
            state.playback_started = Instant::now().checked_sub(period * 2);
        });
        window.simulate_next_frame(app);
    });
    for id in READOUTS {
        assert!(!drawn(visual, id), "{id} is drawn after the stop");
    }
}
