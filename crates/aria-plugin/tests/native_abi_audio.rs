//! Integration tests for native C-ABI audio effects and declared WGSL shaders.
//!
//! Each test uses unique global effect and shader IDs because the audio effect
//! catalog, processor registry, and GPU shader registry are process-wide.

#![allow(unsafe_code)]

use std::fs;
use std::path::Path;
use std::sync::{Arc, LazyLock};
use std::thread;
use std::time::Duration;

use aria_models::{
    AudioInsertTarget, EffectParamKind, MediaSource, Project, Rational, RationalTime, TimeRange,
    TrackKind, audio_effect_spec, effect_spec,
};
use aria_plugin::external::{from_table, host_table};
use aria_plugin::package::{LIBRARY_DIR, MANIFEST_FILE, Package, library_name};
use aria_plugin::{PluginContext, PluginDeactivated, PluginFault};
use aria_plugin_abi::ARIA_API_MAJOR;
use aria_render::{ExportAudioMixer, audio_processor_registered};
use aria_sdk::{
    AudioBlock, AudioEffectSpec, AudioParam, AudioParamSpec, AudioProcessor, Host,
    Plugin as SdkPlugin, PluginRuntime,
};
use parking_lot::Mutex;

const RATE: u32 = 48_000;
const CHANNELS: usize = 2;
const BLOCK: usize = RATE as usize / 100;
const R24: Rational = Rational::FPS_24;
const MS: Rational = Rational::new(1000, 1);
/// Bound on waiting for the event delivery thread to run queued work.
const FLUSH: Duration = Duration::from_secs(10);

fn write_tone(path: &Path, duration_ms: i64, hz: f32) {
    let frames = (duration_ms as usize * RATE as usize) / 1000;
    let total_samples = frames * CHANNELS;
    let data_bytes = (total_samples * 2) as u32;
    let mut bytes = Vec::with_capacity(44 + data_bytes as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&(CHANNELS as u16).to_le_bytes());
    bytes.extend_from_slice(&RATE.to_le_bytes());
    bytes.extend_from_slice(&(RATE * CHANNELS as u32 * 2).to_le_bytes());
    bytes.extend_from_slice(&(CHANNELS as u16 * 2).to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_bytes.to_le_bytes());

    let step = std::f32::consts::TAU * hz / RATE as f32;
    for frame in 0..frames {
        let sample = ((frame as f32 * step).sin() * 16_000.0) as i16;
        for _ in 0..CHANNELS {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
    }
    fs::write(path, bytes).expect("write wav");
}

fn tone_project(dir: &Path, hz: f32) -> Project {
    let mut project = Project::new("native abi audio", R24);
    let wav = dir.join("tone.wav");
    write_tone(&wav, 1000, hz);
    let media = project.add_media(MediaSource::new(&wav, 0, 0, MS, 1000, true));
    let lane = project.add_track(TrackKind::Audio, "A1");
    project
        .add_clip(
            lane,
            media,
            TimeRange::at_rate(0, 1000, MS),
            RationalTime::new(0, R24),
        )
        .expect("add clip");
    project
}

fn mix(project: &Project, from: i64, frames: usize, realtime: bool) -> Vec<f32> {
    let mut mixer = ExportAudioMixer::for_project(project)
        .expect("open mixer")
        .expect("audible project");
    mixer.set_realtime(realtime);
    let mut out = vec![0.0; frames * CHANNELS];
    for (index, block) in out.chunks_mut(BLOCK * CHANNELS).enumerate() {
        mixer
            .mix_into(from + (index * BLOCK) as i64, block)
            .expect("mix block");
    }
    out
}

fn assert_close(got: &[f32], want: &[f32], case: &str) {
    assert_eq!(got.len(), want.len(), "{case}: length");
    assert!(
        want.iter().any(|sample| sample.abs() > 0.1),
        "{case}: reference is silent"
    );
    let worst = got
        .iter()
        .zip(want)
        .map(|(g, w)| (g - w).abs())
        .fold(0.0_f32, f32::max);
    assert!(worst <= 1e-6, "{case}: sample diff {worst}");
}

fn create_test_package(root: &Path, id: &str, extra_toml: &str) -> Package {
    let pkg_dir = root.join(id);
    let lib_dir = pkg_dir.join(LIBRARY_DIR);
    fs::create_dir_all(&lib_dir).expect("create lib dir");
    fs::write(lib_dir.join(library_name()), b"stub").expect("write stub lib");
    let toml = format!(
        r#"[plugin]
id = "{id}"
name = "Native Audio {id}"
version = "0.1.0"
api = "1"
{extra_toml}
"#
    );
    fs::write(pkg_dir.join(MANIFEST_FILE), toml).expect("write plugin.toml");
    Package::open(&pkg_dir).expect("open package")
}

#[derive(Clone, Copy)]
enum Mode {
    Gain,
    Panic,
    Slow,
}

struct NativeAudioGuest {
    effect_id: &'static str,
    mode: Mode,
}

struct GuestGainProcessor;

impl AudioProcessor for GuestGainProcessor {
    fn process(&mut self, params: &[AudioParam<'_>], block: &mut AudioBlock<'_>) {
        let gain = params.first().expect("slot 0");
        let (left, right) = block.channels();
        for (frame, (l, r)) in left.iter_mut().zip(right.iter_mut()).enumerate() {
            let factor = gain.at(frame);
            *l *= factor;
            *r *= factor;
        }
    }
}

struct GuestPanicProcessor;

impl AudioProcessor for GuestPanicProcessor {
    fn process(&mut self, _params: &[AudioParam<'_>], _block: &mut AudioBlock<'_>) {
        panic!("native guest audio panic");
    }
}

struct GuestSlowProcessor;

impl AudioProcessor for GuestSlowProcessor {
    fn process(&mut self, _params: &[AudioParam<'_>], _block: &mut AudioBlock<'_>) {
        thread::sleep(Duration::from_millis(25));
    }
}

impl SdkPlugin for NativeAudioGuest {
    fn mount(&self, host: &Host) -> Result<(), String> {
        let spec = AudioEffectSpec {
            id: self.effect_id.to_owned(),
            category: "Native Tools".to_owned(),
            label: "Native Gain".to_owned(),
            params: vec![AudioParamSpec::scalar("gain", "Gain", 0.5, 0.0, 2.0)],
            latency: 0,
            sidechain: false,
        };
        host.register_audio_effect(&spec)?;
        host.register_command(&format!("{}.ping", self.effect_id), "Ping")?;
        Ok(())
    }

    fn audio_create(
        &self,
        _host: &Host,
        effect_id: &str,
        _sample_rate: u32,
    ) -> Result<Box<dyn AudioProcessor>, String> {
        if effect_id != self.effect_id {
            return Err(format!("unexpected effect id {effect_id}"));
        }
        match self.mode {
            Mode::Gain => Ok(Box::new(GuestGainProcessor)),
            Mode::Panic => Ok(Box::new(GuestPanicProcessor)),
            Mode::Slow => Ok(Box::new(GuestSlowProcessor)),
        }
    }
}

#[macro_use]
#[path = "support/plugin_table.rs"]
mod plugin_table;

#[test]
fn native_abi_audio_effect_processes_samples_and_cleans_up_on_unmount() {
    static RUNTIME: LazyLock<PluginRuntime<NativeAudioGuest>> = LazyLock::new(|| {
        let rt = PluginRuntime::new(NativeAudioGuest {
            effect_id: "native_abi.gain",
            mode: Mode::Gain,
        });
        // SAFETY: `host_table()` points to the valid static `AriaHostV1` table.
        assert!(unsafe { rt.bind_host(host_table() as *const _, ARIA_API_MAJOR) });
        rt
    });

    let tmp = tempfile::tempdir().expect("tempdir");
    let pkg = create_test_package(tmp.path(), "com.test.native-gain", "");
    let plugin = from_table(&pkg, plugin_table!(RUNTIME), Some(tmp.path())).expect("plugin");

    let ctx = PluginContext::new();
    ctx.mount(plugin.as_ref()).expect("mount gain plugin");
    let spec = audio_effect_spec("native_abi.gain").expect("spec registered");
    assert_eq!(
        spec.category(),
        "Native Tools",
        "the guest's category is listed"
    );
    assert_eq!(spec.params.len(), 1);
    assert_eq!(spec.params[0].kind, EffectParamKind::Scalar);
    assert!((spec.params[0].default - 0.5).abs() < 1e-6);
    assert!((spec.params[0].min - 0.0).abs() < 1e-6);
    assert!((spec.params[0].max - 2.0).abs() < 1e-6);
    assert!(audio_processor_registered("native_abi.gain"));

    let base_project = tone_project(tmp.path(), 440.0);
    let plain = mix(&base_project, 0, 4 * BLOCK, false);

    let mut project = base_project;
    project
        .add_audio_effect(AudioInsertTarget::Master, "native_abi.gain")
        .expect("add native_abi.gain");
    let halved: Vec<f32> = plain.iter().map(|s| s * 0.5).collect();
    assert_close(&mix(&project, 0, 4 * BLOCK, false), &halved, "default 0.5x");

    ctx.unmount("com.test.native-gain").expect("unmount");
    assert!(audio_effect_spec("native_abi.gain").is_none());
    assert!(!audio_processor_registered("native_abi.gain"));
    assert_close(&mix(&project, 0, 4 * BLOCK, false), &plain, "after unmount");
}

#[test]
fn native_abi_audio_processor_panic_fails_plugin_and_drops_registrations() {
    static RUNTIME: LazyLock<PluginRuntime<NativeAudioGuest>> = LazyLock::new(|| {
        let rt = PluginRuntime::new(NativeAudioGuest {
            effect_id: "native_abi.panic",
            mode: Mode::Panic,
        });
        // SAFETY: `host_table()` points to the valid static `AriaHostV1` table.
        assert!(unsafe { rt.bind_host(host_table() as *const _, ARIA_API_MAJOR) });
        rt
    });

    let tmp = tempfile::tempdir().expect("tempdir");
    let pkg = create_test_package(tmp.path(), "com.test.native-panic", "");
    let plugin = from_table(&pkg, plugin_table!(RUNTIME), Some(tmp.path())).expect("plugin");

    let ctx = PluginContext::new();
    let deactivated: Arc<Mutex<Vec<String>>> = Arc::default();
    let sink = Arc::clone(&deactivated);
    let _sub = ctx.on_event(move |ev: &PluginDeactivated| sink.lock().push(ev.id.to_string()));

    ctx.mount(plugin.as_ref()).expect("mount panic plugin");
    assert!(
        ctx.commands()
            .iter()
            .any(|c| c.id == "native_abi.panic.ping")
    );

    let base_project = tone_project(tmp.path(), 440.0);
    let plain = mix(&base_project, 0, 4 * BLOCK, false);

    let mut project = base_project;
    project
        .add_audio_effect(AudioInsertTarget::Master, "native_abi.panic")
        .expect("add native_abi.panic");
    let mixed = mix(&project, 0, 4 * BLOCK, false);

    // Block 0 faulted and was zeroed; blocks 1..4 bypassed cleanly.
    assert!(
        mixed[..BLOCK * CHANNELS].iter().all(|&s| s == 0.0),
        "faulting block must be silenced"
    );
    assert_close(
        &mixed[BLOCK * CHANNELS..],
        &plain[BLOCK * CHANNELS..],
        "subsequent blocks bypass",
    );

    // The audio thread defers the failure to the event delivery thread, which
    // fails the plugin and then posts its deactivation: two rounds.
    assert!(ctx.events().flush(FLUSH));
    assert!(ctx.events().flush(FLUSH));
    assert!(!ctx.is_mounted("com.test.native-panic"));
    assert!(
        !ctx.commands()
            .iter()
            .any(|c| c.id == "native_abi.panic.ping")
    );
    assert!(audio_effect_spec("native_abi.panic").is_none());
    assert!(!audio_processor_registered("native_abi.panic"));
    assert_eq!(
        deactivated.lock().as_slice(),
        &["com.test.native-panic".to_owned()]
    );
    assert!(
        ctx.failures().iter().any(|f| {
            f.id == "com.test.native-panic" && f.reason.contains("native guest audio panic")
        }),
        "failures: {:?}",
        ctx.failures()
    );
}

#[test]
fn native_abi_audio_missed_deadlines_report_fault_without_unmounting() {
    static RUNTIME: LazyLock<PluginRuntime<NativeAudioGuest>> = LazyLock::new(|| {
        let rt = PluginRuntime::new(NativeAudioGuest {
            effect_id: "native_abi.slow",
            mode: Mode::Slow,
        });
        // SAFETY: `host_table()` points to the valid static `AriaHostV1` table.
        assert!(unsafe { rt.bind_host(host_table() as *const _, ARIA_API_MAJOR) });
        rt
    });

    let tmp = tempfile::tempdir().expect("tempdir");
    let pkg = create_test_package(tmp.path(), "com.test.native-slow", "");
    let plugin = from_table(&pkg, plugin_table!(RUNTIME), Some(tmp.path())).expect("plugin");

    let ctx = PluginContext::new();
    ctx.mount(plugin.as_ref()).expect("mount slow plugin");

    let mut project = tone_project(tmp.path(), 440.0);
    project
        .add_audio_effect(AudioInsertTarget::Master, "native_abi.slow")
        .expect("add native_abi.slow");

    mix(&project, 0, 3 * BLOCK, true);
    let faults = ctx.faults();
    let [
        PluginFault {
            plugin: fault_plugin,
            message,
        },
    ] = faults.as_slice()
    else {
        panic!("expected one fault, got {faults:?}");
    };
    assert_eq!(fault_plugin, "com.test.native-slow");
    assert!(
        message.contains("native_abi.slow missed three playback deadlines"),
        "{message}"
    );
    assert!(ctx.is_mounted("com.test.native-slow"));
    ctx.unmount("com.test.native-slow").expect("unmount");
}

#[test]
fn native_abi_package_registers_declared_wgsl_effects_before_guest_mount() {
    static RUNTIME: LazyLock<PluginRuntime<NativeAudioGuest>> = LazyLock::new(|| {
        let rt = PluginRuntime::new(NativeAudioGuest {
            effect_id: "native_abi.with_wgsl",
            mode: Mode::Gain,
        });
        // SAFETY: `host_table()` points to the valid static `AriaHostV1` table.
        assert!(unsafe { rt.bind_host(host_table() as *const _, ARIA_API_MAJOR) });
        rt
    });

    let tmp = tempfile::tempdir().expect("tempdir");
    let extra = r#"
[[effects]]
id = "declared-tint"
name = "Declared Tint"
shader = "shaders/tint.wgsl"
"#;
    let pkg_dir = tmp.path().join("com.test.native-wgsl");
    fs::create_dir_all(pkg_dir.join("shaders")).expect("create shaders dir");
    let shader_src = "@fragment\nfn fs(in: VsOut) -> @location(0) vec4<f32> {\n    return vec4<f32>(in.uv, 0.0, 1.0);\n}";
    fs::write(pkg_dir.join("shaders/tint.wgsl"), shader_src).expect("write wgsl");

    let pkg = create_test_package(tmp.path(), "com.test.native-wgsl", extra);
    let plugin = from_table(&pkg, plugin_table!(RUNTIME), Some(tmp.path())).expect("plugin");

    let ctx = PluginContext::new();
    ctx.mount(plugin.as_ref())
        .expect("mount wgsl+native plugin");
    let declared = effect_spec("com.test.native-wgsl.declared-tint").expect("declared effect");
    assert_eq!(declared.label, "Declared Tint");
    assert!(audio_effect_spec("native_abi.with_wgsl").is_some());

    ctx.unmount("com.test.native-wgsl").expect("unmount");
    assert!(effect_spec("com.test.native-wgsl.declared-tint").is_none());
    assert!(audio_effect_spec("native_abi.with_wgsl").is_none());
}
