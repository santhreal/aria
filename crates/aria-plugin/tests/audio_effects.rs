//! Audio effects a plugin registers, and the mix a plugin asks the audio host
//! for.
//!
//! Its own test binary: the audio effect catalog and the processor registry
//! are process-wide, and every test uses ids no other test uses.
//!
//! The class this closes: a plugin's audio effect whose lifetime or faults
//! escape the plugin. A registration that outlives its plugin or its context, a
//! refusal that leaves half a registration behind, a processor panic that
//! leaves its plugin mounted, a missed deadline nobody hears about, and a host
//! mix that covers other samples than the range asked for. The mixer's own
//! handling of chains, latency and keys is `aria-render/tests/audio_inserts.rs`.

use std::ops::ControlFlow;
use std::sync::Arc;
use std::time::Duration;

use aria_models::{
    AudioInsertTarget, EffectParamKind, MediaSource, ModelError, Project, Rational, RationalTime,
    TimeRange, TrackId, TrackKind, audio_effect_spec,
};
use aria_plugin::{
    AudioBlock, AudioEffectSpec, AudioHost, AudioProcessor, AudioSource, EffectParamSpec, Plugin,
    PluginContext, PluginDeactivated, PluginError, PluginFault, PluginManifest,
};
use aria_render::{
    ExportAudioMixer, RenderError, audio_processor_registered, register_audio_processor,
};
use parking_lot::Mutex;

#[path = "../../aria-render/tests/support/tone.rs"]
mod tone;
use tone::{CHANNELS, RATE, write_tone};

const MS: Rational = Rational::new(1000, 1);
const R24: Rational = Rational::FPS_24;
/// 10 ms of sound: the deadline of one block during playback.
const BLOCK: usize = RATE as usize / 100;
const FLUSH: Duration = Duration::from_secs(10);

/// Multiplies by its one parameter.
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

/// Panics on its first block.
struct Panics;

impl AudioProcessor for Panics {
    fn process(&mut self, _block: &mut AudioBlock<'_>) {
        panic!("the processor faulted");
    }

    fn reset(&mut self) {}
}

/// Takes two and a half blocks of real time over every block.
struct Slow;

impl AudioProcessor for Slow {
    fn process(&mut self, _block: &mut AudioBlock<'_>) {
        std::thread::sleep(Duration::from_millis(25));
    }

    fn reset(&mut self) {}
}

/// A service the crashing plugin registers beside its effect, to show that
/// failing drops every registration and not only the effect.
struct Marker;

/// A plugin that registers one audio effect with a gain parameter at 0.5.
struct AudioPlugin {
    manifest: PluginManifest,
    effect: &'static str,
    make: fn() -> Box<dyn AudioProcessor>,
}

impl AudioPlugin {
    fn new(id: &'static str, effect: &'static str, make: fn() -> Box<dyn AudioProcessor>) -> Self {
        Self {
            manifest: PluginManifest {
                id,
                name: id,
                version: "1.0.0",
                requires: &[],
                uses: &[],
            },
            effect,
            make,
        }
    }
}

impl Plugin for AudioPlugin {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        ctx.register(Marker);
        let make = self.make;
        ctx.register_audio_effect(spec(self.effect), move |_rate: u32| make())?;
        Ok(())
    }
}

fn spec(id: &str) -> AudioEffectSpec {
    AudioEffectSpec {
        id: id.into(),
        label: format!("{id} label"),
        params: vec![EffectParamSpec {
            name: "gain".into(),
            label: "Gain".into(),
            kind: EffectParamKind::Scalar,
            default: 0.5,
            min: 0.0,
            max: 1.0,
            default_color: [0; 4],
            default_vec2: [0.0; 2],
        }],
        latency: 0,
        sidechain: false,
    }
}

fn gain() -> Box<dyn AudioProcessor> {
    Box::new(Gain)
}

fn panics() -> Box<dyn AudioProcessor> {
    Box::new(Panics)
}

fn slow() -> Box<dyn AudioProcessor> {
    Box::new(Slow)
}

/// Whether `id` is anywhere: in the catalog or in the processor registry.
fn registered(id: &str) -> (bool, bool) {
    (
        audio_effect_spec(id).is_some(),
        audio_processor_registered(id),
    )
}

struct Session {
    _dir: tempfile::TempDir,
    project: Project,
    lanes: Vec<TrackId>,
    clips: Vec<aria_models::ClipId>,
}

/// One audio lane per tone, each holding one second of it from the start.
fn session(tones: &[f32]) -> Session {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut project = Project::new("plugin audio", R24);
    let mut lanes = Vec::new();
    let mut clips = Vec::new();
    for (index, &hz) in tones.iter().enumerate() {
        let path = dir.path().join(format!("tone{index}.wav"));
        write_tone(&path, 1000, hz);
        let media = project.add_media(MediaSource::new(&path, 0, 0, MS, 1000, true));
        let lane = project.add_track(TrackKind::Audio, format!("A{}", index + 1));
        let clip = project
            .add_clip(
                lane,
                media,
                TimeRange::at_rate(0, 1000, MS),
                RationalTime::new(0, R24),
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

/// `frames` of `project`, mixed from `from` in blocks of [`BLOCK`].
fn mix(project: &Project, from: i64, frames: usize, realtime: bool) -> Vec<f32> {
    let mut mixer = ExportAudioMixer::for_project(project)
        .expect("the mix opens")
        .expect("the project is audible");
    mixer.set_realtime(realtime);
    let mut out = vec![0.0; frames * CHANNELS];
    for (index, block) in out.chunks_mut(BLOCK * CHANNELS).enumerate() {
        mixer
            .mix_into(from + (index * BLOCK) as i64, block)
            .expect("a block");
    }
    out
}

fn assert_close(got: &[f32], want: &[f32], case: &str) {
    assert_eq!(got.len(), want.len(), "{case}: length");
    assert!(
        want.iter().any(|sample| sample.abs() > 0.1),
        "{case}: the reference is silent, so it proves nothing"
    );
    let worst = got
        .iter()
        .zip(want)
        .map(|(got, want)| (got - want).abs())
        .fold(0.0_f32, f32::max);
    assert!(worst <= 1e-6, "{case}: a sample differs by {worst}");
}

/// The ids of every `PluginDeactivated` the context posts while the returned
/// registration lives.
fn deactivations(
    ctx: &PluginContext,
) -> (Arc<Mutex<Vec<&'static str>>>, aria_plugin::Registration) {
    let seen: Arc<Mutex<Vec<&'static str>>> = Arc::default();
    let sink = Arc::clone(&seen);
    let registration = ctx.on_event(move |event: &PluginDeactivated| sink.lock().push(event.id));
    (seen, registration)
}

// ------------------------------------------------------------------ cases --

#[test]
fn a_plugins_audio_effect_runs_in_the_mix_until_the_plugin_unmounts() {
    let ctx = PluginContext::new();
    let plugin = AudioPlugin::new("test.audio_listed", "plugin_audio.half", gain);
    ctx.mount(&plugin).expect("mounts");
    assert_eq!(registered("plugin_audio.half"), (true, true));

    let base = session(&[440.0]);
    let plain = mix(&base.project, 0, 4 * BLOCK, false);
    let mut project = base.project.clone();
    project
        .add_audio_effect(AudioInsertTarget::Master, "plugin_audio.half")
        .expect("the catalog lists the effect");
    let halved: Vec<f32> = plain.iter().map(|sample| sample * 0.5).collect();
    assert_close(&mix(&project, 0, 4 * BLOCK, false), &halved, "mounted");

    ctx.unmount("test.audio_listed").expect("unmounts");
    assert_eq!(registered("plugin_audio.half"), (false, false));
    // The project keeps the insert; with its spec gone it passes the sound.
    assert_close(&mix(&project, 0, 4 * BLOCK, false), &plain, "unmounted");
    let mut fresh = base.project;
    assert!(
        fresh
            .add_audio_effect(AudioInsertTarget::Master, "plugin_audio.half")
            .is_err(),
        "an unregistered effect cannot be added"
    );
}

#[test]
fn a_refused_audio_effect_leaves_nothing_registered() {
    let ctx = PluginContext::new();

    // The catalog refuses an id without a namespace.
    let refused = ctx.register_audio_effect(spec("nonamespace"), |_rate: u32| gain());
    assert!(
        matches!(&refused, Err(PluginError::AudioEffectRefused(_))),
        "{:?}",
        refused.err()
    );
    assert_eq!(registered("nonamespace"), (false, false));

    // A processor already registered under the id: the spec does not stay.
    let _taken =
        register_audio_processor("plugin_audio.taken", Arc::new(|_rate: u32| gain()), None)
            .expect("the bare processor registers");
    let refused = ctx.register_audio_effect(spec("plugin_audio.taken"), |_rate: u32| gain());
    let Err(PluginError::AudioEffectRefused(reason)) = refused else {
        panic!("a taken processor id is refused");
    };
    assert!(reason.contains("plugin_audio.taken"), "{reason}");
    assert!(audio_effect_spec("plugin_audio.taken").is_none());

    // Inside a mount the refusal fails the mount and drops what came before.
    let plugin = AudioPlugin::new("test.audio_refused", "plugin_audio.taken", gain);
    assert!(ctx.mount(&plugin).is_err());
    assert!(!ctx.plugins().contains(&"test.audio_refused"));
    assert!(ctx.try_get::<Marker>().is_none());
    let failure = ctx
        .failures()
        .into_iter()
        .find(|failure| failure.id == "test.audio_refused")
        .expect("the failed mount is recorded");
    assert!(
        failure.reason.contains("audio effect refused"),
        "{}",
        failure.reason
    );
}

#[test]
fn a_processor_panic_fails_its_plugin_and_drops_every_registration() {
    let ctx = PluginContext::new();
    let (seen, _watch) = deactivations(&ctx);
    let plugin = AudioPlugin::new("test.audio_crash", "plugin_audio.panics", panics);
    ctx.mount(&plugin).expect("mounts");
    assert!(ctx.try_get::<Marker>().is_some());

    let base = session(&[440.0]);
    let plain = mix(&base.project, 0, 2 * BLOCK, false);
    let mut project = base.project;
    project
        .add_audio_effect(AudioInsertTarget::Master, "plugin_audio.panics")
        .expect("listed");
    // The block the processor panicked in is silenced; every later block
    // passes the insert by.
    let heard = mix(&project, 0, 2 * BLOCK, false);
    let (faulted, after) = heard.split_at(BLOCK * CHANNELS);
    assert!(
        faulted.iter().all(|sample| *sample == 0.0),
        "the faulting block is silent"
    );
    assert_close(
        after,
        &plain[BLOCK * CHANNELS..],
        "bypassed after the panic",
    );

    // The plugin is failed on the delivery thread, which then posts its
    // deactivation: two rounds.
    assert!(ctx.events().flush(FLUSH));
    assert!(ctx.events().flush(FLUSH));
    assert!(!ctx.plugins().contains(&"test.audio_crash"));
    assert_eq!(registered("plugin_audio.panics"), (false, false));
    assert!(ctx.try_get::<Marker>().is_none());
    assert_eq!(*seen.lock(), vec!["test.audio_crash"]);
    let failure = ctx
        .failures()
        .into_iter()
        .find(|failure| failure.id == "test.audio_crash")
        .expect("the failure is recorded");
    assert!(
        failure.reason.contains("plugin_audio.panics panicked")
            && failure.reason.contains("the processor faulted"),
        "{}",
        failure.reason
    );

    // Mounting again clears the failure and registers the effect anew.
    ctx.mount(&plugin).expect("mounts again");
    assert!(
        ctx.failures()
            .iter()
            .all(|failure| failure.id != "test.audio_crash")
    );
    assert_eq!(registered("plugin_audio.panics"), (true, true));
}

#[test]
fn missed_deadlines_report_a_fault_and_keep_the_plugin_mounted() {
    let ctx = PluginContext::new();
    let plugin = AudioPlugin::new("test.audio_slow", "plugin_audio.slow", slow);
    ctx.mount(&plugin).expect("mounts");
    let base = session(&[440.0]);
    let mut project = base.project;
    project
        .add_audio_effect(AudioInsertTarget::Master, "plugin_audio.slow")
        .expect("listed");

    // An export has no deadline.
    mix(&project, 0, 3 * BLOCK, false);
    assert!(ctx.faults().is_empty(), "{:?}", ctx.faults());

    mix(&project, 0, 3 * BLOCK, true);
    let faults = ctx.faults();
    let [PluginFault { plugin, message }] = faults.as_slice() else {
        panic!("one fault, got {faults:?}");
    };
    assert_eq!(*plugin, "test.audio_slow");
    assert!(
        message.contains("plugin_audio.slow missed three playback deadlines"),
        "{message}"
    );
    assert!(ctx.plugins().contains(&"test.audio_slow"));
    assert_eq!(registered("plugin_audio.slow"), (true, true));

    // A fault for a plugin that is not mounted is not kept.
    ctx.report_fault("test.not_mounted", "lost");
    assert_eq!(ctx.faults().len(), 1);

    ctx.unmount("test.audio_slow").expect("unmounts");
    assert!(ctx.faults().is_empty());
}

#[test]
fn a_dropped_context_takes_its_plugins_audio_effects_with_it() {
    let ctx = PluginContext::new();
    let plugin = AudioPlugin::new("test.audio_dropped", "plugin_audio.dropped", gain);
    ctx.mount(&plugin).expect("mounts");
    assert_eq!(registered("plugin_audio.dropped"), (true, true));
    drop(ctx);
    assert_eq!(registered("plugin_audio.dropped"), (false, false));
}

#[test]
fn the_audio_host_mixes_the_range_asked_for() {
    let base = session(&[440.0, 660.0]);
    let host = AudioHost::new();
    // Ticks 6..18 at 24 fps: samples 12 000..36 000.
    let range = TimeRange::at_rate(6, 12, R24);
    let mut starts = Vec::new();
    let mut got = Vec::new();
    host.mix(&base.project, AudioSource::Open, range, |at, block| {
        starts.push(at);
        got.extend_from_slice(block);
        ControlFlow::Continue(())
    })
    .expect("mixes");
    assert_eq!(starts.first(), Some(&12_000));
    let mut next = 12_000;
    for at in &starts {
        assert_eq!(*at, next, "blocks are contiguous");
        next += aria_plugin::AUDIO_HOST_BLOCK as i64;
    }
    let continuous = mix(&base.project, 0, 36_000, false);
    assert_close(&got, &continuous[12_000 * CHANNELS..], "sequence");

    // One clip: every other clip of its sequence is removed.
    let mut solo = base.project.clone();
    solo.timeline_mut()
        .track_mut(base.lanes[0])
        .expect("lane")
        .muted = true;
    let mut clip = Vec::new();
    host.mix(
        &base.project,
        AudioSource::Clip(base.clips[1]),
        range,
        |_, block| {
            clip.extend_from_slice(block);
            ControlFlow::Continue(())
        },
    )
    .expect("mixes");
    assert_close(
        &clip,
        &mix(&solo, 0, 36_000, false)[12_000 * CHANNELS..],
        "clip",
    );

    // Past the sound: silence covering the whole range.
    let mut tail = Vec::new();
    host.mix(
        &base.project,
        AudioSource::Sequence(None),
        TimeRange::at_rate(48, 24, R24),
        |_, block| {
            tail.extend_from_slice(block);
            ControlFlow::Continue(())
        },
    )
    .expect("mixes");
    assert_eq!(tail.len(), 48_000 * CHANNELS);
    assert!(tail.iter().all(|sample| *sample == 0.0));

    // A break stops the mix.
    let mut calls = 0;
    host.mix(&base.project, AudioSource::Open, range, |_, _| {
        calls += 1;
        ControlFlow::Break(())
    })
    .expect("mixes");
    assert_eq!(calls, 1);

    let missing = aria_models::ClipId::from_raw(9_999);
    let refused = host.mix(&base.project, AudioSource::Clip(missing), range, |_, _| {
        ControlFlow::Continue(())
    });
    assert!(
        matches!(refused, Err(RenderError::Model(ModelError::UnknownClip(clip))) if clip == missing),
        "{refused:?}"
    );
}
