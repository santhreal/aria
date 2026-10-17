//! Where a lane's sound goes, and what refuses to route.
//!
//! Every case here is about the graph, not about arithmetic: the mixer's own
//! sums are proved in `aria-render`. What matters at this level is that the
//! graph is total (every lane in the mix appears exactly once, in feed order),
//! that it is acyclic (both at the setter and against a file that was edited by
//! hand), and that what a removal changes can be put back.

use super::*;
use crate::Rational;
use crate::track::{Track, TrackKind};
use crate::track_catalog::SHIPPED;

const R24: Rational = Rational::FPS_24;

fn timeline() -> Timeline {
    Timeline::new(R24)
}

fn lane(timeline: &mut Timeline, kind: TrackKind, name: &str) -> TrackId {
    timeline.add_track(Track::new(kind, name))
}

fn bus_of(buses: &[AudioBus<'_>], track: TrackId) -> AudioBus<'static> {
    static UNITY: Param<f32> = Param::Constant(1.0);
    static CENTER: Param<f32> = Param::Constant(0.0);
    let found = buses
        .iter()
        .find(|bus| bus.track == track)
        .expect("lane is not in the mix graph");
    AudioBus {
        track: found.track,
        is_bus: found.is_bus,
        gain: found.gain,
        pan: found.pan,
        automation: found.automation,
        volume_envelope: &UNITY,
        pan_envelope: &CENTER,
        output: found.output,
        // The borrow is what makes the graph allocation-free; a test only needs
        // the shape, so the copy carries an empty slice rather than a lifetime.
        sends: &[],
        inserts: &[],
        audible: found.audible,
        reaches_master: found.reaches_master,
    }
}

fn position(buses: &[AudioBus<'_>], track: TrackId) -> usize {
    buses
        .iter()
        .position(|bus| bus.track == track)
        .expect("lane is not in the mix graph")
}

/// Which shipped lane kinds take part in the mix, derived from the rows rather
/// than listed here: a new lane kind joins or stays out by its own columns.
#[test]
fn the_mixer_holds_the_lanes_whose_rows_say_so() {
    let mixing: Vec<&str> = SHIPPED
        .iter()
        .filter(|spec| spec.in_mixer())
        .map(|spec| spec.id)
        .collect();
    assert_eq!(mixing, ["video", "audio", "submix"]);

    let mut timeline = timeline();
    let video = lane(&mut timeline, TrackKind::Video, "V1");
    let audio = lane(&mut timeline, TrackKind::Audio, "A1");
    let text = lane(&mut timeline, TrackKind::Text, "T1");
    let bus = lane(&mut timeline, TrackKind::Submix, "Music");
    let buses = timeline.audio_buses().unwrap();
    let present: Vec<TrackId> = buses.iter().map(|node| node.track).collect();
    assert_eq!(present.len(), 3, "{present:?}");
    assert!(present.contains(&video) && present.contains(&audio) && present.contains(&bus));
    assert!(!present.contains(&text), "a text lane is not in the mix");
}

#[test]
fn a_fresh_lane_goes_straight_to_the_master_at_unity() {
    let mut timeline = timeline();
    let audio = lane(&mut timeline, TrackKind::Audio, "A1");
    let buses = timeline.audio_buses().unwrap();
    let node = bus_of(&buses, audio);
    assert_eq!(node.output, TrackOutput::Master);
    assert_eq!(node.gain, 1.0);
    assert!(node.audible && node.reaches_master);
    assert!(!node.is_bus);
}

/// Feed order is the whole point of `audio_buses`: a mixer folds it front to
/// back with no lookahead, so every bus has to appear after everything that
/// feeds it, however the lanes are stacked.
#[test]
fn a_bus_is_folded_after_everything_that_feeds_it() {
    let mut timeline = timeline();
    let master_bus = lane(&mut timeline, TrackKind::Submix, "Mix");
    let music = lane(&mut timeline, TrackKind::Submix, "Music");
    let a1 = lane(&mut timeline, TrackKind::Audio, "A1");
    let a2 = lane(&mut timeline, TrackKind::Audio, "A2");

    timeline
        .set_track_output(a1, TrackOutput::Bus(music))
        .unwrap();
    timeline
        .set_track_output(a2, TrackOutput::Bus(music))
        .unwrap();
    timeline
        .set_track_output(music, TrackOutput::Bus(master_bus))
        .unwrap();
    timeline.add_send(a1, master_bus, 0.5, true).unwrap();

    let buses = timeline.audio_buses().unwrap();
    assert!(position(&buses, a1) < position(&buses, music));
    assert!(position(&buses, a2) < position(&buses, music));
    assert!(position(&buses, music) < position(&buses, master_bus));
    assert!(position(&buses, a1) < position(&buses, master_bus));
    assert!(buses.iter().all(|node| node.reaches_master));
}

#[test]
fn a_route_that_would_feed_a_lane_back_into_itself_is_refused() {
    let mut timeline = timeline();
    let left = lane(&mut timeline, TrackKind::Submix, "Left");
    let right = lane(&mut timeline, TrackKind::Submix, "Right");
    timeline
        .set_track_output(left, TrackOutput::Bus(right))
        .unwrap();

    assert_eq!(
        timeline.set_track_output(right, TrackOutput::Bus(left)),
        Err(ModelError::RoutingCycle(right))
    );
    assert_eq!(
        timeline.set_track_output(left, TrackOutput::Bus(left)),
        Err(ModelError::RoutingCycle(left))
    );
    // A send closes a loop just as an output does.
    assert_eq!(
        timeline.add_send(right, left, 1.0, false),
        Err(ModelError::RoutingCycle(right))
    );
    // And the refusal changed nothing.
    assert_eq!(
        timeline.track(right).unwrap().output,
        TrackOutput::Master,
        "a refused route was written anyway"
    );
    assert!(timeline.track(right).unwrap().sends.is_empty());
}

/// The setters refuse a cycle, but a project is a file. A hand-edited cycle is
/// reported by name instead of deciding the mixdown by whichever edge happened
/// to be folded first.
#[test]
fn a_cycle_that_arrives_in_a_project_file_is_reported_rather_than_mixed() {
    let mut timeline = timeline();
    let left = lane(&mut timeline, TrackKind::Submix, "Left");
    let right = lane(&mut timeline, TrackKind::Submix, "Right");
    timeline
        .set_track_output(left, TrackOutput::Bus(right))
        .unwrap();
    let json = serde_json::to_string(&timeline).unwrap();
    // Only a file can say this: turn Right's master output into a route back
    // into Left, which is exactly what `set_track_output` refused above.
    let cyclic = json.replace(
        &format!("\"id\":{}", right.raw()),
        &format!(
            "\"id\":{},\"output\":{{\"Bus\":{}}}",
            right.raw(),
            left.raw()
        ),
    );
    let loaded: Timeline = serde_json::from_str(&cyclic).unwrap();
    assert!(matches!(
        loaded.audio_buses(),
        Err(ModelError::RoutingCycle(_))
    ));
}

#[test]
fn every_refusal_names_what_is_wrong() {
    let mut timeline = timeline();
    let audio = lane(&mut timeline, TrackKind::Audio, "A1");
    let text = lane(&mut timeline, TrackKind::Text, "T1");
    let bus = lane(&mut timeline, TrackKind::Submix, "Music");
    let gone = TrackId::next();

    for (case, got, want) in [
        (
            "a lane that carries no sound has nothing to route",
            timeline.set_track_output(text, TrackOutput::Bus(bus)),
            ModelError::LaneCarriesNoSound(text),
        ),
        (
            "only a bus can be routed into",
            timeline.set_track_output(bus, TrackOutput::Bus(audio)),
            ModelError::NotABus(audio),
        ),
        (
            "a destination has to exist",
            timeline.set_track_output(audio, TrackOutput::Bus(gone)),
            ModelError::UnknownTrack(gone),
        ),
        (
            "the lane has to exist",
            timeline.set_track_output(gone, TrackOutput::Master),
            ModelError::UnknownTrack(gone),
        ),
        (
            "a send index has to exist",
            timeline.set_send_level(audio, 0, 1.0),
            ModelError::UnknownSend {
                track: audio,
                index: 0,
            },
        ),
    ] {
        assert_eq!(got, Err(want), "{case}");
    }

    // Levels and faders are refused rather than clamped, so the number beside
    // the fader is always the number in the mix.
    assert!(timeline.add_send(audio, bus, 9.0, false).is_err());
    assert!(timeline.add_send(audio, bus, -0.1, false).is_err());
    assert!(timeline.add_send(audio, bus, f32::NAN, false).is_err());
    assert!(timeline.set_track_gain(audio, 9.0).is_err());
    assert!(timeline.set_track_gain(audio, f32::INFINITY).is_err());
    assert_eq!(timeline.track(audio).unwrap().gain, 1.0);

    // One lane, one send per bus: a second copy is a level, not a route.
    timeline.add_send(audio, bus, 0.5, false).unwrap();
    assert_eq!(
        timeline.add_send(audio, bus, 0.25, false),
        Err(ModelError::DuplicateSend {
            track: audio,
            to: bus
        })
    );
}

#[test]
fn a_send_is_addressed_by_its_index_and_comes_back_the_way_it_left() {
    let mut timeline = timeline();
    let audio = lane(&mut timeline, TrackKind::Audio, "A1");
    let reverb = lane(&mut timeline, TrackKind::Submix, "Reverb");
    let parallel = lane(&mut timeline, TrackKind::Submix, "Parallel");

    assert_eq!(timeline.add_send(audio, reverb, 0.5, true).unwrap(), 0);
    assert_eq!(timeline.add_send(audio, parallel, 1.0, false).unwrap(), 1);
    timeline.set_send_level(audio, 0, 0.25).unwrap();
    timeline.set_send_pre_fader(audio, 1, true).unwrap();
    assert_eq!(
        timeline.track(audio).unwrap().sends,
        vec![
            AudioSend {
                to: reverb,
                level: 0.25,
                pre_fader: true
            },
            AudioSend {
                to: parallel,
                level: 1.0,
                pre_fader: true
            },
        ]
    );

    let dropped = timeline.remove_send(audio, 0).unwrap();
    assert_eq!(dropped.to, reverb);
    assert_eq!(timeline.track(audio).unwrap().sends.len(), 1);
    timeline.insert_send(audio, 0, dropped).unwrap();
    assert_eq!(timeline.track(audio).unwrap().sends[0].to, reverb);
    assert_eq!(timeline.sends_into(reverb), vec![(audio, dropped)]);
    assert_eq!(timeline.outputs_into(reverb), Vec::<TrackId>::new());
}

/// A muted bus is a wall: nothing behind it is heard, so nothing behind it is
/// worth decoding. A send past the wall is still heard.
#[test]
fn a_lane_behind_a_muted_bus_stops_reaching_the_master() {
    let mut timeline = timeline();
    let music = lane(&mut timeline, TrackKind::Submix, "Music");
    let reverb = lane(&mut timeline, TrackKind::Submix, "Reverb");
    let a1 = lane(&mut timeline, TrackKind::Audio, "A1");
    let a2 = lane(&mut timeline, TrackKind::Audio, "A2");
    timeline
        .set_track_output(a1, TrackOutput::Bus(music))
        .unwrap();
    timeline
        .set_track_output(a2, TrackOutput::Bus(music))
        .unwrap();
    timeline.add_send(a1, reverb, 0.5, true).unwrap();
    timeline.track_mut(music).unwrap().muted = true;

    let buses = timeline.audio_buses().unwrap();
    assert!(!bus_of(&buses, music).reaches_master);
    assert!(
        bus_of(&buses, a1).reaches_master,
        "the send to the reverb is still heard"
    );
    assert!(
        !bus_of(&buses, a2).reaches_master,
        "nothing else carries A2 to the master"
    );

    // Muting the lane itself takes it out whatever it is routed into.
    timeline.track_mut(a1).unwrap().muted = true;
    let buses = timeline.audio_buses().unwrap();
    assert!(!bus_of(&buses, a1).audible);
    assert!(!bus_of(&buses, a1).reaches_master);
}

/// Soloing is per lane kind, and a bus is its own kind: soloing one music lane
/// must not silence the bus that lane is routed into.
#[test]
fn soloing_a_clip_lane_leaves_the_buses_alone() {
    let mut timeline = timeline();
    let music = lane(&mut timeline, TrackKind::Submix, "Music");
    let a1 = lane(&mut timeline, TrackKind::Audio, "A1");
    let a2 = lane(&mut timeline, TrackKind::Audio, "A2");
    timeline
        .set_track_output(a1, TrackOutput::Bus(music))
        .unwrap();
    timeline
        .set_track_output(a2, TrackOutput::Bus(music))
        .unwrap();
    timeline.track_mut(a1).unwrap().solo = true;

    let buses = timeline.audio_buses().unwrap();
    assert!(bus_of(&buses, music).reaches_master);
    assert!(bus_of(&buses, a1).reaches_master);
    assert!(!bus_of(&buses, a2).reaches_master);
}

/// Deleting a bus returns its feeders to the master instead of pointing them at
/// a lane that is gone — and one undo puts the whole mix back.
#[test]
fn removing_a_bus_returns_its_feeders_to_the_master_and_an_undo_restores_them() {
    let mut timeline = timeline();
    let music = lane(&mut timeline, TrackKind::Submix, "Music");
    let reverb = lane(&mut timeline, TrackKind::Submix, "Reverb");
    let a1 = lane(&mut timeline, TrackKind::Audio, "A1");
    timeline
        .set_track_output(a1, TrackOutput::Bus(music))
        .unwrap();
    timeline.add_send(a1, music, 0.5, true).unwrap();
    timeline.add_send(a1, reverb, 0.25, false).unwrap();
    let order = timeline
        .order()
        .iter()
        .position(|&id| id == music)
        .expect("the bus is in the stack");

    let removed = timeline.remove_track(music).unwrap();
    assert_eq!(removed.detached.len(), 1);
    let lane_now = timeline.track(a1).unwrap();
    assert_eq!(lane_now.output, TrackOutput::Master);
    assert_eq!(
        lane_now.sends,
        vec![AudioSend {
            to: reverb,
            level: 0.25,
            pre_fader: false
        }],
        "only the sends that pointed at the deleted bus are gone"
    );
    assert!(
        timeline.audio_buses().is_ok(),
        "nothing is left addressing the removed bus"
    );

    timeline.restore_track(removed, order).unwrap();
    let lane_now = timeline.track(a1).unwrap();
    assert_eq!(lane_now.output, TrackOutput::Bus(music));
    assert_eq!(lane_now.sends.len(), 2);
    assert_eq!(lane_now.sends[0].to, music);
    assert_eq!(lane_now.sends[1].to, reverb);
}

/// A bus lane sinks below the audio floor, so the mix reads bottom-up the way a
/// desk does and nothing is ever dragged past it into a lane that holds nothing.
#[test]
fn bus_lanes_sit_below_the_audio_floor() {
    let mut timeline = timeline();
    let video = lane(&mut timeline, TrackKind::Video, "V1");
    let audio = lane(&mut timeline, TrackKind::Audio, "A1");
    let bus = lane(&mut timeline, TrackKind::Submix, "Music");
    let order = timeline.order().to_vec();
    assert_eq!(order, vec![bus, audio, video]);

    // And a move cannot lift it out of its zone.
    timeline.move_track(bus, 2).unwrap();
    assert_eq!(timeline.order()[0], bus);
}

/// A bus holds nothing: no imported media, and no generated content, since no
/// generator row names it.
#[test]
fn nothing_can_be_placed_on_a_bus() {
    let bus = crate::track_kind_spec(TrackKind::Submix);
    assert!(!bus.accepts_media);
    for generator_row in crate::clip::generator_catalog::SHIPPED {
        let content = crate::ClipSource::Generated((generator_row.census)());
        assert!(
            !bus.accepts_content(&content),
            "'{}' is placeable on a bus",
            generator_row.id
        );
    }
}

/// A project that has never been mixed writes no routing keys at all, so the
/// file it saves is the file it loaded.
#[test]
fn routing_is_additive_on_disk() {
    let mut timeline = timeline();
    let audio = lane(&mut timeline, TrackKind::Audio, "A1");
    let json = serde_json::to_string(&timeline).unwrap();
    assert!(!json.contains("\"gain\""), "{json}");
    assert!(!json.contains("\"output\""), "{json}");
    assert!(!json.contains("\"sends\""), "{json}");

    let bus = lane(&mut timeline, TrackKind::Submix, "Music");
    timeline
        .set_track_output(audio, TrackOutput::Bus(bus))
        .unwrap();
    timeline.add_send(audio, bus, 0.5, true).unwrap();
    timeline.set_track_gain(audio, 0.75).unwrap();
    let json = serde_json::to_string(&timeline).unwrap();
    let loaded: Timeline = serde_json::from_str(&json).unwrap();
    let lane_now = loaded.track(audio).unwrap();
    assert_eq!(lane_now.gain, 0.75);
    assert_eq!(lane_now.output, TrackOutput::Bus(bus));
    assert_eq!(
        lane_now.sends,
        vec![AudioSend {
            to: bus,
            level: 0.5,
            pre_fader: true
        }]
    );
}

/// An insert on `holder` keyed from `key`, bypassed or not. Written straight
/// to the lane: the graph reads the field, and the setter is proved apart.
fn keyed(timeline: &mut Timeline, holder: Option<TrackId>, key: TrackId, bypass: bool) {
    let mut insert = AudioEffectInstance::new("example.ducker");
    insert.sidechain = Some(key);
    insert.bypass = bypass;
    match holder {
        Some(lane) => timeline.track_mut(lane).unwrap().audio_effects.push(insert),
        None => timeline.master_audio_effects_mut().push(insert),
    }
}

/// A key is read after the key lane's inserts, so the key lane is mixed first
/// whichever way the two lanes are stacked.
#[test]
fn a_key_lane_is_mixed_before_the_lane_it_keys() {
    for flip in [false, true] {
        let mut timeline = timeline();
        let first = lane(&mut timeline, TrackKind::Audio, "A1");
        let second = lane(&mut timeline, TrackKind::Audio, "A2");
        let (key, holder) = if flip {
            (first, second)
        } else {
            (second, first)
        };
        keyed(&mut timeline, Some(holder), key, false);
        let buses = timeline.audio_buses().unwrap();
        assert!(
            position(&buses, key) < position(&buses, holder),
            "flip {flip}: key at {}, holder at {}",
            position(&buses, key),
            position(&buses, holder)
        );
        let node = buses.iter().find(|bus| bus.track == holder).unwrap();
        assert_eq!(node.keys().collect::<Vec<_>>(), [key]);
    }
}

#[test]
fn a_key_that_would_close_a_loop_is_refused_and_a_filed_one_is_reported() {
    let mut timeline = timeline();
    let voice = lane(&mut timeline, TrackKind::Audio, "Voice");
    let bus = lane(&mut timeline, TrackKind::Submix, "Mix");
    timeline
        .set_track_output(voice, TrackOutput::Bus(bus))
        .unwrap();

    // The bus is downstream of the voice, so it cannot key the voice.
    assert_eq!(
        timeline.sidechain_refusal(bus, AudioInsertTarget::Track(voice)),
        Err(ModelError::RoutingCycle(bus))
    );
    // A lane does not key itself.
    assert_eq!(
        timeline.sidechain_refusal(voice, AudioInsertTarget::Track(voice)),
        Err(ModelError::RoutingCycle(voice))
    );
    // The other way round is a sidechain like any other.
    assert_eq!(
        timeline.sidechain_refusal(voice, AudioInsertTarget::Track(bus)),
        Ok(())
    );

    // A key already in place is an edge a route has to respect: with the bus
    // keyed from a lane, that lane cannot be routed out of the bus.
    let music = lane(&mut timeline, TrackKind::Audio, "Music");
    let room = lane(&mut timeline, TrackKind::Submix, "Room");
    keyed(&mut timeline, Some(music), room, false);
    assert_eq!(
        timeline.set_track_output(music, TrackOutput::Bus(room)),
        Err(ModelError::RoutingCycle(music))
    );
    assert_eq!(
        timeline.add_send(music, room, 1.0, true),
        Err(ModelError::RoutingCycle(music))
    );

    // A file can hold a lane keyed by itself; the graph reports it.
    keyed(&mut timeline, Some(voice), voice, false);
    assert!(matches!(
        timeline.audio_buses(),
        Err(ModelError::RoutingCycle(_))
    ));
}

#[test]
fn a_key_comes_from_a_lane_with_sound_and_lands_on_a_lane_or_the_master() {
    let mut timeline = timeline();
    let voice = lane(&mut timeline, TrackKind::Audio, "Voice");
    let text = lane(&mut timeline, TrackKind::Text, "T1");
    let gone = TrackId::next();
    assert_eq!(
        timeline.sidechain_refusal(text, AudioInsertTarget::Master),
        Err(ModelError::LaneCarriesNoSound(text))
    );
    assert_eq!(
        timeline.sidechain_refusal(gone, AudioInsertTarget::Master),
        Err(ModelError::UnknownTrack(gone))
    );
    assert!(matches!(
        timeline.sidechain_refusal(voice, AudioInsertTarget::Clip(crate::ClipId::from_raw(1))),
        Err(ModelError::InvalidParam(_))
    ));
    assert_eq!(
        timeline.sidechain_refusal(voice, AudioInsertTarget::Master),
        Ok(())
    );
}

/// A key naming a lane that is gone reads silence: it adds no edge and no
/// error, and the insert keeps it for an undo to bring the lane back to.
#[test]
fn a_key_to_a_lane_that_is_gone_leaves_the_graph_whole() {
    let mut timeline = timeline();
    let music = lane(&mut timeline, TrackKind::Audio, "Music");
    let gone = TrackId::next();
    keyed(&mut timeline, Some(music), gone, false);
    keyed(&mut timeline, None, gone, false);
    let buses = timeline.audio_buses().unwrap();
    assert_eq!(buses.len(), 1);
    assert!(buses[0].reaches_master);
    assert_eq!(
        timeline.track(music).unwrap().audio_effects[0].sidechain,
        Some(gone)
    );
}

/// A lane whose only path to the master is as a key is still decoded, unless
/// the insert that reads it is bypassed or the lane is muted.
#[test]
fn a_lane_heard_only_as_a_key_still_reaches_the_master() {
    for (case, holder_is_master, bypass, mute_voice, reaches) in [
        ("keys a master insert", true, false, false, true),
        ("keys an insert on a lane", false, false, false, true),
        ("keys a bypassed master insert", true, true, false, false),
        ("keys a bypassed lane insert", false, true, false, false),
        ("is muted", true, false, true, false),
    ] {
        let mut timeline = timeline();
        let voice = lane(&mut timeline, TrackKind::Audio, "Voice");
        let music = lane(&mut timeline, TrackKind::Audio, "Music");
        let dead = lane(&mut timeline, TrackKind::Submix, "Dead");
        timeline
            .set_track_output(voice, TrackOutput::Bus(dead))
            .unwrap();
        timeline.track_mut(dead).unwrap().muted = true;
        timeline.track_mut(voice).unwrap().muted = mute_voice;
        let holder = (!holder_is_master).then_some(music);
        keyed(&mut timeline, holder, voice, bypass);
        let buses = timeline.audio_buses().unwrap();
        assert_eq!(bus_of(&buses, voice).reaches_master, reaches, "{case}");
        assert!(bus_of(&buses, music).reaches_master, "{case}");
    }
}

#[test]
fn insert_chains_are_additive_on_disk_and_round_trip() {
    let mut timeline = timeline();
    let voice = lane(&mut timeline, TrackKind::Audio, "Voice");
    let music = lane(&mut timeline, TrackKind::Audio, "Music");
    let json = serde_json::to_string(&timeline).unwrap();
    assert!(!json.contains("audio_effects"), "{json}");

    keyed(&mut timeline, Some(music), voice, false);
    keyed(&mut timeline, None, voice, true);
    let json = serde_json::to_string(&timeline).unwrap();
    let loaded: Timeline = serde_json::from_str(&json).unwrap();
    assert_eq!(
        loaded.track(music).unwrap().audio_effects,
        timeline.track(music).unwrap().audio_effects
    );
    assert_eq!(
        loaded.master_audio_effects(),
        timeline.master_audio_effects()
    );
}

#[path = "track_automation_tests.rs"]
mod automation_tests;
