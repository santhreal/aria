//! Contracts for mixer strip pan, automation modes, volume/pan envelopes, and
//! solo-in-place across source lanes and submix buses.
//!
//! WHY: a mixer strip has two gain/pan sources — the static fader/knob and the
//! sequence-tick automation curve — gated by [`AutomationMode`]. The class
//! closed here is a strip whose `Off` mode still plays its curve, whose `Read`
//! mode forgets the static fader before any keyframes exist, whose curves slide
//! when the sequence frame rate changes, or whose solo-in-place leaves an
//! unsoloed video lane's embedded audio or an unsoloed direct-to-master lane
//! bleeding into the mix.

use super::*;
use crate::{AutomationMode, Easing, Keyframe, Param, Rational};

#[test]
fn pan_and_automation_envelopes_validate_and_round_trip() {
    let mut timeline = timeline();
    let audio = lane(&mut timeline, TrackKind::Audio, "A1");
    let text = lane(&mut timeline, TrackKind::Text, "T1");

    // Non-mixer lanes refuse pan and automation.
    assert!(matches!(
        timeline.set_track_pan(text, -0.5),
        Err(ModelError::LaneCarriesNoSound(id)) if id == text
    ));
    for bad in [f32::NAN, f32::INFINITY, -1.01, 1.01] {
        assert!(timeline.set_track_pan(audio, bad).is_err(), "{bad}");
    }

    timeline.set_track_pan(audio, -0.75).unwrap();
    assert_eq!(timeline.track(audio).unwrap().pan, -0.75);
    assert_eq!(timeline.track(audio).unwrap().pan_at(12), -0.75);

    let mut vol = Param::Constant(1.0);
    vol.set_keyframe(0, 1.0, Easing::Linear);
    vol.set_keyframe(24, 0.25, Easing::Linear);
    let mut pan = Param::Constant(0.0);
    pan.set_keyframe(0, -1.0, Easing::Linear);
    pan.set_keyframe(24, 1.0, Easing::Linear);

    timeline
        .set_track_automation(audio, AutomationMode::Read, vol.clone(), pan.clone())
        .unwrap();
    let track = timeline.track(audio).unwrap();
    assert!((track.gain_at(12) - 0.625).abs() < 1e-6);
    assert!(track.pan_at(12).abs() < 1e-6);

    // Switching to Off bypasses the envelopes without discarding them.
    timeline
        .set_track_automation(audio, AutomationMode::Off, vol, pan)
        .unwrap();
    let track = timeline.track(audio).unwrap();
    assert_eq!(track.gain_at(12), 1.0);
    assert_eq!(track.pan_at(12), -0.75);
}

/// Every strip curve, lane and master, rejects spatial tangents and a
/// keyframe outside its range, states its name in the error, and keeps the
/// previous curve.
#[test]
fn strip_envelopes_refuse_tangents_and_out_of_range_keyframes() {
    use crate::SpatialTangents;

    let keyed = |value: f32| {
        let mut curve = Param::Constant(0.5);
        curve.set_keyframe(0, 0.5, Easing::Linear);
        curve.set_keyframe(24, value, Easing::Linear);
        curve
    };
    let bent = || {
        let mut curve = keyed(0.5);
        curve
            .set_keyframe_tangents(24, Some(SpatialTangents::ZERO))
            .unwrap();
        curve
    };
    let tangents =
        |what: &str| format!("{what} is a scalar curve and cannot carry spatial tangents");
    let flat = || Param::Constant(0.5);
    // (what, volume curve, pan curve, expected refusal)
    let cases = [
        (
            "lane volume envelope",
            bent(),
            flat(),
            tangents("lane volume envelope"),
        ),
        (
            "lane pan envelope",
            flat(),
            bent(),
            tangents("lane pan envelope"),
        ),
        (
            "lane volume envelope",
            keyed(4.5),
            flat(),
            "lane volume envelope 4.5 is outside 0..=4".to_string(),
        ),
        (
            "lane volume envelope",
            keyed(-0.25),
            flat(),
            "lane volume envelope -0.25 is outside 0..=4".to_string(),
        ),
        (
            "lane pan envelope",
            flat(),
            keyed(1.5),
            "lane pan envelope 1.5 is outside -1..=1".to_string(),
        ),
        (
            "master volume envelope",
            bent(),
            flat(),
            tangents("master volume envelope"),
        ),
        (
            "master pan envelope",
            flat(),
            bent(),
            tangents("master pan envelope"),
        ),
        (
            "master volume envelope",
            keyed(4.5),
            flat(),
            "master volume envelope 4.5 is outside 0..=4".to_string(),
        ),
        (
            "master pan envelope",
            flat(),
            keyed(-1.5),
            "master pan envelope -1.5 is outside -1..=1".to_string(),
        ),
    ];
    for (what, volume, pan, expected) in cases {
        let mut timeline = timeline();
        let audio = lane(&mut timeline, TrackKind::Audio, "A1");
        let result = if what.starts_with("lane") {
            timeline.set_track_automation(audio, AutomationMode::Read, volume, pan)
        } else {
            timeline.set_master_mix(1.0, 0.0, false, AutomationMode::Read, volume, pan)
        };
        assert!(
            matches!(&result, Err(ModelError::InvalidParam(msg)) if *msg == expected),
            "{what}: {result:?}, expected {expected}"
        );
        assert!(
            timeline
                .track(audio)
                .unwrap()
                .volume_envelope
                .keyframes()
                .is_empty()
        );
        assert!(timeline.master_volume_envelope().keyframes().is_empty());
    }

    let mut timeline = timeline();
    let audio = lane(&mut timeline, TrackKind::Audio, "A1");
    timeline
        .set_track_automation(audio, AutomationMode::Read, keyed(4.0), keyed(-1.0))
        .unwrap();
    timeline
        .set_master_mix(
            1.0,
            0.0,
            false,
            AutomationMode::Read,
            keyed(0.0),
            keyed(1.0),
        )
        .unwrap();
}

#[test]
fn master_mix_and_resample_preserve_strip_curves() {
    let mut timeline = timeline();
    let audio = lane(&mut timeline, TrackKind::Audio, "A1");

    let mut lane_vol = Param::Constant(1.0);
    lane_vol.set_keyframe(24, 0.5, Easing::Linear);
    let mut lane_pan = Param::Constant(0.0);
    lane_pan.set_keyframe(24, 0.5, Easing::Linear);
    timeline
        .set_track_automation(audio, AutomationMode::Touch, lane_vol, lane_pan)
        .unwrap();

    let mut master_vol = Param::Constant(1.0);
    master_vol.set_keyframe(0, 1.0, Easing::Linear);
    master_vol.set_keyframe(24, 0.5, Easing::Linear);
    let mut master_pan = Param::Constant(0.0);
    master_pan.set_keyframe(24, -0.5, Easing::Linear);
    timeline
        .set_master_mix(
            0.8,
            0.25,
            false,
            AutomationMode::Latch,
            master_vol,
            master_pan,
        )
        .unwrap();
    assert!((timeline.master_gain_at(12) - 0.75).abs() < 1e-6);
    assert_eq!(timeline.master_pan_at(24), -0.5);

    timeline
        .resample_frame_rate(Rational::new(48, 1))
        .expect("resample 24 -> 48");
    assert_eq!(
        timeline.track(audio).unwrap().volume_envelope.keyframes()[0].tick,
        48
    );
    assert_eq!(
        timeline.track(audio).unwrap().pan_envelope.keyframes()[0].tick,
        48
    );
    assert_eq!(
        timeline.master_volume_envelope().keyframes(),
        &[
            Keyframe::new(0, 1.0, Easing::Linear),
            Keyframe::new(48, 0.5, Easing::Linear),
        ]
    );
    assert_eq!(timeline.master_pan_envelope().keyframes()[0].tick, 48);
}

#[test]
fn solo_in_place_silences_unsoloed_video_audio_and_direct_lanes_when_bus_soloed() {
    let mut timeline = timeline();
    let bus = lane(&mut timeline, TrackKind::Submix, "Bus 1");
    let a1 = lane(&mut timeline, TrackKind::Audio, "A1");
    let a2 = lane(&mut timeline, TrackKind::Audio, "A2");
    let v1 = lane(&mut timeline, TrackKind::Video, "V1");
    timeline
        .set_track_output(a1, TrackOutput::Bus(bus))
        .unwrap();

    // Soloing an audio lane silences unsoloed audio and video-embedded-audio
    // lanes in the mix while leaving the video lane visible in the composite.
    timeline.track_mut(a1).unwrap().solo = true;
    let buses = timeline.audio_buses().unwrap();
    assert!(bus_of(&buses, a1).reaches_master);
    assert!(bus_of(&buses, bus).reaches_master);
    assert!(!bus_of(&buses, a2).reaches_master);
    assert!(!bus_of(&buses, v1).reaches_master);
    assert!(timeline.track_is_visible(timeline.track(v1).unwrap()));

    // Soloing a submix bus silences unsoloed lanes that bypass it to Master,
    // while keeping unmuted lanes that feed the soloed bus in the mix.
    timeline.track_mut(a1).unwrap().solo = false;
    timeline.track_mut(bus).unwrap().solo = true;
    let buses = timeline.audio_buses().unwrap();
    assert!(bus_of(&buses, bus).reaches_master);
    assert!(bus_of(&buses, a1).reaches_master);
    assert!(!bus_of(&buses, a2).reaches_master);
    assert!(!bus_of(&buses, v1).reaches_master);
}
