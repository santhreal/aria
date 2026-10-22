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
use aria_engine::{Engine, EngineConfig};
use aria_models::TrackKind;
use aria_plugin_abi::AriaPluginV1;
use aria_sdk::{
    DropAction as SdkDropAction, EncoderAudio as SdkEncoderAudio,
    EncoderConfig as SdkEncoderConfig, EncoderFrame as SdkEncoderFrame,
    EncoderSession as SdkEncoderSession, EncoderSpec as SdkEncoderSpec, EventResponse,
    ExportReport, Host, ImportResult, ImporterSpec as SdkImporterSpec, LogLevel,
    Plugin as SdkPlugin, Rational as SdkRational, RationalTime as SdkRationalTime,
    TimeRange as SdkTimeRange,
};
use serde_json::{Value, json};

use super::*;
use crate::package::{LIBRARY_DIR, MANIFEST_FILE, Package, library_name};
use crate::{ProjectHost, Service, SettingsService};

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

#[path = "tests_contract.rs"]
mod contract;
#[path = "tests_loading.rs"]
mod loading;
#[path = "tests_panics.rs"]
mod panics;
#[path = "tests_permissions.rs"]
mod permissions;
