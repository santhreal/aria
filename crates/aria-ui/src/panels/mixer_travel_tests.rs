//! Every mixer strip's pan lights the stretch from center to its value, and
//! every fader the stretch from its foot to its gain.
//!
//! The class closed here is a bipolar control filled from its start: a pan
//! drew its fill from hard left, so a centered pan drew half its track lit, as
//! a level of one half would, and a pan hard left drew none. A lane strip and
//! the master strip are drawn at pans from hard left to hard right and at gains
//! from silence to the ceiling, and each fill is read against the stretch
//! worked out here: a pan's from the middle of its track to its value's place,
//! even between hard left and hard right, and a fader's from its foot to its
//! gain's place, even in decibels from -60 dB to the ceiling.
//!
//! Not caught: a bus strip's pan and fader, which draw through the same track
//! as the lane strip's; and the thumb, which has no debug selector.

use aria_commands::{Command, EditCommand};
use aria_models::{AudioSend, TrackId, TrackKind};
use gpui::{Bounds, Entity, Pixels, TestAppContext, VisualTestContext};

use crate::app::AppState;
use crate::interned::intern;
use crate::test_support::drawn_shell;

/// Pans from hard left to hard right, center and either side of it.
const PANS: [f32; 5] = [-1.0, -0.5, 0.0, 0.5, 1.0];
/// Gains from silence to the ceiling, unity among them.
const GAINS: [f32; 4] = [0.0, 0.5, 1.0, AudioSend::MAX_LEVEL];

/// Where the last frame drew `selector`.
fn drawn(visual: &mut VisualTestContext, selector: &str) -> Bounds<Pixels> {
    visual
        .debug_bounds(intern(selector))
        .unwrap_or_else(|| panic!("the mixer draws no {selector}"))
}

/// Sets `lane`'s and the master's gain and pan, and draws.
fn mix(
    visual: &mut VisualTestContext,
    state: &Entity<AppState>,
    lane: TrackId,
    gain: f32,
    pan: f32,
) {
    visual.update(|window, app| {
        state.update(app, |state, _| {
            let master = state.facts.mixer.master.clone();
            for command in [
                EditCommand::SetTrackGain { track: lane, gain },
                EditCommand::SetTrackPan { track: lane, pan },
                EditCommand::SetMasterMix {
                    gain,
                    pan,
                    muted: master.muted,
                    automation: master.automation,
                    volume: master.volume_envelope.clone(),
                    pan_envelope: master.pan_envelope.clone(),
                },
            ] {
                state
                    .apply_command(Command::Edit(command))
                    .unwrap_or_else(|err| panic!("set gain {gain} pan {pan}: {err:?}"));
            }
        });
        window.refresh();
    });
    visual.run_until_parked();
}

/// The fill of the track drawn as `track` covers `from..to` of its run, with
/// `along` reading a box's start and length on the run.
fn covers(
    visual: &mut VisualTestContext,
    track: &str,
    (from, to): (f32, f32),
    along: fn(Bounds<Pixels>) -> (f32, f32),
) {
    let (start, length) = along(drawn(visual, track));
    let (fill_start, fill_length) = along(drawn(visual, &format!("{track}-fill")));
    let (want_start, want_length) = (start + from * length, (to - from) * length);
    assert!(
        (fill_start - want_start).abs() < 1.0 && (fill_length - want_length).abs() < 1.0,
        "{track} is filled {fill_length} px from {fill_start} px, and its value covers \
         {from}..{to} of its {length} px from {start} px: {want_length} px from {want_start} px"
    );
}

/// A box's start and length left to right.
fn across(bounds: Bounds<Pixels>) -> (f32, f32) {
    (f32::from(bounds.left()), f32::from(bounds.size.width))
}

/// A box's start and length bottom to top, measured down from the window's
/// top edge as negative numbers so the start is the foot.
fn up(bounds: Bounds<Pixels>) -> (f32, f32) {
    (-f32::from(bounds.bottom()), f32::from(bounds.size.height))
}

/// Where `gain` stands on a fader even in decibels from -60 dB at its foot to
/// the ceiling at its head.
fn gain_place(gain: f32) -> f32 {
    let above_floor = |gain: f32| 20.0 * gain.log10() + 60.0;
    if gain <= 0.0 {
        0.0
    } else {
        above_floor(gain) / above_floor(AudioSend::MAX_LEVEL)
    }
}

#[gpui::test]
fn every_pan_lights_from_center_and_every_fader_from_its_foot(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, "mixer-travel");
    visual.update(|_, app| app.set_reduce_motion(true));
    let lane = visual.update(|_, app| {
        state.update(app, |state, cx| {
            assert!(state.apply_workspace("win-workspace-audio", cx));
            state
                .add_lane_of_kind(TrackKind::Audio, None)
                .unwrap_or_else(|err| panic!("an audio lane: {err:?}"))
        })
    });
    for pan in PANS {
        mix(visual, &state, lane, 1.0, pan);
        let place = (pan + 1.0) / 2.0;
        for id in [lane.raw(), 0] {
            let track = format!("mixer-pan-{id}");
            covers(visual, &track, (place.min(0.5), place.max(0.5)), across);
        }
    }
    for gain in GAINS {
        mix(visual, &state, lane, gain, 0.0);
        for id in [lane.raw(), 0] {
            let track = format!("mixer-fader-{id}");
            covers(visual, &track, (0.0, gain_place(gain)), up);
        }
    }
}
