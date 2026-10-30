//! Every meter draws a level at one place on one dBFS scale, from the reading
//! the Audio Meters panel draws.
//!
//! WHY: the Viewer's peak strip and the Audio Track Mixer's bars drew the
//! linear amplitude, so a programme at -18 dBFS filled an eighth of the
//! Viewer's bars and seven tenths of the Audio Meters panel's, and the master
//! strip read the last block without the panel's fall. The class: a meter
//! drawn on a scale of its own, or from a reading of its own. The Audio
//! workspace draws the Viewer, the mixer and the Audio Meters panel at once.
//! Levels rise through three steps and then drop, and the drop is ticked a
//! tenth of a second later, so the reading stands above the level published.
//! At every step every bar is measured against [`meter_fraction`] of the
//! reading: the panel's peak, the Viewer's two bars and the master's mixer
//! bars against the programme meter's, the lane's mixer bars against the
//! lane's.
//!
//! Not covered: a meter this test does not measure; the rates of the
//! ballistics, which `app::meter_tests` covers; the colours.

use std::time::{Duration, Instant};

use gpui::{Bounds, Pixels, TestAppContext, VisualTestContext, point, px, size};

use crate::app::AppState;
use crate::app::meter::meter_fraction;
use crate::interned::intern;
use crate::test_support::drawn_shell_at;

/// Layout is in floats, and the Viewer's bar rounds its fill to a whole pixel.
const EPS: f32 = 0.51;

/// The levels published, in dBFS, and how long after the step before each is
/// ticked: three rises, then a drop the bars fall towards.
const STEPS: [(f32, u64); 4] = [(-42.0, 10), (-18.0, 10), (-6.0, 10), (-40.0, 100)];

fn drawn(visual: &mut VisualTestContext, selector: &str, dbfs: f32) -> Bounds<Pixels> {
    visual
        .debug_bounds(intern(selector))
        .unwrap_or_else(|| panic!("at {dbfs} dBFS nothing drew `{selector}`"))
}

fn redraw(state: &gpui::Entity<AppState>, visual: &mut VisualTestContext) {
    for _ in 0..2 {
        visual.update(|_, app| state.update(app, |_, cx| cx.notify()));
        visual.run_until_parked();
    }
    visual.update(|window, _| window.refresh());
    visual.run_until_parked();
}

#[gpui::test]
fn every_meter_draws_a_level_at_one_place_on_the_dbfs_scale(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let at = Bounds::new(point(px(0.), px(0.)), size(px(1920.), px(1080.)));
    let (state, mut visual) = drawn_shell_at(cx, "meter-scale", at);
    let visual = &mut visual;
    visual.update(|_, app| app.set_reduce_motion(true));
    let lane = visual.update(|_, app| {
        state.update(app, |state, cx| {
            let lane = state
                .add_lane_of_kind(aria_models::TrackKind::Audio, None)
                .unwrap_or_else(|err| panic!("an audio lane: {err:?}"));
            assert!(state.apply_workspace("win-workspace-audio", cx));
            lane
        })
    });
    redraw(&state, visual);
    let strip = lane.raw();

    let mut now = Instant::now();
    for (step, (dbfs, after_ms)) in STEPS.into_iter().enumerate() {
        let level = 10f32.powf(dbfs / 20.0);
        now += Duration::from_millis(after_ms);
        visual.update(|_, app| {
            state.update(app, |state, _| {
                state.audio.publish_block(&[level, level], 2);
                state
                    .audio
                    .arm_lane_meters([lane].into_iter())
                    .publish([(lane, [level, level])].into_iter());
                state.meter_tick(now);
            })
        });
        redraw(&state, visual);
        let (programme, own) = visual.read(|app| {
            (
                state.read(app).meter().peaks,
                state.read(app).strip_meter(lane),
            )
        });
        if step + 1 < STEPS.len() {
            assert_eq!(
                (programme, own),
                ([level; 2], [level; 2]),
                "a rise at {dbfs} dBFS reads at once"
            );
        } else {
            assert!(
                programme[0] > 2.0 * level && own[0] > 2.0 * level,
                "the drop to {dbfs} dBFS reads {programme:?} and {own:?}, not above it"
            );
        }

        // Vertical bars fill up from their foot.
        let mut tall = vec![(
            "the Audio Meters panel's peak".to_owned(),
            "audio-meter-bar-0".to_owned(),
            "audio-meter-peak-0".to_owned(),
            programme[0],
        )];
        for (side, name) in ["l", "r"].into_iter().enumerate() {
            for (raw, whose, reading) in [
                (strip, "the lane's mixer strip", own),
                (0, "the master strip", programme),
            ] {
                tall.push((
                    format!("{whose}, {name}"),
                    format!("mixer-meter-{raw}-{name}"),
                    format!("mixer-meter-{raw}-{name}-fill"),
                    reading[side],
                ));
            }
        }
        for (whose, track, fill, reading) in tall {
            let (track, fill) = (drawn(visual, &track, dbfs), drawn(visual, &fill, dbfs));
            let expected = f32::from(track.size.height) * meter_fraction(reading);
            assert!(
                (f32::from(fill.size.height) - expected).abs() <= EPS
                    && (f32::from(fill.bottom()) - f32::from(track.bottom())).abs() <= EPS,
                "at {dbfs} dBFS {whose} reads {reading} and fills {:?} of a {:?} bar up from \
                 {:?}, not {expected} up from its foot at {:?}",
                fill.size.height,
                track.size.height,
                fill.bottom(),
                track.bottom()
            );
        }
        // The Viewer's bars fill rightwards from their left end.
        for (side, name) in ["l", "r"].into_iter().enumerate() {
            let track = drawn(visual, &format!("preview-meter-{name}"), dbfs);
            let fill = drawn(visual, &format!("preview-meter-{name}-fill"), dbfs);
            let expected = f32::from(track.size.width) * meter_fraction(programme[side]);
            assert!(
                (f32::from(fill.size.width) - expected).abs() <= EPS
                    && (f32::from(fill.left()) - f32::from(track.left())).abs() <= EPS,
                "at {dbfs} dBFS the Viewer's {name} bar reads {} and fills {:?} of {:?} from \
                 {:?}, not {expected} from its left end at {:?}",
                programme[side],
                fill.size.width,
                track.size.width,
                fill.left(),
                track.left()
            );
        }
    }
}
