//! Driving the mixer's strips: every gesture, on a project with a lane, a bus
//! and something to send.
//!
//! The fader is the reason this census is a sequence rather than a set. A grab
//! writes nothing, a ride writes nothing, and the release writes the one command
//! for the whole gesture, so pressing the three phases independently would
//! measure nothing at all and report the fader as unreached. The send chip is the
//! same shape in the other direction: one chip sends three different commands
//! depending on what the lane already holds, so it is clicked three times.

use std::collections::BTreeSet;

use aria_models::TrackId;

use crate::app::AppState;
use crate::app::mixer::FaderTarget;
use crate::app::sent_census::Watch;
use crate::surfaces::mixer_strip::{self, Gesture};

/// A project with an audio lane and two submix buses: the smallest mix where a
/// lane has somewhere to point, somewhere to send, and a fader that is not the
/// master's.
struct Mix {
    state: AppState,
    lane: TrackId,
    bus: TrackId,
    other: TrackId,
}

fn dressed() -> Mix {
    let (mut state, _) = crate::app::fixtures::with_audio();
    // The buses arrive through the gesture that makes one, so the dressing is
    // the product's own path rather than a second way to build a mix.
    for _ in 0..2 {
        mixer_strip::apply(&mut state, Gesture::AddBus).expect("a submix bus lands");
    }
    let lane = state
        .store
        .sequence
        .tracks
        .iter()
        .find(|track| track.kind == crate::types::UiTrackKind::Audio && !track.clips.is_empty())
        .map(|track| TrackId::from_raw(track.id))
        .expect("the fixture puts a clip on an audio lane");
    let buses: Vec<TrackId> = state
        .mixer_strips()
        .expect("the mix reads")
        .iter()
        .filter(|strip| strip.is_bus)
        .map(|strip| TrackId::from_raw(strip.track))
        .collect();
    assert_eq!(buses.len(), 2, "the dressing wants two buses");
    Mix {
        state,
        lane,
        bus: buses[0],
        other: buses[1],
    }
}

/// Every gesture the strip offers, in the order a pointer would make them.
///
/// The send chip appears three times because its three clicks are three
/// commands: the send is added, made pre-fader, then dropped.
fn gestures(mix: &Mix) -> Vec<Gesture> {
    vec![
        Gesture::AddBus,
        Gesture::PointOutput {
            track: mix.lane,
            to: Some(mix.bus),
        },
        Gesture::PointOutput {
            track: mix.lane,
            to: None,
        },
        Gesture::CycleSend {
            track: mix.lane,
            to: mix.other,
        },
        Gesture::CycleSend {
            track: mix.lane,
            to: mix.other,
        },
        Gesture::CycleSend {
            track: mix.lane,
            to: mix.other,
        },
        Gesture::GrabFader {
            target: FaderTarget::Lane(mix.lane),
            at: 100.0,
            travel: TRAVEL,
        },
        Gesture::RideFader { at: 140.0 },
        Gesture::ReleaseFader,
    ]
}

/// The height of a fader's travel, as the panel draws it. A ride is relative to
/// the grab, so the numbers above are pointer positions on that travel.
const TRAVEL: f32 = 132.0;

/// Every command the strips asked for.
pub(super) fn dispatched() -> BTreeSet<&'static str> {
    let mut mix = dressed();
    let watch = Watch::on(&mix.state);
    for gesture in gestures(&mix) {
        let what = gesture.what();
        mixer_strip::apply(&mut mix.state, gesture)
            .unwrap_or_else(|err| panic!("{what} was refused: {err}"));
    }
    watch.take().into_iter().collect()
}

/// Every gesture named by `Gesture::what`, read out of its own match.
///
/// Derived from source rather than listed: the compiler forces a new gesture to
/// have an arm there, so reading the arms makes a gesture nobody drives go red.
fn gestures_in_source() -> BTreeSet<String> {
    let source = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/surfaces/mixer_strip.rs"),
    )
    .expect("the strip registry is readable");
    let body = source
        .split_once("pub fn what(&self)")
        .expect("`Gesture::what` names the gestures")
        .1
        .split_once("\n    }")
        .expect("`Gesture::what` closes")
        .0;
    body.lines()
        .filter_map(|line| line.split_once("=> \""))
        .map(|(_, rest)| {
            rest.split_once('"')
                .expect("a gesture is a quoted string")
                .0
                .to_string()
        })
        .collect()
}

#[test]
fn every_gesture_the_strip_offers_is_driven() {
    let mix = dressed();
    let driven: BTreeSet<String> = gestures(&mix)
        .iter()
        .map(|gesture| gesture.what().to_string())
        .collect();
    let missing: Vec<String> = gestures_in_source()
        .into_iter()
        .filter(|what| !driven.contains(what))
        .collect();
    assert!(
        missing.is_empty(),
        "the strip offers gestures nothing drives: {missing:?}. Each wants a \
         row in `gestures`, at the point in the sequence a pointer would make it"
    );
}

#[test]
fn the_census_reaches_every_routing_command() {
    let sent = dispatched();
    for label in [
        "Add track",
        "Lane output",
        "Add send",
        "Set send",
        "Remove send",
        "Lane fader",
    ] {
        assert!(
            sent.contains(label),
            "driving every strip gesture did not send `{label}`; it sent {sent:?}"
        );
    }
}

/// The rule the drag exists for: riding a fader is one undo entry, whatever the
/// pointer did on the way.
#[test]
fn riding_a_fader_writes_once_and_only_on_release() {
    let mut mix = dressed();
    let before = mix.state.history_entries().len();
    mixer_strip::apply(
        &mut mix.state,
        Gesture::GrabFader {
            target: FaderTarget::Lane(mix.lane),
            at: 20.0,
            travel: TRAVEL,
        },
    )
    .expect("the fader is grabbed");
    for at in [40.0, 60.0, 90.0] {
        mixer_strip::apply(&mut mix.state, Gesture::RideFader { at }).expect("the fader rides");
    }
    assert_eq!(
        mix.state.history_entries().len(),
        before,
        "riding the fader wrote history before the pointer was released"
    );
    let drafted = mix
        .state
        .fader_dragging()
        .expect("a drag is in flight")
        .gain;
    assert!(drafted < 1.0, "riding down is quieter, got {drafted}");

    assert!(
        mixer_strip::apply(&mut mix.state, Gesture::ReleaseFader).expect("the fader is released"),
        "the release that wrote the level reported no change"
    );
    assert_eq!(
        mix.state.history_entries().len(),
        before + 1,
        "the whole ride is one undo entry"
    );
    assert!(mix.state.fader_dragging().is_none());
}

/// A gesture the pointer can make with nothing held: entering the strip
/// mid-drag, or releasing over it. Neither is an edit and neither is an error.
#[test]
fn a_fader_gesture_with_nothing_held_is_inert() {
    let mut mix = dressed();
    let before = mix.state.history_entries().len();
    assert!(
        !mixer_strip::apply(&mut mix.state, Gesture::RideFader { at: 40.0 })
            .expect("a ride with nothing held is not an error"),
        "a ride with nothing held asked for a repaint"
    );
    assert!(
        !mixer_strip::apply(&mut mix.state, Gesture::ReleaseFader)
            .expect("a release with nothing held is not an error"),
        "a release with nothing held asked for a repaint"
    );
    assert_eq!(mix.state.history_entries().len(), before);
}

/// A click on a fader moves nothing, so it must not be an undo entry either.
#[test]
fn a_fader_pressed_and_released_in_place_writes_nothing() {
    let mut mix = dressed();
    let before = mix.state.history_entries().len();
    mixer_strip::apply(
        &mut mix.state,
        Gesture::GrabFader {
            target: FaderTarget::Lane(mix.lane),
            at: 40.0,
            travel: TRAVEL,
        },
    )
    .expect("grab");
    mixer_strip::apply(&mut mix.state, Gesture::ReleaseFader).expect("release");
    assert_eq!(
        mix.state.history_entries().len(),
        before,
        "a click on a fader is not an edit"
    );
}

#[test]
fn a_lane_pointed_at_a_bus_comes_back_to_the_master() {
    let mut mix = dressed();
    mixer_strip::apply(
        &mut mix.state,
        Gesture::PointOutput {
            track: mix.lane,
            to: Some(mix.bus),
        },
    )
    .expect("point the lane at the bus");
    let targets = mix.state.output_targets(mix.lane);
    let live = targets
        .iter()
        .find(|target| target.current)
        .expect("something is live");
    assert_eq!(
        live.to,
        Some(mix.bus.raw()),
        "the lane did not follow the chip that was clicked"
    );

    mixer_strip::apply(
        &mut mix.state,
        Gesture::PointOutput {
            track: mix.lane,
            to: None,
        },
    )
    .expect("point it back at the master");
    let live = mix
        .state
        .output_targets(mix.lane)
        .iter()
        .find(|target| target.current)
        .expect("something is live")
        .to;
    assert_eq!(live, None, "the lane did not come back to the master");
}

/// The chip's three states are the model's cycle, so the census holds the whole
/// round trip rather than the first click.
#[test]
fn a_send_chip_cycles_off_post_pre_and_off_again() {
    let mut mix = dressed();
    let click = |mix: &mut Mix| {
        mixer_strip::apply(
            &mut mix.state,
            Gesture::CycleSend {
                track: mix.lane,
                to: mix.other,
            },
        )
        .expect("the chip is clicked");
    };
    assert!(sends(&mix).is_empty(), "the dressed lane already sends");
    click(&mut mix);
    assert_eq!(sends(&mix).len(), 1, "the first click adds a send");
    assert!(!sends(&mix)[0], "the first click is post-fader");
    click(&mut mix);
    assert!(sends(&mix)[0], "the second click is pre-fader");
    click(&mut mix);
    assert!(sends(&mix).is_empty(), "the third click drops the send");
}

#[test]
fn the_panel_files_every_gesture_through_the_registry() {
    let source = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/panels/mixer.rs"),
    )
    .expect("the mixer panel is readable");
    let filed = source.matches("mixer_strip::apply").count();
    assert_eq!(
        filed, 6,
        "the mixer draws six controls and files {filed} of them through the \
         registry: the bus button, the output chip, the send chip, and the \
         fader's press, move and release. A control that calls `AppState` \
         directly is one the census cannot press"
    );
}

/// Whether each send on the dressed lane is pre-fader, in send order.
fn sends(mix: &Mix) -> Vec<bool> {
    mix.state
        .mixer_strips()
        .expect("the mix reads")
        .iter()
        .find(|strip| strip.track == mix.lane.raw())
        .map(|strip| strip.sends.iter().map(|send| send.pre_fader).collect())
        .unwrap_or_default()
}
