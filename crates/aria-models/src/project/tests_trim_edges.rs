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

#[test]
fn an_inward_trim_of_a_multicam_clip_keeps_the_edge_it_leaves() {
    for sequence in RATES {
        for (speed, reversed) in PLAYS {
            let mut project = Project::new("test", sequence);
            let m1 = project.add_media(sample_media(sequence, second(sequence) * 2));
            let m2 = project.add_media(sample_media(sequence, second(sequence) * 2));
            let group = project
                .create_multicam_group(
                    "Rig",
                    vec![
                        crate::multicam::MulticamAngle::new(m1, 0),
                        crate::multicam::MulticamAngle::new(m2, 6),
                    ],
                )
                .unwrap();
            let track = project.add_track(TrackKind::Video, "V1");
            let clip = project
                .add_multicam_clip(
                    track,
                    group,
                    TimeRange::at_rate(0, second(sequence), sequence),
                    RationalTime::new(second(sequence), sequence),
                )
                .unwrap();
            project.set_clip_speed(clip, speed, reversed).unwrap();
            holds_each_inward_trim(
                &project,
                clip,
                &format!("{sequence:?} multicam, {speed:?} {reversed}"),
            );

            let tl = project.clip(clip).unwrap().timeline;
            let before_zero = if reversed {
                TimeRange::at_rate(
                    tl.start.value,
                    tl.duration.value + second(sequence),
                    sequence,
                )
            } else {
                TimeRange::at_rate(0, tl.end_tick(), sequence)
            };
            assert_eq!(
                project.trim_clip(clip, before_zero),
                Err(ModelError::SourceOutOfBounds),
                "{sequence:?} multicam {speed:?} {reversed} trimmed before source 0"
            );
        }
    }
}

/// The kind of source `clip` plays, with its window replaced by `to` of it.
/// A new kind of source fails to compile here until the test below places it.
fn window_of(clip: &mut Clip, to: impl Fn(TimeRange) -> TimeRange) -> &'static str {
    match &mut clip.content {
        ClipSource::Media { source, .. } => {
            *source = to(*source);
            "media"
        }
        ClipSource::Multicam { source, .. } => {
            *source = to(*source);
            "multicam"
        }
        ClipSource::Sequence { source, .. } => {
            *source = to(*source);
            "sequence"
        }
        ClipSource::Generated(_) => "generated",
    }
}

/// Closes the class where a trim saturated each edge of a source window at
/// the last tick a time holds and kept one frame past a start that sat
/// there: a window ending past the last tick, which a project file does not
/// open with. Each kind of source, a still, a freeze frame, forward and
/// reversed, has its window moved to end at the last tick and is trimmed
/// right and longer by up to 2^40 ticks through [`Project::trim_source`],
/// the window every trim takes: it is refused, or its window ends at a tick
/// a time holds. Does not catch a window that ends on a tick but past a
/// nested sequence or angle, which has no take length to hold it to.
#[test]
fn a_trim_past_a_window_at_the_last_tick_is_refused_or_ends_on_one() {
    let mut project = Project::new("test", R24);
    let video = project.add_media(sample_media(R24, 100));
    let angle = project.add_media(sample_media(R24, 100));
    let still = project.add_media(MediaSource::image("still.png", 800, 600));
    let still_window = project.media(still).unwrap().full_range();
    let v1 = project.add_track(TrackKind::Video, "V1");
    let fx = project.add_track(TrackKind::Adjustment, "FX");
    let nest = project.create_sequence("Nest", R24);
    let group = project
        .create_multicam_group(
            "Rig",
            vec![
                crate::multicam::MulticamAngle::new(video, 0),
                crate::multicam::MulticamAngle::new(angle, 6),
            ],
        )
        .unwrap();
    let clips = [
        project.add_clip(v1, video, tr(0, 10), rt(0)).unwrap(),
        project.add_clip(v1, still, still_window, rt(20)).unwrap(),
        project
            .add_sequence_clip(v1, nest, tr(0, 10), rt(200))
            .unwrap(),
        project
            .add_multicam_clip(v1, group, tr(0, 10), rt(300))
            .unwrap(),
        project
            .add_generated(fx, Generator::Adjustment, tr(0, 10))
            .unwrap(),
    ];
    let mut kinds = Vec::new();
    for id in clips {
        let placed = project.clip(id).unwrap().clone();
        for (freeze, reversed) in [(false, false), (false, true), (true, false)] {
            let mut clip = placed.clone();
            clip.freeze_frame = freeze;
            clip.reversed = reversed;
            let kind = window_of(&mut clip, |w| {
                TimeRange::at_rate(i64::MAX - 10, 10, w.start.rate)
            });
            kinds.push(kind);
            let (start, length) = (clip.timeline.start.value, clip.timeline.duration.value);
            for shift in [1, 10, 100, 1 << 40] {
                for new in [tr(start + shift, length), tr(start, length + shift)] {
                    match project.trim_source(&clip, new) {
                        Ok(Some(window)) => assert!(
                            window.end().is_ok(),
                            "{kind} freeze {freeze} reversed {reversed} trimmed to {new:?} \
                             plays {window:?}"
                        ),
                        Ok(None) => assert_eq!(kind, "generated"),
                        Err(_) => {}
                    }
                }
            }
        }
    }
    kinds.sort_unstable();
    kinds.dedup();
    assert_eq!(kinds, ["generated", "media", "multicam", "sequence"]);
}
