//! The fold: what a lane's fader, its output and its sends actually do to a
//! block of samples.
//!
//! The graph itself is proved in `aria-models` (feed order, cycles, what
//! reaches the master). What is proved here is that the mixer *spends* that
//! graph: a bus fader rides everything routed into it, a pre-fader send is
//! deaf to the lane's own fader while a post-fader send follows it, and a lane
//! nothing carries to the master is never even opened.

use super::*;
use crate::audio_dsp::SpanWarp;
use aria_core::AudioReader;
use aria_models::{MediaSource, Project, Rational, RationalTime, TimeRange, TrackKind};

/// A source that reads the same sample forever, so an expected value is exact
/// rather than approximate.
struct FlatReader {
    pos: i64,
    value: f32,
}

impl AudioReader for FlatReader {
    fn read(&mut self, out: &mut [f32]) -> Result<usize, DecodeError> {
        out.fill(self.value);
        self.pos += (out.len() / CHANNELS) as i64;
        Ok(out.len() / CHANNELS)
    }

    fn seek_to_frame(&mut self, frame: i64) -> Result<(), DecodeError> {
        self.pos = frame;
        Ok(())
    }

    fn position(&self) -> Option<i64> {
        Some(self.pos)
    }
}

const BLOCK: usize = 64;

fn flat_span(value: f32, bus: usize) -> Span {
    Span {
        path: PathBuf::from("/dev/null"),
        start: 0,
        end: BLOCK as i64,
        source_start: 0,
        source_len: BLOCK as i64,
        reversed: false,
        warp: SpanWarp::Linear,
        preserve_pitch: true,
        denoise: false,
        volume: Param::Constant(1.0),
        pan: Param::Constant(0.0),
        fade_in: 0,
        fade_out: 0,
        reader: Some(Box::new(FlatReader { pos: 0, value })),
        exhausted: false,
        bus,
    }
}

fn lane(gain: f32, output: Option<usize>, sends: Vec<(usize, f32, bool)>) -> BusNode {
    BusNode::new(gain, output, sends)
}

/// Every sample of the block, so a fold that only lands on the first frame or
/// drops the tail is caught.
fn mixed(mixer: &mut ExportAudioMixer) -> f32 {
    let mut out = vec![0.0; BLOCK * CHANNELS];
    mixer.mix_into(0, &mut out).unwrap();
    let first = out[0];
    for (index, sample) in out.iter().enumerate() {
        assert!(
            (sample - first).abs() < 1e-6,
            "sample {index} is {sample}, not {first}: the fold is not flat across the block"
        );
    }
    first
}

#[test]
fn a_lane_with_no_routing_sums_straight_to_the_master() {
    let mut mixer = ExportAudioMixer::of_graph(
        vec![flat_span(0.25, 0), flat_span(0.5, 1)],
        vec![lane(1.0, None, Vec::new()), lane(1.0, None, Vec::new())],
    );
    assert!((mixed(&mut mixer) - 0.75).abs() < 1e-6);
}

#[test]
fn a_lane_fader_scales_that_lane_alone() {
    let mut mixer = ExportAudioMixer::of_graph(
        vec![flat_span(0.4, 0), flat_span(0.4, 1)],
        vec![lane(0.5, None, Vec::new()), lane(1.0, None, Vec::new())],
    );
    assert!((mixed(&mut mixer) - 0.6).abs() < 1e-6);
}

/// The point of a submix: one fader over many lanes.
#[test]
fn a_bus_fader_rides_everything_routed_into_it() {
    let mut mixer = ExportAudioMixer::of_graph(
        vec![flat_span(0.2, 0), flat_span(0.3, 1)],
        vec![
            lane(1.0, Some(2), Vec::new()),
            lane(1.0, Some(2), Vec::new()),
            lane(0.5, None, Vec::new()),
        ],
    );
    assert!((mixed(&mut mixer) - 0.25).abs() < 1e-6);
}

/// A bus feeding a bus: both faders apply, once each.
#[test]
fn buses_nest() {
    let mut mixer = ExportAudioMixer::of_graph(
        vec![flat_span(0.8, 0)],
        vec![
            lane(0.5, Some(1), Vec::new()),
            lane(0.5, Some(2), Vec::new()),
            lane(0.5, None, Vec::new()),
        ],
    );
    assert!((mixed(&mut mixer) - 0.1).abs() < 1e-6);
}

/// Which copy a send takes is the whole difference between feeding a reverb and
/// feeding a parallel chain. Riding the lane to silence must not take the
/// pre-fader send with it.
#[test]
fn a_pre_fader_send_is_deaf_to_the_lane_fader_and_a_post_fader_send_follows_it() {
    let pre = ExportAudioMixer::of_graph(
        vec![flat_span(0.4, 0)],
        vec![
            lane(0.0, None, vec![(1, 1.0, true)]),
            lane(1.0, None, Vec::new()),
        ],
    );
    let post = ExportAudioMixer::of_graph(
        vec![flat_span(0.4, 0)],
        vec![
            lane(0.0, None, vec![(1, 1.0, false)]),
            lane(1.0, None, Vec::new()),
        ],
    );
    let mut pre = pre;
    let mut post = post;
    assert!(
        (mixed(&mut pre) - 0.4).abs() < 1e-6,
        "a pre-fader send is taken before the fader"
    );
    assert!(
        mixed(&mut post).abs() < 1e-6,
        "a post-fader send follows the fader down to silence"
    );
}

#[test]
fn a_send_level_scales_only_the_copy() {
    let mut mixer = ExportAudioMixer::of_graph(
        vec![flat_span(0.4, 0)],
        vec![
            lane(1.0, None, vec![(1, 0.5, false)]),
            lane(1.0, None, Vec::new()),
        ],
    );
    // The lane's own output (0.4) plus half of it through the send.
    assert!((mixed(&mut mixer) - 0.6).abs() < 1e-6);
}

/// The clamp is the last thing that happens, after every bus has been folded,
/// so a hot mix clips at the master rather than bus by bus.
#[test]
fn the_master_clamps_once_at_the_end() {
    let mut mixer = ExportAudioMixer::of_graph(
        vec![flat_span(0.9, 0), flat_span(0.9, 1)],
        vec![lane(1.0, None, Vec::new()), lane(1.0, None, Vec::new())],
    );
    assert!((mixed(&mut mixer) - 1.0).abs() < 1e-6);
}

/// Buffers are reused between blocks, so a block must not carry the last one's
/// samples.
#[test]
fn a_bus_buffer_does_not_leak_into_the_next_block() {
    let mut mixer = ExportAudioMixer::of_graph(
        vec![flat_span(0.5, 0)],
        vec![lane(1.0, Some(1), Vec::new()), lane(1.0, None, Vec::new())],
    );
    assert!((mixed(&mut mixer) - 0.5).abs() < 1e-6);
    // The span covers only the first block; the second must be silence, not the
    // first block again.
    let mut out = vec![0.0; BLOCK * CHANNELS];
    mixer.mix_into(BLOCK as i64, &mut out).unwrap();
    assert!(out.iter().all(|sample| *sample == 0.0), "{out:?}");
}

fn audible_project() -> (Project, aria_models::TrackId, aria_models::TrackId) {
    let mut project = Project::new("mix", Rational::FPS_24);
    let a1 = project.add_track(TrackKind::Audio, "A1");
    let bus = project.add_track(TrackKind::Submix, "Music");
    let media = project.add_media(MediaSource::new(
        "/dev/null",
        0,
        0,
        Rational::FPS_24,
        240,
        true,
    ));
    project
        .add_clip(
            a1,
            media,
            TimeRange::at_rate(0, 48, Rational::FPS_24),
            RationalTime::new(0, Rational::FPS_24),
        )
        .unwrap();
    (project, a1, bus)
}

/// A lane whose sound cannot arrive anywhere is not decoded at all: the source
/// is never opened, which is the difference between a muted submix costing
/// nothing and costing a decode per lane per block.
#[test]
fn a_lane_behind_a_muted_bus_is_never_opened() {
    let (mut project, a1, bus) = audible_project();
    project
        .timeline_mut()
        .set_track_output(a1, aria_models::TrackOutput::Bus(bus))
        .unwrap();
    assert_eq!(
        ExportAudioMixer::for_project(&project)
            .unwrap()
            .expect("the lane still reaches the master")
            .spans
            .len(),
        1
    );

    project.timeline_mut().track_mut(bus).unwrap().muted = true;
    assert!(
        ExportAudioMixer::for_project(&project).unwrap().is_none(),
        "nothing reaches the master, so there is no mixer at all"
    );
}

/// The bus index a span carries is the position of its lane in the project's
/// own feed order, not the lane's stack position.
#[test]
fn a_span_is_summed_into_its_own_lanes_bus() {
    let (mut project, a1, bus) = audible_project();
    project
        .timeline_mut()
        .set_track_output(a1, aria_models::TrackOutput::Bus(bus))
        .unwrap();
    project.timeline_mut().set_track_gain(bus, 0.5).unwrap();
    let mixer = ExportAudioMixer::for_project(&project).unwrap().unwrap();
    let span_bus = mixer.spans[0].bus;
    assert_eq!(mixer.buses[span_bus].gain, 1.0, "the clip lane's own fader");
    let output = mixer.buses[span_bus]
        .output
        .expect("the lane feeds the bus, not the master");
    assert!(output > span_bus, "a lane only ever writes forward");
    assert_eq!(mixer.buses[output].gain, 0.5, "the bus's fader");
}

/// The reading a strip draws: the lane's own level, after its own fader, with
/// everything routed into it and nothing that merely sits beside it.
///
/// Two lanes into one bus, each at a different level and one of them faded, so a
/// meter that read the master, its neighbour, or the pre-fader copy reports a
/// different number for at least one lane.
#[test]
fn each_lanes_meter_reads_its_own_post_fader_level() {
    let mut mixer = ExportAudioMixer::of_graph(
        vec![flat_span(0.2, 0), flat_span(0.3, 1)],
        vec![
            lane(1.0, Some(2), Vec::new()),
            lane(0.5, Some(2), Vec::new()),
            lane(0.5, None, Vec::new()),
            // A lane the project has and the mix carries nothing into.
            lane(1.0, None, Vec::new()),
        ],
    );
    let master = mixed(&mut mixer);
    let read: Vec<(u64, f32)> = mixer
        .lane_peaks()
        .map(|(track, peak)| {
            assert!(
                (peak[0] - peak[1]).abs() < 1e-6,
                "lane {} reads {peak:?}: a centred source is the same in both bars",
                track.raw()
            );
            (track.raw(), peak[0])
        })
        .collect();
    let expect = [(0, 0.2), (1, 0.15), (2, 0.175), (3, 0.0)];
    assert_eq!(read.len(), expect.len());
    for ((track, got), (want_track, want)) in read.iter().zip(expect) {
        assert_eq!(*track, want_track);
        assert!(
            (got - want).abs() < 1e-6,
            "lane {track} reads {got}, not {want}"
        );
    }
    assert!(
        (master - 0.175).abs() < 1e-6,
        "the master carries {master}, so the bus meter is not the output's"
    );
}

/// A reading belongs to a lane, not to a position: the ids come out of the
/// project's own feed order, which is what the mixer folds.
#[test]
fn a_meter_is_keyed_by_the_lane_it_belongs_to() {
    let (mut project, a1, bus) = audible_project();
    project
        .timeline_mut()
        .set_track_output(a1, aria_models::TrackOutput::Bus(bus))
        .unwrap();
    let mixer = ExportAudioMixer::for_project(&project).unwrap().unwrap();
    let keyed: Vec<aria_models::TrackId> = mixer.lane_peaks().map(|(track, _)| track).collect();
    let folded: Vec<aria_models::TrackId> = project
        .timeline()
        .audio_buses()
        .unwrap()
        .iter()
        .map(|node| node.track)
        .collect();
    assert_eq!(keyed, folded);
    assert!(keyed.contains(&a1) && keyed.contains(&bus));
}

/// A key keeps a lane in the mix while its output runs into a muted bus. The
/// lane is mixed for the key's sake, but nothing it routes into the dead bus
/// may arrive at the master: the fold skips every bus that reaches nothing.
///
/// The insert has no processor, so this is the plain path; the insert path
/// skips dead buses the same way and is proved in `tests/audio_inserts.rs`.
#[test]
fn a_lane_kept_in_the_mix_by_a_key_does_not_leak_through_the_muted_bus_it_feeds() {
    let (mut project, a1, bus) = audible_project();
    project
        .timeline_mut()
        .set_track_output(a1, aria_models::TrackOutput::Bus(bus))
        .unwrap();
    project.timeline_mut().track_mut(bus).unwrap().muted = true;
    let mut keyed = aria_models::AudioEffectInstance::new("example.duck");
    keyed.sidechain = Some(a1);
    project
        .set_audio_inserts(aria_models::AudioInsertTarget::Master, vec![keyed])
        .unwrap();
    let mut mixer = ExportAudioMixer::for_project(&project)
        .unwrap()
        .expect("the key keeps the lane in the mix");
    assert!(
        mixer.inserts.is_none(),
        "an insert with no processor builds no graph"
    );
    assert_eq!(mixer.spans.len(), 1);
    for span in &mut mixer.spans {
        span.reader = Some(Box::new(FlatReader {
            pos: 0,
            value: 0.25,
        }));
    }
    assert_eq!(mixed(&mut mixer), 0.0);
    let bus_peak = mixer
        .lane_peaks()
        .find(|(track, _)| *track == bus)
        .map(|(_, peak)| peak);
    assert_eq!(bus_peak, Some([0.0; 2]), "a dead bus meters silence");
}
