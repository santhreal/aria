//! Where a trim leaves each edge of a clip's source window.
//!
//! # Why
//!
//! Closes the class where a trim moves the source frame of the edge it
//! leaves in place. A sequence tick is a fraction of a frame of a take at
//! another rate, and rounding the head's offset and the new length apart
//! moved a kept tail by a frame: a take that ends where its media ends
//! refused a head trim by an odd tick as out of bounds, and with it an
//! overwrite drag that covered its head. Every inward trim of either edge,
//! by every tick, holds the other edge's source frame and lands inside the
//! old window, and the edge brought back restores the window. A head trimmed
//! to a tick starts the window where a split there starts its right half, so
//! the moved edge lands on the frame the clip showed there, and a tail
//! trimmed in by some ticks drops as many frames as the head trimmed in by
//! as many. The cases cover each pair of `Rational::FPS_*` sequence and take
//! rates, a 48 kHz take, nested sequences, double and half speed, and
//! reversal.
//!
//! Not covered: speed ramps; a rate outside the listed constants; a head at
//! a tick a split refuses; a window that rounds to under one frame, which
//! keeps the one frame beside the edge that stayed and so cannot restore the
//! window it came from.

use super::*;

/// Every `Rational::FPS_*` rate.
const RATES: [Rational; 8] = [
    Rational::FPS_23_976,
    Rational::FPS_24,
    Rational::FPS_25,
    Rational::FPS_29_97,
    Rational::FPS_30,
    Rational::FPS_50,
    Rational::FPS_59_94,
    Rational::FPS_60,
];

/// A take's sample rate.
const AUDIO: Rational = Rational::new(48_000, 1);

/// The speeds a clip plays at, forward and reversed.
const PLAYS: [(Rational, bool); 6] = [
    (Rational::new(1, 1), false),
    (Rational::new(1, 1), true),
    (Rational::new(2, 1), false),
    (Rational::new(2, 1), true),
    (Rational::new(1, 2), false),
    (Rational::new(1, 2), true),
];

/// One second at `rate`, in its ticks.
fn second(rate: Rational) -> i64 {
    crate::time::resample(RationalTime::new(1, Rational::new(1, 1)), rate).value
}

/// Trim each edge of `clip` inward by every tick, and hold each result
/// against the window the clip had and, for the head, against a split at the
/// new head.
fn holds_each_inward_trim(project: &Project, clip: ClipId, case: &str) {
    let before = project.clip(clip).unwrap();
    let (timeline, window, reversed) = (
        before.timeline,
        before.source_range().unwrap(),
        before.reversed,
    );
    let rate = timeline.start.rate;
    for by in 1..timeline.duration.value {
        let mut dropped = Vec::with_capacity(2);
        for head in [true, false] {
            let start = timeline.start.value + if head { by } else { 0 };
            let range = TimeRange::at_rate(start, timeline.duration.value - by, rate);
            let edge = if head { "head" } else { "tail" };
            let case = format!("{case}, {edge} in by {by}");
            let mut trial = project.clone();
            if let Err(err) = trial.trim_clip(clip, range) {
                panic!("{case}: refused with {err}");
            }
            let got = trial.clip(clip).unwrap().source_range().unwrap();
            assert!(
                window.start.value <= got.start.value && got.end_tick() <= window.end_tick(),
                "{case}: {got:?} leaves {window:?}"
            );
            // A reversed clip shows its window's end at its head.
            if head != reversed {
                assert_eq!(got.end_tick(), window.end_tick(), "{case}: the end moved");
            } else {
                assert_eq!(
                    got.start.value, window.start.value,
                    "{case}: the start moved"
                );
            }
            if head {
                let mut cut = project.clone();
                if let Ok(right) = cut.split_clip(clip, RationalTime::new(start, rate)) {
                    assert_eq!(
                        cut.clip(right).unwrap().source_range(),
                        Some(got),
                        "{case}: the head left where a split there starts"
                    );
                }
            }
            if got.duration.value > 1 {
                dropped.push(window.duration.value - got.duration.value);
                if let Err(err) = trial.trim_clip(clip, timeline) {
                    panic!("{case}: back out refused with {err}");
                }
                assert_eq!(
                    trial.clip(clip).unwrap().source_range(),
                    Some(window),
                    "{case}: back out"
                );
            }
        }
        if let [head, tail] = dropped[..] {
            assert_eq!(head, tail, "{case}: in by {by}, the edges drop unlike");
        }
    }
}

#[test]
fn an_inward_trim_of_a_whole_take_keeps_the_edge_it_leaves() {
    for sequence in RATES {
        for take in RATES.into_iter().chain([AUDIO]) {
            for (speed, reversed) in PLAYS {
                let mut project = Project::new("test", sequence);
                let media = project.add_media(sample_media(take, second(take)));
                let track = project.add_track(TrackKind::Video, "V1");
                let clip = project
                    .add_clip(
                        track,
                        media,
                        TimeRange::at_rate(0, second(take), take),
                        RationalTime::new(0, sequence),
                    )
                    .unwrap();
                project.set_clip_speed(clip, speed, reversed).unwrap();
                holds_each_inward_trim(
                    &project,
                    clip,
                    &format!("{sequence:?} sequence, {take:?} take, {speed:?} {reversed}"),
                );
            }
        }
    }
}

#[test]
fn an_inward_trim_of_a_nested_sequence_keeps_the_edge_it_leaves() {
    for sequence in RATES {
        for nested in RATES {
            for (speed, reversed) in PLAYS {
                let mut project = Project::new("test", sequence);
                let track = project.add_track(TrackKind::Video, "V1");
                let nest = project.create_sequence("Nest", nested);
                let clip = project
                    .add_sequence_clip(
                        track,
                        nest,
                        TimeRange::at_rate(0, second(nested), nested),
                        RationalTime::new(0, sequence),
                    )
                    .unwrap();
                project.set_clip_speed(clip, speed, reversed).unwrap();
                holds_each_inward_trim(
                    &project,
                    clip,
                    &format!("{sequence:?} sequence, {nested:?} nest, {speed:?} {reversed}"),
                );
            }
        }
    }
}
