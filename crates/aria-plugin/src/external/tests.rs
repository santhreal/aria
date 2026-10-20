//! Unit and end-to-end tests for the native C-ABI plugin loader (`aria_plugin::external`).
//!
//! Defect class closed: ABI version/size mismatches, host callback wiring drift
//! across commands, events, importers, exporters, audio effects, panels, and
//! encoders, and guest panics unwinding into the host instead of failing and
//! unmounting the plugin.
//!
//! What this does not catch: GPU render pipeline execution or WASM sandbox fuel
//! and memory enforcement (`aria-plugin-wasm`).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use aria_commands::{Command, EditCommand};
use aria_core::{
    AudioEncoderConfig, ColorSpace, CpuImage, EncoderConfig, FrameData, PixelFormat, Plane,
    Rational, RationalTime, Rect, Rotation, TimeRange, VideoFrame,
};
use aria_engine::{Engine, EngineConfig};
use aria_models::{Generator, MarkerColor, TrackKind};
use aria_plugin_abi::{ARIA_PLUGIN_V1_MIN_SIZE, AriaPluginV1};
use aria_sdk::{
    DropAction as SdkDropAction, EncoderAudio as SdkEncoderAudio,
    EncoderConfig as SdkEncoderConfig, EncoderFrame as SdkEncoderFrame,
    EncoderSession as SdkEncoderSession, EncoderSpec as SdkEncoderSpec, EventResponse,
    ExportReport, Host, ImportResult, ImporterSpec as SdkImporterSpec, LogLevel,
    Plugin as SdkPlugin, PluginRuntime, Rational as SdkRational, RationalTime as SdkRationalTime,
    TimeRange as SdkTimeRange,
};
use serde_json::{Value, json};
use tempfile::TempDir;

use super::*;
use crate::package::{LIBRARY_DIR, MANIFEST_FILE, Package, library_name};
use crate::{
    CommandError, DropAction, EditorView, InterchangeError, Origin, ProjectHost, ResolveDropped,
    SelectionChanged, Service, SettingsService, ViewFrame,
};

fn make_project_host(ctx: &PluginContext) -> Arc<ProjectHost> {
    let engine = Engine::new(EngineConfig::default()).expect("engine");
    let host = Arc::new(ProjectHost::new(Arc::new(Mutex::new(engine)), ctx));
    std::mem::forget(ctx.register_arc(Arc::clone(&host)));
    host
}

struct TestSettings;

impl Service for TestSettings {}

impl SettingsService for TestSettings {
    fn get(&self, key: &str) -> Option<Value> {
        match key {
            "notes.directory" => Some(json!("/srv/notes")),
            "contract.limit" => Some(json!(42)),
            _ => None,
        }
    }

    fn set(&self, _key: &str, _value: Value) {}

    fn save(&self) -> Result<(), PluginError> {
        Ok(())
    }
}

fn write_package(
    root: &Path,
    folder: &str,
    manifest_toml: &str,
    lib_bytes: Option<&[u8]>,
) -> PathBuf {
    let pkg_dir = root.join(folder);
    std::fs::create_dir_all(&pkg_dir).expect("create package dir");
    std::fs::write(pkg_dir.join(MANIFEST_FILE), manifest_toml).expect("write plugin.toml");
    if let Some(bytes) = lib_bytes {
        let lib_dir = pkg_dir.join(LIBRARY_DIR);
        std::fs::create_dir_all(&lib_dir).expect("create lib dir");
        std::fs::write(lib_dir.join(library_name()), bytes).expect("write library");
    }
    pkg_dir
}

fn basic_manifest(id: &str, name: &str, project_access: &str) -> String {
    format!(
        r#"[plugin]
id = "{id}"
name = "{name}"
version = "0.1.0"
api = "1"
publisher = "Test"
description = "Test native plugin"
license = "MIT"

[permissions]
project = "{project_access}"
"#
    )
}

fn find_compiled_cdylib(stem: &str) -> PathBuf {
    let exe = std::env::current_exe().expect("test executable path");
    let deps_dir = exe.parent().expect("deps dir");
    let target_dir = deps_dir.parent().unwrap_or(deps_dir);
    let prefix = if cfg!(target_os = "windows") {
        ""
    } else {
        "lib"
    };
    let ext = std::env::consts::DLL_EXTENSION;
    let exact_name = format!("{prefix}{stem}.{ext}");

    for dir in [target_dir, deps_dir] {
        let candidate = dir.join(&exact_name);
        if candidate.is_file() {
            return candidate;
        }
    }
    let dashed_prefix = format!("{prefix}{stem}-");
    let suffix = format!(".{ext}");
    if let Ok(entries) = std::fs::read_dir(deps_dir) {
        let mut matches: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with(&dashed_prefix) && n.ends_with(&suffix))
            })
            .collect();
        matches.sort();
        if let Some(found) = matches.pop() {
            return found;
        }
    }
    panic!(
        "compiled cdylib `{exact_name}` not found in {} or {}",
        target_dir.display(),
        deps_dir.display()
    );
}

#[test]
fn table_compatibility_checks_api_major_and_struct_size() {
    let exact_size = std::mem::size_of::<AriaPluginV1>();
    assert_eq!(check_compat(ABI_VERSION, exact_size), Ok(()));
    assert_eq!(check_compat(ABI_VERSION, ARIA_PLUGIN_V1_MIN_SIZE), Ok(()));
    assert_eq!(check_compat(ABI_VERSION, exact_size + 64), Ok(()));

    for wrong_major in [0, 2, 8, u32::MAX] {
        let err = check_compat(wrong_major, exact_size).expect_err("wrong ABI major must fail");
        assert!(err.contains(&wrong_major.to_string()), "{err}");
        assert!(err.contains(&ABI_VERSION.to_string()), "{err}");
    }

    for short_size in [0, 1, ARIA_PLUGIN_V1_MIN_SIZE - 1] {
        let err = check_compat(ABI_VERSION, short_size).expect_err("short table must fail");
        assert!(err.contains(&short_size.to_string()), "{err}");
        assert!(err.contains(&ARIA_PLUGIN_V1_MIN_SIZE.to_string()), "{err}");
    }
}

#[test]
fn missing_directory_and_bare_library_and_non_plugin_library_are_handled() {
    let temp = TempDir::new().expect("temp dir");
    let absent = temp.path().join("absent");
    let ctx = PluginContext::new();
    assert!(mount_dir(&ctx, &absent).is_empty());

    // A bare shared library outside a package directory is refused with an actionable message.
    let plugins_dir = temp.path().join("plugins");
    std::fs::create_dir_all(&plugins_dir).expect("create plugins dir");
    let bare_name = format!("broken.{LIBRARY_EXTENSION}");
    std::fs::write(plugins_dir.join(&bare_name), b"not a shared object").expect("write bare file");
    let failures = mount_dir(&ctx, &plugins_dir);
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].id, bare_name);
    assert!(
        failures[0].reason.contains(MANIFEST_FILE),
        "{}",
        failures[0].reason
    );
    assert_eq!(ctx.failures().len(), 1);

    // A real shared library that does not export `aria_plugin_entry` is refused by symbol check.
    if let Some(sys_lib) = [
        "/lib/x86_64-linux-gnu/libc.so.6",
        "/lib64/libc.so.6",
        "/usr/lib/libSystem.B.dylib",
    ]
    .into_iter()
    .map(PathBuf::from)
    .find(|p| p.exists())
    {
        let pkg_root = temp.path().join("sys_pkg");
        let manifest = basic_manifest("test.syslib", "SysLib", "none");
        let pkg_dir = write_package(&pkg_root, "test.syslib", &manifest, None);
        let lib_dir = pkg_dir.join(LIBRARY_DIR);
        std::fs::create_dir_all(&lib_dir).expect("create lib dir");
        std::fs::copy(&sys_lib, lib_dir.join(library_name())).expect("copy sys lib");
        let pkg = Package::open(&pkg_dir).expect("open package");
        let Err(err) = load(&pkg) else {
            panic!("system library without aria_plugin_entry must be refused");
        };
        assert_eq!(err, "not an aria plugin: no aria_plugin_entry symbol");
    }
}

#[test]
fn notes_plugin_cdylib_loads_and_executes_end_to_end() {
    // Ensure the rlib link keeps `aria-notes-plugin` built by Cargo before this test runs.
    let _ = aria_notes_plugin::seen_commands();
    let cdylib_path = find_compiled_cdylib("aria_notes_plugin");

    let temp = TempDir::new().expect("temp dir");
    let data_root = temp.path().join("data");
    let plugins_dir = data_root.join("plugins");
    let pkg_dir = plugins_dir.join("example.notes");
    let lib_dir = pkg_dir.join(LIBRARY_DIR);
    std::fs::create_dir_all(&lib_dir).expect("create package lib dir");

    let manifest_src =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/notes-plugin/plugin.toml");
    std::fs::copy(&manifest_src, pkg_dir.join(MANIFEST_FILE)).expect("copy plugin.toml");
    std::fs::copy(&cdylib_path, lib_dir.join(library_name())).expect("copy notes cdylib");

    let pkg = Package::open(&pkg_dir).expect("open example.notes package");
    assert!(matches!(pkg.tier, Tier::Native { .. }));

    let ctx = PluginContext::new();
    let _settings_reg = ctx.register_arc::<dyn SettingsService>(Arc::new(TestSettings));
    let host = make_project_host(&ctx);

    let plugin = load_with_data_root(&pkg, &data_root).expect("load example.notes cdylib");
    ctx.mount(plugin.as_ref()).expect("mount example.notes");
    assert!(ctx.is_mounted("example.notes"));
    let logs = ctx.plugin_logs("example.notes");
    assert!(
        logs.iter().any(|line| {
            line.level == crate::LogLevel::Info && &*line.text == "mounted example.notes"
        }),
        "expected Info log 'mounted example.notes', got: {logs:?}"
    );

    // 1. ResolveDropped claims `.srt` and ignores `.mp4`.
    let srt_claim = ctx
        .events()
        .serial(&ResolveDropped::path(PathBuf::from("captions.SRT")));
    assert_eq!(srt_claim, Some(DropAction::ImportMedia));

    let mp4_claim: Option<DropAction> = ctx
        .events()
        .serial(&ResolveDropped::path(PathBuf::from("video.mp4")));
    assert_eq!(mp4_claim, None);

    // 2. CommandProposed allows an edit touching <= 8 tracks.
    let outcome = host
        .apply(
            &Origin::Editor,
            Command::Edit(EditCommand::AddTrack {
                kind: TrackKind::Video,
                name: "V2".to_owned(),
                index: None,
                pinned: true,
            }),
        )
        .expect("single-track edit should pass");
    let _ = outcome;

    // 3. Running `notes.count` writes the seen command count to `<data_root>/plugin-data/example.notes/seen_commands`.
    ctx.run_command("notes.count")
        .expect("run notes.count command");
    let stored = std::fs::read_to_string(
        data_root
            .join("plugin-data")
            .join("example.notes")
            .join("seen_commands"),
    )
    .expect("read stored seen_commands");
    let parsed_count: usize = stored.parse().expect("valid integer count");
    assert!(
        parsed_count >= 1,
        "expected at least 1 seen command, got {parsed_count}"
    );

    ctx.unmount("example.notes").expect("unmount example.notes");
    assert!(!ctx.is_mounted("example.notes"));
}

#[derive(Default)]
struct ContractGuest {
    unmounts: Arc<AtomicUsize>,
    events_seen: Arc<Mutex<Vec<(String, String)>>>,
    panic_on_mount: bool,
    panic_on_unmount: bool,
}

struct ContractEncoderSession {
    path: PathBuf,
    header: String,
    frames: usize,
    samples: usize,
    /// `f<value>/<rate>` per frame and `a<value>/<rate>` per audio block.
    times: Vec<String>,
    mode: &'static str,
}

impl SdkEncoderSession for ContractEncoderSession {
    fn push_frame(&mut self, frame: &SdkEncoderFrame<'_>) -> Result<(), String> {
        if self.mode == "panic_push" {
            panic!("encoder push boom");
        }
        self.frames += usize::from(frame.width > 0 && frame.rgba.len() >= frame.stride as usize);
        self.times
            .push(format!("f{}/{}", frame.pts_value, frame.pts_rate));
        Ok(())
    }

    fn push_audio(&mut self, audio: &SdkEncoderAudio<'_>) -> Result<(), String> {
        if self.mode == "panic_audio" {
            panic!("encoder audio boom");
        }
        self.samples += audio.samples.len();
        self.times
            .push(format!("a{}/{}", audio.pts_value, audio.pts_rate));
        Ok(())
    }

    fn finish(self: Box<Self>) -> Result<(), String> {
        if self.mode == "panic_finish" {
            panic!("encoder finish boom");
        }
        if self.mode == "fail_finish" {
            return Err("encoder mux failed".to_owned());
        }
        std::fs::write(
            &self.path,
            format!(
                "{}:{}:{}:{}",
                self.header,
                self.frames,
                self.samples,
                self.times.join(",")
            ),
        )
        .map_err(|err| err.to_string())
    }
}

impl SdkPlugin for ContractGuest {
    fn mount(&self, host: &Host) -> Result<(), String> {
        if self.panic_on_mount {
            panic!("mount explosion");
        }
        host.log(LogLevel::Info, "mounting contract guest");
        host.register_command("contract.exercise", "Exercise host API")?;
        host.register_command("contract.late_register", "Attempt registration after mount")?;
        host.register_command("contract.panic_cmd", "Panic in command_run")?;
        host.subscribe("command-proposed")?;
        host.subscribe("resolve-dropped")?;
        host.subscribe("selection-changed")?;
        host.register_importer(&SdkImporterSpec {
            id: "contract.imp".to_owned(),
            name: "Contract Importer".to_owned(),
            extensions: vec!["cimp".to_owned()],
        })?;
        host.register_exporter(&aria_sdk::ExporterSpec {
            id: "contract.exp".to_owned(),
            name: "Contract Exporter".to_owned(),
            extension: "cexp".to_owned(),
        })?;
        host.register_exporter(&aria_sdk::ExporterSpec {
            id: "contract.exp-fail".to_owned(),
            name: "Failing Exporter".to_owned(),
            extension: "cexp".to_owned(),
        })?;
        host.register_exporter(&aria_sdk::ExporterSpec {
            id: "contract.exp-panic".to_owned(),
            name: "Panicking Exporter".to_owned(),
            extension: "cexp".to_owned(),
        })?;
        host.register_encoder(
            SdkEncoderSpec {
                id: "contract.enc".to_owned(),
                name: "Contract Encoder".to_owned(),
                extension: "cenc".to_owned(),
                audio: true,
            },
            |_host: &Host, path: &Path, cfg: &SdkEncoderConfig| {
                let file_name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or_default();
                let mode = if file_name.contains("panic_open") {
                    panic!("encoder open boom");
                } else if file_name.contains("panic_push") {
                    "panic_push"
                } else if file_name.contains("panic_audio") {
                    "panic_audio"
                } else if file_name.contains("panic_finish") {
                    "panic_finish"
                } else if file_name.contains("fail_finish") {
                    "fail_finish"
                } else {
                    "ok"
                };
                let session: Box<dyn SdkEncoderSession> = Box::new(ContractEncoderSession {
                    path: path.to_path_buf(),
                    header: format!("{}x{}@{}", cfg.width, cfg.height, cfg.fps_num),
                    frames: 0,
                    samples: 0,
                    times: Vec::new(),
                    mode,
                });
                Ok(session)
            },
        )?;
        Ok(())
    }

    fn unmount(&self, _host: &Host) -> Result<(), String> {
        self.unmounts.fetch_add(1, Ordering::SeqCst);
        if self.panic_on_unmount {
            panic!("unmount explosion");
        }
        Ok(())
    }

    fn command_run(&self, host: &Host, id: &str, _args_json: &str) -> Result<(), String> {
        match id {
            "contract.exercise" => {
                // Storage round-trip and traversal refusal.
                assert_eq!(host.storage_get("missing")?, None);
                assert!(host.storage_set("../escape", b"bad").is_err());
                assert!(host.storage_set("nested/key", b"bad").is_err());
                host.storage_set("beta", b"two")?;
                host.storage_set("alpha", b"one")?;
                assert_eq!(host.storage_get("alpha")?, Some(b"one".to_vec()));
                assert_eq!(
                    host.storage_keys()?,
                    vec!["alpha".to_owned(), "beta".to_owned()]
                );
                host.storage_delete("alpha")?;
                assert_eq!(host.storage_get("alpha")?, None);
                assert_eq!(host.storage_keys()?, vec!["beta".to_owned()]);

                // Settings access.
                assert_eq!(host.setting_get("contract.limit")?, Some(json!(42)));
                assert_eq!(host.setting_get("contract.unset")?, None);

                // Project snapshot, editor view, transaction, and undo.
                let snap = host.project_snapshot_json()?;
                assert!(snap.contains("tracks"), "{snap}");
                let view = host.editor_view_json()?;
                assert!(view.contains("playhead"), "{view}");

                // Project queries over C ABI.
                let t0 = SdkRationalTime::new(0, SdkRational::FPS_24);
                let t48 = SdkRationalTime::new(48, SdkRational::FPS_24);
                let range = SdkTimeRange::new(t0, t48);
                let clips = host.clips_in_range(None, range)?;
                assert_eq!(clips.len(), 1, "expected 1 seeded clip in range");
                let seeded = &clips[0];
                assert_eq!(seeded.name, "Color");
                assert_eq!(seeded.kind, "solid");
                let by_id = host
                    .clip(seeded.id)?
                    .expect("seeded clip must resolve by id");
                assert_eq!(by_id, *seeded);
                assert_eq!(host.clip(999_999)?, None);
                let at_12 = host.clips_at(None, SdkRationalTime::new(12, SdkRational::FPS_24))?;
                assert_eq!(at_12, vec![seeded.clone()]);
                let at_48 = host.clips_at(None, t48)?;
                assert!(at_48.is_empty(), "half-open clip end must exclude tick 48");
                let selected = host.selected_clips()?;
                assert_eq!(selected, vec![seeded.clone()]);
                let markers = host.markers_in_range(None, range)?;
                assert_eq!(markers.len(), 1);
                assert_eq!(markers[0].name, "SeedMarker");
                let io = host.in_out()?;
                assert_eq!(
                    io.mark_in,
                    Some(SdkRationalTime::new(4, SdkRational::FPS_24))
                );
                assert_eq!(
                    io.mark_out,
                    Some(SdkRationalTime::new(36, SdkRational::FPS_24))
                );
                let cmd_json = serde_json::to_string(&vec![Command::Edit(EditCommand::AddTrack {
                    kind: TrackKind::Audio,
                    name: "PluginAudio".to_owned(),
                    index: None,
                    pinned: true,
                })])
                .expect("serialize command");
                let receipt = host.project_transact_json("Add audio track", &cmd_json)?;
                assert!(receipt.contains("revision"), "{receipt}");
                let undone = host.project_undo()?;
                assert!(undone, "undo should return true after transaction");
                Ok(())
            }
            "contract.late_register" => host.register_command("contract.illegal", "Illegal"),
            "contract.panic_cmd" => {
                panic!("command boom");
            }
            other => Err(format!("unknown command: {other}")),
        }
    }

    fn event(&self, _host: &Host, kind: &str, event_json: &str) -> Result<EventResponse, String> {
        self.events_seen
            .lock()
            .push((kind.to_owned(), event_json.to_owned()));
        match kind {
            "resolve-dropped" | "resolve-dropped-path" => {
                if event_json.contains("panic.cimp") {
                    panic!("event boom");
                }
                if event_json.contains(".cimp") {
                    return Ok(EventResponse::ClaimDrop(SdkDropAction::ImportTimeline));
                }
                if event_json.contains("https://") {
                    return Ok(EventResponse::ClaimDrop(SdkDropAction::Handled));
                }
                Ok(EventResponse::Pass)
            }
            "command-proposed" => {
                if event_json.contains("VETO_ME") {
                    return Ok(EventResponse::Refuse(
                        "blocked by contract guest".to_owned(),
                    ));
                }
                Ok(EventResponse::Pass)
            }
            _ => Ok(EventResponse::Pass),
        }
    }

    fn import(
        &self,
        _host: &Host,
        _importer_id: &str,
        path: &Path,
    ) -> Result<ImportResult, String> {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if name.contains("panic") {
            panic!("import boom");
        }
        if name.contains("fail") {
            return Err("corrupt cimp file".to_owned());
        }
        let cmds = serde_json::to_string(&vec![
            Command::Edit(EditCommand::AddTrack {
                kind: TrackKind::Video,
                name: "ImportedV1".to_owned(),
                index: None,
                pinned: true,
            }),
            Command::Edit(EditCommand::AddTrack {
                kind: TrackKind::Audio,
                name: "ImportedA1".to_owned(),
                index: None,
                pinned: true,
            }),
        ])
        .expect("serialize import commands");
        Ok(ImportResult::new(
            cmds,
            vec!["unsupported transition".to_owned()],
        ))
    }

    fn export(
        &self,
        _host: &Host,
        exporter_id: &str,
        project_json: &str,
        path: &Path,
    ) -> Result<ExportReport, String> {
        if exporter_id == "contract.exp-panic" {
            panic!("export boom");
        }
        if exporter_id == "contract.exp-fail" {
            return Err("disk quota exceeded".to_owned());
        }
        std::fs::write(path, format!("EXPORTED:{}", project_json.len()))
            .map_err(|err| err.to_string())?;
        Ok(ExportReport {
            skipped: vec!["skipped generator".to_owned()],
        })
    }
}

#[macro_use]
#[path = "../../tests/support/plugin_table.rs"]
mod plugin_table;

fn build_in_process_native_plugin(
    pkg: &Package,
    data_root: &Path,
    table: &AriaPluginV1,
) -> Box<dyn Plugin> {
    from_table(pkg, *table, Some(data_root)).expect("from_table")
}

#[test]
fn full_contract_operations_and_registration_guard_and_interchange_work() {
    static RUNTIME: LazyLock<PluginRuntime<ContractGuest>> = LazyLock::new(|| {
        let rt = PluginRuntime::new(ContractGuest::default());
        // SAFETY: `HOST_V1` is a valid static `AriaHostV1` table.
        assert!(unsafe { rt.bind_host(&raw const HOST_V1, ARIA_API_MAJOR) });
        rt
    });
    let table = plugin_table!(RUNTIME);

    let temp = TempDir::new().expect("temp dir");
    let data_root = temp.path().join("data");
    let manifest_toml = basic_manifest("test.contract", "Contract", "read-write");
    let pkg_dir = write_package(temp.path(), "test.contract", &manifest_toml, None);
    let pkg = Package::open(&pkg_dir).expect("open package");

    let ctx = PluginContext::new();
    let _settings_reg = ctx.register_arc::<dyn SettingsService>(Arc::new(TestSettings));
    let host = make_project_host(&ctx);
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
            timeline: TimeRange::at_rate(0, 48, aria_core::Rational::FPS_24),
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
            at: RationalTime::new(10, aria_core::Rational::FPS_24),
            name: "SeedMarker".to_owned(),
            color: Some(MarkerColor::Blue),
        }),
    )
    .expect("seed marker");

    let view = Arc::new(EditorView::new());
    view.publish(
        ViewFrame {
            selection: &[seeded_clip_id],
            playhead: RationalTime::new(12, aria_core::Rational::FPS_24),
            mark_in: Some(RationalTime::new(4, aria_core::Rational::FPS_24)),
            mark_out: Some(RationalTime::new(36, aria_core::Rational::FPS_24)),
            playing: false,
            speed: 1,
        },
        ctx.events(),
    );
    let _view_reg = ctx.register_arc(view);
    let plugin = build_in_process_native_plugin(&pkg, &data_root, &table);
    ctx.mount(plugin.as_ref()).expect("mount contract plugin");

    // 1. Run `contract.exercise` covering storage, settings, project_snapshot, editor_view,
    //    project_transact, and project_undo.
    ctx.run_command("contract.exercise")
        .expect("contract.exercise should succeed");
    assert!(
        ctx.denials().is_empty(),
        "expected zero denials for read-write plugin"
    );

    // 2. Registration outside `mount` is refused.
    let late_err = ctx
        .run_command("contract.late_register")
        .expect_err("registration outside mount must fail");
    assert_eq!(
        late_err,
        CommandError::Failed("registration calls are permitted only during mount".to_owned())
    );

    // 3. `ResolveDropped` returns `DropAction::ImportTimeline` for paths and `DropAction::Handled` for URLs.
    let claim = ctx
        .events()
        .serial(&ResolveDropped::path(PathBuf::from("timeline.cimp")));
    assert_eq!(claim, Some(DropAction::ImportTimeline));
    let url_claim = ctx
        .events()
        .serial(&ResolveDropped::url("https://example.invalid/asset.mp4"));
    assert_eq!(url_claim, Some(DropAction::Handled));

    // 4. `CommandProposed` veto stops disallowed edits.
    let veto_err = host
        .apply(
            &Origin::Editor,
            Command::Edit(EditCommand::AddTrack {
                kind: TrackKind::Video,
                name: "VETO_ME".to_owned(),
                index: None,
                pinned: true,
            }),
        )
        .expect_err("VETO_ME track must be refused");
    assert!(
        veto_err.to_string().contains("blocked by contract guest"),
        "{veto_err}"
    );

    // 5. `import_file` applies multiple commands as ONE transaction and rolls back on error.
    let before_tracks = host.snapshot().timeline().tracks_ordered().count();
    let import_path = temp.path().join("scene.cimp");
    std::fs::write(&import_path, b"dummy").expect("write import file");
    let (report, _receipt) = ctx
        .import_file(&host, &import_path)
        .expect("import_file should succeed");
    assert_eq!(report.skipped, vec!["unsupported transition".to_owned()]);
    assert_eq!(
        host.snapshot().timeline().tracks_ordered().count(),
        before_tracks + 2
    );
    // One undo reverts both imported tracks at once.
    assert!(host.undo().expect("undo import"));
    assert_eq!(
        host.snapshot().timeline().tracks_ordered().count(),
        before_tracks
    );

    let fail_import_path = temp.path().join("fail.cimp");
    std::fs::write(&fail_import_path, b"dummy").expect("write fail import");
    let imp_err = ctx
        .import_file(&host, &fail_import_path)
        .expect_err("failing import must return error");
    assert!(
        imp_err.to_string().contains("corrupt cimp file"),
        "{imp_err}"
    );
    assert_eq!(
        host.snapshot().timeline().tracks_ordered().count(),
        before_tracks
    );

    // 6. `export_file` writes atomically and leaves destination untouched on error.
    let export_path = temp.path().join("out.cexp");
    let export_report = ctx
        .export_file("contract.exp", &host.snapshot(), &export_path)
        .expect("export_file should succeed");
    assert_eq!(export_report.skipped, vec!["skipped generator".to_owned()]);
    let exported_text = std::fs::read_to_string(&export_path).expect("read exported file");
    assert!(exported_text.starts_with("EXPORTED:"), "{exported_text}");

    let fail_export_path = temp.path().join("fail.cexp");
    std::fs::write(&fail_export_path, b"original").expect("seed destination");
    let exp_err = ctx
        .export_file("contract.exp-fail", &host.snapshot(), &fail_export_path)
        .expect_err("failing export must return error");
    assert!(
        exp_err.to_string().contains("disk quota exceeded"),
        "{exp_err}"
    );
    assert_eq!(
        std::fs::read(&fail_export_path).expect("read unchanged destination"),
        b"original"
    );

    // 6b. `open_encoder` writes video frames and audio blocks through the native encoder.
    // A time at 30000/1001 reaches the guest as whole-rate ticks: frame 3 starts
    // 3 * 1001 / 30000 s in, and an audio block at frame 5 starts 5005 / 30000 s in.
    let enc_out = temp.path().join("out.cenc");
    let enc_cfg = EncoderConfig::new((8, 4), Rational::FPS_24, ColorSpace::BT709)
        .with_audio(AudioEncoderConfig::new(48_000, 2))
        .with_bitrate_kbps(2_000);
    let mut enc_session = ctx
        .open_encoder("contract.enc", &enc_out, &enc_cfg)
        .expect("open contract.enc");
    let plane = Plane::new(vec![0u8; 8 * 4 * 4], 8 * 4, 4);
    let frame = VideoFrame::new(
        RationalTime::new(3, Rational::FPS_29_97),
        PixelFormat::Rgba8,
        ColorSpace::BT709,
        (8, 4),
        Rect::from_size(8, 4),
        Rotation::None,
        FrameData::Cpu(CpuImage::new(vec![plane])),
    );
    enc_session.push(&frame).expect("push frame");
    enc_session
        .push_audio(
            &[0.5, -0.5, 0.25, -0.25],
            RationalTime::new(5, Rational::FPS_29_97),
        )
        .expect("push audio");
    enc_session.finish().expect("finish encoder");
    assert_eq!(
        std::fs::read_to_string(&enc_out).expect("read encoded output"),
        "8x4@24:1:4:f3003/30000,a5005/30000"
    );

    // 7. Unmount calls the guest's `unmount` callback and drops all registrations.
    ctx.unmount("test.contract").expect("unmount test.contract");
    assert!(!ctx.is_mounted("test.contract"));
    assert!(ctx.commands().is_empty());
    assert!(ctx.importers().is_empty());
    assert!(ctx.exporters().is_empty());
    assert!(ctx.encoders().is_empty());
}

#[derive(Default)]
struct PermissionGuest;

impl SdkPlugin for PermissionGuest {
    fn mount(&self, host: &Host) -> Result<(), String> {
        host.register_command("perm.snapshot", "Snapshot")?;
        host.register_command("perm.view", "View")?;
        host.register_command("perm.clip", "Clip")?;
        host.register_command("perm.clips_in_range", "ClipsInRange")?;
        host.register_command("perm.clips_at", "ClipsAt")?;
        host.register_command("perm.selected_clips", "SelectedClips")?;
        host.register_command("perm.markers_in_range", "MarkersInRange")?;
        host.register_command("perm.in_out", "InOut")?;
        host.register_command("perm.transact", "Transact")?;
        host.register_command("perm.undo", "Undo")?;
        Ok(())
    }

    fn command_run(&self, host: &Host, id: &str, _args_json: &str) -> Result<(), String> {
        let t0 = SdkRationalTime::new(0, SdkRational::FPS_24);
        let range = SdkTimeRange::new(t0, SdkRationalTime::new(24, SdkRational::FPS_24));
        match id {
            "perm.snapshot" => host.project_snapshot().map(drop),
            "perm.view" => host.editor_view().map(drop),
            "perm.clip" => host.clip(1).map(drop),
            "perm.clips_in_range" => host.clips_in_range(None, range).map(drop),
            "perm.clips_at" => host.clips_at(None, t0).map(drop),
            "perm.selected_clips" => host.selected_clips().map(drop),
            "perm.markers_in_range" => host.markers_in_range(None, range).map(drop),
            "perm.in_out" => host.in_out().map(drop),
            "perm.transact" => host.project_transact_json("noop", "[]").map(drop),
            "perm.undo" => host.project_undo().map(drop),
            other => Err(format!("unknown: {other}")),
        }
    }
}

#[test]
fn permission_levels_enforce_access_and_record_capability_denials() {
    static PERM_RUNTIME: LazyLock<PluginRuntime<PermissionGuest>> = LazyLock::new(|| {
        let rt = PluginRuntime::new(PermissionGuest);
        // SAFETY: `HOST_V1` is a valid static `AriaHostV1` table.
        assert!(unsafe { rt.bind_host(&raw const HOST_V1, ARIA_API_MAJOR) });
        rt
    });
    let table = plugin_table!(PERM_RUNTIME);

    let temp = TempDir::new().expect("temp dir");
    let data_root = temp.path().join("data");

    // Case 1: `project = "none"` denies snapshot, view, transact, and undo and records CapabilityDenial.
    {
        let manifest_toml = basic_manifest("test.permnone", "PermNone", "none");
        let pkg_dir = write_package(temp.path(), "test.permnone", &manifest_toml, None);
        let pkg = Package::open(&pkg_dir).expect("open package");
        let ctx = PluginContext::new();
        let _host = make_project_host(&ctx);
        let plugin = build_in_process_native_plugin(&pkg, &data_root, &table);
        ctx.mount(plugin.as_ref()).expect("mount test.permnone");

        for cmd in [
            "perm.snapshot",
            "perm.view",
            "perm.clip",
            "perm.clips_in_range",
            "perm.clips_at",
            "perm.selected_clips",
            "perm.markers_in_range",
            "perm.in_out",
            "perm.transact",
            "perm.undo",
        ] {
            let err = ctx.run_command(cmd).expect_err("must be denied");
            assert!(
                err.to_string().contains("permission denied"),
                "{cmd}: {err}"
            );
        }
        let denials = ctx.denials();
        assert!(!denials.is_empty(), "expected CapabilityDenial entries");
        assert!(denials.iter().all(|d| d.plugin == "test.permnone"));
    }

    // Case 2: `project = "read"` allows snapshot and view, denies transact and undo with CapabilityDenial.
    {
        let manifest_toml = basic_manifest("test.permread", "PermRead", "read");
        let pkg_dir = write_package(temp.path(), "test.permread", &manifest_toml, None);
        let pkg = Package::open(&pkg_dir).expect("open package");
        let ctx = PluginContext::new();
        let _host = make_project_host(&ctx);
        let plugin = build_in_process_native_plugin(&pkg, &data_root, &table);
        ctx.mount(plugin.as_ref()).expect("mount test.permread");

        for cmd in [
            "perm.snapshot",
            "perm.view",
            "perm.clip",
            "perm.clips_in_range",
            "perm.clips_at",
            "perm.selected_clips",
            "perm.markers_in_range",
            "perm.in_out",
        ] {
            ctx.run_command(cmd)
                .unwrap_or_else(|err| panic!("{cmd} allowed under read: {err}"));
        }
        assert!(
            ctx.denials().is_empty(),
            "read operations must not record denials when project = read"
        );

        let tx_err = ctx
            .run_command("perm.transact")
            .expect_err("transact denied under read");
        assert!(tx_err.to_string().contains("permission denied"), "{tx_err}");
        let undo_err = ctx
            .run_command("perm.undo")
            .expect_err("undo denied under read");
        assert!(
            undo_err.to_string().contains("permission denied"),
            "{undo_err}"
        );

        let denials = ctx.denials();
        assert_eq!(denials.len(), 1);
        assert_eq!(denials[0].plugin, "test.permread");
        assert!(
            denials[0].service.contains("ProjectWrite"),
            "{:?}",
            denials[0]
        );
    }
}

#[test]
fn panics_in_mount_command_event_import_and_export_isolate_and_fail_plugin() {
    let temp = TempDir::new().expect("temp dir");
    let data_root = temp.path().join("data");

    // 1. Panic in `mount`: mount fails, registrations are dropped, failure recorded.
    {
        static MOUNT_PANIC_RT: LazyLock<PluginRuntime<ContractGuest>> = LazyLock::new(|| {
            let rt = PluginRuntime::new(ContractGuest {
                panic_on_mount: true,
                ..ContractGuest::default()
            });
            // SAFETY: `HOST_V1` is a valid static `AriaHostV1` table.
            assert!(unsafe { rt.bind_host(&raw const HOST_V1, ARIA_API_MAJOR) });
            rt
        });
        let table = plugin_table!(MOUNT_PANIC_RT);
        let manifest_toml = basic_manifest("test.panicmount", "PanicMount", "none");
        let pkg_dir = write_package(temp.path(), "test.panicmount", &manifest_toml, None);
        let pkg = Package::open(&pkg_dir).expect("open package");
        let ctx = PluginContext::new();
        let plugin = build_in_process_native_plugin(&pkg, &data_root, &table);
        let err = ctx
            .mount(plugin.as_ref())
            .expect_err("panicking mount must fail");
        assert!(err.to_string().contains("mount explosion"), "{err}");
        assert!(!ctx.is_mounted("test.panicmount"));
        assert_eq!(ctx.failures().len(), 1);
        assert!(ctx.failures()[0].reason.contains("mount explosion"));
    }

    // 2. Panic in `command_run`: returns `CommandError::Panicked`, fails plugin, drops registrations.
    {
        static CMD_PANIC_RT: LazyLock<PluginRuntime<ContractGuest>> = LazyLock::new(|| {
            let rt = PluginRuntime::new(ContractGuest::default());
            // SAFETY: `HOST_V1` is a valid static `AriaHostV1` table.
            assert!(unsafe { rt.bind_host(&raw const HOST_V1, ARIA_API_MAJOR) });
            rt
        });
        let table = plugin_table!(CMD_PANIC_RT);
        let manifest_toml = basic_manifest("test.paniccmd", "PanicCmd", "none");
        let pkg_dir = write_package(temp.path(), "test.paniccmd", &manifest_toml, None);
        let pkg = Package::open(&pkg_dir).expect("open package");
        let ctx = PluginContext::new();
        let plugin = build_in_process_native_plugin(&pkg, &data_root, &table);
        ctx.mount(plugin.as_ref()).expect("mount test.paniccmd");

        let err = ctx
            .run_command("contract.panic_cmd")
            .expect_err("panicking command must fail");
        assert_eq!(err, CommandError::Panicked("command boom".to_owned()));
        assert!(!ctx.is_mounted("test.paniccmd"));
        assert!(ctx.commands().is_empty());
        assert!(ctx.failures()[0].reason.contains("command boom"));
    }

    // 3. Panic in `event`: fails plugin and drops registrations.
    {
        static EV_PANIC_RT: LazyLock<PluginRuntime<ContractGuest>> = LazyLock::new(|| {
            let rt = PluginRuntime::new(ContractGuest::default());
            // SAFETY: `HOST_V1` is a valid static `AriaHostV1` table.
            assert!(unsafe { rt.bind_host(&raw const HOST_V1, ARIA_API_MAJOR) });
            rt
        });
        let table = plugin_table!(EV_PANIC_RT);
        let manifest_toml = basic_manifest("test.panicev", "PanicEv", "none");
        let pkg_dir = write_package(temp.path(), "test.panicev", &manifest_toml, None);
        let pkg = Package::open(&pkg_dir).expect("open package");
        let ctx = PluginContext::new();
        let plugin = build_in_process_native_plugin(&pkg, &data_root, &table);
        ctx.mount(plugin.as_ref()).expect("mount test.panicev");

        let res: Option<DropAction> = ctx
            .events()
            .serial(&ResolveDropped::path(PathBuf::from("panic.cimp")));
        assert_eq!(res, None);
        assert!(!ctx.is_mounted("test.panicev"));
        assert!(ctx.failures()[0].reason.contains("event boom"));
    }

    // 4. Panic in `import`: rolls back transaction, fails plugin, drops registrations.
    {
        static IMP_PANIC_RT: LazyLock<PluginRuntime<ContractGuest>> = LazyLock::new(|| {
            let rt = PluginRuntime::new(ContractGuest::default());
            // SAFETY: `HOST_V1` is a valid static `AriaHostV1` table.
            assert!(unsafe { rt.bind_host(&raw const HOST_V1, ARIA_API_MAJOR) });
            rt
        });
        let table = plugin_table!(IMP_PANIC_RT);
        let manifest_toml = basic_manifest("test.panicimp", "PanicImp", "read-write");
        let pkg_dir = write_package(temp.path(), "test.panicimp", &manifest_toml, None);
        let pkg = Package::open(&pkg_dir).expect("open package");
        let ctx = PluginContext::new();
        let host = make_project_host(&ctx);
        let plugin = build_in_process_native_plugin(&pkg, &data_root, &table);
        ctx.mount(plugin.as_ref()).expect("mount test.panicimp");

        let panic_file = temp.path().join("panic.cimp");
        std::fs::write(&panic_file, b"x").expect("write panic import file");
        let err = ctx
            .import_file(&host, &panic_file)
            .expect_err("panicking import must fail");
        assert!(err.to_string().contains("import boom"), "{err}");
        assert!(!ctx.is_mounted("test.panicimp"));
        assert!(ctx.importers().is_empty());
        assert!(ctx.failures()[0].reason.contains("import boom"));
    }

    // 5. Panic in `export`: leaves destination unchanged, fails plugin, drops registrations.
    {
        static EXP_PANIC_RT: LazyLock<PluginRuntime<ContractGuest>> = LazyLock::new(|| {
            let rt = PluginRuntime::new(ContractGuest::default());
            // SAFETY: `HOST_V1` is a valid static `AriaHostV1` table.
            assert!(unsafe { rt.bind_host(&raw const HOST_V1, ARIA_API_MAJOR) });
            rt
        });
        let table = plugin_table!(EXP_PANIC_RT);
        let manifest_toml = basic_manifest("test.panicexp", "PanicExp", "read");
        let pkg_dir = write_package(temp.path(), "test.panicexp", &manifest_toml, None);
        let pkg = Package::open(&pkg_dir).expect("open package");
        let ctx = PluginContext::new();
        let host = make_project_host(&ctx);
        let plugin = build_in_process_native_plugin(&pkg, &data_root, &table);
        ctx.mount(plugin.as_ref()).expect("mount test.panicexp");

        let panic_out = temp.path().join("panic.cexp");
        std::fs::write(&panic_out, b"untouched").expect("seed panic export dest");
        let err = ctx
            .export_file("contract.exp-panic", &host.snapshot(), &panic_out)
            .expect_err("panicking export must fail");
        assert!(
            matches!(err, InterchangeError::ExportPanicked { .. }),
            "{err:?}"
        );
        assert_eq!(std::fs::read(&panic_out).expect("read dest"), b"untouched");
        assert!(!ctx.is_mounted("test.panicexp"));
        assert!(ctx.exporters().is_empty());
        assert!(ctx.failures()[0].reason.contains("export boom"));

        // Post an event after failure to verify no dead handler runs.
        ctx.events().post(SelectionChanged { clips: vec![] });
    }

    // 6. Panic in `encoder_push_frame`: fails plugin and drops encoder registration.
    {
        static ENC_PANIC_RT: LazyLock<PluginRuntime<ContractGuest>> = LazyLock::new(|| {
            let rt = PluginRuntime::new(ContractGuest::default());
            // SAFETY: `HOST_V1` is a valid static `AriaHostV1` table.
            assert!(unsafe { rt.bind_host(&raw const HOST_V1, ARIA_API_MAJOR) });
            rt
        });
        let table = plugin_table!(ENC_PANIC_RT);
        let manifest_toml = basic_manifest("test.panicenc", "PanicEnc", "none");
        let pkg_dir = write_package(temp.path(), "test.panicenc", &manifest_toml, None);
        let pkg = Package::open(&pkg_dir).expect("open package");
        let ctx = PluginContext::new();
        let plugin = build_in_process_native_plugin(&pkg, &data_root, &table);
        ctx.mount(plugin.as_ref()).expect("mount test.panicenc");

        let panic_out = temp.path().join("panic_push.cenc");
        let enc_cfg = EncoderConfig::new((4, 2), Rational::FPS_24, ColorSpace::BT709)
            .with_bitrate_kbps(1_000);
        let mut session = ctx
            .open_encoder("contract.enc", &panic_out, &enc_cfg)
            .expect("open session");
        let plane = Plane::new(vec![0u8; 4 * 2 * 4], 4 * 4, 2);
        let frame = VideoFrame::new(
            RationalTime::zero(Rational::FPS_24),
            PixelFormat::Rgba8,
            ColorSpace::BT709,
            (4, 2),
            Rect::from_size(4, 2),
            Rotation::None,
            FrameData::Cpu(CpuImage::new(vec![plane])),
        );
        let err = session.push(&frame).expect_err("panicking push must fail");
        assert!(err.to_string().contains("encoder push boom"), "{err}");
        assert!(!ctx.is_mounted("test.panicenc"));
        assert!(ctx.encoders().is_empty());
        assert!(ctx.failures()[0].reason.contains("encoder push boom"));
    }
}
