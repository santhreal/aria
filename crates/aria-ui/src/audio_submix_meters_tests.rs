//! What a strip's meter reads: its own lane's post-fader level, on the repaint
//! clock, and silence for a lane the mix is not carrying.
//!
//! The class these close is the one a meter is most likely to ship with: every
//! bar showing the same number. The master peak is already published, so a strip
//! that read it would draw a plausible, moving, wrong level. Each case here
//! publishes a different level per lane and reads them back one strip at a time,
//! so a reading that came from the master, from a neighbour, or from a lane that
//! is not in the mix goes red.
//!
//! What they do not cover: the fold that produces the numbers. That is
//! `aria-render`'s (`export_audio::submix`), where a known signal is driven
//! through two lanes into one bus and each lane's reading is asserted exactly.

use super::*;
use crate::app::meter::MeterReading;
use std::time::{Duration, Instant};

#[test]
fn a_strip_reads_its_own_lane_and_not_the_master() {
    let (mut state, lane, buses) = with_lanes(1);
    let meters = state.audio.arm_lane_meters([lane, buses[0]].into_iter());
    // A hot master, so a strip that read the output would draw full scale.
    state.audio.publish_block(&[1.0, 1.0], 2);
    meters.publish([(lane, [0.2, 0.1]), (buses[0], [0.5, 0.4])].into_iter());
    assert!(state.meter_tick(Instant::now()));
    assert_eq!(state.strip_meter(lane), [0.2, 0.1]);
    assert_eq!(state.strip_meter(buses[0]), [0.5, 0.4]);
    assert_eq!(
        state.meter(),
        MeterReading {
            peaks: [1.0, 1.0],
            clipped: true,
        },
        "the master keeps its own reading"
    );
}

#[test]
fn a_lane_that_is_not_in_the_mix_reads_silence() {
    let (mut state, lane, buses) = with_lanes(1);
    // Both lanes metered, and both bars up.
    let both = state.audio.arm_lane_meters([lane, buses[0]].into_iter());
    both.publish([(lane, [0.7, 0.7]), (buses[0], [0.9, 0.9])].into_iter());
    let now = Instant::now();
    state.meter_tick(now);
    assert_eq!(state.strip_meter(buses[0]), [0.9, 0.9]);

    // The mix is rebuilt without the bus — muted, or routed to nothing. Its
    // strip is still drawn, and it must empty rather than borrow the level of
    // whatever lane is still playing.
    let mixed = state.audio.arm_lane_meters([lane].into_iter());
    mixed.publish([(lane, [0.7, 0.7]), (buses[0], [0.9, 0.9])].into_iter());
    assert_eq!(
        mixed.peak(buses[0]),
        [0.0, 0.0],
        "an unarmed lane takes no level, not even one published at it"
    );
    state.meter_tick(now + Duration::from_secs(2));
    assert_eq!(state.strip_meter(lane), [0.7, 0.7]);
    assert_eq!(state.strip_meter(buses[0]), [0.0, 0.0]);
}

#[test]
fn a_strip_bar_falls_on_the_repaint_clock() {
    let (mut state, lane, _buses) = with_lanes(0);
    let now = Instant::now();
    let meters = state.audio.arm_lane_meters([lane].into_iter());
    meters.publish([(lane, [1.0, 1.0])].into_iter());
    assert!(state.meter_tick(now), "a loud lane is a repaint");
    assert_eq!(state.strip_meter(lane), [1.0, 1.0], "a rise is immediate");
    meters.publish([(lane, [0.0, 0.0])].into_iter());
    assert!(
        state.meter_tick(now + Duration::from_millis(100)),
        "a falling bar is a repaint"
    );
    let mid = state.strip_meter(lane)[0];
    assert!(mid > 0.0 && mid < 1.0, "held at {mid}");
    state.meter_tick(now + Duration::from_secs(2));
    assert_eq!(state.strip_meter(lane), [0.0, 0.0]);
    assert!(
        !state.meter_tick(now + Duration::from_secs(3)),
        "a strip at rest asks for no repaint"
    );
}

#[test]
fn stopping_playback_drops_every_strip_to_silence() {
    let (mut state, lane, buses) = with_lanes(1);
    let meters = state.audio.arm_lane_meters([lane, buses[0]].into_iter());
    meters.publish([(lane, [0.6, 0.6]), (buses[0], [0.8, 0.8])].into_iter());
    let now = Instant::now();
    state.meter_tick(now);
    state.audio.publish_stop();
    // Far enough past the stop that the fall has run out: a stopped stream
    // leaves no bars standing on any strip.
    state.meter_tick(now + Duration::from_secs(2));
    assert_eq!(state.strip_meter(lane), [0.0, 0.0]);
    assert_eq!(state.strip_meter(buses[0]), [0.0, 0.0]);
    // The table is dropped with the stream, so the next play arms a fresh one
    // rather than inheriting the last project's lanes.
    assert_eq!(state.audio.lane_meters().len(), 0);
}

#[test]
fn a_lane_the_mix_started_carrying_meters_without_a_restart() {
    let (mut state, lane, buses) = with_lanes(1);
    let meters = state.audio.arm_lane_meters([lane, buses[0]].into_iter());
    let now = Instant::now();
    state.meter_tick(now);
    assert_eq!(state.strip_meter(buses[0]), [0.0, 0.0]);
    meters.publish([(buses[0], [0.45, 0.45])].into_iter());
    assert!(
        state.meter_tick(now + Duration::from_millis(16)),
        "a strip that started moving is a repaint even when the master did not"
    );
    assert_eq!(state.strip_meter(buses[0]), [0.45, 0.45]);
    assert_eq!(
        state.strip_meter(lane),
        [0.0, 0.0],
        "the neighbour is quiet"
    );
}
