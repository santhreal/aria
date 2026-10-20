//! Integration tests for viewer overlays across the native C ABI (`aria_plugin::external` + `aria_sdk`).
//!
//! Defect class closed: ABI table prefix compatibility (`ARIA_PLUGIN_V1_ENCODERS_SIZE`
//! vs full `AriaPluginV1`), serde drift between `aria_sdk` and `aria_plugin`
//! overlay types, unbounded or malformed JSON from `overlay_scene`, cross-plugin
//! `refresh_viewer_overlay` spoofing, late registration outside `mount`, and
//! guest panics in `overlay_scene` or `overlay_event` leaving dead overlays
//! registered.
//!
//! What this does not catch: GPU Viewer canvas rendering (`aria-ui`) or WASM
//! component overlay execution (`aria-plugin-wasm`).

#![allow(unsafe_code)]

#[path = "support/no_decoders.rs"]
mod no_decoders;

use std::ffi::c_void;
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};

use aria_plugin::external::{
    ABI_VERSION, MAX_OVERLAY_SCENE_JSON_BYTES, check_compat, from_table, host_table,
};
use aria_plugin::package::{MANIFEST_FILE, Package};
use aria_plugin::{
    DragPhase, HandleLook, OverlayColor, OverlayError, OverlayEvent, OverlayScene, OverlayShape,
    PluginContext, PluginError,
};
use aria_plugin_abi::{
    ARIA_API_MAJOR, ARIA_HOST_V1_ENCODERS_SIZE, ARIA_HOST_V1_MIN_SIZE, ARIA_HOST_V1_PANELS_SIZE,
    ARIA_PLUGIN_V1_ENCODERS_SIZE, ARIA_PLUGIN_V1_MIN_SIZE, ARIA_PLUGIN_V1_PANELS_SIZE,
    AriaAudioBlock, AriaAudioParam, AriaBytesResult, AriaEncoderAudio, AriaEncoderConfig,
    AriaEncoderFrame, AriaInstanceResult, AriaOwnedBytes, AriaOwnedStr, AriaPluginV1, AriaResult,
    AriaStr, free_rust_bytes, free_rust_string,
};
use aria_sdk::{
    DragPhase as SdkDragPhase, HandleLook as SdkHandleLook, Host, OverlayColor as SdkOverlayColor,
    OverlayEvent as SdkOverlayEvent, OverlayScene as SdkOverlayScene,
    OverlayShape as SdkOverlayShape, Plugin as SdkPlugin, PluginRuntime,
};
use parking_lot::Mutex;
use tempfile::TempDir;

const SETTLE_TIMEOUT: Duration = Duration::from_secs(2);

fn settle(ctx: &PluginContext) {
    assert!(
        ctx.events().flush(SETTLE_TIMEOUT),
        "event bus did not drain within {SETTLE_TIMEOUT:?}"
    );
}

fn wait_until(ctx: &PluginContext, mut pred: impl FnMut() -> bool) {
    let deadline = Instant::now() + SETTLE_TIMEOUT;
    while Instant::now() < deadline {
        settle(ctx);
        if pred() {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    settle(ctx);
    assert!(pred(), "condition not satisfied within {SETTLE_TIMEOUT:?}");
}

fn write_test_package(root: &Path, id: &str) -> Package {
    let dir = root.join(id);
    std::fs::create_dir_all(&dir).expect("create package dir");
    let manifest = format!(
        r#"[plugin]
id = "{id}"
name = "Test Native Overlay ({id})"
version = "0.1.0"
api = 1
publisher = "Aria Tests"
description = "Native C-ABI viewer overlay test package"
license = "MIT"
"#
    );
    std::fs::write(dir.join(MANIFEST_FILE), manifest).expect("write plugin.toml");
    Package::open(&dir).expect("open package")
}

#[macro_use]
#[path = "support/plugin_table.rs"]
mod plugin_table;

#[test]
fn sdk_and_host_overlay_types_round_trip_identically_through_serde() {
    let sdk_scene = SdkOverlayScene::new(vec![
        SdkOverlayShape::Line {
            from: [10.0, 20.0],
            to: [300.0, 400.0],
            color: SdkOverlayColor::Accent,
            width: 1.5,
        },
        SdkOverlayShape::Rect {
            origin: [96.0, 54.0],
            size: [1728.0, 972.0],
            color: SdkOverlayColor::Muted,
            width: 1.0,
            fill: Some(SdkOverlayColor::Rgba([12, 34, 56, 78])),
        },
        SdkOverlayShape::Ellipse {
            center: [960.0, 540.0],
            radii: [120.0, 80.0],
            color: SdkOverlayColor::Success,
            width: 2.0,
            fill: Some(SdkOverlayColor::Warning),
        },
        SdkOverlayShape::Path {
            points: vec![[0.0, 0.0], [100.0, 50.0], [50.0, 100.0]],
            closed: true,
            color: SdkOverlayColor::Danger,
            width: 2.5,
        },
        SdkOverlayShape::Text {
            at: [48.0, 32.0],
            text: Arc::from("Action Safe"),
            color: SdkOverlayColor::Text,
            size: 14.0,
        },
        SdkOverlayShape::Handle {
            id: Arc::from("h-dot"),
            at: [100.0, 100.0],
            color: SdkOverlayColor::Accent,
            look: SdkHandleLook::Dot,
        },
        SdkOverlayShape::Handle {
            id: Arc::from("h-square"),
            at: [200.0, 200.0],
            color: SdkOverlayColor::Text,
            look: SdkHandleLook::Square,
        },
        SdkOverlayShape::Handle {
            id: Arc::from("h-cross"),
            at: [300.0, 300.0],
            color: SdkOverlayColor::Muted,
            look: SdkHandleLook::Cross,
        },
    ]);

    let json = serde_json::to_string(&sdk_scene).expect("serialize sdk scene");
    let host_scene: OverlayScene =
        serde_json::from_str(&json).expect("deserialize into host OverlayScene");
    host_scene.check().expect("host scene check passes");
    assert_eq!(host_scene.shapes.len(), 8);

    let host_json = serde_json::to_string(&host_scene).expect("serialize host scene");
    let round_tripped_sdk: SdkOverlayScene =
        serde_json::from_str(&host_json).expect("deserialize back into sdk OverlayScene");
    assert_eq!(round_tripped_sdk, sdk_scene);

    for (host_phase, sdk_phase) in [
        (DragPhase::Start, SdkDragPhase::Start),
        (DragPhase::Move, SdkDragPhase::Move),
        (DragPhase::End, SdkDragPhase::End),
    ] {
        let host_ev = OverlayEvent::Drag {
            handle: Arc::from("h-cross"),
            phase: host_phase,
            at: [512.5, 288.25],
        };
        let ev_json = serde_json::to_string(&host_ev).expect("serialize host event");
        let decoded_sdk: SdkOverlayEvent =
            serde_json::from_str(&ev_json).expect("deserialize into sdk event");
        assert_eq!(
            decoded_sdk,
            SdkOverlayEvent::Drag {
                handle: Arc::from("h-cross"),
                phase: sdk_phase,
                at: [512.5, 288.25],
            }
        );
    }
}

#[derive(Default)]
struct FullShapesOverlayGuest {
    state: Mutex<GuestOverlayState>,
}

struct GuestOverlayState {
    focus: [f32; 2],
    drags: u32,
    refreshes: u32,
}

impl Default for GuestOverlayState {
    fn default() -> Self {
        Self {
            focus: [960.0, 540.0],
            drags: 0,
            refreshes: 0,
        }
    }
}

impl SdkPlugin for FullShapesOverlayGuest {
    fn mount(&self, host: &Host) -> Result<(), String> {
        host.register_viewer_overlay("native.guides", "Native Frame Guides")?;
        host.register_command("native.refresh-guides", "Refresh Native Guides")?;
        host.register_command("native.late-overlay", "Late Overlay Registration")?;
        host.register_command("native.refresh-foreign", "Refresh Foreign Overlay")?;
        Ok(())
    }

    fn command_run(&self, host: &Host, id: &str, _args_json: &str) -> Result<(), String> {
        match id {
            "native.refresh-guides" => {
                self.state.lock().refreshes += 1;
                host.refresh_viewer_overlay("native.guides")
            }
            "native.late-overlay" => host.register_viewer_overlay("native.late", "Late Overlay"),
            "native.refresh-foreign" => host.refresh_viewer_overlay("foreign.overlay"),
            other => Err(format!("unknown command: {other}")),
        }
    }

    fn overlay_scene(&self, _host: &Host, id: &str) -> SdkOverlayScene {
        if id != "native.guides" {
            return SdkOverlayScene::default();
        }
        let st = self.state.lock();
        let [fx, fy] = st.focus;
        SdkOverlayScene::new(vec![
            SdkOverlayShape::Line {
                from: [fx - 30.0, fy],
                to: [fx + 30.0, fy],
                color: SdkOverlayColor::Accent,
                width: 1.5,
            },
            SdkOverlayShape::Rect {
                origin: [96.0, 54.0],
                size: [1728.0, 972.0],
                color: SdkOverlayColor::Muted,
                width: 1.0,
                fill: Some(SdkOverlayColor::Rgba([0, 0, 0, 32])),
            },
            SdkOverlayShape::Ellipse {
                center: [fx, fy],
                radii: [40.0, 40.0],
                color: SdkOverlayColor::Success,
                width: 1.0,
                fill: None,
            },
            SdkOverlayShape::Path {
                points: vec![
                    [fx - 10.0, fy - 10.0],
                    [fx + 10.0, fy - 10.0],
                    [fx, fy + 10.0],
                ],
                closed: true,
                color: SdkOverlayColor::Warning,
                width: 1.0,
            },
            SdkOverlayShape::Text {
                at: [fx + 12.0, fy - 12.0],
                text: Arc::from(format!(
                    "focus=({fx:.0},{fy:.0}) drags={} refreshes={}",
                    st.drags, st.refreshes
                )),
                color: SdkOverlayColor::Text,
                size: 12.0,
            },
            SdkOverlayShape::Handle {
                id: Arc::from("focus"),
                at: [fx, fy],
                color: SdkOverlayColor::Danger,
                look: SdkHandleLook::Cross,
            },
        ])
    }

    fn overlay_event(&self, _host: &Host, id: &str, event: &SdkOverlayEvent) -> Result<(), String> {
        if id != "native.guides" {
            return Err(format!("no overlay {id}"));
        }
        let SdkOverlayEvent::Drag { handle, phase, at } = event;
        if &**handle != "focus" {
            return Err(format!("unknown handle {handle}"));
        }
        let mut st = self.state.lock();
        st.focus = *at;
        if *phase == SdkDragPhase::End {
            st.drags += 1;
        }
        Ok(())
    }
}

#[derive(Default)]
struct ForeignOverlayGuest;

impl SdkPlugin for ForeignOverlayGuest {
    fn mount(&self, host: &Host) -> Result<(), String> {
        host.register_viewer_overlay("foreign.overlay", "Foreign Overlay")
    }
}

#[test]
fn sdk_overlay_renders_all_shapes_handles_drags_refreshes_and_enforces_ownership() {
    static GUEST_RT: LazyLock<PluginRuntime<FullShapesOverlayGuest>> = LazyLock::new(|| {
        let rt = PluginRuntime::new(FullShapesOverlayGuest::default());
        // SAFETY: `host_table()` is the valid static `AriaHostV1` table.
        let ok = unsafe { rt.bind_host(host_table(), ARIA_API_MAJOR) };
        assert!(ok, "bind_host must succeed");
        rt
    });
    static FOREIGN_RT: LazyLock<PluginRuntime<ForeignOverlayGuest>> = LazyLock::new(|| {
        let rt = PluginRuntime::new(ForeignOverlayGuest);
        // SAFETY: `host_table()` is the valid static `AriaHostV1` table.
        let ok = unsafe { rt.bind_host(host_table(), ARIA_API_MAJOR) };
        assert!(ok, "bind_host must succeed");
        rt
    });

    let temp = TempDir::new().expect("temp dir");
    let pkg = write_test_package(temp.path(), "test.nativeoverlay");
    let foreign_pkg = write_test_package(temp.path(), "test.foreignoverlay");

    let ctx = PluginContext::new();
    let plugin =
        from_table(&pkg, plugin_table!(GUEST_RT), Some(temp.path())).expect("from_table guest");
    let foreign = from_table(&foreign_pkg, plugin_table!(FOREIGN_RT), Some(temp.path()))
        .expect("from_table foreign");

    ctx.mount(plugin.as_ref()).expect("mount guest");
    ctx.mount(foreign.as_ref()).expect("mount foreign");

    // Trigger initial async scene build and wait for it to settle.
    let _ = ctx.viewer_overlay("native.guides");
    wait_until(&ctx, || {
        ctx.viewer_overlay("native.guides")
            .and_then(|o| o.scene)
            .is_some_and(|s| s.shapes.len() == 6)
    });

    let state0 = ctx
        .viewer_overlay("native.guides")
        .expect("native.guides registered");
    assert_eq!(&*state0.spec.title, "Native Frame Guides");
    assert_eq!(state0.refused, None);
    assert_eq!(
        state0.scene.as_ref().expect("scene built").shapes,
        vec![
            OverlayShape::Line {
                from: [930.0, 540.0],
                to: [990.0, 540.0],
                color: OverlayColor::Accent,
                width: 1.5,
            },
            OverlayShape::Rect {
                origin: [96.0, 54.0],
                size: [1728.0, 972.0],
                color: OverlayColor::Muted,
                width: 1.0,
                fill: Some(OverlayColor::Rgba([0, 0, 0, 32])),
            },
            OverlayShape::Ellipse {
                center: [960.0, 540.0],
                radii: [40.0, 40.0],
                color: OverlayColor::Success,
                width: 1.0,
                fill: None,
            },
            OverlayShape::Path {
                points: vec![[950.0, 530.0], [970.0, 530.0], [960.0, 550.0]],
                closed: true,
                color: OverlayColor::Warning,
                width: 1.0,
            },
            OverlayShape::Text {
                at: [972.0, 528.0],
                text: Arc::from("focus=(960,540) drags=0 refreshes=0"),
                color: OverlayColor::Text,
                size: 12.0,
            },
            OverlayShape::Handle {
                id: Arc::from("focus"),
                at: [960.0, 540.0],
                color: OverlayColor::Danger,
                look: HandleLook::Cross,
            },
        ]
    );

    // Drag Start -> Move -> End moves the focus handle and increments `drags`.
    for (phase, pt) in [
        (DragPhase::Start, [960.0, 540.0]),
        (DragPhase::Move, [640.0, 360.0]),
        (DragPhase::End, [400.0, 300.0]),
    ] {
        ctx.send_overlay_event(
            "native.guides",
            OverlayEvent::Drag {
                handle: Arc::from("focus"),
                phase,
                at: pt,
            },
        )
        .expect("send_overlay_event");
    }
    wait_until(&ctx, || {
        ctx.viewer_overlay("native.guides")
            .and_then(|o| o.scene)
            .is_some_and(|scene| {
                scene.shapes.contains(&OverlayShape::Text {
                    at: [412.0, 288.0],
                    text: Arc::from("focus=(400,300) drags=1 refreshes=0"),
                    color: OverlayColor::Text,
                    size: 12.0,
                })
            })
    });

    // `refresh_viewer_overlay` from a command rebuilds the scene.
    ctx.run_command("native.refresh-guides")
        .expect("run refresh command");
    wait_until(&ctx, || {
        ctx.viewer_overlay("native.guides")
            .and_then(|o| o.scene)
            .is_some_and(|scene| {
                scene.shapes.contains(&OverlayShape::Text {
                    at: [412.0, 288.0],
                    text: Arc::from("focus=(400,300) drags=1 refreshes=1"),
                    color: OverlayColor::Text,
                    size: 12.0,
                })
            })
    });

    // Registration outside `mount` is refused.
    let late_err = ctx
        .run_command("native.late-overlay")
        .expect_err("late overlay registration must fail");
    assert!(
        late_err.to_string().contains("permitted only during mount"),
        "unexpected late registration error: {late_err}"
    );

    // Ownership check: refreshing another plugin's overlay is refused.
    let foreign_err = ctx
        .run_command("native.refresh-foreign")
        .expect_err("refreshing another plugin's overlay must fail");
    assert!(
        foreign_err.to_string().contains("belongs to"),
        "unexpected foreign refresh error: {foreign_err}"
    );

    // Dragging an unknown handle is refused at the host gate before reaching the plugin.
    let no_handle = ctx
        .send_overlay_event(
            "native.guides",
            OverlayEvent::Drag {
                handle: Arc::from("nonexistent"),
                phase: DragPhase::Start,
                at: [0.0, 0.0],
            },
        )
        .expect_err("unknown handle must be refused");
    assert!(matches!(no_handle, OverlayError::NoHandle { .. }));

    // Unmounting drops the plugin's overlay registration while leaving the other plugin's overlay intact.
    ctx.unmount("test.nativeoverlay").expect("unmount");
    let remaining = ctx.viewer_overlays();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].id, "foreign.overlay");
}

static RAW_SCENE_MODE: AtomicU32 = AtomicU32::new(0);

unsafe extern "C" fn raw_mount(host_ctx: *mut c_void) -> AriaResult {
    let spec = r#"{"id":"raw.overlay","title":"Raw Overlay"}"#;
    // SAFETY: `host_table()` is valid and `host_ctx` is provided by `call_mount`.
    unsafe { (host_table().register_viewer_overlay)(host_ctx, AriaStr::from_str(spec)) }
}

unsafe extern "C" fn raw_noop(_host_ctx: *mut c_void) -> AriaResult {
    AriaResult::ok_empty()
}

unsafe extern "C" fn raw_cmd(_host_ctx: *mut c_void, _id: AriaStr, _args: AriaStr) -> AriaResult {
    AriaResult::err("no commands")
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
    AriaResult::err("no importer")
}

unsafe extern "C" fn raw_export(
    _host_ctx: *mut c_void,
    _id: AriaStr,
    _proj: AriaStr,
    _path: AriaStr,
) -> AriaResult {
    AriaResult::err("no exporter")
}

unsafe extern "C" fn raw_audio_create(
    _host_ctx: *mut c_void,
    _id: AriaStr,
    _rate: u32,
) -> AriaInstanceResult {
    AriaInstanceResult::err("no audio")
}

unsafe extern "C" fn raw_audio_process(
    _host_ctx: *mut c_void,
    _inst: u64,
    _params: *const AriaAudioParam,
    _len: usize,
    _block: *mut AriaAudioBlock,
) -> AriaResult {
    AriaResult::err("no audio")
}

unsafe extern "C" fn raw_audio_u64(_host_ctx: *mut c_void, _inst: u64) -> AriaResult {
    AriaResult::ok_empty()
}

unsafe extern "C" fn raw_panel_view(_host_ctx: *mut c_void, _id: AriaStr) -> AriaBytesResult {
    AriaBytesResult::err("no panel")
}

unsafe extern "C" fn raw_panel_event(
    _host_ctx: *mut c_void,
    _id: AriaStr,
    _ev: AriaStr,
) -> AriaResult {
    AriaResult::err("no panel")
}

unsafe extern "C" fn raw_encoder_open(
    _host_ctx: *mut c_void,
    _id: AriaStr,
    _path: AriaStr,
    _cfg: *const AriaEncoderConfig,
) -> AriaInstanceResult {
    AriaInstanceResult::err("no encoder")
}

unsafe extern "C" fn raw_encoder_frame(
    _host_ctx: *mut c_void,
    _inst: u64,
    _frame: *const AriaEncoderFrame,
) -> AriaResult {
    AriaResult::err("no encoder")
}

unsafe extern "C" fn raw_encoder_audio(
    _host_ctx: *mut c_void,
    _inst: u64,
    _audio: *const AriaEncoderAudio,
) -> AriaResult {
    AriaResult::err("no encoder")
}

unsafe extern "C" fn raw_overlay_scene(
    _host_ctx: *mut c_void,
    _overlay_id: AriaStr,
) -> AriaBytesResult {
    match RAW_SCENE_MODE.load(Ordering::Acquire) {
        0 => {
            let scene = OverlayScene {
                shapes: vec![
                    OverlayShape::Text {
                        at: [50.0, 50.0],
                        text: Arc::from("Good Scene"),
                        color: OverlayColor::Text,
                        size: 14.0,
                    },
                    OverlayShape::Handle {
                        id: Arc::from("act"),
                        at: [100.0, 100.0],
                        color: OverlayColor::Accent,
                        look: HandleLook::Dot,
                    },
                ],
            };
            AriaBytesResult::found(serde_json::to_vec(&scene).expect("serialize good scene"))
        }
        1 => AriaBytesResult::found(b"{not valid json".to_vec()),
        2 => {
            let oversized = vec![b' '; MAX_OVERLAY_SCENE_JSON_BYTES + 64];
            AriaBytesResult::found(oversized)
        }
        3 => AriaBytesResult::panic("raw overlay_scene panic"),
        _ => AriaBytesResult::err("raw overlay_scene error"),
    }
}

unsafe extern "C" fn raw_overlay_event(
    _host_ctx: *mut c_void,
    _overlay_id: AriaStr,
    _event_json: AriaStr,
) -> AriaResult {
    match RAW_SCENE_MODE.load(Ordering::Acquire) {
        4 => AriaResult::panic("raw overlay_event panic"),
        5 => AriaResult::err("raw overlay_event rejected"),
        _ => AriaResult::ok_empty(),
    }
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
        import_file: raw_import,
        export_file: raw_export,
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
        overlay_scene: raw_overlay_scene,
        overlay_event: raw_overlay_event,
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
fn malformed_json_oversized_json_panics_and_legacy_table_sizes_are_handled() {
    let temp = TempDir::new().expect("temp dir");
    let pkg = write_test_package(temp.path(), "test.rawoverlay");
    RAW_SCENE_MODE.store(0, Ordering::Release);

    let ctx = PluginContext::new();
    let plugin = from_table(&pkg, raw_plugin_table(), Some(temp.path())).expect("from_table");
    ctx.mount(plugin.as_ref())
        .expect("mount raw overlay plugin");

    let expected_good = OverlayScene {
        shapes: vec![
            OverlayShape::Text {
                at: [50.0, 50.0],
                text: Arc::from("Good Scene"),
                color: OverlayColor::Text,
                size: 14.0,
            },
            OverlayShape::Handle {
                id: Arc::from("act"),
                at: [100.0, 100.0],
                color: OverlayColor::Accent,
                look: HandleLook::Dot,
            },
        ],
    };

    let _ = ctx.viewer_overlay("raw.overlay");
    wait_until(&ctx, || {
        ctx.viewer_overlay("raw.overlay")
            .is_some_and(|s| s.scene.as_deref() == Some(&expected_good))
    });
    let state0 = ctx.viewer_overlay("raw.overlay").expect("raw.overlay");
    assert_eq!(state0.scene.as_deref(), Some(&expected_good));
    assert_eq!(state0.refused, None);

    // 1. Malformed JSON from `overlay_scene` keeps the previous valid scene, records a fault, and sets `refused`.
    RAW_SCENE_MODE.store(1, Ordering::Release);
    ctx.send_overlay_event(
        "raw.overlay",
        OverlayEvent::Drag {
            handle: Arc::from("act"),
            phase: DragPhase::Start,
            at: [100.0, 100.0],
        },
    )
    .expect("send event triggering malformed scene rebuild");
    wait_until(&ctx, || {
        ctx.viewer_overlay("raw.overlay")
            .and_then(|s| s.refused)
            .is_some()
    });
    let state1 = ctx.viewer_overlay("raw.overlay").expect("raw.overlay");
    assert_eq!(state1.scene.as_deref(), Some(&expected_good));
    let refused1 = state1.refused.as_deref().expect("malformed JSON refused");
    assert!(
        refused1.contains("malformed overlay scene JSON"),
        "expected malformed JSON error in refused reason, got: {refused1}"
    );
    assert!(!ctx.faults().is_empty());

    // 2. Oversized JSON (> MAX_OVERLAY_SCENE_JSON_BYTES) is rejected before parsing and preserves the last valid scene.
    RAW_SCENE_MODE.store(2, Ordering::Release);
    ctx.send_overlay_event(
        "raw.overlay",
        OverlayEvent::Drag {
            handle: Arc::from("act"),
            phase: DragPhase::Move,
            at: [110.0, 110.0],
        },
    )
    .expect("send event triggering oversized scene rebuild");
    wait_until(&ctx, || {
        ctx.viewer_overlay("raw.overlay")
            .and_then(|s| s.refused)
            .is_some_and(|r| r.contains("overlay JSON exceeds limit"))
    });
    let state2 = ctx.viewer_overlay("raw.overlay").expect("raw.overlay");
    assert_eq!(state2.scene.as_deref(), Some(&expected_good));

    // 3. Panic in `overlay_event` faults the plugin, unmounts it, and drops the overlay registration.
    RAW_SCENE_MODE.store(4, Ordering::Release);
    ctx.send_overlay_event(
        "raw.overlay",
        OverlayEvent::Drag {
            handle: Arc::from("act"),
            phase: DragPhase::End,
            at: [120.0, 120.0],
        },
    )
    .expect("send event triggering panic");
    wait_until(&ctx, || !ctx.is_mounted("test.rawoverlay"));
    assert!(ctx.viewer_overlays().is_empty());
    assert!(
        ctx.failures().iter().any(|f| {
            &*f.id == "test.rawoverlay" && f.reason.contains("raw overlay_event panic")
        }),
        "failures: {:?}",
        ctx.failures()
    );

    // 4. Panic in `overlay_scene` also faults the plugin and drops its overlay registration.
    let pkg_scene_panic = write_test_package(temp.path(), "test.scenepanic");
    RAW_SCENE_MODE.store(0, Ordering::Release);
    let plugin_scene_panic =
        from_table(&pkg_scene_panic, raw_plugin_table(), Some(temp.path())).expect("from_table");
    ctx.mount(plugin_scene_panic.as_ref())
        .expect("mount test.scenepanic");
    let _ = ctx.viewer_overlay("raw.overlay");
    wait_until(&ctx, || {
        ctx.viewer_overlay("raw.overlay")
            .is_some_and(|s| s.scene.as_deref() == Some(&expected_good))
    });
    RAW_SCENE_MODE.store(3, Ordering::Release);
    ctx.send_overlay_event(
        "raw.overlay",
        OverlayEvent::Drag {
            handle: Arc::from("act"),
            phase: DragPhase::Start,
            at: [100.0, 100.0],
        },
    )
    .expect("send event triggering scene panic");
    wait_until(&ctx, || !ctx.is_mounted("test.scenepanic"));
    assert!(ctx.viewer_overlays().is_empty());
    assert!(
        ctx.failures().iter().any(|f| {
            &*f.id == "test.scenepanic" && f.reason.contains("raw overlay_scene panic")
        }),
        "failures: {:?}",
        ctx.failures()
    );

    // 5. Legacy `ENCODERS_SIZE` plugin table (without overlay callbacks) is accepted by `check_compat`,
    //    fills overlay stubs via `copy_from_prefix`, and refuses `register_viewer_overlay` during mount.
    assert_eq!(check_compat(ABI_VERSION, ARIA_PLUGIN_V1_MIN_SIZE), Ok(()));
    assert_eq!(
        check_compat(ABI_VERSION, ARIA_PLUGIN_V1_PANELS_SIZE),
        Ok(())
    );
    assert_eq!(
        check_compat(ABI_VERSION, ARIA_PLUGIN_V1_ENCODERS_SIZE),
        Ok(())
    );
    assert!(ARIA_HOST_V1_MIN_SIZE < ARIA_HOST_V1_PANELS_SIZE);
    assert!(ARIA_HOST_V1_PANELS_SIZE < ARIA_HOST_V1_ENCODERS_SIZE);
    assert!(host_table().has_overlays());

    let mut legacy_encoders_table = raw_plugin_table();
    legacy_encoders_table.struct_size = ARIA_PLUGIN_V1_ENCODERS_SIZE;
    assert!(legacy_encoders_table.has_panels());
    assert!(legacy_encoders_table.has_encoders());
    assert!(!legacy_encoders_table.has_overlays());

    // SAFETY: `legacy_encoders_table` has a valid `ARIA_PLUGIN_V1_ENCODERS_SIZE` prefix.
    let copied = unsafe {
        AriaPluginV1::copy_from_prefix(
            &raw const legacy_encoders_table,
            ARIA_PLUGIN_V1_ENCODERS_SIZE,
        )
    };
    let pkg_legacy = write_test_package(temp.path(), "test.legacyencoders");
    let legacy_plugin = from_table(&pkg_legacy, copied, Some(temp.path())).expect("from_table");
    let mount_err = ctx
        .mount(legacy_plugin.as_ref())
        .expect_err("legacy table without overlay callbacks must fail register_viewer_overlay");
    match mount_err {
        PluginError::MountFailed(_, msg) => assert!(
            msg.contains("overlay_scene"),
            "expected overlay_scene mentioned in error, got: {msg}"
        ),
        other => panic!("unexpected error: {other:?}"),
    }

    let _ = (AriaOwnedStr::empty(), AriaOwnedBytes::empty());
}
