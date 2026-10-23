//! The Audio workspace shows every mixer strip whole at the windows the
//! editor opens in, and lines its faders up.
//!
//! The class closed here: a workspace whose split weights leave the panel it
//! is built around too short for its content in the window the editor opens
//! in, so the panel scrolls before the first edit. The Audio workspace is
//! applied in the window [`UiSettings::resolve_window_placement`] opens on a
//! 1920 x 1080 display when it is not maximized, and in the whole display,
//! with an audio lane added beside the new project's video lane. The Audio
//! Track Mixer's strips must end above the bottom of the frame that holds
//! them.
//!
//! And a fader drawn at one fixed height under strip heads of different
//! heights: a lane with an `Out` row stood its fader lower than the master's,
//! and every fader stopped short of a tall frame. Every strip's fader must
//! start and end level with the others, every strip's head must reach down to
//! its foot so the strip's card stays whole, the strips must fill their frame,
//! and a drag must move a fader or a pan by the distance dragged over the
//! track as drawn, so a fader taller than another rides at the same rate.
//!
//! Not covered: the panels of the other workspaces, which scroll by design; a
//! strip taller than a new lane's, such as one with a long insert chain; a
//! window smaller than the one the editor opens in, where the strips scroll;
//! and the glyphs, which the test text system does not raster.

use gpui::{
    Bounds, Modifiers, MouseButton, Pixels, TestAppContext, VisualTestContext, point, px, size,
};

use super::tab_overflow_tests::{frame_bounds, redraw};
use crate::app::AppState;
use crate::app::mixer::FaderTarget;
use crate::dock::PANEL_MIXER;
use crate::interned::intern;
use crate::settings::UiSettings;
use crate::test_support::drawn_shell_at;
use crate::theme::Theme;

/// Layout is in floats; an edge two boxes share lands within this.
const EPS: f32 = 0.51;
const DISPLAY: [f32; 4] = [0.0, 0.0, 1920.0, 1080.0];

#[gpui::test]
fn the_audio_workspace_shows_every_mixer_strip_whole_in_the_opening_window(
    cx: &mut TestAppContext,
) {
    cx.executor().allow_parking();
    let opening = UiSettings::default().resolve_window_placement(&[DISPLAY]);
    for (width, height) in [(opening.width, opening.height), (DISPLAY[2], DISPLAY[3])] {
        let at = Bounds::new(point(px(0.), px(0.)), size(px(width), px(height)));
        let tag = format!("audio-workspace-fit-{width}x{height}");
        let (state, mut visual) = drawn_shell_at(cx, &tag, at);
        let visual = &mut visual;
        // A split's weights glide; reduced motion lands them on the next draw.
        visual.update(|_, app| app.set_reduce_motion(true));
        visual.update(|_, app| {
            state.update(app, |state, cx| {
                state
                    .add_lane_of_kind(aria_models::TrackKind::Audio, None)
                    .unwrap_or_else(|err| panic!("an audio lane: {err:?}"));
                assert!(state.apply_workspace("win-workspace-audio", cx));
            })
        });
        redraw(&state, visual);
        visual.update(|window, _| window.refresh());
        visual.run_until_parked();
        let holder = visual
            .read(|app| state.read(app).dock_tree.find(PANEL_MIXER))
            .expect("the Audio workspace docks the Audio Track Mixer");
        let frame = frame_bounds(&state, visual, holder);
        let strips = visual
            .debug_bounds("mixer-strips")
            .unwrap_or_else(|| panic!("the mixer drew no strips at {width} x {height}"));
        assert!(
            f32::from(strips.bottom()) <= f32::from(frame.bottom()) + EPS,
            "at {width} x {height} the mixer strips end at {:?}, past their frame's bottom \
             {:?}: the frame is {:?} tall and the strips need {:?} from its top",
            strips.bottom(),
            frame.bottom(),
            frame.size.height,
            strips.bottom() - frame.origin.y,
        );
    }
}

/// The Audio workspace at `width` x `height` with an audio lane and a submix
/// bus added, so the strips are a video lane, an audio lane, a bus and the
/// master, whose heads hold different rows.
fn audio_workspace(
    cx: &mut TestAppContext,
    width: f32,
    height: f32,
) -> (gpui::Entity<AppState>, VisualTestContext) {
    let at = Bounds::new(point(px(0.), px(0.)), size(px(width), px(height)));
    let (state, mut visual) = drawn_shell_at(cx, &format!("mixer-faders-{width}x{height}"), at);
    visual.update(|_, app| app.set_reduce_motion(true));
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state
                .add_lane_of_kind(aria_models::TrackKind::Audio, None)
                .unwrap_or_else(|err| panic!("an audio lane: {err:?}"));
            state
                .add_submix_lane()
                .unwrap_or_else(|err| panic!("a submix bus: {err:?}"));
            assert!(state.apply_workspace("win-workspace-audio", cx));
        })
    });
    redraw(&state, &mut visual);
    visual.update(|window, _| window.refresh());
    visual.run_until_parked();
    (state, visual)
}

/// Where `selector`, with the strip's raw id appended, was painted.
fn strip_part(visual: &mut VisualTestContext, part: &str, id: u64, win: &str) -> Bounds<Pixels> {
    let selector = intern(&format!("{part}-{id}"));
    visual
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("at {win} the mixer drew no `{selector}`"))
}

/// Every strip's raw id, the master's `0` last, as the mixer draws them.
fn strip_ids(state: &gpui::Entity<AppState>, visual: &mut VisualTestContext) -> Vec<u64> {
    let mut ids: Vec<u64> = visual.read(|app| {
        state
            .read(app)
            .mixer_strips()
            .map(|strips| strips.iter().map(|strip| strip.track).collect())
            .unwrap_or_else(|refusal| panic!("the mixer refused to draw: {refusal}"))
    });
    ids.push(0);
    ids
}

#[gpui::test]
fn every_mixer_fader_stands_level_with_the_others_and_fills_the_frame(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let opening = UiSettings::default().resolve_window_placement(&[DISPLAY]);
    for (width, height) in [(opening.width, opening.height), (DISPLAY[2], DISPLAY[3])] {
        let win = format!("{width} x {height}");
        let (state, mut visual) = audio_workspace(cx, width, height);
        let visual = &mut visual;
        let ids = strip_ids(&state, visual);
        assert!(
            ids.len() >= 3,
            "at {win} the mixer drew {ids:?}, not a lane, a bus and the master"
        );
        let strips = visual
            .debug_bounds("mixer-strips")
            .unwrap_or_else(|| panic!("at {win} the mixer drew no strips"));
        let down = visual
            .debug_bounds("mixer-strips-down")
            .unwrap_or_else(|| panic!("at {win} the mixer drew no strip region"));
        let at = |a: Pixels, b: Pixels| (f32::from(a) - f32::from(b)).abs() <= EPS;
        assert!(
            at(strips.bottom(), down.bottom()),
            "at {win} the strips end at {:?}, their region at {:?}",
            strips.bottom(),
            down.bottom()
        );
        let level = strip_part(visual, "mixer-fader", ids[0], &win);
        for &id in &ids {
            let fader = strip_part(visual, "mixer-fader", id, &win);
            let gain = strip_part(visual, "mixer-gain", id, &win);
            let foot = strip_part(visual, "mixer-foot", id, &win);
            let head = strip_part(visual, "mixer-head", id, &win);
            assert!(
                at(head.top(), strips.top() + px(Theme::SPACE_MD)) && at(head.bottom(), foot.top()),
                "at {win} strip {id}'s head runs {:?}..{:?} and its foot starts at {:?}: the \
                 head stops short of the foot, so the strip's card is drawn in two pieces",
                head.top(),
                head.bottom(),
                foot.top()
            );
            assert!(
                at(fader.top(), level.top()) && at(fader.bottom(), level.bottom()),
                "at {win} strip {id}'s fader runs {:?}..{:?}, strip {}'s {:?}..{:?}",
                fader.top(),
                fader.bottom(),
                ids[0],
                level.top(),
                level.bottom()
            );
            assert!(
                at(gain.top(), fader.bottom() + px(Theme::SPACE_XXS))
                    && at(foot.bottom(), gain.bottom() + px(Theme::SPACE_SM + 1.))
                    && at(foot.bottom(), strips.bottom() - px(Theme::SPACE_MD)),
                "at {win} strip {id}'s fader ends at {:?}, its readout runs {:?}..{:?}, its \
                 foot ends at {:?} and the strips at {:?}: the fader does not fill the strip",
                fader.bottom(),
                gain.top(),
                gain.bottom(),
                foot.bottom(),
                strips.bottom()
            );
        }
    }
}

#[gpui::test]
fn a_drag_moves_every_fader_and_pan_by_the_distance_over_its_drawn_track(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let opening = UiSettings::default().resolve_window_placement(&[DISPLAY]);
    for (width, height) in [(opening.width, opening.height), (DISPLAY[2], DISPLAY[3])] {
        let win = format!("{width} x {height}");
        let (state, mut visual) = audio_workspace(cx, width, height);
        let visual = &mut visual;
        for id in strip_ids(&state, visual) {
            let track = aria_models::TrackId::from_raw(id);
            for (part, target, horizontal) in [
                (
                    "mixer-fader",
                    if id == 0 {
                        FaderTarget::Master
                    } else {
                        FaderTarget::Lane(track)
                    },
                    false,
                ),
                (
                    "mixer-pan",
                    if id == 0 {
                        FaderTarget::MasterPan
                    } else {
                        FaderTarget::LanePan(track)
                    },
                    true,
                ),
            ] {
                let drawn = strip_part(visual, part, id, &win);
                // A quarter of the track: down a fader, which sits near its
                // top at unity, and right of a pan, which sits at its centre.
                let from = drawn.center();
                let to = if horizontal {
                    point(from.x + drawn.size.width / 4., from.y)
                } else {
                    point(from.x, from.y + drawn.size.height / 4.)
                };
                visual.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
                visual.run_until_parked();
                let grabbed = visual.read(|app| state.read(app).fader_dragging());
                visual.simulate_mouse_move(to, MouseButton::Left, Modifiers::none());
                visual.run_until_parked();
                let ridden = visual.read(|app| state.read(app).fader_dragging());
                visual.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
                visual.run_until_parked();
                let (Some(grabbed), Some(ridden)) = (grabbed, ridden) else {
                    panic!("at {win} a press on {part}-{id} at {from:?} grabbed nothing");
                };
                assert_eq!(grabbed.target, target, "at {win} {part}-{id}");
                let moved = target.value_to_frac(ridden.gain) - target.value_to_frac(grabbed.gain);
                let wanted = if horizontal { 0.25 } else { -0.25 };
                assert!(
                    (moved - wanted).abs() <= 0.01,
                    "at {win} a drag over a quarter of {part}-{id}, drawn {:?} long, moved \
                     it {moved} of its range",
                    if horizontal {
                        drawn.size.width
                    } else {
                        drawn.size.height
                    }
                );
            }
        }
    }
}
