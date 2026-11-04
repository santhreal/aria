//! The Viewer's peak strip draws nothing while the output is silent, and its
//! two tracks while the meter reads a level or holds a clip.
//!
//! Closes the class "an idle meter drawn in the control row": two gray tracks
//! at rest read as a grip glyph between the scopes toggle and the display
//! menu. Each reading the strip draws from is reached in turn: silence before
//! any block, a level, the level fallen back to silence, and a clip held over
//! silence until it is cleared. Not covered: the fill widths, which
//! `meter_scale_tests` measures.

use std::time::{Duration, Instant};

use gpui::{Entity, TestAppContext, VisualTestContext, point, px};

use crate::app::AppState;

/// The quads with area painted inside the Viewer's two peak tracks.
fn drawn(visual: &mut VisualTestContext) -> usize {
    let tracks = ["preview-meter-l", "preview-meter-r"].map(|id| {
        visual
            .debug_bounds(id)
            .unwrap_or_else(|| panic!("the Viewer lays out `{id}`"))
    });
    visual.update(|window, _| {
        let scale = window.scale_factor();
        window
            .painted_quads()
            .iter()
            .filter(|quad| {
                let b = quad.bounds;
                let (w, h) = (b.size.width.0 / scale, b.size.height.0 / scale);
                let center = point(
                    px(b.origin.x.0 / scale + w / 2.0),
                    px(b.origin.y.0 / scale + h / 2.0),
                );
                w > 0.0 && h > 0.0 && tracks.iter().any(|track| track.contains(&center))
            })
            .count()
    })
}

/// Publishes one stereo block at `level` full scale, ticks the meter at `now`
/// and draws the window.
fn publish(visual: &mut VisualTestContext, state: &Entity<AppState>, level: f32, now: Instant) {
    visual.update(|window, app| {
        state.update(app, |state, cx| {
            state.audio.publish_block(&[level, level], 2);
            state.meter_tick(now);
            cx.notify();
        });
        window.refresh();
    });
    visual.run_until_parked();
}

#[gpui::test]
fn the_viewer_peak_strip_draws_only_while_the_meter_reads(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = crate::test_support::drawn_shell(cx, "viewer-peak");
    visual.update(|_, app| app.set_reduce_motion(true));
    let mut now = Instant::now();
    publish(visual, &state, 0.0, now);
    assert_eq!(drawn(visual), 0, "silence before any level");

    now += Duration::from_millis(10);
    publish(visual, &state, 10f32.powf(-18.0 / 20.0), now);
    assert_eq!(
        drawn(visual),
        4,
        "a level at -18 dBFS: two tracks and two fills"
    );

    now += Duration::from_secs(10);
    publish(visual, &state, 0.0, now);
    assert_eq!(drawn(visual), 0, "the level fallen back to silence");

    now += Duration::from_millis(10);
    publish(visual, &state, 1.0, now);
    now += Duration::from_secs(10);
    publish(visual, &state, 0.0, now);
    assert_eq!(drawn(visual), 2, "a clip held over silence: the two tracks");

    visual.update(|window, app| {
        state.update(app, |state, cx| {
            state.clear_clip();
            cx.notify();
        });
        window.refresh();
    });
    visual.run_until_parked();
    assert_eq!(drawn(visual), 0, "the clip cleared over silence");
}
