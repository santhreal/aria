//! End-to-end integration tests for `aria-plugin-wasm`.
//!
//! Defect class closed: WIT host/guest contract drift, capability/permission
//! bypasses (including unscoped `fs-write` outside an active encode target),
//! fuel/memory/deadline exhaustion hanging the host, and guest traps leaving
//! half-registered contributions mounted instead of failing the plugin.
//!
//! What this does not catch: native C-ABI shared library loading (`aria-plugin`)
//! or GPU shader execution (`aria-compositor`).
//!
//! Exercises the compiled reference WASM component (`examples/wasm-plugin`) against a
//! real `PluginContext`, `ProjectHost`, `Engine`, `EditorView`, `SettingsService`, and
//! `ExportAudioMixer`. Because audio effect registration uses process-wide catalogs in
//! `aria-models` and `aria-render`, tests in this binary serialize on `SERIAL`.

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use aria_commands::{Command, EditCommand};
use aria_compositor::{
    CompositeLayer, Compositor, CompositorConfig, CompositorLayer, CpuFrameIn, CpuFrameOut,
    F16_ONE, GpuContext, LayerPlacement, PassInstance, RgbaImage, f32_to_f16, pass_id,
    plugin_texture, registered_effect, renders_effect, renders_transition,
};
use aria_core::{
    AudioEncoderConfig, ColorSpace, CpuImage, EncoderConfig, FrameData, PixelFormat, Plane,
    Rational, RationalTime, Rect, Rotation, TimeRange, VideoFrame,
};
use aria_engine::{Engine, EngineConfig};
use aria_models::{
    AudioInsertTarget, CanvasAspect, ClipId, ClipParam, DriverCode, EffectBounds, Generator,
    MarkerColor, MediaSource, Param, ParamValue, Project, TrackKind, audio_effect_spec,
    clip_type_spec, effect_spec, transition_spec,
};
use aria_plugin::package::Package;
use aria_plugin::{
    AiToolError, AiToolOutput, AiToolTier, CommandError, DragPhase, DriverClipInfo, DriverScope,
    DropAction, EditorView, HandleLook, InputClip, JobManager, JobSnapshot, JobState, Origin,
    OverlayColor, OverlayEvent, OverlayShape, PanelEvent, PanelLocation, PanelSpec, PlayheadMoved,
    PluginContext, PluginDeactivated, PluginError, ProjectHost, Registration, ResolveDropped,
    ScopeError, ScopeFrame, ScopeImage, Service, SettingsService, TransactError, UiNode, ViewFrame,
};
use aria_plugin_wasm::{Limits, LoadError, ReloadOutcome, Reloader, load};
use aria_render::{
    EXPORT_AUDIO_CHANNELS, EXPORT_AUDIO_RATE, ExportAudioMixer, audio_processor_registered,
};
use parking_lot::Mutex;

static SERIAL: Mutex<()> = Mutex::new(());

const FIXTURE_WASM: &[u8] = include_bytes!("fixtures/example_plugin.wasm");
const MARKER_ICON_SVG: &str = include_str!("../../../examples/wasm-plugin/icons/marker.svg");
const LEGACY_FIXTURE_WASM: &[u8] = include_bytes!("fixtures/legacy_v1_0_plugin.wasm");
const FLUSH_TIMEOUT: Duration = Duration::from_secs(2);
const BLOCK_FRAMES: usize = 256;

struct TestContext {
    ctx: PluginContext,
    _services: Vec<Registration>,
}

impl std::ops::Deref for TestContext {
    type Target = PluginContext;

    fn deref(&self) -> &Self::Target {
        &self.ctx
    }
}

impl Drop for TestContext {
    fn drop(&mut self) {
        if self.ctx.is_mounted("com.example.wasm-tools") {
            let _ = self.ctx.unmount("com.example.wasm-tools");
        }
    }
}

struct TestSettings {
    values: Mutex<HashMap<String, serde_json::Value>>,
}

impl TestSettings {
    fn new() -> Self {
        Self {
            values: Mutex::new(HashMap::new()),
        }
    }
}

impl Service for TestSettings {}

impl SettingsService for TestSettings {
    fn get(&self, key: &str) -> Option<serde_json::Value> {
        self.values.lock().get(key).cloned()
    }

    fn set(&self, key: &str, value: serde_json::Value) {
        self.values.lock().insert(key.to_owned(), value);
    }

    fn save(&self) -> Result<(), PluginError> {
        Ok(())
    }
}

fn manifest_toml(project_perm: &str, fs_scopes: &[&str]) -> String {
    let fs_list = fs_scopes
        .iter()
        .map(|s| format!("{s:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        r#"[plugin]
id = "com.example.wasm-tools"
name = "Example WASM Tools"
version = "0.1.0"
api = 1
publisher = "Aria Tests"
description = "Test WASM component package"
license = "Apache-2.0"

[permissions]
project = {project_perm:?}
fs = [{fs_list}]

[[commands]]
id = "example.add-marker"
title = "Add Example Marker"
icon = "icons/marker.svg"
enablement = "!playing"
"#
    )
}

fn write_package(dir: &Path, project_perm: &str, fs_scopes: &[&str], wasm_bytes: &[u8]) -> Package {
    fs::create_dir_all(dir).expect("create package dir");
    let icons_dir = dir.join("icons");
    fs::create_dir_all(&icons_dir).expect("create icons dir");
    fs::write(icons_dir.join("marker.svg"), MARKER_ICON_SVG).expect("write marker.svg");
    fs::write(
        dir.join("plugin.toml"),
        manifest_toml(project_perm, fs_scopes),
    )
    .expect("write plugin.toml");
    fs::write(dir.join("plugin.wasm"), wasm_bytes).expect("write plugin.wasm");
    Package::open(dir).expect("open valid wasm package")
}

fn setup_host_context() -> (
    TestContext,
    Arc<ProjectHost>,
    Arc<EditorView>,
    Arc<TestSettings>,
) {
    let ctx = PluginContext::new();
    let engine = Engine::new(EngineConfig::default()).expect("engine");
    let host = Arc::new(ProjectHost::new(Arc::new(Mutex::new(engine)), &ctx));
    let host_reg = ctx.register_arc(Arc::clone(&host));

    let view = Arc::new(EditorView::new());
    view.publish(
        ViewFrame {
            selection: &[ClipId::from_raw(7)],
            playhead: RationalTime::new(15, Rational::FPS_24),
            mark_in: Some(RationalTime::new(0, Rational::FPS_24)),
            mark_out: Some(RationalTime::new(90, Rational::FPS_24)),
            playing: false,
            speed: 1,
        },
        ctx.events(),
    );
    let view_reg = ctx.register_arc(Arc::clone(&view));

    let settings = Arc::new(TestSettings::new());
    let settings_dyn: Arc<dyn SettingsService> = settings.clone();
    let settings_reg = ctx.register_arc(settings_dyn);

    (
        TestContext {
            ctx,
            _services: vec![host_reg, view_reg, settings_reg],
        },
        host,
        view,
        settings,
    )
}

fn write_wav_tone(path: &Path, duration_ms: u64, hz: f32) {
    let sample_rate = EXPORT_AUDIO_RATE;
    let channels = EXPORT_AUDIO_CHANNELS;
    let frames = (duration_ms * u64::from(sample_rate) / 1000) as usize;
    let samples_len = frames * usize::from(channels);
    let data_bytes = (samples_len * 2) as u32;
    let mut wav = Vec::with_capacity(44 + data_bytes as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&channels.to_le_bytes());
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    let byte_rate = sample_rate * u32::from(channels) * 2;
    wav.extend_from_slice(&byte_rate.to_le_bytes());
    let block_align = channels * 2;
    wav.extend_from_slice(&block_align.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_bytes.to_le_bytes());
    for frame in 0..frames {
        let t = frame as f32 / sample_rate as f32;
        let v = ((t * hz * std::f32::consts::TAU).sin() * 0.5 * f32::from(i16::MAX)) as i16;
        for _ in 0..channels {
            wav.extend_from_slice(&v.to_le_bytes());
        }
    }
    fs::write(path, wav).expect("write wav tone");
}

/// Mounting a WASM package registers commands, importers, exporters, audio effects,
/// and event subscriptions; running commands edits the project, reads snapshots/views/settings,
/// undoes edits, and writes plugin storage.
#[test]
fn command_execution_settings_snapshot_undo_and_storage_work_end_to_end() {
    let _serial = SERIAL.lock();
    let temp = tempfile::tempdir().expect("tempdir");
    let pkg = write_package(temp.path(), "read-write", &["pick"], FIXTURE_WASM);
    let (ctx, host, _view, settings) = setup_host_context();

    let plugin = load(&pkg, &Limits::default()).expect("load wasm plugin");
    ctx.mount(plugin.as_ref()).expect("mount wasm plugin");
    assert!(ctx.is_mounted("com.example.wasm-tools"));
    let logs = ctx.plugin_logs("com.example.wasm-tools");
    assert!(
        logs.iter().any(|line| {
            line.level == aria_plugin::LogLevel::Info
                && &*line.text == "mounting com.example.wasm-tools"
        }),
        "expected Info log 'mounting com.example.wasm-tools', got: {logs:?}"
    );

    let example_pkg_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/wasm-plugin");
    let example_pkg = Package::open(&example_pkg_dir).expect("open examples/wasm-plugin package");
    let declared_marker =
        example_pkg.command_spec("example.add-marker".to_owned(), "Add Marker".to_owned());
    assert!(declared_marker.icon.is_some());
    assert_eq!(
        declared_marker
            .enablement
            .as_ref()
            .map(ToString::to_string)
            .as_deref(),
        Some("!playing")
    );

    let add_marker_spec = ctx
        .commands()
        .into_iter()
        .find(|s| s.id == "example.add-marker")
        .expect("registered example.add-marker spec");
    assert!(add_marker_spec.icon.is_some());
    assert_eq!(
        add_marker_spec
            .enablement
            .as_ref()
            .map(ToString::to_string)
            .as_deref(),
        Some("!playing")
    );

    // Configure setting `example.marker_color` to `"pink"` and run `example.add-marker`.
    settings.set("example.marker_color", serde_json::json!("pink"));
    ctx.run_command("example.add-marker")
        .expect("run example.add-marker");

    let snap = host.snapshot();
    let markers = snap.timeline().markers();
    assert_eq!(markers.len(), 1);
    assert_eq!(markers[0].name, "wasm-marker-v1");
    assert_eq!(markers[0].color, MarkerColor::Pink);
    assert_eq!(markers[0].tick, RationalTime::new(24, Rational::FPS_24));

    // Verify receipt was written to plugin storage (`<pkg>/plugin-data/com.example.wasm-tools/last-receipt.json`).
    let data_dir = pkg.data_dir(temp.path());
    let receipt_bytes = fs::read(data_dir.join("last-receipt.json")).expect("receipt stored");
    let receipt_json: serde_json::Value =
        serde_json::from_slice(&receipt_bytes).expect("valid receipt json");
    assert_eq!(receipt_json["revision"], 1);

    // Run `example.inspect-and-undo`, which calls `project-snapshot`, `editor-view`, and `project-undo`.
    ctx.run_command("example.inspect-and-undo")
        .expect("run inspect-and-undo");
    assert!(
        host.snapshot().timeline().markers().is_empty(),
        "project-undo reverted the marker addition"
    );
    let inspection_bytes =
        fs::read(data_dir.join("last-inspection.json")).expect("inspection stored");
    let inspection: serde_json::Value =
        serde_json::from_slice(&inspection_bytes).expect("valid inspection json");
    assert_eq!(inspection["undid"], true);
    assert_eq!(inspection["playhead"]["value"], 15);

    // Run `example.storage-roundtrip` to verify `storage-set`, `storage-get`, `storage-keys`, `storage-delete`.
    ctx.run_command("example.storage-roundtrip")
        .expect("run storage-roundtrip");
    assert!(!data_dir.join("alpha").exists(), "alpha was deleted");
    assert_eq!(
        fs::read(data_dir.join("beta")).expect("beta exists"),
        b"two"
    );
    let check_json: serde_json::Value = serde_json::from_slice(
        &fs::read(data_dir.join("storage-check.json")).expect("storage-check.json"),
    )
    .expect("valid json");
    let keys = check_json["keys_before_delete"]
        .as_array()
        .expect("keys array")
        .iter()
        .filter_map(serde_json::Value::as_str)
        .collect::<Vec<_>>();
    assert!(keys.contains(&"alpha") && keys.contains(&"beta"));

    // Registration outside `mount` is refused without faulting the plugin.
    let late = ctx.run_command("example.late-register");
    assert_eq!(
        late,
        Err(CommandError::Failed(
            "com.example.wasm-tools: register-command is permitted only during mount".to_owned()
        ))
    );
    assert!(ctx.is_mounted("com.example.wasm-tools"));

    ctx.unmount("com.example.wasm-tools").expect("unmount");
}

/// Importing a `.mrk` file applies all parsed `AddMarker` commands as a single undoable
/// transaction on `ProjectHost`, and exporting writes the exact `.mrk` bytes to disk.
#[test]
fn import_applies_commands_in_one_undo_step_and_export_writes_exact_bytes() {
    let _serial = SERIAL.lock();
    let temp = tempfile::tempdir().expect("tempdir");
    let pkg = write_package(temp.path(), "read-write", &["pick"], FIXTURE_WASM);
    let (ctx, host, _view, _settings) = setup_host_context();

    let plugin = load(&pkg, &Limits::default()).expect("load wasm plugin");
    ctx.mount(plugin.as_ref()).expect("mount wasm plugin");

    let input_path = temp.path().join("scene.mrk");
    fs::write(
        &input_path,
        "# comment line\n12 teal Opening Shot\nmalformed_line\n45 orange Cut To Wide\n",
    )
    .expect("write input .mrk");

    let (report, _receipt) = ctx
        .import_file(&host, &input_path)
        .expect("import .mrk file");
    assert_eq!(report.skipped, vec!["malformed_line".to_owned()]);

    let markers = host.snapshot().timeline().markers().to_vec();
    assert_eq!(markers.len(), 2);
    assert_eq!(markers[0].tick, RationalTime::new(12, Rational::FPS_24));
    assert_eq!(markers[0].color, MarkerColor::Teal);
    assert_eq!(markers[0].name, "Opening Shot");
    assert_eq!(markers[1].tick, RationalTime::new(45, Rational::FPS_24));
    assert_eq!(markers[1].color, MarkerColor::Orange);
    assert_eq!(markers[1].name, "Cut To Wide");

    // Export the project to `.mrk` and assert exact bytes.
    let output_path = temp.path().join("exported.mrk");
    let export_report = ctx
        .export_file("example.markers", host.snapshot().as_ref(), &output_path)
        .expect("export .mrk file");
    assert!(export_report.skipped.is_empty());
    let exported_text = fs::read_to_string(&output_path).expect("read exported .mrk");
    assert_eq!(
        exported_text,
        "12 teal Opening Shot\n45 orange Cut To Wide\n"
    );

    // One undo reverts BOTH imported markers in a single step.
    assert!(host.undo().expect("undo import transaction"));
    assert!(
        host.snapshot().timeline().markers().is_empty(),
        "both imported markers must undo in one step"
    );

    ctx.unmount("com.example.wasm-tools").expect("unmount");
}

/// Subscribed notification, waterfall, and serial events are delivered to the WASM guest.
#[test]
fn event_subscriptions_handle_notifications_waterfalls_and_serial_claims() {
    let _serial = SERIAL.lock();
    let temp = tempfile::tempdir().expect("tempdir");
    let pkg = write_package(temp.path(), "read-write", &["pick"], FIXTURE_WASM);
    let (ctx, host, _view, _settings) = setup_host_context();

    let plugin = load(&pkg, &Limits::default()).expect("load wasm plugin");
    ctx.mount(plugin.as_ref()).expect("mount wasm plugin");

    // Post PlayheadMoved and flush the event bus.
    ctx.events().post(PlayheadMoved {
        at: RationalTime::new(60, Rational::FPS_24),
    });
    assert!(ctx.events().flush(FLUSH_TIMEOUT));

    let data_dir = pkg.data_dir(temp.path());
    let count_str = fs::read_to_string(data_dir.join("event-count")).expect("event-count written");
    let count: u64 = count_str.trim().parse().expect("u64 count");
    assert!(count >= 1);

    // Waterfall `CommandProposed`: a marker named `"veto-by-wasm"` is refused by the guest.
    let vetoed = host.transact(
        Origin::Editor,
        "Add marker",
        vec![Command::Edit(EditCommand::AddMarker {
            at: RationalTime::new(10, Rational::FPS_24),
            name: "veto-by-wasm".to_owned(),
            color: Some(MarkerColor::Red),
        })],
    );
    assert_eq!(
        vetoed,
        Err(TransactError::Refused {
            label: "Add marker",
            reason: "vetoed by wasm plugin".to_owned(),
        })
    );

    // Serial `ResolveDropped`: dropping a `.mrk` path is claimed as `ImportMedia`,
    // and dropping an `https://` URL is claimed as `Handled`.
    let claimed: Option<DropAction> = ctx
        .events()
        .serial(&ResolveDropped::path(temp.path().join("markers.mrk")));
    assert_eq!(claimed, Some(DropAction::ImportMedia));
    let claimed_url: Option<DropAction> = ctx
        .events()
        .serial(&ResolveDropped::url("https://example.invalid/clip"));
    assert_eq!(claimed_url, Some(DropAction::Handled));
    ctx.unmount("com.example.wasm-tools").expect("unmount");
}

/// Registered WASM audio effect processes stereo blocks, reports 3 consecutive missed
/// deadlines without unmounting, and faults the plugin on a processor trap.
#[test]
fn audio_effect_processes_samples_and_handles_missed_deadlines_and_traps() {
    let _serial = SERIAL.lock();
    let temp = tempfile::tempdir().expect("tempdir");
    let pkg = write_package(temp.path(), "read-write", &["pick"], FIXTURE_WASM);
    let (ctx, _host, _view, _settings) = setup_host_context();

    let plugin = load(&pkg, &Limits::default()).expect("load wasm plugin");
    ctx.mount(plugin.as_ref()).expect("mount wasm plugin");
    assert!(audio_effect_spec("example.gain").is_some());
    assert!(audio_processor_registered("example.gain"));

    // Build an audible project with a 440 Hz tone on an audio lane.
    let wav_path = temp.path().join("tone.wav");
    write_wav_tone(&wav_path, 500, 440.0);
    let mut base_project = Project::new("audio test", Rational::FPS_24);
    let media = base_project.add_media(MediaSource::new(
        &wav_path,
        0,
        0,
        Rational::new(1, 1000),
        500,
        true,
    ));
    let lane = base_project.add_track(TrackKind::Audio, "A1");
    base_project
        .add_clip(
            lane,
            media,
            TimeRange::at_rate(0, 500, Rational::new(1, 1000)),
            RationalTime::new(0, Rational::FPS_24),
        )
        .expect("add audio clip");

    let mix_blocks = |project: &Project, blocks: usize, realtime: bool| -> Vec<f32> {
        let mut mixer = ExportAudioMixer::for_project(project)
            .expect("mixer opens")
            .expect("project is audible");
        mixer.set_realtime(realtime);
        let channels = usize::from(EXPORT_AUDIO_CHANNELS);
        let total_frames = blocks * BLOCK_FRAMES;
        let mut out = vec![0.0; total_frames * channels];
        for (idx, chunk) in out.chunks_mut(BLOCK_FRAMES * channels).enumerate() {
            mixer
                .mix_into((idx * BLOCK_FRAMES) as i64, chunk)
                .expect("mix block");
        }
        out
    };

    let dry = mix_blocks(&base_project, 4, false);
    assert!(dry.iter().any(|s| s.abs() > 0.1));

    // Add `example.gain` with `gain = 0.5` on Master and assert exact halved samples.
    let mut gain_project = base_project.clone();
    gain_project
        .add_audio_effect(AudioInsertTarget::Master, "example.gain")
        .expect("add example.gain");
    gain_project
        .set_audio_effect_param(AudioInsertTarget::Master, 0, 0, Param::Constant(0.5))
        .expect("set gain = 0.5");

    let wet = mix_blocks(&gain_project, 4, false);
    let max_diff = dry
        .iter()
        .zip(wet.iter())
        .map(|(d, w)| (d * 0.5 - w).abs())
        .fold(0.0_f32, f32::max);
    assert!(
        max_diff <= 1e-6,
        "expected exact 0.5x gain, max_diff={max_diff}"
    );

    // Set `gain = -1.0` (slow block mode): 3 missed deadlines report a fault and bypass the insert.
    let mut slow_project = gain_project.clone();
    slow_project
        .set_audio_effect_param(AudioInsertTarget::Master, 0, 0, Param::Constant(-1.0))
        .expect("set gain = -1.0");
    let _ = mix_blocks(&slow_project, 4, true);
    let faults = ctx.faults();
    assert_eq!(faults.len(), 1);
    assert_eq!(faults[0].plugin, "com.example.wasm-tools");
    assert!(
        faults[0]
            .message
            .contains("example.gain missed three playback deadlines"),
        "unexpected fault message: {}",
        faults[0].message
    );
    assert!(
        ctx.is_mounted("com.example.wasm-tools"),
        "missed audio deadlines bypass the insert without unmounting the plugin"
    );

    // Set `gain = -2.0` (trap mode): the processor traps, silences the block, and fails the plugin.
    let mut trap_project = gain_project;
    trap_project
        .set_audio_effect_param(AudioInsertTarget::Master, 0, 0, Param::Constant(-2.0))
        .expect("set gain = -2.0");
    let trapped_out = mix_blocks(&trap_project, 2, false);
    let block_samples = BLOCK_FRAMES * usize::from(EXPORT_AUDIO_CHANNELS);
    let (first_block, second_block) = trapped_out.split_at(block_samples);
    assert!(
        first_block.iter().all(|s| *s == 0.0),
        "faulting block is silenced"
    );
    let bypass_diff = second_block
        .iter()
        .zip(&dry[block_samples..2 * block_samples])
        .map(|(got, want)| (got - want).abs())
        .fold(0.0_f32, f32::max);
    assert!(
        bypass_diff <= 1e-6,
        "second block bypasses the faulted processor"
    );

    assert!(ctx.events().flush(FLUSH_TIMEOUT));
    assert!(ctx.events().flush(FLUSH_TIMEOUT));
    assert!(!ctx.is_mounted("com.example.wasm-tools"));
    assert!(audio_effect_spec("example.gain").is_none());
    assert!(!audio_processor_registered("example.gain"));
}

/// Denied permissions return the exact error string, record a `CapabilityDenial` in
/// `PluginContext::denials()`, and leave the plugin mounted.
#[test]
fn permission_denials_return_exact_error_and_record_capability_denial() {
    let _serial = SERIAL.lock();
    let temp = tempfile::tempdir().expect("tempdir");

    // Case 1: `project = "none"` denies both read and write project operations.
    let pkg_none = write_package(temp.path(), "none", &[], FIXTURE_WASM);
    let (ctx, _host, _view, _settings) = setup_host_context();
    let plugin = load(&pkg_none, &Limits::default()).expect("load wasm plugin");
    ctx.mount(plugin.as_ref()).expect("mount wasm plugin");

    let err_write = ctx
        .run_command("example.add-marker")
        .expect_err("project write must fail when project = none");
    assert_eq!(
        err_write,
        CommandError::Failed(
            "permission denied: com.example.wasm-tools: project write requires permissions.project = \"read-write\""
                .to_owned()
        )
    );
    assert_eq!(ctx.denials().len(), 1);
    assert_eq!(ctx.denials()[0].plugin, "com.example.wasm-tools");
    assert!(ctx.denials()[0].service.contains("ProjectHost"));

    let err_fs = ctx
        .run_command("example.read-unpermitted-path")
        .expect_err("unpermitted fs read must fail");
    assert_eq!(
        err_fs,
        CommandError::Failed(
            "permission denied: com.example.wasm-tools: fs read for \"/etc/hosts\" is not granted by permissions.fs"
                .to_owned()
        )
    );
    assert_eq!(ctx.denials().len(), 2);
    assert!(ctx.denials()[1].service.contains("FsPermission"));
    assert!(ctx.is_mounted("com.example.wasm-tools"));
    ctx.unmount("com.example.wasm-tools").expect("unmount");

    // Case 2: `project = "read"` permits `project-snapshot` and `editor-view`, but denies `project-undo` / `project-transact`.
    let pkg_read = write_package(temp.path(), "read", &[], FIXTURE_WASM);
    let plugin_read = load(&pkg_read, &Limits::default()).expect("load wasm plugin");
    ctx.mount(plugin_read.as_ref()).expect("mount wasm plugin");

    let err_readonly = ctx
        .run_command("example.inspect-and-undo")
        .expect_err("project-undo must fail when project = read");
    assert_eq!(
        err_readonly,
        CommandError::Failed(
            "permission denied: com.example.wasm-tools: project write requires permissions.project = \"read-write\" (have \"read\")"
                .to_owned()
        )
    );
    assert_eq!(ctx.denials().len(), 1);
    assert!(ctx.denials()[0].service.contains("ProjectWritePermission"));
    ctx.unmount("com.example.wasm-tools").expect("unmount");
}

/// An infinite-loop guest export is terminated by fuel exhaustion or wall-clock epoch
/// deadline within a bounded time, marking the plugin failed and dropping all registrations.
#[test]
fn infinite_loop_guest_terminates_by_fuel_and_by_epoch_deadline_and_faults_plugin() {
    let _serial = SERIAL.lock();
    let temp = tempfile::tempdir().expect("tempdir");
    let pkg = write_package(temp.path(), "read-write", &["pick"], FIXTURE_WASM);
    let (ctx, _host, _view, _settings) = setup_host_context();

    let deactivated: Arc<Mutex<Vec<String>>> = Arc::default();
    let deactivated_sink = Arc::clone(&deactivated);
    let _sub = ctx.on_event(move |ev: &PluginDeactivated| {
        deactivated_sink.lock().push(ev.id.to_string());
    });

    // 1. Fuel exhaustion during `export_file` on an infinite-loop guest export.
    let fuel_limits = Limits {
        fuel_per_call: 500_000,
        memory_bytes: 16 * 1024 * 1024,
        call_deadline: Duration::from_secs(5),
    };
    let plugin = load(&pkg, &fuel_limits).expect("load with fuel limit");
    ctx.mount(plugin.as_ref()).expect("mount");
    assert!(!ctx.commands().is_empty());
    assert!(!ctx.exporters().is_empty());

    let loop_project = Project::new("INFINITE_LOOP", Rational::FPS_30);
    let out_path = temp.path().join("loop.mrk");
    let started = Instant::now();
    let export_err = ctx
        .export_file("example.markers", &loop_project, &out_path)
        .expect_err("infinite loop in export must fail by fuel");
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "fuel termination took too long: {:?}",
        started.elapsed()
    );
    assert!(
        export_err.to_string().contains("fuel exhausted"),
        "expected fuel exhausted error, got: {export_err}"
    );
    assert!(!ctx.is_mounted("com.example.wasm-tools"));
    assert!(ctx.commands().is_empty(), "commands dropped on fault");
    assert!(ctx.importers().is_empty(), "importers dropped on fault");
    assert!(ctx.exporters().is_empty(), "exporters dropped on fault");
    assert!(
        audio_effect_spec("example.gain").is_none(),
        "audio effect dropped on fault"
    );
    let failures = ctx.failures();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].id, "com.example.wasm-tools");
    assert!(failures[0].reason.contains("fuel exhausted"));

    // 2. Wall-clock epoch deadline exhaustion (with unlimited fuel) during `command-run`.
    let deadline_limits = Limits {
        fuel_per_call: u64::MAX,
        memory_bytes: 16 * 1024 * 1024,
        call_deadline: Duration::from_millis(25),
    };
    let plugin_deadline = load(&pkg, &deadline_limits).expect("load with deadline limit");
    ctx.mount(plugin_deadline.as_ref()).expect("remount");
    assert!(ctx.failures().is_empty(), "remount clears previous failure");

    let started = Instant::now();
    let cmd_err = ctx
        .run_command("example.infinite-loop")
        .expect_err("infinite loop command must fail by epoch deadline");
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_secs(2),
        "epoch deadline took too long: {elapsed:?}"
    );
    let CommandError::Failed(reason) = cmd_err else {
        panic!("expected CommandError::Failed, got {cmd_err:?}");
    };
    assert!(
        reason.contains("call deadline exceeded"),
        "expected deadline exceeded, got: {reason}"
    );
    assert!(!ctx.is_mounted("com.example.wasm-tools"));
    assert!(ctx.commands().is_empty());
    assert!(ctx.failures()[0].reason.contains("call deadline exceeded"));

    assert!(ctx.events().flush(FLUSH_TIMEOUT));
    assert_eq!(
        *deactivated.lock(),
        vec![
            "com.example.wasm-tools".to_owned(),
            "com.example.wasm-tools".to_owned()
        ]
    );
}

/// A memory-hungry guest exceeding `Limits::memory_bytes` is stopped at the limit,
/// marked failed for the session, and has all registrations removed.
#[test]
fn memory_hungry_guest_is_stopped_at_limit_and_faults_plugin() {
    let _serial = SERIAL.lock();
    let temp = tempfile::tempdir().expect("tempdir");
    let pkg = write_package(temp.path(), "read-write", &["pick"], FIXTURE_WASM);
    let (ctx, _host, _view, _settings) = setup_host_context();

    let tight_memory_limits = Limits {
        fuel_per_call: 500_000_000,
        memory_bytes: 4 * 1024 * 1024,
        call_deadline: Duration::from_secs(2),
    };
    let plugin = load(&pkg, &tight_memory_limits).expect("load wasm plugin");
    ctx.mount(plugin.as_ref()).expect("mount");

    let err = ctx
        .run_command("example.eat-memory")
        .expect_err("64 MiB allocation must be stopped by 4 MiB limit");
    let CommandError::Failed(reason) = err else {
        panic!("expected CommandError::Failed, got {err:?}");
    };
    assert!(
        reason.contains("wasm trap"),
        "expected wasm trap on OOM, got: {reason}"
    );
    assert!(!ctx.is_mounted("com.example.wasm-tools"));
    assert!(ctx.commands().is_empty());
    assert!(audio_effect_spec("example.gain").is_none());
    assert_eq!(ctx.failures().len(), 1);
    assert_eq!(ctx.failures()[0].id, "com.example.wasm-tools");
}

/// `Reloader::poll` mounts discovered packages, hot-reloads a package when `plugin.wasm`
/// changes on disk (swapping its runtime behaviour), and unmounts when removed.
#[test]
fn reloader_swaps_behaviour_when_wasm_fixture_changes_on_disk() {
    let _serial = SERIAL.lock();
    let temp = tempfile::tempdir().expect("tempdir");
    let pkg_dir = temp.path().join("wasm-tools");
    let pkg = write_package(&pkg_dir, "read-write", &["pick"], FIXTURE_WASM);
    let (ctx, host, _view, _settings) = setup_host_context();

    let plugin = load(&pkg, &Limits::default()).expect("load v1");
    ctx.mount(plugin.as_ref()).expect("mount v1");

    let mut reloader = Reloader::new(temp.path(), Limits::default());
    // No files changed since `Reloader::new`: poll is a no-op.
    assert!(reloader.poll(&ctx).is_empty());

    ctx.run_command("example.add-marker")
        .expect("run v1 command");
    assert_eq!(
        host.snapshot().timeline().markers()[0].name,
        "wasm-marker-v1"
    );

    // Patch `"wasm-marker-v1"` -> `"wasm-marker-v2"` in `plugin.wasm` on disk.
    let needle = b"wasm-marker-v1";
    let replacement = b"wasm-marker-v2";
    let mut patched = FIXTURE_WASM.to_vec();
    let pos = patched
        .windows(needle.len())
        .position(|w| w == needle)
        .expect("DEFAULT_MARKER_NAME present in wasm data section");
    patched[pos..pos + replacement.len()].copy_from_slice(replacement);
    fs::write(pkg_dir.join("plugin.wasm"), &patched).expect("write patched plugin.wasm");

    let outcomes = reloader.poll(&ctx);
    assert_eq!(
        outcomes,
        vec![ReloadOutcome::Reloaded {
            id: "com.example.wasm-tools".to_owned(),
            dir: pkg_dir.clone(),
        }]
    );
    assert!(ctx.is_mounted("com.example.wasm-tools"));

    // Undo the v1 marker and run `"example.add-marker"` again: the reloaded plugin creates `"wasm-marker-v2"`.
    host.undo().expect("undo v1 marker");
    ctx.run_command("example.add-marker")
        .expect("run v2 command");
    let markers = host.snapshot().timeline().markers().to_vec();
    assert_eq!(markers.len(), 1);
    assert_eq!(markers[0].name, "wasm-marker-v2");

    // Removing the package directory and polling unmounts the plugin.
    fs::remove_dir_all(&pkg_dir).expect("remove package dir");
    let removed_outcomes = reloader.poll(&ctx);
    assert_eq!(
        removed_outcomes,
        vec![ReloadOutcome::Unmounted {
            id: "com.example.wasm-tools".to_owned(),
            dir: pkg_dir,
        }]
    );
    assert!(!ctx.is_mounted("com.example.wasm-tools"));
    assert!(ctx.commands().is_empty());
}

/// Invalid component binaries and non-WASM packages fail closed with descriptive `LoadError` variants.
#[test]
fn invalid_component_or_non_wasm_tier_is_refused_with_exact_load_error() {
    let _serial = SERIAL.lock();
    let temp = tempfile::tempdir().expect("tempdir");

    // 1. Data-only package (no `plugin.wasm`) is refused with `LoadError::NotWasmTier`.
    let data_dir = temp.path().join("data-only");
    fs::create_dir_all(&data_dir).expect("mkdir");
    fs::write(
        data_dir.join("plugin.toml"),
        r#"[plugin]
id = "com.example.data-only"
name = "Data Only"
version = "0.1.0"
api = 1
"#,
    )
    .expect("write plugin.toml");
    let data_pkg = Package::open(&data_dir).expect("open data pkg");
    let err = load(&data_pkg, &Limits::default())
        .err()
        .expect("data tier must be refused");
    assert_eq!(err, LoadError::NotWasmTier { path: data_dir });

    // 2. Corrupted `plugin.wasm` is refused with `LoadError::Compile`.
    let bad_dir = temp.path().join("bad-wasm");
    let bad_pkg = write_package(&bad_dir, "none", &[], b"not a valid wasm component");
    let bad_err = load(&bad_pkg, &Limits::default())
        .err()
        .expect("corrupt wasm must be refused");
    match &bad_err {
        LoadError::Compile { path, .. } => assert_eq!(path, &bad_dir.join("plugin.wasm")),
        other => panic!("unexpected error: {other:?}"),
    }
}

/// A valid component that does not implement the host's world is refused by
/// `load`, before any guest code runs, with the world and the mismatch in
/// the reason: one with none of the world's exports, and one importing a host
/// function this host does not provide, as a component built for a later
/// world would. Closes "a component for
/// another WIT version loads and fails only when mounted"; `aria plugin
/// check` relies on it. It does not cover a guest that traps in `mount`.
#[test]
fn a_component_for_another_world_is_refused_at_load() {
    let _serial = SERIAL.lock();
    let temp = tempfile::tempdir().expect("tempdir");
    const PREAMBLE: [u8; 8] = [0x00, 0x61, 0x73, 0x6d, 0x0d, 0x00, 0x01, 0x00];
    let foreign = "next-host-call";
    let mut imports = PREAMBLE.to_vec();
    // Type section: one function type, no parameters, no result.
    imports.extend([0x07, 0x05, 0x01, 0x40, 0x00, 0x01, 0x00]);
    // Import section: a function of that type under the foreign name.
    let name_len = u8::try_from(foreign.len()).expect("short name");
    imports.extend([0x0a, name_len + 5, 0x01, 0x00, name_len]);
    imports.extend(foreign.as_bytes());
    imports.extend([0x01, 0x00]);

    for (case, bytes, names) in [
        ("empty", PREAMBLE.to_vec(), "no export `"),
        ("foreign-import", imports, foreign),
    ] {
        let dir = temp.path().join(case);
        let package = write_package(&dir, "none", &[], &bytes);
        let Err(err) = load(&package, &Limits::default()) else {
            panic!("{case}: a component for another world loaded");
        };
        let LoadError::Compile { path, reason } = &err else {
            panic!("{case}: unexpected error: {err:?}");
        };
        assert_eq!(path, &dir.join("plugin.wasm"), "{case}");
        assert!(
            reason.starts_with(&format!("does not match world {}", aria_plugin_wasm::WORLD)),
            "{case}: {reason}"
        );
        assert!(reason.contains(names), "{case}: {reason}");
    }
}

fn expected_tools_tree(presses: u32, level: f64, note: &str) -> UiNode {
    UiNode::Column {
        children: vec![
            UiNode::heading(format!("Tools ({presses})")),
            UiNode::button("bump", format!("Count: {presses}")),
            UiNode::Slider {
                id: "level".into(),
                label: "Level".into(),
                value: level,
                min: 0.0,
                max: 1.0,
                step: 0.25,
            },
            UiNode::input("note", note, "Enter note"),
        ],
    }
}

/// A WASM sidebar panel builds its initial `UiNode` tree on first read, rebuilds after
/// button, slider, and input events, records guest event errors, and rebuilds on
/// `refresh-panel`.
#[test]
fn panel_view_events_and_refresh_rebuild_exact_ui_tree() {
    let _serial = SERIAL.lock();
    let temp = tempfile::tempdir().expect("tempdir");
    let pkg = write_package(temp.path(), "none", &[], FIXTURE_WASM);
    let (ctx, _host, _view, _settings) = setup_host_context();

    let plugin = load(&pkg, &Limits::default()).expect("load wasm plugin");
    ctx.mount(plugin.as_ref()).expect("mount wasm plugin");

    let panels = ctx.panels();
    assert_eq!(
        panels,
        vec![Arc::new(PanelSpec {
            id: "example.tools-panel".to_owned(),
            title: "WASM Tools".into(),
            location: PanelLocation::Sidebar,
        })]
    );

    let _ = ctx.panel("example.tools-panel").expect("panel registered");
    assert!(ctx.events().flush(FLUSH_TIMEOUT));
    let initial = ctx.panel("example.tools-panel").expect("panel state");
    assert_eq!(initial.refused, None);
    assert_eq!(initial.event_error, None);
    assert_eq!(
        initial.tree.as_deref(),
        Some(&expected_tools_tree(0, 0.5, "ready"))
    );

    ctx.send_panel_event(
        "example.tools-panel",
        PanelEvent::Pressed { id: "bump".into() },
    )
    .expect("press bump");
    ctx.send_panel_event(
        "example.tools-panel",
        PanelEvent::Slid {
            id: "level".into(),
            value: 0.74,
        },
    )
    .expect("slide level");
    ctx.send_panel_event(
        "example.tools-panel",
        PanelEvent::Submitted {
            id: "note".into(),
            text: "updated".into(),
        },
    )
    .expect("submit note");
    assert!(ctx.events().flush(FLUSH_TIMEOUT));

    let updated = ctx.panel("example.tools-panel").expect("panel state");
    assert_eq!(updated.refused, None);
    assert_eq!(updated.event_error, None);
    assert_eq!(
        updated.tree.as_deref(),
        Some(&expected_tools_tree(1, 0.75, "updated"))
    );

    ctx.send_panel_event(
        "example.tools-panel",
        PanelEvent::Submitted {
            id: "note".into(),
            text: "ERROR".into(),
        },
    )
    .expect("submit error sentinel");
    assert!(ctx.events().flush(FLUSH_TIMEOUT));
    let with_err = ctx.panel("example.tools-panel").expect("panel state");
    assert_eq!(
        with_err.event_error.as_deref(),
        Some("note rejected by guest")
    );

    ctx.run_command("example.refresh-panel")
        .expect("run refresh-panel command");
    assert!(ctx.events().flush(FLUSH_TIMEOUT));
    let refreshed = ctx.panel("example.tools-panel").expect("panel state");
    assert_eq!(
        refreshed.tree.as_deref(),
        Some(&expected_tools_tree(11, 0.75, "updated"))
    );

    ctx.unmount("com.example.wasm-tools").expect("unmount");
    assert!(ctx.panels().is_empty());
}

/// A malformed flat `list<ui-node>` returned by `panel-view` is rejected with the bad
/// node index, keeping the last valid tree and recording a `PluginFault`; a trapping
/// `panel-view` fails the plugin for the session, drops its panel, and records the failure.
#[test]
fn panel_malformed_list_records_fault_and_trapping_view_fails_plugin() {
    let _serial = SERIAL.lock();
    let temp = tempfile::tempdir().expect("tempdir");
    let pkg = write_package(temp.path(), "none", &[], FIXTURE_WASM);
    let (ctx, _host, _view, _settings) = setup_host_context();

    let plugin = load(&pkg, &Limits::default()).expect("load wasm plugin");
    ctx.mount(plugin.as_ref()).expect("mount wasm plugin");

    let _ = ctx.panel("example.tools-panel").expect("panel registered");
    assert!(ctx.events().flush(FLUSH_TIMEOUT));
    assert_eq!(
        ctx.panel("example.tools-panel")
            .expect("panel")
            .tree
            .as_deref(),
        Some(&expected_tools_tree(0, 0.5, "ready"))
    );

    // 1. Trigger a malformed flat `list<ui-node>` (node 0 referencing child index 99).
    ctx.send_panel_event(
        "example.tools-panel",
        PanelEvent::Submitted {
            id: "note".into(),
            text: "MALFORMED".into(),
        },
    )
    .expect("trigger malformed flat list");
    assert!(ctx.events().flush(FLUSH_TIMEOUT));

    assert!(ctx.is_mounted("com.example.wasm-tools"));
    let after_malformed = ctx
        .panel("example.tools-panel")
        .expect("panel still registered");
    assert_eq!(
        after_malformed.tree.as_deref(),
        Some(&expected_tools_tree(0, 0.5, "ready")),
        "last valid tree stays when malformed flat list is refused"
    );
    let refused = after_malformed
        .refused
        .as_deref()
        .expect("refused reason set");
    assert!(
        refused.contains("node 0") && refused.contains("child index 99"),
        "unexpected refused reason: {refused}"
    );
    let faults = ctx.faults();
    assert_eq!(faults.len(), 1, "{faults:?}");
    assert_eq!(faults[0].plugin, "com.example.wasm-tools");
    assert!(
        faults[0].message.contains("node 0") && faults[0].message.contains("child index 99"),
        "unexpected fault message: {}",
        faults[0].message
    );

    // 2. Trigger a guest trap inside `panel-view`.
    ctx.send_panel_event(
        "example.tools-panel",
        PanelEvent::Submitted {
            id: "note".into(),
            text: "TRAP_VIEW".into(),
        },
    )
    .expect("trigger trapping view");
    assert!(ctx.events().flush(FLUSH_TIMEOUT));

    assert!(!ctx.is_mounted("com.example.wasm-tools"));
    assert!(ctx.panels().is_empty());
    assert_eq!(ctx.failures().len(), 1);
    assert_eq!(ctx.failures()[0].id, "com.example.wasm-tools");
    assert!(
        ctx.failures()[0]
            .reason
            .contains("panel example.tools-panel panicked while drawing"),
        "unexpected failure reason: {}",
        ctx.failures()[0].reason
    );
}

/// A WASM encoder registered during `mount` opens a session, encodes video frames
/// and interleaved audio blocks, writes its output to the active export target path
/// via `fs-write` without needing `permissions.fs`, denies writes to any other path,
/// and faults the plugin on a guest trap.
#[test]
fn wasm_encoder_writes_output_enforces_fs_scope_and_faults_on_trap() {
    let _serial = SERIAL.lock();
    let temp = tempfile::tempdir().expect("tempdir");
    let pkg = write_package(temp.path(), "none", &[], FIXTURE_WASM);
    let (ctx, _host, _view, _settings) = setup_host_context();

    let plugin = load(&pkg, &Limits::default()).expect("load wasm plugin");
    ctx.mount(plugin.as_ref()).expect("mount wasm plugin");

    let encoders = ctx.encoders();
    assert_eq!(encoders.len(), 1);
    assert_eq!(encoders[0].id, "example.raw-rgba");
    assert_eq!(encoders[0].extension, "rgba");
    assert!(encoders[0].audio);

    let cfg = EncoderConfig::new((4, 2), Rational::FPS_24, ColorSpace::BT709)
        .with_audio(AudioEncoderConfig::new(48_000, 2))
        .with_bitrate_kbps(2_000);
    let out_path = temp.path().join("rendered.rgba");
    let mut session = ctx
        .open_encoder("example.raw-rgba", &out_path, &cfg)
        .expect("open wasm encoder");
    let make_frame = || {
        let plane = Plane::new(vec![0u8; 4 * 2 * 4], 4 * 4, 2);
        VideoFrame::new(
            RationalTime::zero(Rational::FPS_24),
            PixelFormat::Rgba8,
            ColorSpace::BT709,
            (4, 2),
            Rect::from_size(4, 2),
            Rotation::None,
            FrameData::Cpu(CpuImage::new(vec![plane])),
        )
    };
    let frame = make_frame();
    session.push(&frame).expect("push frame 1");
    session.push(&frame).expect("push frame 2");
    session
        .push_audio(
            &[0.1, -0.1, 0.2, -0.2],
            RationalTime::zero(Rational::new(48_000, 1)),
        )
        .expect("push audio");
    session.finish().expect("finish wasm encoder");
    assert_eq!(
        fs::read_to_string(&out_path).expect("read encoded output"),
        "RGBA1 4x2@24/1:frames=2:bytes=64:samples=4\n"
    );

    // Attempting `fs-write` to a path other than the active encode path is denied and records a CapabilityDenial.
    let bad_fs_path = temp.path().join("unpermitted_fs.rgba");
    let mut bad_session = ctx
        .open_encoder("example.raw-rgba", &bad_fs_path, &cfg)
        .expect("open bad_fs session");
    let fs_err = bad_session
        .push(&frame)
        .expect_err("unpermitted fs_write inside encoder must fail");
    assert!(
        fs_err.to_string().contains("permission denied"),
        "unexpected error: {fs_err}"
    );
    drop(bad_session);
    assert!(
        !ctx.denials().is_empty(),
        "expected CapabilityDenial for unpermitted fs_write"
    );
    assert!(
        ctx.is_mounted("com.example.wasm-tools"),
        "normal guest error does not fault plugin"
    );

    // A guest trap during `encoder-push-frame` faults the plugin, removes its encoder, and records a failure.
    let trap_path = temp.path().join("trap_push.rgba");
    let mut trap_session = ctx
        .open_encoder("example.raw-rgba", &trap_path, &cfg)
        .expect("open trap session");
    let trap_err = trap_session
        .push(&frame)
        .expect_err("trapping push must fail");
    assert!(
        trap_err.to_string().contains("wasm trap"),
        "expected wasm trap in error: {trap_err}"
    );
    assert!(!ctx.is_mounted("com.example.wasm-tools"));
    assert!(ctx.encoders().is_empty());
    assert_eq!(ctx.failures().len(), 1);
    assert!(
        ctx.failures()[0]
            .reason
            .contains("encoder example.raw-rgba panicked in push"),
        "unexpected failure reason: {}",
        ctx.failures()[0].reason
    );
}

/// The reference WASM plugin registers its `example.safe-frame` viewer overlay,
/// computes exact safe-frame and focus crosshair shapes for the sequence frame
/// size, updates the handle position across `Start`/`Move`/`End` drags, supports
/// `refresh-viewer-overlay` outside `mount` while refusing `register-viewer-overlay`
/// outside `mount`, and faults the plugin on a guest trap inside `overlay-scene`.
#[test]
fn wasm_viewer_overlay_renders_frame_guides_drags_handle_refreshes_and_faults_on_trap() {
    let _serial = SERIAL.lock();
    let temp = tempfile::tempdir().expect("tempdir");
    let pkg = write_package(temp.path(), "read-write", &[], FIXTURE_WASM);
    let (ctx, host, _view, _settings) = setup_host_context();

    let plugin = load(&pkg, &Limits::default()).expect("load wasm plugin");
    ctx.mount(plugin.as_ref()).expect("mount wasm plugin");

    let settle_overlay = |pred: &dyn Fn(&aria_plugin::OverlayState) -> bool| {
        let deadline = Instant::now() + FLUSH_TIMEOUT;
        while Instant::now() < deadline {
            assert!(ctx.events().flush(FLUSH_TIMEOUT));
            if let Some(st) = ctx.viewer_overlay("example.safe-frame")
                && pred(&st)
            {
                return st;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(ctx.events().flush(FLUSH_TIMEOUT));
        let st = ctx
            .viewer_overlay("example.safe-frame")
            .expect("example.safe-frame overlay present");
        assert!(pred(&st), "overlay predicate not satisfied: {st:?}");
        st
    };

    // 1. Trigger initial scene build on default 1920x1080 frame and assert exact shapes.
    assert_eq!(ctx.viewer_overlays().len(), 1);
    let _ = ctx.viewer_overlay("example.safe-frame");
    let initial = settle_overlay(&|st| st.scene.as_ref().is_some_and(|s| s.shapes.len() == 6));
    assert_eq!(&*initial.spec.title, "Safe Frame & Focus");
    assert_eq!(initial.refused, None);
    assert_eq!(
        initial.scene.as_ref().expect("initial scene").shapes,
        vec![
            OverlayShape::Rect {
                origin: [96.0, 54.0],
                size: [1728.0, 972.0],
                color: OverlayColor::Muted,
                width: 1.0,
                fill: None,
            },
            OverlayShape::Rect {
                origin: [192.0, 108.0],
                size: [1536.0, 864.0],
                color: OverlayColor::Text,
                width: 1.0,
                fill: None,
            },
            OverlayShape::Line {
                from: [936.0, 540.0],
                to: [984.0, 540.0],
                color: OverlayColor::Accent,
                width: 1.5,
            },
            OverlayShape::Line {
                from: [960.0, 516.0],
                to: [960.0, 564.0],
                color: OverlayColor::Accent,
                width: 1.5,
            },
            OverlayShape::Text {
                at: [970.0, 530.0],
                text: Arc::from("focus (960, 540) drags=0 refreshes=0"),
                color: OverlayColor::Text,
                size: 12.0,
            },
            OverlayShape::Handle {
                id: Arc::from("focus"),
                at: [960.0, 540.0],
                color: OverlayColor::Accent,
                look: HandleLook::Cross,
            },
        ]
    );

    // 2. Drag Start -> Move -> End moves the focus handle and updates the crosshair and label.
    for (phase, pt) in [
        (DragPhase::Start, [960.0, 540.0]),
        (DragPhase::Move, [700.0, 400.0]),
        (DragPhase::End, [500.0, 300.0]),
    ] {
        ctx.send_overlay_event(
            "example.safe-frame",
            OverlayEvent::Drag {
                handle: Arc::from("focus"),
                phase,
                at: pt,
            },
        )
        .expect("send_overlay_event");
    }
    let after_drag = settle_overlay(&|st| {
        st.scene.as_ref().is_some_and(|s| {
            s.shapes.contains(&OverlayShape::Handle {
                id: Arc::from("focus"),
                at: [500.0, 300.0],
                color: OverlayColor::Accent,
                look: HandleLook::Cross,
            }) && s.shapes.contains(&OverlayShape::Text {
                at: [510.0, 290.0],
                text: Arc::from("focus (500, 300) drags=1 refreshes=0"),
                color: OverlayColor::Text,
                size: 12.0,
            })
        })
    });
    assert_eq!(
        after_drag.scene.as_ref().expect("after_drag scene").shapes[2],
        OverlayShape::Line {
            from: [476.0, 300.0],
            to: [524.0, 300.0],
            color: OverlayColor::Accent,
            width: 1.5,
        }
    );

    // 3. Change sequence canvas aspect to 9:16 (1080x1920) and refresh outside mount.
    host.apply(
        &Origin::Editor,
        Command::Edit(EditCommand::SetCanvas {
            aspect: CanvasAspect::Tall9x16,
            background: [0, 0, 0],
            resolution: None,
        }),
    )
    .expect("set canvas aspect to 9:16");
    ctx.run_command("example.refresh-overlay")
        .expect("refresh overlay outside mount");
    let refreshed = settle_overlay(&|st| {
        st.scene.as_ref().is_some_and(|s| {
            s.shapes.contains(&OverlayShape::Text {
                at: [510.0, 290.0],
                text: Arc::from("focus (500, 300) drags=1 refreshes=1"),
                color: OverlayColor::Text,
                size: 12.0,
            })
        })
    });
    assert_eq!(
        refreshed.scene.as_ref().expect("refreshed scene").shapes[0],
        OverlayShape::Rect {
            origin: [54.0, 96.0],
            size: [972.0, 1728.0],
            color: OverlayColor::Muted,
            width: 1.0,
            fill: None,
        }
    );
    assert_eq!(
        refreshed.scene.as_ref().expect("refreshed scene").shapes[1],
        OverlayShape::Rect {
            origin: [108.0, 192.0],
            size: [864.0, 1536.0],
            color: OverlayColor::Text,
            width: 1.0,
            fill: None,
        }
    );

    // 4. Attempting `register-viewer-overlay` outside `mount` is refused.
    let late_err = ctx
        .run_command("example.late-register-overlay")
        .expect_err("register-viewer-overlay outside mount must fail");
    assert!(
        late_err
            .to_string()
            .contains("register-viewer-overlay is permitted only during mount"),
        "unexpected error: {late_err}"
    );

    // 5. A guest trap inside `overlay-scene` faults the plugin, unmounts it, and drops its overlay.
    ctx.run_command("example.trap-overlay-scene")
        .expect("arm trap-overlay-scene and queue refresh");
    let deadline = Instant::now() + FLUSH_TIMEOUT;
    while Instant::now() < deadline && ctx.is_mounted("com.example.wasm-tools") {
        assert!(ctx.events().flush(FLUSH_TIMEOUT));
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(!ctx.is_mounted("com.example.wasm-tools"));
    assert!(ctx.viewer_overlays().is_empty());
    assert_eq!(ctx.failures().len(), 1);
    assert!(
        ctx.failures()[0]
            .reason
            .contains("viewer overlay example.safe-frame panicked while drawing"),
        "unexpected failure reason: {}",
        ctx.failures()[0].reason
    );
}

/// The job labelled `label` on `manager` once `done` holds for it, within
/// five seconds.
fn wait_job(manager: &JobManager, label: &str, done: impl Fn(&JobSnapshot) -> bool) -> JobSnapshot {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let found = manager
            .snapshot()
            .into_iter()
            .find(|job| job.label == label);
        if let Some(job) = &found
            && done(job)
        {
            return found.expect("found above");
        }
        assert!(
            Instant::now() < deadline,
            "job {label:?} never reached the state: {found:?}"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// A job a WASM command starts steps on the host's jobs board under the
/// plugin's name to its last step, and a job that steps until cancelled ends
/// cancelled with `job-end` told so.
#[test]
fn wasm_jobs_step_to_done_and_end_cancelled_when_cancelled() {
    let _serial = SERIAL.lock();
    let temp = tempfile::tempdir().expect("tempdir");
    let pkg = write_package(temp.path(), "read-write", &["pick"], FIXTURE_WASM);
    let (mut ctx, _host, _view, _settings) = setup_host_context();
    let manager = Arc::new(JobManager::new());
    let held = ctx.register_arc(Arc::clone(&manager));
    ctx._services.push(held);
    let plugin = load(&pkg, &Limits::default()).expect("load wasm plugin");
    ctx.mount(plugin.as_ref()).expect("mount wasm plugin");
    let data_dir = pkg.data_dir(temp.path());
    let stored = |key: &str| fs::read_to_string(data_dir.join(key)).expect("stored by the guest");

    ctx.run_command("example.count-job")
        .expect("starts the job");
    let counted = wait_job(&manager, "Example WASM Tools: count", |job| {
        job.state.is_terminal()
    });
    assert_eq!(counted.state, JobState::Done);
    assert_eq!(counted.detail, "counted to 4");
    assert_eq!(counted.progress, Some(1.0));
    assert_eq!(stored("job-end"), format!("{}:false", stored("last-job")));

    ctx.run_command("example.endless-job")
        .expect("starts the job");
    let running = wait_job(&manager, "Example WASM Tools: endless", |job| {
        job.detail.starts_with("step ")
    });
    assert_eq!(
        running.progress, None,
        "a step without a fraction stays indeterminate"
    );
    assert!(manager.cancel(running.id));
    let ended = wait_job(&manager, "Example WASM Tools: endless", |job| {
        job.state.is_terminal()
    });
    assert_eq!(ended.state, JobState::Cancelled);
    assert_eq!(stored("job-end"), format!("{}:true", stored("last-job")));
    assert!(ctx.is_mounted("com.example.wasm-tools"));
}

/// Luma of pixel `(x, y)` in frame `index` of [`write_y4m`]'s files: distinct
/// per pixel and per frame, so a swapped plane or frame shows.
fn y4m_luma(index: u8, x: u32, y: u32) -> u8 {
    index * 40 + (y * 8 + x) as u8
}

/// Write an 8x4 YUV4MPEG2 file of `frames` 4:2:0 frames at 24 fps whose
/// frame `i` has luma [`y4m_luma`], U `100 + i`, and V `200 - i`.
fn write_y4m(path: &Path, frames: u8, extra: &str) {
    let mut bytes = format!("YUV4MPEG2 W8 H4 F24:1 Ip A1:1 C420jpeg{extra}\n").into_bytes();
    for index in 0..frames {
        bytes.extend_from_slice(b"FRAME\n");
        for y in 0..4 {
            for x in 0..8 {
                bytes.push(y4m_luma(index, x, y));
            }
        }
        bytes.extend_from_slice(&[100 + index; 8]);
        bytes.extend_from_slice(&[200 - index; 8]);
    }
    fs::write(path, bytes).expect("write y4m");
}

/// The planes of a CPU frame as `(bytes, stride, rows)`.
fn cpu_planes(frame: &VideoFrame) -> Vec<(Vec<u8>, usize, usize)> {
    let FrameData::Cpu(cpu) = &frame.data else {
        panic!("a plugin decoder returns CPU frames");
    };
    cpu.planes
        .iter()
        .map(|plane| (plane.data.clone(), plane.stride, plane.rows))
        .collect()
}

fn y4m_planes(index: u8) -> Vec<(Vec<u8>, usize, usize)> {
    let luma = (0..4)
        .flat_map(|y| (0..8).map(move |x| y4m_luma(index, x, y)))
        .collect();
    vec![
        (luma, 8, 4),
        (vec![100 + index; 8], 4, 2),
        (vec![200 - index; 8], 4, 2),
    ]
}

/// The example's YUV4MPEG2 decoder reads a `.y4m` file outside its
/// `permissions.fs` scopes through the host's decoder list: the probe, the
/// stream description, exact planes at a seek target, forward decode to end
/// of stream, and a backward seek. A trap while decoding fails the plugin
/// and unclaims the extension.
#[test]
fn wasm_decoder_reads_y4m_frames_exactly_and_a_trap_fails_the_plugin() {
    let _serial = SERIAL.lock();
    let temp = tempfile::tempdir().expect("tempdir");
    let pkg = write_package(
        &temp.path().join("pkg"),
        "read-write",
        &["pick"],
        FIXTURE_WASM,
    );
    let (ctx, _host, _view, _settings) = setup_host_context();
    let plugin = load(&pkg, &Limits::default()).expect("load wasm plugin");
    ctx.mount(plugin.as_ref()).expect("mount wasm plugin");

    let listed = ctx.decoders();
    assert_eq!(listed.len(), 1);
    assert_eq!(&*listed[0].plugin_id, "com.example.wasm-tools");
    assert_eq!(listed[0].spec.id, "example.y4m");
    assert_eq!(listed[0].spec.extensions, ["y4m"]);

    let clip = temp.path().join("ramp.y4m");
    write_y4m(&clip, 5, "");
    let probed = aria_decoder::probe(&clip).expect("probes through the plugin");
    assert_eq!((probed.width, probed.height), (8, 4));
    assert_eq!(
        (probed.frame_rate, probed.frame_count),
        (Rational::new(24, 1), 5)
    );
    assert_eq!(probed.video_codec.as_deref(), Some("yuv4mpeg2"));
    assert!(!probed.has_audio);

    let mut decoder = aria_decoder::open_video_decoder(&clip, aria_decoder::OutputMode::Cpu)
        .expect("opens through the plugin");
    let info = decoder.info().clone();
    assert_eq!(info.coded_size, (8, 4));
    assert_eq!(info.pixel_format, PixelFormat::Yuv420p);
    assert_eq!(info.color, ColorSpace::BT601);
    assert_eq!(
        info.duration,
        Some(RationalTime::new(5, Rational::new(24, 1)))
    );

    let third = decoder
        .frame_at(RationalTime::new(3, Rational::new(24, 1)))
        .expect("decodes")
        .expect("frame 3 exists");
    assert_eq!(third.pts, RationalTime::new(3, Rational::new(24, 1)));
    assert_eq!(cpu_planes(&third), y4m_planes(3));
    let fourth = decoder.next_frame().expect("decodes").expect("frame 4");
    assert_eq!(cpu_planes(&fourth), y4m_planes(4));
    assert!(
        decoder.next_frame().expect("end of stream").is_none(),
        "a frame past the last one"
    );
    let first = decoder
        .frame_at(RationalTime::new(1, Rational::new(24, 1)))
        .expect("seeks back")
        .expect("frame 1 exists");
    assert_eq!(cpu_planes(&first), y4m_planes(1));
    drop(decoder);
    assert!(ctx.is_mounted("com.example.wasm-tools"));

    let trapping = temp.path().join("trap.y4m");
    write_y4m(&trapping, 2, " XARIA=TRAP");
    let mut decoder = aria_decoder::open_video_decoder(&trapping, aria_decoder::OutputMode::Cpu)
        .expect("opens through the plugin");
    let err = decoder
        .frame_at(RationalTime::new(0, Rational::new(24, 1)))
        .expect_err("a trap is an error");
    assert!(err.to_string().contains("trap"), "{err}");
    drop(decoder);
    let failures = ctx.failures();
    assert!(
        failures
            .iter()
            .any(|failure| failure.id == "com.example.wasm-tools"),
        "the trap did not fail the plugin: {failures:?}"
    );
    assert!(
        ctx.decoders().is_empty(),
        "a failed plugin's decoder is listed"
    );
    assert!(
        !aria_decoder::external_decoder_extensions().contains(&"y4m".to_owned()),
        "a failed plugin's decoder still claims .y4m"
    );
}

/// The component's AI tools are listed as `plugin_<name>` with their tier and
/// schema, a Workspace tool edits the project through `project-transact`, a
/// ReadOnly tool returns its PNG, a refusal reaches the caller as written, and
/// the tools leave the list when the plugin unmounts.
#[test]
fn wasm_ai_tools_list_edit_the_project_return_images_and_leave_on_unmount() {
    let _serial = SERIAL.lock();
    let temp = tempfile::tempdir().expect("tempdir");
    let pkg = write_package(temp.path(), "read-write", &["pick"], FIXTURE_WASM);
    let (ctx, host, _view, _settings) = setup_host_context();
    let plugin = load(&pkg, &Limits::default()).expect("load wasm plugin");
    ctx.mount(plugin.as_ref()).expect("mount wasm plugin");

    let listed: Vec<_> = ctx
        .ai_tools()
        .into_iter()
        .map(|tool| (tool.call_name, tool.plugin_name.to_string(), tool.spec.tier))
        .collect();
    assert_eq!(
        listed,
        [
            (
                "plugin_example_add_marker".to_owned(),
                "Example WASM Tools".to_owned(),
                AiToolTier::Workspace
            ),
            (
                "plugin_example_swatch".to_owned(),
                "Example WASM Tools".to_owned(),
                AiToolTier::ReadOnly
            ),
        ]
    );
    let add = ctx
        .ai_tool("plugin_example_add_marker")
        .expect("add marker is listed");
    assert_eq!(
        add.spec.parameters["required"],
        serde_json::json!(["frame"])
    );

    let cancel = AtomicBool::new(false);
    let output = ctx
        .call_ai_tool(
            "plugin_example_add_marker",
            &serde_json::json!({ "frame": 48, "name": "from the assistant" }),
            AiToolTier::Workspace,
            &cancel,
        )
        .expect("add marker runs");
    assert_eq!(
        output,
        AiToolOutput::text("added marker from the assistant at frame 48")
    );
    let snap = host.snapshot();
    let markers = snap.timeline().markers();
    assert_eq!(markers.len(), 1);
    assert_eq!(markers[0].name, "from the assistant");
    assert_eq!(markers[0].tick, RationalTime::new(48, Rational::FPS_24));

    let swatch = ctx
        .call_ai_tool(
            "plugin_example_swatch",
            &serde_json::json!({}),
            AiToolTier::ReadOnly,
            &cancel,
        )
        .expect("swatch runs");
    assert_eq!(swatch.text, "the example swatch");
    assert_eq!(swatch.images.len(), 1);
    assert_eq!(swatch.images[0].label, "swatch");
    assert_eq!(
        swatch.images[0].png[12..24],
        *b"IHDR\0\0\0\x01\0\0\0\x01",
        "a 1x1 PNG"
    );

    assert_eq!(
        ctx.call_ai_tool(
            "plugin_example_add_marker",
            &serde_json::json!({ "name": "no frame" }),
            AiToolTier::Workspace,
            &cancel,
        ),
        Err(AiToolError::Failed(
            "frame must be a whole number".to_owned()
        ))
    );
    assert!(ctx.is_mounted("com.example.wasm-tools"));

    ctx.unmount("com.example.wasm-tools").expect("unmount");
    assert!(ctx.ai_tools().is_empty());
}

/// The example's luma histogram measures a frame inside the component: a frame
/// half black and half white fills the first and last of its 64 columns to the
/// top, leaves every other column empty, and reads back the mean luma. The
/// scope leaves the strip when its plugin unmounts.
#[test]
fn wasm_scope_draws_a_histogram_of_the_frame_and_leaves_on_unmount() {
    let _serial = SERIAL.lock();
    let temp = tempfile::tempdir().expect("tempdir");
    let pkg = write_package(temp.path(), "read-write", &["pick"], FIXTURE_WASM);
    let (ctx, _host, _view, _settings) = setup_host_context();
    let plugin = load(&pkg, &Limits::default()).expect("load wasm plugin");
    ctx.mount(plugin.as_ref()).expect("mount wasm plugin");

    let listed: Vec<_> = ctx
        .scopes()
        .into_iter()
        .map(|scope| {
            (
                scope.spec.id.clone(),
                scope.spec.name.clone(),
                scope.spec.width,
                scope.spec.height,
                scope.plugin_name.to_string(),
            )
        })
        .collect();
    assert_eq!(
        listed,
        [(
            "example.luma-histogram".to_owned(),
            "Luma Histogram".to_owned(),
            64,
            32,
            "Example WASM Tools".to_owned()
        )]
    );

    let pixels: Vec<u8> = [[0, 0, 0, 255], [255, 255, 255, 255]]
        .iter()
        .flat_map(|px| [*px, *px])
        .flatten()
        .collect();
    let frame = ScopeFrame::new(2, 2, &pixels).expect("a 2x2 frame");
    let mut image = ScopeImage::default();
    ctx.measure_scope("example.luma-histogram", &frame, &mut image)
        .expect("the histogram measures");
    assert_eq!((image.width(), image.height()), (64, 32));
    assert_eq!(image.readout(), "mean luma 127");
    let lit = |column: u32| {
        (0..32)
            .filter(|row| image.pixels()[((row * 64 + column) * 4 + 3) as usize] != 0)
            .count()
    };
    assert_eq!(lit(0), 32, "black fills the first column");
    assert_eq!(lit(63), 32, "white fills the last column");
    assert_eq!(
        (1..63).map(lit).sum::<usize>(),
        0,
        "nothing else is in the frame"
    );
    let cost = ctx.cost_of("com.example.wasm-tools").expect("mounted");
    assert_eq!(cost.frame.calls, 1, "{cost:?}");

    assert_eq!(
        ctx.measure_scope("example.missing", &frame, &mut image),
        Err(ScopeError::Unknown("example.missing".to_owned()))
    );

    ctx.unmount("com.example.wasm-tools").expect("unmount");
    assert!(ctx.scopes().is_empty());
}

struct ParityDriverScope {
    time: f64,
    value: ParamValue,
}

impl DriverScope for ParityDriverScope {
    fn time(&self) -> f64 {
        self.time
    }
    fn property(&self) -> ClipParam {
        ClipParam::Opacity
    }
    fn clip_time(&self) -> f64 {
        self.time
    }
    fn frame_rate(&self) -> f64 {
        24.0
    }
    fn duration(&self) -> f64 {
        10.0
    }
    fn canvas(&self) -> [u32; 2] {
        [1920, 1080]
    }
    fn value(&self) -> ParamValue {
        self.value
    }
    fn value_at(&self, _seconds: f64) -> ParamValue {
        self.value
    }
    fn key_count(&self) -> usize {
        0
    }
    fn key_time(&self, _index: usize) -> Option<f64> {
        None
    }
    fn input(&mut self, _index: usize) -> Result<ParamValue, String> {
        Err("unused".to_owned())
    }
    fn input_at(&mut self, _index: usize, _seconds: f64) -> Result<ParamValue, String> {
        Err("unused".to_owned())
    }
    fn clip_info(&mut self, _clip: &InputClip) -> Result<DriverClipInfo, String> {
        Err("unused".to_owned())
    }
    fn seed(&self) -> u64 {
        7
    }
    fn expired(&self) -> bool {
        false
    }
}

#[track_caller]
fn assert_px(img: &RgbaImage, x: u32, y: u32, expect: [u8; 4], tol: i32) {
    let got = img.pixel(x, y);
    for ch in 0..4 {
        let d = i32::from(got[ch]) - i32::from(expect[ch]);
        assert!(
            d.abs() <= tol,
            "pixel({x},{y}) = {got:?}, expected ~{expect:?} (channel {ch} off by {d}, tol {tol})"
        );
    }
}

#[test]
fn wasm_parity_contributions_register_render_on_lavapipe_fault_and_clean_up_on_unmount() {
    let _serial = SERIAL.lock();
    let temp = tempfile::tempdir().expect("tempdir");
    let pkg = write_package(temp.path(), "read-write", &["pick"], FIXTURE_WASM);
    let (ctx, _host, _view, settings) = setup_host_context();
    settings.set("example.register_parity", serde_json::json!(true));
    let plugin = load(&pkg, &Limits::default()).expect("load wasm plugin");
    ctx.mount(plugin.as_ref()).expect("mount wasm plugin");

    // 1. Audio effect category set via `set-effect-category`.
    let gain_spec = audio_effect_spec("example.gain").expect("example.gain registered");
    assert_eq!(gain_spec.category, "Dynamics");

    // 2. Catalog presence for WGSL effect, CPU effect, generator, transition, clip type, driver, preset, theme, workspace.
    let wgsl_fx = effect_spec("example.wgsl-invert").expect("wgsl effect in catalog");
    assert_eq!(wgsl_fx.category, "Stylize");
    assert!(!wgsl_fx.generator);
    assert!(renders_effect("example.wgsl-invert"));

    let cpu_fx = effect_spec("example.cpu-invert").expect("cpu effect in catalog");
    assert_eq!(cpu_fx.category, "Stylize");
    assert!(renders_effect("example.cpu-invert"));

    let gen_fx = effect_spec("example.wgsl-solid").expect("generator in catalog");
    assert!(gen_fx.generator);
    assert!(renders_effect("example.wgsl-solid"));

    let graph_fx = effect_spec("example.wgsl-graph").expect("effect graph in catalog");
    assert_eq!(graph_fx.category, "Stylize");
    assert_eq!(graph_fx.bounds, EffectBounds::new(1.0, 2.0, 3.0, 4.0));
    assert!(graph_fx.reads_outside, "reads-outside was dropped");
    assert!(graph_fx.time_dependent, "time-dependent was dropped");
    assert!(renders_effect("example.wgsl-graph"));
    let lut = plugin_texture("example.lut").expect("texture registered");
    assert_eq!((lut.width(), lut.height()), (1, 1));
    drop(lut);

    let tr = transition_spec("example.split-wipe").expect("transition in catalog");
    assert_eq!(tr.label, "WASM Split Wipe");
    assert!(renders_transition("example.split-wipe"));

    let ct = clip_type_spec("example.solid-clip").expect("clip type in catalog");
    assert_eq!(ct.generator, "example.wgsl-solid");
    assert_eq!(ct.colour, MarkerColor::Teal);
    let meter = clip_type_spec("example.meter").expect("defined clip type in catalog");
    assert_eq!(meter.generator, "example.meter");
    let meter_fx = effect_spec("example.meter").expect("the meter's generator in catalog");
    assert!(meter_fx.generator);
    assert_eq!(meter_fx.label, "WASM Meter");
    let fields: Vec<&str> = meter_fx.params.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(fields, ["level"]);

    assert!(
        ctx.driver_languages()
            .iter()
            .any(|d| d.spec.id == "wasm-expr")
    );
    assert!(ctx.presets().iter().any(|p| p.preset.name == "WASM Preset"));
    assert!(ctx.themes().iter().any(|t| &*t.id == "wasm-dark"));
    assert!(
        ctx.workspaces()
            .iter()
            .any(|w| w.spec.id == "example.wasm-workspace")
    );
    assert!(
        ctx.analyzers()
            .iter()
            .any(|a| a.spec.id == "example.wasm-peaks")
    );

    // 3. Driver evaluation.
    let prog = aria_render::compile_driver(&DriverCode::new("wasm-expr", "double_time"))
        .expect("compile double_time");
    let mut scope = ParityDriverScope {
        time: 4.25,
        value: ParamValue::Scalar(2.0),
    };
    assert_eq!(
        prog.evaluate(&mut scope).expect("evaluate double_time"),
        ParamValue::Scalar(8.5)
    );
    let prog_plus = aria_render::compile_driver(&DriverCode::new("wasm-expr", "plus_ten"))
        .expect("compile plus_ten");
    assert_eq!(
        prog_plus.evaluate(&mut scope).expect("evaluate plus_ten"),
        ParamValue::Scalar(12.0)
    );
    let prog_canvas = aria_render::compile_driver(&DriverCode::new("wasm-expr", "canvas_vec"))
        .expect("compile canvas_vec");
    assert_eq!(
        prog_canvas
            .evaluate(&mut scope)
            .expect("evaluate canvas_vec"),
        ParamValue::Vec2([1920.0, 1080.0])
    );

    // 4. Compositor rendering on lavapipe: WGSL effect, CPU effect, transition, and CPU guest error fault.
    let gpu = GpuContext::new_headless_blocking().expect("lavapipe headless GPU");
    let mut comp = Compositor::new(&gpu);
    let w = 32;
    let h = 16;
    let config = CompositorConfig::new(w, h).with_background([0, 0, 0, 255]);
    let plate = RgbaImage::new(w, h, [200, 100, 50, 255].repeat((w * h) as usize));

    // WGSL effect
    let wgsl_pass = PassInstance::new(
        pass_id("example.wgsl-invert").expect("wgsl pass_id"),
        vec![1.0],
    );
    let wgsl_layer = CompositeLayer::rgba(&plate, LayerPlacement::full_canvas(&config))
        .with_effects(std::slice::from_ref(&wgsl_pass));
    let wgsl_img = comp
        .render(&gpu, &config, &[wgsl_layer])
        .expect("render wgsl effect");
    assert_px(&wgsl_img, 16, 8, [55, 155, 205, 255], 2);

    // WGSL effect graph: red and green halved by the compute pass, blue from
    // the registered texture.
    let graph_pass = PassInstance::new(
        pass_id("example.wgsl-graph").expect("graph pass_id"),
        Vec::new(),
    );
    let graph_layer = CompositeLayer::rgba(&plate, LayerPlacement::full_canvas(&config))
        .with_effects(std::slice::from_ref(&graph_pass));
    let graph_img = comp
        .render(&gpu, &config, &[graph_layer])
        .expect("render effect graph");
    assert_px(&graph_img, 16, 8, [100, 50, 255, 255], 2);

    // CPU effect (exact RGB inversion)
    let cpu_pass = PassInstance::new(
        pass_id("example.cpu-invert").expect("cpu pass_id"),
        vec![1.0],
    );
    let cpu_layer = CompositeLayer::rgba(&plate, LayerPlacement::full_canvas(&config))
        .with_effects(std::slice::from_ref(&cpu_pass));
    let cpu_img = comp
        .render(&gpu, &config, &[cpu_layer])
        .expect("render cpu effect");
    assert_px(&cpu_img, 16, 8, [55, 155, 205, 255], 1);

    // 4b. Foreign WASM RGBA16F identity pass (amount = 0.0) preserves HDR (> 1.0),
    // negative values, padded row strides, and a 4096-channel f16 gradient bit-exact.
    let wasm_cpu = registered_effect("example.cpu-invert").expect("registered wasm cpu effect");
    let hdr_stride = 12; // 2 pixels wide (8 active u16s) + 4 u16 padding per row
    let mut hdr_in = vec![0xbeef_u16; hdr_stride * 2];
    let hdr_row0 = [
        f32_to_f16(4.0),
        f32_to_f16(-1.25),
        f32_to_f16(12.5),
        F16_ONE,
        f32_to_f16(-0.00390625),
        f32_to_f16(0.0),
        f32_to_f16(65504.0),
        F16_ONE,
    ];
    let hdr_row1 = [
        0x0001_u16,
        0x8001_u16,
        f32_to_f16(2.75),
        f32_to_f16(0.5),
        f32_to_f16(-8.0),
        f32_to_f16(0.125),
        f32_to_f16(1.5),
        F16_ONE,
    ];
    hdr_in[..8].copy_from_slice(&hdr_row0);
    hdr_in[hdr_stride..hdr_stride + 8].copy_from_slice(&hdr_row1);
    let mut hdr_out = vec![0u16; hdr_stride * 2];
    assert!(wasm_cpu.run_cpu(
        CpuFrameIn::new(&hdr_in, 2, 2, hdr_stride),
        &mut CpuFrameOut::new(&mut hdr_out, 2, 2, hdr_stride),
        &[0.0],
        0.25,
    ));
    assert_eq!(&hdr_out[..8], &hdr_row0);
    assert_eq!(&hdr_out[hdr_stride..hdr_stride + 8], &hdr_row1);

    let grad_in: Vec<u16> = (0u16..4096)
        .map(|i| {
            if i < 2048 {
                i * 15
            } else {
                0x8000 | ((i - 2048) * 15)
            }
        })
        .collect();
    assert_eq!(grad_in.len(), 4096);
    let mut grad_out = vec![0u16; 4096];
    assert!(wasm_cpu.run_cpu(
        CpuFrameIn::new(&grad_in, 32, 32, 32 * 4),
        &mut CpuFrameOut::new(&mut grad_out, 32, 32, 32 * 4),
        &[0.0],
        0.5,
    ));
    assert_eq!(grad_out, grad_in);
    // The defined clip type draws its `level` field through cpu-effect-process.
    let meter_pass = registered_effect("example.meter").expect("registered meter generator");
    let blank = [0u16; 16];
    let mut drawn = [0u16; 16];
    assert!(meter_pass.run_cpu(
        CpuFrameIn::new(&blank, 2, 2, 8),
        &mut CpuFrameOut::new(&mut drawn, 2, 2, 8),
        &[0.5],
        0.0,
    ));
    let half = f32_to_f16(0.5);
    assert_eq!(&drawn[..4], &[half, half, half, F16_ONE]);
    assert_eq!(&drawn[12..], &[half, half, half, F16_ONE]);
    drop(meter_pass);
    // CPU effect guest error (amount = 2.0): bypasses frame, records fault in ctx.faults(), plugin stays mounted.
    let err_pass = PassInstance::new(
        pass_id("example.cpu-invert").expect("cpu pass_id"),
        vec![2.0],
    );
    let err_layer = CompositeLayer::rgba(&plate, LayerPlacement::full_canvas(&config))
        .with_effects(std::slice::from_ref(&err_pass));
    let err_img = comp
        .render(&gpu, &config, &[err_layer])
        .expect("render bypassed cpu effect");
    assert_px(&err_img, 16, 8, [200, 100, 50, 255], 1);
    let faults = ctx.faults();
    assert!(
        faults.iter().any(|f| f.plugin == "com.example.wasm-tools"
            && f.message.contains("example.cpu-invert")
            && f.message.contains("cpu effect guest error")),
        "expected CPU effect fault in {faults:?}"
    );
    assert!(ctx.is_mounted("com.example.wasm-tools"));

    // WGSL transition
    let out_bmp = RgbaImage::new(w, h, [255, 0, 0, 255].repeat((w * h) as usize));
    let in_bmp = RgbaImage::new(w, h, [0, 0, 255, 255].repeat((w * h) as usize));
    let out_layer = CompositeLayer::rgba(&out_bmp, LayerPlacement::full_canvas(&config));
    let in_layer = CompositeLayer::rgba(&in_bmp, LayerPlacement::full_canvas(&config));
    let tr_img = comp
        .render_compositor_layers(
            &gpu,
            &config,
            &[CompositorLayer::Transition {
                outgoing: &out_layer,
                incoming: &in_layer,
                transition_id: "example.split-wipe",
                progress: 0.5,
            }],
        )
        .expect("render transition");
    assert_px(&tr_img, 8, 8, [255, 0, 0, 255], 1);
    assert_px(&tr_img, 24, 8, [0, 0, 255, 255], 1);

    // 5. Unmount removes every contribution.
    ctx.unmount("com.example.wasm-tools").expect("unmount");
    assert!(!ctx.is_mounted("com.example.wasm-tools"));
    assert!(effect_spec("example.wgsl-invert").is_none());
    assert!(!renders_effect("example.wgsl-invert"));
    assert!(effect_spec("example.cpu-invert").is_none());
    assert!(!renders_effect("example.cpu-invert"));
    assert!(effect_spec("example.wgsl-solid").is_none());
    assert!(effect_spec("example.wgsl-graph").is_none());
    assert!(!renders_effect("example.wgsl-graph"));
    assert!(plugin_texture("example.lut").is_none());
    assert!(transition_spec("example.split-wipe").is_none());
    assert!(!renders_transition("example.split-wipe"));
    assert!(clip_type_spec("example.solid-clip").is_none());
    assert!(clip_type_spec("example.meter").is_none());
    assert!(effect_spec("example.meter").is_none());
    assert!(ctx.driver_languages().is_empty());
    assert!(ctx.presets().is_empty());
    assert!(ctx.themes().is_empty());
    assert!(ctx.workspaces().is_empty());
    assert!(ctx.analyzers().is_empty());
}

#[test]
fn trapping_wasm_cpu_effect_bypasses_frame_records_fault_and_fails_plugin() {
    let _serial = SERIAL.lock();
    let temp = tempfile::tempdir().expect("tempdir");
    let pkg = write_package(temp.path(), "read-write", &["pick"], FIXTURE_WASM);
    let (ctx, _host, _view, settings) = setup_host_context();
    settings.set("example.register_parity", serde_json::json!(true));
    let plugin = load(&pkg, &Limits::default()).expect("load wasm plugin");
    ctx.mount(plugin.as_ref()).expect("mount wasm plugin");

    let gpu = GpuContext::new_headless_blocking().expect("lavapipe headless GPU");
    let mut comp = Compositor::new(&gpu);
    let w = 32;
    let h = 16;
    let config = CompositorConfig::new(w, h).with_background([0, 0, 0, 255]);
    let plate = RgbaImage::new(w, h, [200, 100, 50, 255].repeat((w * h) as usize));

    let trap_pass = PassInstance::new(
        pass_id("example.cpu-invert").expect("cpu pass_id"),
        vec![-1.0],
    );
    let trap_layer = CompositeLayer::rgba(&plate, LayerPlacement::full_canvas(&config))
        .with_effects(std::slice::from_ref(&trap_pass));
    let img = comp
        .render(&gpu, &config, &[trap_layer])
        .expect("compositor catches trap and bypasses frame");
    assert_px(&img, 16, 8, [200, 100, 50, 255], 1);

    assert!(
        !ctx.is_mounted("com.example.wasm-tools"),
        "trapping CPU effect must fail and unmount the plugin"
    );
    let failures = ctx.failures();
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert!(
        failures[0].reason.contains("example.cpu-invert panicked")
            && failures[0].reason.contains("wasm trap:"),
        "{}",
        failures[0].reason
    );
    assert!(effect_spec("example.cpu-invert").is_none());
    assert!(effect_spec("example.wgsl-invert").is_none());
    assert!(transition_spec("example.split-wipe").is_none());
    assert!(clip_type_spec("example.solid-clip").is_none());
}

#[test]
fn legacy_v1_0_wasm_component_without_parity_exports_still_loads_and_runs() {
    let _serial = SERIAL.lock();
    let temp = tempfile::tempdir().expect("tempdir");
    let pkg = write_package(temp.path(), "read-write", &["pick"], LEGACY_FIXTURE_WASM);
    let (ctx, host, _view, _settings) = setup_host_context();
    let plugin = load(&pkg, &Limits::default()).expect("load legacy @1.0.0 wasm plugin");
    ctx.mount(plugin.as_ref())
        .expect("mount legacy @1.0.0 wasm plugin");
    assert!(ctx.is_mounted("com.example.wasm-tools"));

    ctx.run_command("example.add-marker")
        .expect("run legacy command");
    assert_eq!(host.snapshot().timeline().markers().len(), 1);

    ctx.unmount("com.example.wasm-tools")
        .expect("unmount legacy plugin");
}

#[test]
fn wasm_project_queries_round_trip_and_refuse_without_read_permission() {
    let _serial = SERIAL.lock();
    let temp = tempfile::tempdir().expect("tempdir");
    let (ctx, host, view, _settings) = setup_host_context();

    host.apply(
        &Origin::Editor,
        Command::Edit(EditCommand::AddTrack {
            kind: TrackKind::Sticker,
            name: "S1".to_owned(),
            index: None,
            pinned: false,
        }),
    )
    .expect("add sticker track");
    let track_id = host
        .snapshot()
        .timeline()
        .tracks_ordered()
        .find(|t| t.kind == TrackKind::Sticker)
        .expect("sticker track")
        .id;
    host.apply(
        &Origin::Editor,
        Command::Edit(EditCommand::AddGenerated {
            track: track_id,
            generator: Generator::SolidColor {
                rgba: [255, 0, 0, 255],
            },
            timeline: TimeRange::at_rate(0, 48, Rational::FPS_24),
        }),
    )
    .expect("seed clip");
    let seeded_clip_id = host
        .snapshot()
        .timeline()
        .track(track_id)
        .expect("track")
        .clips_ordered()
        .next()
        .expect("seeded clip")
        .id;
    host.apply(
        &Origin::Editor,
        Command::Edit(EditCommand::AddMarker {
            at: RationalTime::new(12, Rational::FPS_24),
            name: "WasmSeedMarker".to_owned(),
            color: Some(MarkerColor::Teal),
        }),
    )
    .expect("seed marker");
    view.publish(
        ViewFrame {
            selection: &[seeded_clip_id, ClipId::from_raw(999_999)],
            playhead: RationalTime::new(12, Rational::FPS_24),
            mark_in: Some(RationalTime::new(4, Rational::FPS_24)),
            mark_out: Some(RationalTime::new(36, Rational::FPS_24)),
            playing: false,
            speed: 1,
        },
        ctx.events(),
    );

    // 1. With `project = "none"`, `example.query-project` is refused and records a CapabilityDenial.
    let pkg_none = write_package(&temp.path().join("none"), "none", &[], FIXTURE_WASM);
    let plugin_none = load(&pkg_none, &Limits::default()).expect("load wasm plugin (none)");
    ctx.mount(plugin_none.as_ref()).expect("mount wasm plugin");
    let err = ctx
        .run_command("example.query-project")
        .expect_err("query-project must fail when project = none");
    assert!(
        err.to_string().contains("permission denied"),
        "unexpected error: {err}"
    );
    assert_eq!(ctx.denials().len(), 1);
    assert!(ctx.denials()[0].service.contains("ProjectHost"));
    ctx.unmount("com.example.wasm-tools").expect("unmount");

    // 2. With `project = "read"`, `example.query-project` succeeds and writes exact query results.
    let pkg_read = write_package(&temp.path().join("read"), "read", &[], FIXTURE_WASM);
    let plugin_read = load(&pkg_read, &Limits::default()).expect("load wasm plugin (read)");
    ctx.mount(plugin_read.as_ref()).expect("mount wasm plugin");
    ctx.run_command("example.query-project")
        .expect("query-project allowed under read");
    assert!(ctx.denials().is_empty());

    let data_dir = pkg_read.data_dir(&temp.path().join("read"));
    let summary_bytes = fs::read(data_dir.join("last-queries.json")).expect("last-queries.json");
    let summary: serde_json::Value =
        serde_json::from_slice(&summary_bytes).expect("valid summary json");
    let cid = seeded_clip_id.raw();
    assert_eq!(summary["in_range_ids"], serde_json::json!([cid]));
    assert_eq!(summary["first_by_id"]["id"], cid);
    assert_eq!(summary["first_by_id"]["track"], track_id.raw());
    assert_eq!(summary["first_by_id"]["start"], 0);
    assert_eq!(summary["first_by_id"]["end"], 48);
    assert_eq!(summary["first_by_id"]["kind"], "solid");
    assert_eq!(summary["first_by_id"]["name"], "Color");
    assert_eq!(summary["missing_is_none"], true);
    assert_eq!(summary["at_12_ids"], serde_json::json!([cid]));
    assert_eq!(summary["at_48_ids"], serde_json::json!([]));
    assert_eq!(summary["selected_ids"], serde_json::json!([cid]));
    assert_eq!(
        summary["marker_names"],
        serde_json::json!(["WasmSeedMarker"])
    );
    assert_eq!(summary["mark_in"], 4);
    assert_eq!(summary["mark_out"], 36);
    ctx.unmount("com.example.wasm-tools").expect("unmount");
}
