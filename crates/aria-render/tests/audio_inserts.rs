//! Audio processors in the mix: the code behind registered audio effects, run
//! in clip, lane and master insert chains while the mixer streams a project.
//!
//! Its own test binary: the audio effect catalog and the processor registry
//! are process-wide. Every test registers ids no other test uses, so the tests
//! stay independent while they share both.
//!
//! The class this closes: an insert that sits in a chain and does something
//! other than what the chain states. An insert processes the sound once per
//! block, at its place in the signal order, with parameters and keys that line
//! up with the sound they apply to. A processor with latency delays its path,
//! and every parallel path is delayed to match, so nothing smears. A processor
//! that panics or overruns is bypassed without taking the mix down. Every case
//! compares the insert path against the plain mix of a project decoded from
//! real files, sample by sample.
//!
//! Not covered here: the plugin API that registers processors (`aria-plugin`)
//! and the editor surfaces that place inserts (`aria-ui`).

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use parking_lot::Mutex;

use aria_core::RationalTime;
use aria_models::{
    AudioEffectInstance, AudioEffectSpec, AudioInsertTarget, ClipId, Easing, EffectParamKind,
    EffectParamSpec, Keyframe, MediaSource, ModelError, Param, Project, Rational, Registered,
    TimeRange, TrackId, TrackKind, TrackOutput, register_audio_effect,
};
use aria_render::{
    AudioBlock, AudioProcessor, ExportAudioMixer, FaultKind, FaultSink, ProcessorFault,
    ProcessorRegistration, RenderError, audio_processor_registered, register_audio_processor,
};

#[path = "support/tone.rs"]
mod tone;
use tone::{CHANNELS, RATE, write_tone};

const MS: Rational = Rational::new(1000, 1);
const R24: Rational = Rational::FPS_24;
/// One timeline frame of sound: the block the export pushes.
const BLOCK: usize = RATE as usize / 24;
/// One second, in blocks.
const SECOND: usize = 24;
/// Long enough that a misaligned path is a 10 ms echo.
const LATENCY: u32 = 480;

// ---------------------------------------------------------------- effects --

/// A registered audio effect: its spec in the model catalog and the processor
/// behind it, both removed when this drops.
struct Effect {
    _spec: Registered,
    _processor: ProcessorRegistration,
}

/// The faults one effect reported, in order.
type Ledger = Arc<Mutex<Vec<ProcessorFault>>>;

fn scalar(name: &str, default: f32, min: f32, max: f32) -> EffectParamSpec {
    EffectParamSpec {
        name: name.into(),
        label: name.into(),
        kind: EffectParamKind::Scalar,
        default,
        min,
        max,
        default_color: [0; 4],
        default_vec2: [0.0; 2],
    }
}

fn spec(id: &str, params: Vec<EffectParamSpec>, latency: u32, sidechain: bool) -> AudioEffectSpec {
    AudioEffectSpec {
        id: id.into(),
        label: format!("{id} label"),
        params,
        latency,
        sidechain,
    }
}

/// Register `spec` and a processor made by `make`, reporting to `ledger`.
fn register<P: AudioProcessor + 'static>(
    spec: AudioEffectSpec,
    ledger: Option<&Ledger>,
    make: impl Fn() -> P + Send + Sync + 'static,
) -> Effect {
    let id = spec.id.clone();
    let (spec, _) = register_audio_effect(spec).expect("the spec registers");
    let sink = ledger.map(|ledger| {
        let ledger = Arc::clone(ledger);
        let sink: FaultSink = Arc::new(move |fault: &ProcessorFault| {
            ledger.lock().push(fault.clone());
        });
        sink
    });
    let processor = register_audio_processor(
        id,
        Arc::new(move |_rate: u32| -> Box<dyn AudioProcessor> { Box::new(make()) }),
        sink,
    )
    .expect("the processor registers");
    Effect {
        _spec: spec,
        _processor: processor,
    }
}

/// Multiplies by parameter 0, frame by frame.
struct Gain;

impl AudioProcessor for Gain {
    fn process(&mut self, block: &mut AudioBlock<'_>) {
        let gain = block.param(0).expect("slot 0");
        let (left, right) = block.channels();
        for (frame, (left, right)) in left.iter_mut().zip(right.iter_mut()).enumerate() {
            *left *= gain.at(frame);
            *right *= gain.at(frame);
        }
    }

    fn reset(&mut self) {}
}

fn gain(id: &str, default: f32, max: f32) -> Effect {
    register(
        spec(id, vec![scalar("gain", default, 0.0, max)], 0, false),
        None,
        || Gain,
    )
}

/// Clips at plus or minus parameter 0.
struct Limit;

impl AudioProcessor for Limit {
    fn process(&mut self, block: &mut AudioBlock<'_>) {
        let ceiling = block.param(0).expect("slot 0").at(0);
        let (left, right) = block.channels();
        for sample in left.iter_mut().chain(right.iter_mut()) {
            *sample = sample.clamp(-ceiling, ceiling);
        }
    }

    fn reset(&mut self) {}
}

/// Delays its input by [`LATENCY`] frames and declares that latency: the shape
/// of every look-ahead processor, with an output that is exact to predict.
struct Delay {
    left: VecDeque<f32>,
    right: VecDeque<f32>,
}

impl Delay {
    fn new() -> Self {
        let silence = || VecDeque::from(vec![0.0; LATENCY as usize]);
        Self {
            left: silence(),
            right: silence(),
        }
    }
}

impl AudioProcessor for Delay {
    fn process(&mut self, block: &mut AudioBlock<'_>) {
        let (left, right) = block.channels();
        for (line, channel) in [(&mut self.left, left), (&mut self.right, right)] {
            for sample in channel.iter_mut() {
                line.push_back(*sample);
                *sample = line.pop_front().expect("a full line");
            }
        }
    }

    fn reset(&mut self) {
        *self = Self::new();
    }
}

fn delay(id: &str) -> Effect {
    register(spec(id, Vec::new(), LATENCY, false), None, Delay::new)
}

/// Replaces its input with its key. With no key set, its input passes.
struct KeyOut;

impl AudioProcessor for KeyOut {
    fn process(&mut self, block: &mut AudioBlock<'_>) {
        let Some((key_left, key_right)) = block.sidechain() else {
            return;
        };
        let (left, right) = block.channels();
        left.copy_from_slice(key_left);
        right.copy_from_slice(key_right);
    }

    fn reset(&mut self) {}
}

/// Adds its key to its input, so the holder's own sound has to line up too.
struct KeyAdd;

impl AudioProcessor for KeyAdd {
    fn process(&mut self, block: &mut AudioBlock<'_>) {
        let (key_left, key_right) = block.sidechain().expect("a key");
        let (left, right) = block.channels();
        for (sample, key) in left.iter_mut().zip(key_left) {
            *sample += key;
        }
        for (sample, key) in right.iter_mut().zip(key_right) {
            *sample += key;
        }
    }

    fn reset(&mut self) {}
}

/// A [`Delay`] that panics on the first block recorded at or after `at`.
struct PanicsAt {
    delay: Delay,
    at: i64,
}

impl AudioProcessor for PanicsAt {
    fn process(&mut self, block: &mut AudioBlock<'_>) {
        assert!(block.position() < self.at, "boom");
        self.delay.process(block);
    }

    fn reset(&mut self) {
        self.delay.reset();
    }
}

/// Sleeps past a 10 ms block's budget on the calls `slow` lists.
struct Slow {
    slow: &'static [usize],
    call: usize,
    calls: Arc<AtomicUsize>,
}

impl AudioProcessor for Slow {
    fn process(&mut self, _block: &mut AudioBlock<'_>) {
        if self.slow.contains(&self.call) {
            std::thread::sleep(Duration::from_millis(15));
        }
        self.call += 1;
        self.calls.fetch_add(1, Ordering::SeqCst);
    }

    fn reset(&mut self) {}
}

/// Writes values that are not numbers.
struct NotANumber;

impl AudioProcessor for NotANumber {
    fn process(&mut self, block: &mut AudioBlock<'_>) {
        let (left, right) = block.channels();
        left.fill(f32::NAN);
        right.fill(f32::INFINITY);
    }

    fn reset(&mut self) {}
}

// ---------------------------------------------------------------- session --

/// A project over tones written for it. The files live as long as this.
struct Session {
    _dir: tempfile::TempDir,
    project: Project,
    lanes: Vec<TrackId>,
    clips: Vec<ClipId>,
}

/// One audio lane per `(hz, start tick)`, each holding one second of that tone.
fn session(tones: &[(f32, i64)]) -> Session {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut project = Project::new("inserts", R24);
    let mut lanes = Vec::new();
    let mut clips = Vec::new();
    for (index, &(hz, start)) in tones.iter().enumerate() {
        let path = dir.path().join(format!("tone{index}.wav"));
        write_tone(&path, 1000, hz);
        let media = project.add_media(MediaSource::new(&path, 0, 0, MS, 1000, true));
        let lane = project.add_track(TrackKind::Audio, format!("A{}", index + 1));
        let clip = project
            .add_clip(
                lane,
                media,
                TimeRange::at_rate(0, 1000, MS),
                RationalTime::new(start, R24),
            )
            .expect("the clip");
        lanes.push(lane);
        clips.push(clip);
    }
    Session {
        _dir: dir,
        project,
        lanes,
        clips,
    }
}

fn mixer(project: &Project) -> ExportAudioMixer {
    ExportAudioMixer::for_project(project)
        .expect("the mix opens")
        .expect("the project is audible")
}

/// Blocks `from..from + count` of `frames` each, mixed in order.
fn mix_blocks(mixer: &mut ExportAudioMixer, from: usize, count: usize, frames: usize) -> Vec<f32> {
    let mut out = Vec::with_capacity(count * frames * CHANNELS);
    let mut block = vec![0.0; frames * CHANNELS];
    for index in from..from + count {
        mixer
            .mix_into((index * frames) as i64, &mut block)
            .expect("a block");
        out.extend_from_slice(&block);
    }
    out
}

/// `blocks` timeline frames of `project`, mixed from the start.
fn render(project: &Project, blocks: usize) -> Vec<f32> {
    mix_blocks(&mut mixer(project), 0, blocks, BLOCK)
}

/// Every sample of `got` within `tolerance` of `want`, which is audible.
fn assert_close(got: &[f32], want: &[f32], tolerance: f32, case: &str) {
    assert_eq!(got.len(), want.len(), "{case}: length");
    assert!(
        want.iter().any(|sample| sample.abs() > 0.1),
        "{case}: the reference is silent, so it proves nothing"
    );
    let (at, worst) = got
        .iter()
        .zip(want)
        .map(|(got, want)| (got - want).abs())
        .enumerate()
        .fold((0, 0.0_f32), |worst, (at, diff)| {
            if diff > worst.1 || diff.is_nan() {
                (at, diff)
            } else {
                worst
            }
        });
    assert!(
        worst <= tolerance,
        "{case}: frame {} differs by {worst} (got {}, want {})",
        at / CHANNELS,
        got[at],
        want[at]
    );
}

fn assert_silent(got: &[f32], case: &str) {
    if let Some(at) = got.iter().position(|sample| *sample != 0.0) {
        panic!("{case}: frame {} is {}", at / CHANNELS, got[at]);
    }
}

fn keyframed(points: &[(i64, f32)]) -> Param<f32> {
    Param::Keyframed {
        keyframes: points
            .iter()
            .map(|&(tick, value)| Keyframe::new(tick, value, Easing::Linear))
            .collect(),
    }
}

// ------------------------------------------------------------------ cases --

#[test]
fn every_kind_of_chain_processes_the_sound_once() {
    let _half = gain("render.half", 1.0, 2.0);
    let base = session(&[(440.0, 0)]);
    let plain = render(&base.project, SECOND);
    let halved: Vec<f32> = plain.iter().map(|sample| sample * 0.5).collect();
    for (case, target) in [
        ("clip", AudioInsertTarget::Clip(base.clips[0])),
        ("lane", AudioInsertTarget::Track(base.lanes[0])),
        ("master", AudioInsertTarget::Master),
    ] {
        let mut project = base.project.clone();
        let index = project.add_audio_effect(target, "render.half").unwrap();
        project
            .set_audio_effect_param(target, index, 0, Param::Constant(0.5))
            .unwrap();
        assert_close(&render(&project, SECOND), &halved, 1e-6, case);

        project
            .set_audio_effect_bypass(target, index, true)
            .unwrap();
        assert_close(&render(&project, SECOND), &plain, 0.0, case);
    }
}

/// A latency on one path delays that path; every other path the master hears
/// is delayed to match, and the output as a whole is pulled back into place.
/// Two tones of different pitch, so a path that runs early or late is an
/// audible difference, never a silent one.
#[test]
fn latency_on_any_path_leaves_every_path_aligned() {
    let _delay = delay("render.delay");
    let mut base = session(&[(440.0, 0), (660.0, 0)]);
    let (a, b) = (base.lanes[0], base.lanes[1]);
    let bus = base.project.add_track(TrackKind::Submix, "Bus");
    let plain = render(&base.project, SECOND);

    let mut into_bus = base.project.clone();
    into_bus
        .timeline_mut()
        .set_track_output(a, TrackOutput::Bus(bus))
        .unwrap();
    // A send whose bus also takes a later input, so the send itself waits.
    let mut sends = base.project.clone();
    sends.timeline_mut().add_send(a, bus, 0.5, false).unwrap();
    sends
        .timeline_mut()
        .set_track_output(b, TrackOutput::Bus(bus))
        .unwrap();

    for (case, reference, target) in [
        (
            "clip",
            &base.project,
            AudioInsertTarget::Clip(base.clips[0]),
        ),
        ("lane", &base.project, AudioInsertTarget::Track(a)),
        ("master", &base.project, AudioInsertTarget::Master),
        ("bus", &into_bus, AudioInsertTarget::Track(bus)),
        ("send", &sends, AudioInsertTarget::Track(b)),
        ("send source", &sends, AudioInsertTarget::Track(a)),
    ] {
        let want = render(reference, SECOND);
        let mut project = reference.clone();
        project.add_audio_effect(target, "render.delay").unwrap();
        assert_close(&render(&project, SECOND), &want, 1e-6, case);
    }
    base.project
        .add_audio_effect(AudioInsertTarget::Track(a), "render.delay")
        .unwrap();
    base.project
        .add_audio_effect(AudioInsertTarget::Track(b), "render.delay")
        .unwrap();
    base.project
        .add_audio_effect(AudioInsertTarget::Clip(base.clips[1]), "render.delay")
        .unwrap();
    assert_close(&render(&base.project, SECOND), &plain, 1e-6, "stacked");
}

/// An insert the mix cannot run passes its input through and adds no
/// latency, so the lane beside it stays where it was.
#[test]
fn a_bypassed_or_unprocessed_insert_passes_through_with_no_latency() {
    let _delay = delay("render.idle");
    let (_spec_only, _) =
        register_audio_effect(spec("render.spec_only", Vec::new(), LATENCY, false)).unwrap();
    let _processor_only = register_audio_processor(
        "render.processor_only",
        Arc::new(|_rate: u32| -> Box<dyn AudioProcessor> { Box::new(Delay::new()) }),
        None,
    )
    .unwrap();
    let base = session(&[(440.0, 0), (660.0, 0)]);
    let a = AudioInsertTarget::Track(base.lanes[0]);
    let plain = render(&base.project, SECOND);

    let mut bypassed = AudioEffectInstance::new("render.idle");
    bypassed.bypass = true;
    for (case, insert) in [
        ("bypassed", bypassed),
        ("spec only", AudioEffectInstance::new("render.spec_only")),
        (
            "processor only",
            AudioEffectInstance::new("render.processor_only"),
        ),
        ("unknown", AudioEffectInstance::new("render.nowhere")),
    ] {
        let mut project = base.project.clone();
        project.set_audio_inserts(a, vec![insert]).unwrap();
        assert_close(&render(&project, SECOND), &plain, 0.0, case);
    }
}

/// A clip chain runs before the clip's volume, a lane chain before its fader,
/// the master chain before the final clamp. A limiter at 0.5 tells the first
/// two apart: before a halving level it flattens the 0.8 peaks, after it
/// touches nothing.
#[test]
fn each_chain_runs_before_its_level() {
    let _limit = register(
        spec(
            "render.limit",
            vec![scalar("ceiling", 0.5, 0.0, 1.0)],
            0,
            false,
        ),
        None,
        || Limit,
    );
    let _half = gain("render.master_half", 0.5, 1.0);
    let base = session(&[(440.0, 0), (660.0, 0)]);
    let (a, b) = (base.lanes[0], base.lanes[1]);
    let mut solo = base.project.clone();
    solo.timeline_mut().track_mut(b).unwrap().muted = true;

    let mut clip_level = solo.clone();
    clip_level.clip_mut(base.clips[0]).unwrap().volume = Param::Constant(0.5);
    let mut lane_level = solo.clone();
    lane_level.timeline_mut().set_track_gain(a, 0.5).unwrap();
    for (case, project, target) in [
        ("clip", clip_level, AudioInsertTarget::Clip(base.clips[0])),
        ("lane", lane_level, AudioInsertTarget::Track(a)),
    ] {
        let plain = render(&project, SECOND);
        let want: Vec<f32> = plain
            .iter()
            .map(|sample| (sample * 2.0).clamp(-0.5, 0.5) * 0.5)
            .collect();
        let mut limited = project.clone();
        limited.add_audio_effect(target, "render.limit").unwrap();
        assert_close(&render(&limited, SECOND), &want, 1e-6, case);
    }

    // Both tones together peak past 1. Halved before the clamp, nothing clips.
    let mut faded = base.project.clone();
    faded.timeline_mut().set_track_gain(a, 0.5).unwrap();
    faded.timeline_mut().set_track_gain(b, 0.5).unwrap();
    let want = render(&faded, SECOND);
    let mut master = base.project;
    master
        .add_audio_effect(AudioInsertTarget::Master, "render.master_half")
        .unwrap();
    assert_close(&render(&master, SECOND), &want, 1e-6, "master");
}

/// Keys on a clip insert count clip ticks, on a lane or the master sequence
/// ticks, and an automated parameter reaches the processor once per frame. An
/// insert after a latent one reads its curve at the time of the sound it
/// receives, so the delay moves the curve with the sound. A value outside the
/// declared range is clamped into it.
#[test]
fn parameter_curves_reach_the_processor_per_frame_on_their_chains_clock() {
    let _ramp = gain("render.ramp", 1.0, 1.0);
    let _late = delay("render.ramp_delay");
    let _ranged = gain("render.ranged", 0.25, 0.5);
    // Half a second in, so a clip clock and a sequence clock disagree.
    let base = session(&[(440.0, 12)]);
    let length = SECOND + SECOND / 2;
    let plain = render(&base.project, length);
    let start = RATE as usize / 2;
    let ramped = |from: usize, frames: usize| -> Vec<f32> {
        plain
            .iter()
            .enumerate()
            .map(|(at, sample)| {
                let frame = at / CHANNELS;
                let level = (frame.saturating_sub(from) as f32 / frames as f32).min(1.0);
                sample * level
            })
            .collect()
    };
    for (case, target, curve, want) in [
        (
            "clip",
            AudioInsertTarget::Clip(base.clips[0]),
            keyframed(&[(0, 0.0), (12, 1.0)]),
            ramped(start, start),
        ),
        (
            "lane",
            AudioInsertTarget::Track(base.lanes[0]),
            keyframed(&[(0, 0.0), (24, 1.0)]),
            ramped(0, RATE as usize),
        ),
        (
            "master",
            AudioInsertTarget::Master,
            keyframed(&[(12, 0.0), (36, 1.0)]),
            ramped(start, RATE as usize),
        ),
    ] {
        for latent in [false, true] {
            let mut project = base.project.clone();
            if latent {
                project
                    .add_audio_effect(target, "render.ramp_delay")
                    .unwrap();
            }
            let index = project.add_audio_effect(target, "render.ramp").unwrap();
            project
                .set_audio_effect_param(target, index, 0, curve.clone())
                .unwrap();
            let case = format!("{case}, latent {latent}");
            assert_close(&render(&project, length), &want, 1e-5, &case);
        }
    }

    let mut loud = AudioEffectInstance::new("render.ranged");
    loud.params.insert("gain".into(), Param::Constant(5.0));
    let mut project = base.project;
    project
        .set_audio_inserts(AudioInsertTarget::Master, vec![loud])
        .unwrap();
    let halved: Vec<f32> = plain.iter().map(|sample| sample * 0.5).collect();
    assert_close(&render(&project, length), &halved, 1e-6, "clamped");
}

/// The key is the key lane's sound after its own chain and before its fader,
/// aligned with the input of the insert reading it, however late either side
/// runs. A muted key lane keys silence.
#[test]
fn a_key_arrives_after_its_lanes_chain_before_its_fader_and_in_time() {
    let _delay = delay("render.key_delay");
    let _key = register(spec("render.key", Vec::new(), 0, true), None, || KeyOut);
    let _add = register(spec("render.key_add", Vec::new(), 0, true), None, || KeyAdd);
    let mut base = session(&[(440.0, 0), (660.0, 0)]);
    let (a, b) = (base.lanes[0], base.lanes[1]);
    let bus = base.project.add_track(TrackKind::Submix, "Bus");
    let key = |project: &mut Project, target: AudioInsertTarget, effect: &str, from: TrackId| {
        let index = project.add_audio_effect(target, effect).unwrap();
        project
            .set_audio_effect_sidechain(target, index, Some(from))
            .unwrap();
    };
    let mut keyed = base.project.clone();
    keyed
        .add_audio_effect(AudioInsertTarget::Track(a), "render.key_delay")
        .unwrap();
    let mut solo_a = base.project.clone();
    solo_a.timeline_mut().track_mut(b).unwrap().muted = true;

    // The master hears only the key: lane A at unity, despite its fader.
    let mut master = keyed.clone();
    master.timeline_mut().set_track_gain(a, 0.5).unwrap();
    key(&mut master, AudioInsertTarget::Master, "render.key", a);
    assert_close(
        &render(&master, SECOND),
        &render(&solo_a, SECOND),
        1e-6,
        "master",
    );

    // Lane B adds lane A's key to its own sound, under B's fader, beside A's
    // own output: 0.25 A + 0.5 (B + A).
    let mut lane = keyed.clone();
    lane.timeline_mut().set_track_gain(a, 0.25).unwrap();
    lane.timeline_mut().set_track_gain(b, 0.5).unwrap();
    key(&mut lane, AudioInsertTarget::Track(b), "render.key_add", a);
    let mut want = base.project.clone();
    want.timeline_mut().set_track_gain(a, 0.75).unwrap();
    want.timeline_mut().set_track_gain(b, 0.5).unwrap();
    assert_close(&render(&lane, SECOND), &render(&want, SECOND), 1e-6, "lane");

    // A key that is ready before the insert reading it: the master delays
    // its own input, so it waits for that.
    let mut early = base.project.clone();
    early
        .add_audio_effect(AudioInsertTarget::Master, "render.key_delay")
        .unwrap();
    key(&mut early, AudioInsertTarget::Master, "render.key", b);
    let mut solo_b = base.project.clone();
    solo_b.timeline_mut().track_mut(a).unwrap().muted = true;
    assert_close(
        &render(&early, SECOND),
        &render(&solo_b, SECOND),
        1e-6,
        "early key",
    );

    // Lane A reaches the master only as a key: its own output runs into a
    // muted bus, which must stay silent.
    let mut key_only = keyed.clone();
    key_only
        .timeline_mut()
        .set_track_output(a, TrackOutput::Bus(bus))
        .unwrap();
    key_only.timeline_mut().track_mut(bus).unwrap().muted = true;
    key_only.timeline_mut().set_track_gain(b, 0.125).unwrap();
    key(
        &mut key_only,
        AudioInsertTarget::Master,
        "render.key_add",
        a,
    );
    let mut want = base.project.clone();
    want.timeline_mut().set_track_gain(b, 0.125).unwrap();
    assert_close(
        &render(&key_only, SECOND),
        &render(&want, SECOND),
        1e-6,
        "key only",
    );

    let mut muted = master.clone();
    muted.timeline_mut().track_mut(a).unwrap().muted = true;
    assert_silent(&render(&muted, SECOND), "muted key");

    // A file can hold a lane keyed by itself, which no edit allows. The mix
    // refuses it by name rather than keying the lane from its own last block.
    let mut own = AudioEffectInstance::new("render.key_add");
    own.sidechain = Some(a);
    let mut looped = keyed;
    looped
        .set_audio_inserts(AudioInsertTarget::Track(a), vec![own])
        .unwrap();
    match ExportAudioMixer::for_project(&looped) {
        Err(RenderError::Model(ModelError::RoutingCycle(lane))) => assert_eq!(lane, a),
        other => panic!("a lane keyed by itself was mixed: {other:?}"),
    }
}

/// A processor that panics is dropped and its insert passes its input through
/// for the rest of the mix, still delayed by its declared latency, so every
/// other path stays aligned. The fault is reported once, to the sink its
/// effect was registered with. A factory that panics is the same fault before
/// the first block.
#[test]
fn a_panic_bypasses_its_insert_keeps_its_latency_and_is_reported_once() {
    const FAULT_BLOCK: usize = 4;
    let ledger = Ledger::default();
    let _panics = register(
        spec("render.panics", Vec::new(), LATENCY, false),
        Some(&ledger),
        || PanicsAt {
            delay: Delay::new(),
            at: (FAULT_BLOCK * BLOCK) as i64,
        },
    );
    let base = session(&[(440.0, 0), (660.0, 0)]);
    let a = base.lanes[0];
    let plain = render(&base.project, SECOND);
    let mut only_b = base.project.clone();
    only_b.timeline_mut().track_mut(a).unwrap().muted = true;
    let plain_b = render(&only_b, SECOND);

    let mut project = base.project.clone();
    project
        .add_audio_effect(AudioInsertTarget::Track(a), "render.panics")
        .unwrap();
    let got = render(&project, SECOND);
    let block = |index: usize| index * BLOCK * CHANNELS..(index + 1) * BLOCK * CHANNELS;
    let before = 0..block(FAULT_BLOCK).start;
    assert_close(
        &got[before.clone()],
        &plain[before],
        1e-6,
        "before the fault",
    );
    // The block the processor panicked in carries lane B alone.
    let failed = block(FAULT_BLOCK);
    assert_close(
        &got[failed.clone()],
        &plain_b[failed],
        1e-6,
        "the failed block",
    );
    // The bypass delay starts empty, so lane A returns one latency later.
    let after = block(FAULT_BLOCK + 1).start + LATENCY as usize * CHANNELS..got.len();
    assert_close(&got[after.clone()], &plain[after], 1e-6, "after the fault");
    assert_eq!(
        *ledger.lock(),
        [ProcessorFault {
            effect_id: "render.panics".into(),
            kind: FaultKind::Panicked("boom".into()),
        }]
    );
    let factory_ledger = Ledger::default();
    let _broken = register(
        spec("render.broken", Vec::new(), LATENCY, false),
        Some(&factory_ledger),
        || -> Delay { panic!("no processor today") },
    );
    let mut project = base.project;
    project
        .add_audio_effect(AudioInsertTarget::Track(a), "render.broken")
        .unwrap();
    assert_close(&render(&project, SECOND), &plain, 1e-6, "factory panic");
    assert_eq!(
        *factory_ledger.lock(),
        [ProcessorFault {
            effect_id: "render.broken".into(),
            kind: FaultKind::Panicked("no processor today".into()),
        }]
    );
}

/// During playback a block that takes longer than the time it covers is a
/// miss, and three in a row bypass the insert. A fast block in between starts
/// the count again. Export has no deadline.
#[test]
fn three_missed_deadlines_in_a_row_bypass_an_insert_during_playback_only() {
    // 10 ms of sound, and a processor that takes 15 ms on the calls listed.
    const FRAMES: usize = RATE as usize / 100;
    const SLOW: &[usize] = &[0, 1, 3, 4, 5, 6];
    let calls = Arc::new(AtomicUsize::new(0));
    let ledger = Ledger::default();
    let counted = Arc::clone(&calls);
    let _slow = register(
        spec("render.slow", Vec::new(), 0, false),
        Some(&ledger),
        move || Slow {
            slow: SLOW,
            call: 0,
            calls: Arc::clone(&counted),
        },
    );
    let mut base = session(&[(440.0, 0)]);
    base.project
        .add_audio_effect(AudioInsertTarget::Master, "render.slow")
        .unwrap();

    let mut playback = mixer(&base.project);
    playback.set_realtime(true);
    mix_blocks(&mut playback, 0, 8, FRAMES);
    assert_eq!(
        calls.load(Ordering::SeqCst),
        6,
        "bypassed after the sixth call"
    );
    let faults = ledger.lock().clone();
    assert_eq!(faults.len(), 1, "{faults:?}");
    match &faults[0].kind {
        FaultKind::MissedDeadlines { budget, took } => {
            assert_eq!(*budget, Duration::from_millis(10));
            assert!(*took >= Duration::from_millis(15), "{took:?}");
        }
        other => panic!("not a deadline fault: {other:?}"),
    }

    calls.store(0, Ordering::SeqCst);
    let mut export = mixer(&base.project);
    mix_blocks(&mut export, 0, 8, FRAMES);
    assert_eq!(calls.load(Ordering::SeqCst), 8, "export runs every block");
    assert_eq!(ledger.lock().len(), 1, "and reports nothing");
}

/// A block that does not continue the last one resets every processor and
/// pre-rolls the chains, so what follows a seek is what a continuous mix
/// would have played there.
#[test]
fn a_seek_pre_rolls_so_the_next_block_matches_a_continuous_mix() {
    let _delay = delay("render.seek_delay");
    let mut base = session(&[(440.0, 0), (660.0, 0)]);
    base.project
        .add_audio_effect(AudioInsertTarget::Track(base.lanes[0]), "render.seek_delay")
        .unwrap();
    base.project
        .add_audio_effect(AudioInsertTarget::Clip(base.clips[1]), "render.seek_delay")
        .unwrap();
    base.project
        .add_audio_effect(AudioInsertTarget::Master, "render.seek_delay")
        .unwrap();
    let continuous = render(&base.project, 12);
    let tail = 8 * BLOCK * CHANNELS..continuous.len();

    let mut cold = mixer(&base.project);
    assert_close(
        &mix_blocks(&mut cold, 8, 4, BLOCK),
        &continuous[tail.clone()],
        1e-6,
        "opened at the seek",
    );
    let mut jumped = mixer(&base.project);
    mix_blocks(&mut jumped, 0, 2, BLOCK);
    assert_close(
        &mix_blocks(&mut jumped, 8, 4, BLOCK),
        &continuous[tail.clone()],
        1e-6,
        "forward",
    );
    let mut back = mixer(&base.project);
    mix_blocks(&mut back, 10, 2, BLOCK);
    assert_close(
        &mix_blocks(&mut back, 8, 4, BLOCK),
        &continuous[tail],
        1e-6,
        "backward",
    );
}

#[test]
fn processor_output_that_is_not_a_number_is_silenced() {
    let _nan = register(spec("render.nan", Vec::new(), 0, false), None, || {
        NotANumber
    });
    let mut base = session(&[(440.0, 0)]);
    base.project
        .add_audio_effect(AudioInsertTarget::Master, "render.nan")
        .unwrap();
    assert_silent(&render(&base.project, 4), "master");
}

#[test]
fn registration_refuses_an_empty_or_taken_id_and_a_dropped_one_is_gone() {
    let factory = || -> Arc<dyn aria_render::AudioProcessorFactory> {
        Arc::new(|_rate: u32| -> Box<dyn AudioProcessor> { Box::new(Gain) })
    };
    let held = register_audio_processor("render.held", factory(), None).unwrap();
    assert!(audio_processor_registered("render.held"));
    assert!(register_audio_processor("render.held", factory(), None).is_err());
    assert!(register_audio_processor("", factory(), None).is_err());
    assert!(!audio_processor_registered(""));
    drop(held);
    assert!(!audio_processor_registered("render.held"));
    let again = register_audio_processor("render.held", factory(), None).unwrap();
    assert!(audio_processor_registered("render.held"));
    drop(again);
}
