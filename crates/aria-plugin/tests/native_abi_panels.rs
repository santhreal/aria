//! End-to-end integration tests for native C-ABI declarative UI panels.
//!
//! Exercises the compiled `aria-notes-plugin` (`cdylib`) sidebar panel through
//! [`PluginContext::panel`], [`PluginContext::send_panel_event`], and
//! [`EventBus::flush`](aria_plugin::EventBus::flush), plus malformed JSON,
//! oversized JSON, panicking `panel_view` / `panel_event`, and legacy v1 table
//! (`ARIA_PLUGIN_V1_MIN_SIZE`) handling.

#![allow(unsafe_code)]

#[path = "support/no_decoders.rs"]
mod no_decoders;

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use aria_notes_plugin::NOTES_PANEL_ID;
use aria_plugin::external::{
    LIBRARY_EXTENSION, MAX_PANEL_VIEW_JSON_BYTES, from_table, host_table, load_with_data_root,
};
use aria_plugin::package::{LIBRARY_DIR, MANIFEST_FILE, Package, library_name};
use aria_plugin::{PanelEvent, PanelLocation, PanelSpec, PluginContext, Tone, TreeError, UiNode};
use aria_plugin_abi::{
    ARIA_API_MAJOR, ARIA_PLUGIN_V1_MIN_SIZE, AriaAudioBlock, AriaAudioParam, AriaBytesResult,
    AriaEncoderAudio, AriaEncoderConfig, AriaEncoderFrame, AriaInstanceResult, AriaPluginV1,
    AriaResult, AriaStr, free_rust_bytes, free_rust_string,
};
use aria_sdk::{
    PanelEvent as SdkPanelEvent, PanelLocation as SdkPanelLocation, PanelSpec as SdkPanelSpec,
    Tone as SdkTone, UiNode as SdkUiNode,
};
use tempfile::TempDir;

const SETTLE: Duration = Duration::from_secs(10);

fn settle(ctx: &PluginContext) {
    assert!(
        ctx.events().flush(SETTLE),
        "the delivery thread did not settle"
    );
}

fn find_compiled_cdylib(stem: &str) -> PathBuf {
    let exe = std::env::current_exe().expect("current_exe");
    let deps_dir = exe.parent().expect("test binary parent dir");
    let target_dir = deps_dir.parent().unwrap_or(deps_dir);
    let prefix = if cfg!(target_os = "windows") {
        ""
    } else {
        "lib"
    };
    let exact_name = format!("{prefix}{stem}.{LIBRARY_EXTENSION}");
    let direct = target_dir.join(&exact_name);
    if direct.exists() {
        return direct;
    }
    if let Ok(entries) = std::fs::read_dir(deps_dir) {
        let mut matches: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| {
                p.extension().is_some_and(|ext| ext == LIBRARY_EXTENSION)
                    && p.file_stem().and_then(|s| s.to_str()).is_some_and(|s| {
                        s == format!("{prefix}{stem}") || s.starts_with(&format!("{prefix}{stem}-"))
                    })
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

fn stage_notes_package(root: &Path) -> Package {
    let pkg_dir = root.join("plugins").join("example.notes");
    let lib_dir = pkg_dir.join(LIBRARY_DIR);
    std::fs::create_dir_all(&lib_dir).expect("create package lib dir");
    let manifest_src = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/notes-plugin")
        .join(MANIFEST_FILE);
    std::fs::copy(&manifest_src, pkg_dir.join(MANIFEST_FILE)).expect("copy plugin.toml");
    let cdylib = find_compiled_cdylib("aria_notes_plugin");
    std::fs::copy(&cdylib, lib_dir.join(library_name())).expect("copy cdylib");
    Package::open(&pkg_dir).expect("open staged notes package")
}

fn write_test_package(root: &Path, id: &str) -> Package {
    let pkg_dir = root.join("plugins").join(id);
    let lib_dir = pkg_dir.join(LIBRARY_DIR);
    std::fs::create_dir_all(&lib_dir).expect("create package lib dir");
    let manifest = format!(
        r#"[plugin]
id = "{id}"
name = "{id}"
version = "0.1.0"
api = "1"
publisher = "Test"
description = "Test package"
license = "MIT"
"#
    );
    std::fs::write(pkg_dir.join(MANIFEST_FILE), manifest).expect("write plugin.toml");
    std::fs::write(lib_dir.join(library_name()), b"stub").expect("write stub lib");
    Package::open(&pkg_dir).expect("open test package")
}

#[test]
fn sdk_panel_types_round_trip_with_host_panel_types() {
    let sdk_spec = SdkPanelSpec::new("demo.panel", "Demo", SdkPanelLocation::Inspector);
    let spec_json = serde_json::to_string(&sdk_spec).expect("serialize sdk PanelSpec");
    assert!(
        spec_json.contains(r#""location":"inspector""#),
        "{spec_json}"
    );

    let sdk_tree = SdkUiNode::column([
        SdkUiNode::heading("Header"),
        SdkUiNode::text("Normal text"),
        SdkUiNode::toned("Muted text", SdkTone::Muted),
        SdkUiNode::toned("Success text", SdkTone::Success),
        SdkUiNode::toned("Warning text", SdkTone::Warning),
        SdkUiNode::toned("Danger text", SdkTone::Danger),
        SdkUiNode::field("Key", "Value"),
        SdkUiNode::progress("Half", 0.5),
        SdkUiNode::Separator,
        SdkUiNode::row([
            SdkUiNode::button("btn", "Click"),
            SdkUiNode::Button {
                id: "btn_dis".into(),
                label: "Disabled".into(),
                disabled: true,
            },
            SdkUiNode::toggle("tog", "Flag", true),
            SdkUiNode::slider("sld", "Gain", 0.5, 0.0..=1.0),
            SdkUiNode::input("inp", "hello", "Type here"),
            SdkUiNode::choice("chc", "Mode", ["A", "B"], 1),
            SdkUiNode::Image {
                width: 1,
                height: 1,
                rgba: Arc::from([255u8, 0, 128, 255].as_slice()),
            },
        ]),
    ]);
    let json = serde_json::to_string(&sdk_tree).expect("serialize sdk UiNode");
    let host_tree: UiNode = serde_json::from_str(&json).expect("deserialize host UiNode");
    assert_eq!(host_tree.check(), Ok(()));
    assert_eq!(
        host_tree,
        UiNode::column([
            UiNode::heading("Header"),
            UiNode::text("Normal text"),
            UiNode::toned("Muted text", Tone::Muted),
            UiNode::toned("Success text", Tone::Success),
            UiNode::toned("Warning text", Tone::Warning),
            UiNode::toned("Danger text", Tone::Danger),
            UiNode::field("Key", "Value"),
            UiNode::progress("Half", 0.5),
            UiNode::Separator,
            UiNode::row([
                UiNode::button("btn", "Click"),
                UiNode::Button {
                    id: "btn_dis".into(),
                    label: "Disabled".into(),
                    disabled: true,
                },
                UiNode::toggle("tog", "Flag", true),
                UiNode::slider("sld", "Gain", 0.5, 0.0..=1.0),
                UiNode::input("inp", "hello", "Type here"),
                UiNode::choice("chc", "Mode", ["A", "B"], 1),
                UiNode::Image {
                    width: 1,
                    height: 1,
                    rgba: Arc::from([255u8, 0, 128, 255].as_slice()),
                },
            ]),
        ])
    );
    let back_json = serde_json::to_string(&host_tree).expect("serialize host UiNode");
    let round_tripped: SdkUiNode =
        serde_json::from_str(&back_json).expect("deserialize sdk UiNode");
    assert_eq!(round_tripped, sdk_tree);

    let host_events = [
        PanelEvent::Pressed { id: "btn".into() },
        PanelEvent::Toggled {
            id: "tog".into(),
            on: false,
        },
        PanelEvent::Slid {
            id: "sld".into(),
            value: 0.75,
        },
        PanelEvent::Submitted {
            id: "inp".into(),
            text: "updated".into(),
        },
        PanelEvent::Chose {
            id: "chc".into(),
            index: 0,
        },
    ];
    for host_ev in host_events {
        let ev_json = serde_json::to_string(&host_ev).expect("serialize host PanelEvent");
        let sdk_ev: SdkPanelEvent =
            serde_json::from_str(&ev_json).expect("deserialize sdk PanelEvent");
        let sdk_json = serde_json::to_string(&sdk_ev).expect("serialize sdk PanelEvent");
        let back_ev: PanelEvent =
            serde_json::from_str(&sdk_json).expect("deserialize host PanelEvent");
        assert_eq!(back_ev, host_ev);
    }
}

#[test]
fn notes_cdylib_sidebar_panel_renders_handles_events_refuses_bad_tree_and_isolates_view_panic() {
    let _ = aria_notes_plugin::seen_commands();
    let temp = TempDir::new().expect("temp dir");
    let data_root = temp.path().join("data");
    let pkg = stage_notes_package(temp.path());

    let ctx = PluginContext::new();
    let plugin = load_with_data_root(&pkg, &data_root).expect("load notes cdylib");
    ctx.mount(plugin.as_ref()).expect("mount notes cdylib");

    assert_eq!(
        ctx.panels(),
        vec![Arc::new(PanelSpec {
            id: NOTES_PANEL_ID.to_owned(),
            title: "Notes".into(),
            location: PanelLocation::Sidebar,
        })]
    );

    // First `panel()` call queues initial view build; `flush()` runs `panel_view` across FFI.
    assert!(
        ctx.panel(NOTES_PANEL_ID)
            .expect("registered")
            .tree
            .is_none()
    );
    settle(&ctx);

    let initial = ctx.panel(NOTES_PANEL_ID).expect("panel state");
    assert_eq!(initial.refused, None);
    let expected_initial = UiNode::column([
        UiNode::heading("Notes (0)"),
        UiNode::button("add", "Add note (0)"),
        UiNode::toggle("claim_srt", "Claim .srt files", true),
        UiNode::input("draft", "ready", "Write a note"),
    ]);
    assert_eq!(initial.tree.as_deref(), Some(&expected_initial));

    // Deliver button press, toggle flip, and text input submission through `send_panel_event`.
    ctx.send_panel_event(NOTES_PANEL_ID, PanelEvent::Pressed { id: "add".into() })
        .expect("queue press");
    ctx.send_panel_event(
        NOTES_PANEL_ID,
        PanelEvent::Toggled {
            id: "claim_srt".into(),
            on: false,
        },
    )
    .expect("queue toggle");
    ctx.send_panel_event(
        NOTES_PANEL_ID,
        PanelEvent::Submitted {
            id: "draft".into(),
            text: "scene 4 cut".into(),
        },
    )
    .expect("queue submit");
    settle(&ctx);

    let updated = ctx.panel(NOTES_PANEL_ID).expect("updated panel state");
    assert_eq!(updated.refused, None);
    let expected_updated = UiNode::column([
        UiNode::heading("Notes (1)"),
        UiNode::button("add", "Add note (1)"),
        UiNode::toggle("claim_srt", "Claim .srt files", false),
        UiNode::input("draft", "scene 4 cut", "Write a note"),
    ]);
    assert_eq!(updated.tree.as_deref(), Some(&expected_updated));
    let draft_file = data_root
        .join("plugin-data")
        .join("example.notes")
        .join("draft");
    assert_eq!(
        std::fs::read_to_string(&draft_file).expect("read persisted draft"),
        "scene 4 cut"
    );

    // Trigger a tree with duplicate control IDs: host refuses the new tree, keeps the last
    // good tree, records a fault, and keeps the plugin mounted.
    ctx.send_panel_event(
        NOTES_PANEL_ID,
        PanelEvent::Submitted {
            id: "draft".into(),
            text: "REFUSE_VIEW".into(),
        },
    )
    .expect("queue refuse view");
    settle(&ctx);

    let after_refuse = ctx.panel(NOTES_PANEL_ID).expect("panel after refuse");
    let dup_reason = TreeError::DuplicateId("dup".to_owned()).to_string();
    assert_eq!(after_refuse.refused.as_deref(), Some(dup_reason.as_str()));
    assert_eq!(after_refuse.tree.as_deref(), Some(&expected_updated));
    assert!(ctx.is_mounted("example.notes"));
    assert_eq!(ctx.faults().len(), 1);
    assert_eq!(
        ctx.faults()[0].message,
        format!("panel {NOTES_PANEL_ID} returned a tree the host refused: {dup_reason}")
    );

    // Refreshing the panel rebuilds a valid tree and clears `refused`.
    ctx.refresh_panel(NOTES_PANEL_ID).expect("refresh panel");
    settle(&ctx);
    assert_eq!(ctx.panel(NOTES_PANEL_ID).expect("cleared").refused, None);

    // Trigger a panic inside `view` on the real cdylib: plugin is failed and unmounted,
    // its panel registration is dropped, and the failure reason is recorded.
    ctx.send_panel_event(
        NOTES_PANEL_ID,
        PanelEvent::Submitted {
            id: "draft".into(),
            text: "PANIC_VIEW".into(),
        },
    )
    .expect("queue panic view");
    settle(&ctx);

    assert!(!ctx.is_mounted("example.notes"));
    assert!(ctx.panels().is_empty());
    let failures = ctx.failures();
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert_eq!(
        failures[0].reason,
        format!("panel {NOTES_PANEL_ID} panicked while drawing: notes panel view panic")
    );
}

static RAW_VIEW_MODE: AtomicU32 = AtomicU32::new(0);

unsafe extern "C" fn raw_mount(host_ctx: *mut c_void) -> AriaResult {
    let spec = r#"{"id":"raw.panel","title":"Raw Panel","location":"sidebar"}"#;
    // SAFETY: `host_ctx` is valid during `mount`.
    unsafe { (host_table().register_panel)(host_ctx, AriaStr::from_str(spec)) }
}

unsafe extern "C" fn raw_noop(_host_ctx: *mut c_void) -> AriaResult {
    AriaResult::ok_empty()
}

unsafe extern "C" fn raw_cmd(_host_ctx: *mut c_void, _id: AriaStr, _args: AriaStr) -> AriaResult {
    AriaResult::ok_empty()
}

unsafe extern "C" fn raw_event(
    _host_ctx: *mut c_void,
    _kind: AriaStr,
    _json: AriaStr,
) -> AriaResult {
    AriaResult::ok_empty()
}

unsafe extern "C" fn raw_import(
    _host_ctx: *mut c_void,
    _id: AriaStr,
    _path: AriaStr,
) -> AriaResult {
    AriaResult::err("unused")
}

unsafe extern "C" fn raw_export(
    _host_ctx: *mut c_void,
    _id: AriaStr,
    _proj: AriaStr,
    _path: AriaStr,
) -> AriaResult {
    AriaResult::err("unused")
}

unsafe extern "C" fn raw_audio_create(
    _host_ctx: *mut c_void,
    _id: AriaStr,
    _rate: u32,
) -> AriaInstanceResult {
    AriaInstanceResult::err("unused")
}

unsafe extern "C" fn raw_audio_process(
    _host_ctx: *mut c_void,
    _inst: u64,
    _params: *const AriaAudioParam,
    _len: usize,
    _block: *mut AriaAudioBlock,
) -> AriaResult {
    AriaResult::err("unused")
}

unsafe extern "C" fn raw_audio_u64(_host_ctx: *mut c_void, _inst: u64) -> AriaResult {
    AriaResult::ok_empty()
}

unsafe extern "C" fn raw_panel_view(_host_ctx: *mut c_void, _id: AriaStr) -> AriaBytesResult {
    match RAW_VIEW_MODE.load(Ordering::Acquire) {
        0 => {
            let tree = UiNode::column([UiNode::heading("Good Tree"), UiNode::button("act", "Run")]);
            AriaBytesResult::found(serde_json::to_vec(&tree).expect("serialize good tree"))
        }
        1 => AriaBytesResult::found(b"{malformed json".to_vec()),
        2 => AriaBytesResult::found(vec![b' '; MAX_PANEL_VIEW_JSON_BYTES + 1]),
        _ => AriaBytesResult::panic("raw view panic"),
    }
}

unsafe extern "C" fn raw_panel_event(
    _host_ctx: *mut c_void,
    _id: AriaStr,
    event_json: AriaStr,
) -> AriaResult {
    // SAFETY: `event_json` is valid for the duration of `raw_panel_event`.
    let text = unsafe { event_json.as_str() }.unwrap_or("");
    if text.contains("act") {
        AriaResult::panic("raw event panic")
    } else {
        AriaResult::ok_empty()
    }
}

unsafe extern "C" fn raw_encoder_open(
    _host_ctx: *mut c_void,
    _id: AriaStr,
    _path: AriaStr,
    _cfg: *const AriaEncoderConfig,
) -> AriaInstanceResult {
    AriaInstanceResult::err("not an encoder")
}

unsafe extern "C" fn raw_encoder_frame(
    _host_ctx: *mut c_void,
    _inst: u64,
    _frame: *const AriaEncoderFrame,
) -> AriaResult {
    AriaResult::err("not an encoder")
}

unsafe extern "C" fn raw_encoder_audio(
    _host_ctx: *mut c_void,
    _inst: u64,
    _audio: *const AriaEncoderAudio,
) -> AriaResult {
    AriaResult::err("not an encoder")
}

unsafe extern "C" fn raw_job_step(
    _host_ctx: *mut c_void,
    _job: u64,
    _step: *mut aria_plugin_abi::AriaJobStep,
) -> AriaResult {
    AriaResult::err("not a job")
}

unsafe extern "C" fn raw_job_end(_host_ctx: *mut c_void, _job: u64, _cancelled: u32) -> AriaResult {
    AriaResult::ok_empty()
}

fn raw_plugin_table() -> AriaPluginV1 {
    AriaPluginV1 {
        struct_size: std::mem::size_of::<AriaPluginV1>(),
        api_major: ARIA_API_MAJOR,
        reserved: 0,
        free_string: free_rust_string,
        free_bytes: free_rust_bytes,
        mount: raw_mount,
        unmount: raw_noop,
        command_run: raw_cmd,
        event: raw_event,
        import: raw_import,
        export: raw_export,
        audio_create: raw_audio_create,
        audio_process: raw_audio_process,
        audio_reset: raw_audio_u64,
        audio_drop: raw_audio_u64,
        panel_view: raw_panel_view,
        panel_event: raw_panel_event,
        encoder_open: raw_encoder_open,
        encoder_push_frame: raw_encoder_frame,
        encoder_push_audio: raw_encoder_audio,
        encoder_finish: raw_audio_u64,
        encoder_drop: raw_audio_u64,
        overlay_scene: raw_panel_view,
        overlay_event: raw_panel_event,
        job_step: raw_job_step,
        job_end: raw_job_end,
        decoder_probe: no_decoders::probe,
        decoder_open_video: no_decoders::open_video,
        decoder_seek: no_decoders::seek,
        decoder_next_frame: no_decoders::next_frame,
        decoder_open_audio: no_decoders::open_audio,
        decoder_read_audio: no_decoders::read_audio,
        decoder_seek_audio: no_decoders::seek,
        decoder_close: no_decoders::close,
        ai_tool_call: no_decoders::ai_tool_call,
        scope_measure: no_decoders::scope_measure,
        cpu_effect_process: no_decoders::cpu_effect_process,
        driver_evaluate: no_decoders::driver_evaluate,
        analyzer_open: no_decoders::analyzer_open,
        analyzer_push: no_decoders::analyzer_push,
        analyzer_finish: no_decoders::analyzer_finish,
        analyzer_drop: no_decoders::analyzer_drop,
    }
}

#[test]
fn malformed_json_and_oversized_json_and_event_panic_and_legacy_table_size_work() {
    let temp = TempDir::new().expect("temp dir");
    let pkg = write_test_package(temp.path(), "test.rawpanel");
    RAW_VIEW_MODE.store(0, Ordering::Release);

    let ctx = PluginContext::new();
    let plugin = from_table(&pkg, raw_plugin_table(), Some(temp.path())).expect("from_table");
    ctx.mount(plugin.as_ref()).expect("mount raw panel plugin");

    let _ = ctx.panel("raw.panel");
    settle(&ctx);

    let expected_good =
        UiNode::column([UiNode::heading("Good Tree"), UiNode::button("act", "Run")]);
    let state0 = ctx.panel("raw.panel").expect("panel state");
    assert_eq!(state0.refused, None);
    assert_eq!(state0.tree.as_deref(), Some(&expected_good));

    // 1. Malformed JSON from `panel_view` maps to a refused tree, keeps the last good tree,
    // records a fault, and keeps the plugin mounted.
    RAW_VIEW_MODE.store(1, Ordering::Release);
    ctx.refresh_panel("raw.panel").expect("refresh malformed");
    settle(&ctx);

    let state1 = ctx.panel("raw.panel").expect("panel after malformed JSON");
    assert_eq!(state1.tree.as_deref(), Some(&expected_good));
    let refused1 = state1.refused.as_deref().expect("refused message set");
    assert!(refused1.contains("malformed panel tree JSON"), "{refused1}");
    assert!(ctx.is_mounted("test.rawpanel"));
    assert_eq!(ctx.faults().len(), 1);
    assert!(
        ctx.faults()[0]
            .message
            .contains("panel raw.panel returned a tree the host refused"),
        "{}",
        ctx.faults()[0].message
    );

    // 2. Oversized JSON (> `MAX_PANEL_VIEW_JSON_BYTES`) is refused before `serde_json` parsing,
    // keeps the last good tree, records a fault, and keeps the plugin mounted.
    RAW_VIEW_MODE.store(2, Ordering::Release);
    ctx.refresh_panel("raw.panel").expect("refresh oversized");
    settle(&ctx);

    let state2 = ctx.panel("raw.panel").expect("panel after oversized JSON");
    assert_eq!(state2.tree.as_deref(), Some(&expected_good));
    let refused2 = state2.refused.as_deref().expect("refused message set");
    assert!(refused2.contains("panel JSON exceeds limit"), "{refused2}");
    assert!(ctx.is_mounted("test.rawpanel"));
    assert_eq!(ctx.faults().len(), 1);
    assert!(
        ctx.faults()[0].message.contains("panel JSON exceeds limit"),
        "{}",
        ctx.faults()[0].message
    );

    // 3. Panic in `panel_event` fails the plugin, unmounts its panels, and records a failure.
    ctx.send_panel_event("raw.panel", PanelEvent::Pressed { id: "act".into() })
        .expect("queue press");
    settle(&ctx);

    assert!(!ctx.is_mounted("test.rawpanel"));
    assert!(ctx.panels().is_empty());
    assert_eq!(ctx.failures().len(), 1);
    assert_eq!(
        ctx.failures()[0].reason,
        "panel raw.panel panicked while handling an event: raw event panic"
    );

    // 4. An older v1 plugin table with `struct_size == ARIA_PLUGIN_V1_MIN_SIZE` (without panel
    // callbacks) is accepted by `from_table`, cannot register panels, and mounts cleanly when
    // it does not register panels.
    let legacy_pkg = write_test_package(temp.path(), "test.legacyv1");
    let mut legacy_table = raw_plugin_table();
    legacy_table.struct_size = ARIA_PLUGIN_V1_MIN_SIZE;
    let legacy_plugin =
        from_table(&legacy_pkg, legacy_table, Some(temp.path())).expect("legacy table accepted");
    let legacy_ctx = PluginContext::new();
    let mount_err = legacy_ctx
        .mount(legacy_plugin.as_ref())
        .expect_err("legacy table must not register panels");
    assert!(
        mount_err
            .to_string()
            .contains("plugin table does not include panel callbacks"),
        "{mount_err}"
    );

    legacy_table.mount = raw_noop;
    let legacy_no_panels =
        from_table(&legacy_pkg, legacy_table, Some(temp.path())).expect("legacy table accepted");
    legacy_ctx
        .mount(legacy_no_panels.as_ref())
        .expect("legacy v1 plugin without panels mounts cleanly");
    assert!(legacy_ctx.is_mounted("test.legacyv1"));
}
