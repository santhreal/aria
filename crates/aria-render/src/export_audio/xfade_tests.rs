//! Audio crossfades at cuts: each side plays into its source handle across the
//! transition window and ramps on the transition's curve.
//!
//! Closes the class "a crossfade that clicks or dips": the two ramps are
//! checked against the curve sample by sample across the whole window for
//! every [`AudioTransition`], the handle reads are checked against the source
//! position they should land on in both play directions, and a side with too
//! little handle is checked to reach silence where its media ends. Not caught:
//! how a crossfade sounds against material whose two sides are partly
//! correlated, which no curve choice makes exact.

use super::*;
use crate::audio_dsp::SpanWarp;
use aria_core::AudioReader;
use aria_models::{
    AudioTransition, MediaSource, Project, Rational, RationalTime, TimeRange, TrackKind,
};

/// Divides a source position into a sample value inside full scale, so the
/// output says which source sample was read.
const SCALE: f32 = 4096.0;

/// A source whose every sample is its own position over [`SCALE`].
struct PositionReader {
    pos: i64,
}

impl AudioReader for PositionReader {
    fn read(&mut self, out: &mut [f32]) -> Result<usize, DecodeError> {
        let frames = out.len() / CHANNELS;
        for frame in 0..frames {
            let sample = (self.pos + frame as i64) as f32 / SCALE;
            out[frame * CHANNELS] = sample;
            out[frame * CHANNELS + 1] = sample;
        }
        self.pos += frames as i64;
        Ok(frames)
    }

    fn seek_to_frame(&mut self, frame: i64) -> Result<(), DecodeError> {
        self.pos = frame;
        Ok(())
    }

    fn position(&self) -> Option<i64> {
        Some(self.pos)
    }
}

fn span(start: i64, end: i64, source_start: i64, reversed: bool, warp: SpanWarp) -> Span {
    Span {
        path: PathBuf::from("/dev/null"),
        start,
        end,
        source_start,
        source_len: end - start,
        reversed,
        warp,
        preserve_pitch: false,
        denoise: false,
        volume: Param::Constant(1.0),
        pan: Param::Constant(0.0),
        fade_in: 0,
        fade_out: 0,
        crossfade_in: None,
        crossfade_out: None,
        reader: Some(Box::new(PositionReader { pos: 0 })),
        exhausted: false,
        bus: 0,
    }
}

/// The cut sits at 100; the window is 80..120.
const CUT: i64 = 100;
const WINDOW: (i64, i64) = (80, 120);
const MEDIA: i64 = 1_000;

fn gain_at(span: &Span, at: i64) -> f32 {
    span.crossfade_gain(at - span.start)
}

/// Both ramps follow the curve over the same window, so their sum (constant
/// gain) or their power (constant power) holds at every sample, and each ramp
/// replaces its side's own fade.
#[test]
fn the_two_ramps_meet_on_the_curve_across_the_window() {
    for curve in AudioTransition::ALL {
        let mut left = span(0, CUT, 0, false, SpanWarp::Linear);
        let mut right = span(CUT, 200, 500, false, SpanWarp::Linear);
        (left.fade_in, left.fade_out, right.fade_in, right.fade_out) = (7, 7, 7, 7);
        xfade::fade_outgoing(&mut left, MEDIA, WINDOW, curve);
        xfade::fade_incoming(&mut right, MEDIA, WINDOW, curve);
        // The outgoing side runs on to the window's end; the incoming side
        // starts at the window's start.
        assert_eq!((left.end, right.start), (WINDOW.1, WINDOW.0), "{curve:?}");
        assert_eq!((left.fade_in, left.fade_out), (7, 0), "{curve:?}");
        assert_eq!((right.fade_in, right.fade_out), (0, 7), "{curve:?}");
        assert_eq!(gain_at(&left, 0), 1.0, "{curve:?} before the window");
        assert_eq!(gain_at(&left, WINDOW.0), 1.0, "{curve:?}");
        assert_eq!(gain_at(&right, WINDOW.0), 0.0, "{curve:?}");
        assert_eq!(gain_at(&right, WINDOW.1), 1.0, "{curve:?} after the window");
        for at in WINDOW.0..WINDOW.1 {
            let progress = (at - WINDOW.0) as f32 / (WINDOW.1 - WINDOW.0) as f32;
            let (out, into) = (gain_at(&left, at), gain_at(&right, at));
            assert!(
                (out - curve.outgoing(progress)).abs() < 1e-6,
                "{curve:?} at {at}"
            );
            assert!(
                (into - curve.incoming(progress)).abs() < 1e-6,
                "{curve:?} at {at}"
            );
            match curve {
                AudioTransition::ConstantPower => {
                    assert!(
                        (out * out + into * into - 1.0).abs() < 1e-5,
                        "power at {at}"
                    );
                }
                AudioTransition::ConstantGain => {
                    assert!((out + into - 1.0).abs() < 1e-6, "sum at {at}");
                }
                AudioTransition::ExponentialFade => {}
            }
        }
        let mid = (WINDOW.0 + WINDOW.1) / 2;
        assert!((gain_at(&left, mid) - curve.outgoing(0.5)).abs() < 1e-6);
    }
}

/// Mixed, each side reads the source sample its handle holds: the outgoing
/// clip runs on past the cut and the incoming one starts early, both
/// continuous with the material inside the clip.
#[test]
fn each_side_plays_into_its_source_handle() {
    let curve = AudioTransition::ConstantGain;
    let mut left = span(0, CUT, 0, false, SpanWarp::Linear);
    let mut right = span(CUT, 200, 500, false, SpanWarp::Linear);
    xfade::fade_outgoing(&mut left, MEDIA, WINDOW, curve);
    xfade::fade_incoming(&mut right, MEDIA, WINDOW, curve);
    assert_eq!((right.source_start, right.source_len), (480, 120));
    assert_eq!((left.source_start, left.source_len), (0, 120));
    let gains: Vec<(f32, f32)> = (0..200)
        .map(|at| {
            let out = if at < left.end {
                gain_at(&left, at)
            } else {
                0.0
            };
            let into = if at >= right.start {
                gain_at(&right, at)
            } else {
                0.0
            };
            (out, into)
        })
        .collect();
    let mut mixer = ExportAudioMixer::of_spans(vec![left, right]);
    let mut out = vec![0.0; 200 * CHANNELS];
    mixer.mix_into(0, &mut out).unwrap();
    for at in 0..200_i64 {
        let (out_gain, in_gain) = gains[at as usize];
        let outgoing = at as f32 / SCALE;
        let incoming = (500 + at - CUT) as f32 / SCALE;
        let want = outgoing * out_gain + incoming * in_gain;
        let got = out[at as usize * CHANNELS];
        assert!(
            (got - want).abs() < 1e-5,
            "sample {at}: got {got} want {want}"
        );
    }
}

/// A side with less handle than the window asks for plays what it has and
/// ramps over the part of the window it covers, reaching silence where its
/// media ends: forward and reversed, outgoing and incoming.
#[test]
fn a_short_handle_reaches_silence_where_its_media_ends() {
    let curve = AudioTransition::ConstantPower;

    // Outgoing, forward: 10 samples of tail.
    let mut left = span(0, CUT, 0, false, SpanWarp::Linear);
    xfade::fade_outgoing(&mut left, CUT + 10, WINDOW, curve);
    assert_eq!((left.end, left.source_len), (CUT + 10, 110));
    assert_eq!(gain_at(&left, CUT + 10), 0.0);
    assert!(gain_at(&left, CUT + 9) < curve.outgoing(0.95));

    // Outgoing, reversed: the tail is the material before the source window.
    let mut left = span(0, CUT, 4, true, SpanWarp::Linear);
    xfade::fade_outgoing(&mut left, MEDIA, WINDOW, curve);
    assert_eq!(
        (left.end, left.source_start, left.source_len),
        (CUT + 4, 0, 104)
    );
    assert_eq!(gain_at(&left, CUT + 4), 0.0);

    // Incoming, forward: 5 samples of head.
    let mut right = span(CUT, 200, 5, false, SpanWarp::Linear);
    xfade::fade_incoming(&mut right, MEDIA, WINDOW, curve);
    assert_eq!(
        (right.start, right.source_start, right.source_len),
        (CUT - 5, 0, 105)
    );
    assert_eq!(gain_at(&right, CUT - 5), 0.0);

    // Incoming, reversed: the head is the material after the source window.
    let mut right = span(CUT, 200, 0, true, SpanWarp::Linear);
    xfade::fade_incoming(&mut right, 103, WINDOW, curve);
    assert_eq!(
        (right.start, right.source_start, right.source_len),
        (CUT - 3, 0, 103)
    );
    assert_eq!(gain_at(&right, CUT - 3), 0.0);

    // No handle at all: each side ramps over its own half of the window.
    let mut left = span(0, CUT, 0, false, SpanWarp::Linear);
    xfade::fade_outgoing(&mut left, CUT, WINDOW, curve);
    assert_eq!(left.end, CUT);
    assert!(gain_at(&left, CUT - 1) < 0.1);
}

/// A reversed side keeps walking its source backward through the handle: the
/// sample at the old edge is unchanged and the handle continues from it.
#[test]
fn a_reversed_side_walks_its_handle_backward() {
    let curve = AudioTransition::ConstantGain;
    let mut left = span(0, CUT, 50, true, SpanWarp::Linear);
    xfade::fade_outgoing(&mut left, MEDIA, WINDOW, curve);
    assert_eq!(
        (left.source_start, left.source_len, left.end),
        (30, 120, 120)
    );
    let gains: Vec<f32> = (0..120).map(|at| gain_at(&left, at)).collect();
    let mut mixer = ExportAudioMixer::of_spans(vec![left]);
    let mut out = vec![0.0; 120 * CHANNELS];
    mixer.mix_into(0, &mut out).unwrap();
    for at in 0..120_i64 {
        // Inside the clip the walk is 149..=50, as before; the handle goes on
        // to 30.
        let want = (149 - at) as f32 / SCALE * gains[at as usize];
        let got = out[at as usize * CHANNELS];
        assert!(
            (got - want).abs() < 1e-5,
            "sample {at}: got {got} want {want}"
        );
    }
}

/// A retimed side has no one-to-one source past its edge, so it keeps its
/// length and ramps over its half of the window.
#[test]
fn a_retimed_side_ramps_without_growing() {
    let curve = AudioTransition::ConstantGain;
    let mut left = span(0, CUT, 0, false, SpanWarp::FlatSpeed { num: 2, den: 1 });
    xfade::fade_outgoing(&mut left, MEDIA, WINDOW, curve);
    assert_eq!((left.end, left.source_len), (CUT, CUT));
    let ramp = left.crossfade_out.unwrap();
    assert_eq!((ramp.start, ramp.len), (WINDOW.0, CUT - WINDOW.0));
}

/// The incoming side's envelopes move with its start, so a keyframe stays on
/// the sample it was set on.
#[test]
fn an_early_start_keeps_the_envelope_on_its_samples() {
    let mut right = span(CUT, 200, 500, false, SpanWarp::Linear);
    right.volume = Param::Keyframed {
        keyframes: vec![
            aria_models::Keyframe::new(0, 0.25, aria_models::Easing::Linear),
            aria_models::Keyframe::new(50, 0.75, aria_models::Easing::Linear),
        ],
    };
    let before = right.volume.sample(25);
    assert_eq!(before, 0.5);
    xfade::fade_incoming(&mut right, MEDIA, WINDOW, AudioTransition::ConstantGain);
    assert_eq!(right.start, WINDOW.0);
    assert_eq!(right.volume.sample(25 + CUT - WINDOW.0), before);
    assert_eq!(right.volume.sample(0), 0.25);
}

/// Through the project: no transition leaves a hard switch at the cut; an
/// audio transition grows both sides into the window the model stores.
#[test]
fn the_project_mixer_crossfades_only_the_cuts_that_carry_a_transition() {
    let fps = Rational::FPS_24;
    let mut project = Project::new("xfade", fps);
    let track = project.add_track(TrackKind::Audio, "A1");
    let media = project.add_media(MediaSource::new("/dev/null", 0, 0, fps, 240, true));
    let left = project
        .add_clip(
            track,
            media,
            TimeRange::at_rate(0, 48, fps),
            RationalTime::new(0, fps),
        )
        .unwrap();
    project
        .add_clip(
            track,
            media,
            TimeRange::at_rate(96, 48, fps),
            RationalTime::new(48, fps),
        )
        .unwrap();
    let cut = ticks_to_samples(48, fps.num, fps.den);

    let mixer = ExportAudioMixer::for_project(&project).unwrap().unwrap();
    let [l, r] = &mixer.spans[..] else {
        panic!("two spans, got {:?}", mixer.spans)
    };
    assert_eq!((l.end, r.start), (cut, cut));
    assert!(l.crossfade_out.is_none() && r.crossfade_in.is_none());

    for curve in AudioTransition::ALL {
        project.add_transition(left, curve.id()).unwrap();
        let duration = project
            .timeline()
            .track(track)
            .unwrap()
            .transition_at(left)
            .unwrap()
            .duration;
        let start = 48 - duration / 2;
        let window = (
            ticks_to_samples(start, fps.num, fps.den),
            ticks_to_samples(start + duration, fps.num, fps.den),
        );
        let mixer = ExportAudioMixer::for_project(&project).unwrap().unwrap();
        let [l, r] = &mixer.spans[..] else {
            panic!("two spans, got {:?}", mixer.spans)
        };
        assert_eq!((l.end, r.start), (window.1, window.0), "{curve:?}");
        assert_eq!(l.crossfade_out.unwrap().curve, curve);
        assert_eq!(r.crossfade_in.unwrap().curve, curve);
        assert_eq!(
            r.source_start,
            ticks_to_samples(96, fps.num, fps.den) - (cut - window.0)
        );
    }
}
#[test]
fn audio_crossfade_window_follows_transition_alignment() {
    use aria_models::TransitionAlignment;

    let fps = Rational::FPS_24;
    let mut project = Project::new("xfade-align", fps);
    let media = project.add_media(MediaSource::new("/dev/null", 0, 0, fps, 240, true));
    let track = project.add_track(TrackKind::Audio, "A1");
    let left = project
        .add_clip(
            track,
            media,
            TimeRange::at_rate(24, 48, fps),
            RationalTime::new(0, fps),
        )
        .unwrap();
    project
        .add_clip(
            track,
            media,
            TimeRange::at_rate(96, 48, fps),
            RationalTime::new(48, fps),
        )
        .unwrap();
    project
        .add_transition(left, AudioTransition::ConstantPower.id())
        .unwrap();
    project.set_transition_duration(left, 24).unwrap();

    for (align, expected_ticks) in [
        (TransitionAlignment::Center, (36, 60)),
        (TransitionAlignment::Start, (48, 72)),
        (TransitionAlignment::End, (24, 48)),
    ] {
        project.set_transition_alignment(left, align).unwrap();
        let window = (
            ticks_to_samples(expected_ticks.0, fps.num, fps.den),
            ticks_to_samples(expected_ticks.1, fps.num, fps.den),
        );
        let mixer = ExportAudioMixer::for_project(&project).unwrap().unwrap();
        let [l, r] = &mixer.spans[..] else {
            panic!("two spans, got {:?}", mixer.spans)
        };
        assert_eq!((l.end, r.start), (window.1, window.0), "{align:?}");
    }
}
