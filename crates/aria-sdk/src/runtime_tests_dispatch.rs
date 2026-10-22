//! End-to-end [`PluginRuntime`] callback dispatch, panic recovery, and legacy
//! host-table size checks.

use parking_lot::Mutex;
use std::ffi::c_void;
use std::path::Path;

use aria_plugin_abi::{
    ARIA_API_MAJOR, ARIA_BYTES_FOUND, ARIA_BYTES_PANIC, ARIA_STATUS_ERR, ARIA_STATUS_OK,
    ARIA_STATUS_PANIC, AriaAudioBlock, AriaAudioParam, AriaEncoderAudio, AriaEncoderConfig,
    AriaEncoderFrame, AriaStr, free_rust_bytes, free_rust_string,
};

use crate::{
    AudioBlock, AudioEffectSpec, AudioParam, AudioParamSpec, AudioProcessor, DecodeFailure,
    Decoder, DecoderProbe, DecoderSpec, DropAction, EncoderAudio, EncoderConfig, EncoderFrame,
    EncoderSession, EncoderSpec, EventResponse, ExportReport, ExporterSpec, HandleLook, Host,
    ImportResult, ImporterSpec, LogLevel, OverlayColor, OverlayEvent, OverlayScene, OverlayShape,
    Panel, PanelEvent, PanelLocation, PanelSpec, Plugin, PluginRuntime, UiNode, VideoInfo,
    VideoStream,
};

use super::{MOCK_HOST, MockHostState};

struct ScaleAudio {
    scale: f32,
}

impl AudioProcessor for ScaleAudio {
    fn process(&mut self, params: &[AudioParam<'_>], block: &mut AudioBlock<'_>) {
        let gain = params.first().map_or(self.scale, |p| p.constant());
        if gain < 0.0 {
            panic!("negative gain panic");
        }
        let sc_boost = block
            .sidechain()
            .and_then(|(l, _)| l.first().copied())
            .unwrap_or(0.0);
        let (left, right) = block.channels();
        for (i, (l, r)) in left.iter_mut().zip(right.iter_mut()).enumerate() {
            let p = params.first().map_or(gain, |param| param.at(i));
            *l = (*l + sc_boost) * p;
            *r = (*r + sc_boost) * p;
        }
    }

    fn reset(&mut self) {
        if self.scale.is_nan() {
            panic!("nan reset panic");
        }
        self.scale = 1.0;
    }
}

struct DemoPanel {
    clicks: Mutex<u32>,
}

impl Panel for DemoPanel {
    fn view(&self, _host: &Host) -> UiNode {
        let clicks = *self.clicks.lock();
        if clicks == 99 {
            panic!("panel view panicked");
        }
        UiNode::column([
            UiNode::heading(format!("Clicks {clicks}")),
            UiNode::button("inc", "Increment"),
        ])
    }

    fn event(&self, host: &Host, event: &PanelEvent) -> Result<(), String> {
        match event {
            PanelEvent::Pressed { id } if id.as_ref() == "inc" => {
                *self.clicks.lock() += 1;
                host.refresh_panel("test.panel")
            }
            PanelEvent::Pressed { id } if id.as_ref() == "panic_event" => {
                panic!("panel event panicked");
            }
            PanelEvent::Pressed { id } if id.as_ref() == "arm_view_panic" => {
                *self.clicks.lock() = 99;
                Ok(())
            }
            other => Err(format!("unexpected event: {other:?}")),
        }
    }
}

struct DemoEncoderSession {
    frames: u32,
    samples: usize,
    panic_on_finish: bool,
}

impl EncoderSession for DemoEncoderSession {
    fn push_frame(&mut self, frame: &EncoderFrame<'_>) -> Result<(), String> {
        if frame.width == 0 {
            panic!("zero-width frame panic");
        }
        self.frames += 1;
        Ok(())
    }

    fn push_audio(&mut self, audio: &EncoderAudio<'_>) -> Result<(), String> {
        if audio.channels == 0 {
            panic!("zero-channel audio panic");
        }
        self.samples += audio.samples.len();
        Ok(())
    }

    fn finish(self: Box<Self>) -> Result<(), String> {
        if self.panic_on_finish {
            panic!("finish encoder panic");
        }
        Ok(())
    }
}

/// Reads nothing: registration through the mock host is what is under test.
struct NoFiles;

impl Decoder for NoFiles {
    fn probe(&self, _host: &Host, path: &Path) -> Result<DecoderProbe, DecodeFailure> {
        Err(DecodeFailure::Unsupported(path.display().to_string()))
    }

    fn open_video(
        &self,
        _host: &Host,
        path: &Path,
    ) -> Result<(VideoInfo, Box<dyn VideoStream>), DecodeFailure> {
        Err(DecodeFailure::Unsupported(path.display().to_string()))
    }
}

struct TestPlugin;

impl Plugin for TestPlugin {
    fn mount(&self, host: &Host) -> Result<(), String> {
        host.log(LogLevel::Info, "mounting test plugin");
        host.register_command("test.run", "Run Test")?;
        host.subscribe("command-proposed")?;
        host.register_importer(&ImporterSpec {
            id: "test.imp".to_owned(),
            name: "Test Importer".to_owned(),
            extensions: vec!["tst".to_owned()],
        })?;
        host.register_exporter(&ExporterSpec {
            id: "test.exp".to_owned(),
            name: "Test Exporter".to_owned(),
            extension: "tst".to_owned(),
        })?;
        host.register_audio_effect(&AudioEffectSpec {
            id: "test.gain".to_owned(),
            label: "Test Gain".to_owned(),
            category: String::new(),
            params: vec![AudioParamSpec::scalar("gain", "Gain", 0.5, 0.0, 2.0)],
            latency: 0,
            sidechain: true,
        })?;
        host.register_panel(
            PanelSpec::new("test.panel", "Test Panel", PanelLocation::Sidebar),
            DemoPanel {
                clicks: Mutex::new(0),
            },
        )?;
        host.register_encoder(
            EncoderSpec {
                id: "test.enc".to_owned(),
                name: "Test Encoder".to_owned(),
                extension: "mov".to_owned(),
                audio: true,
            },
            |_host: &Host, path: &Path, cfg: &EncoderConfig| {
                if path.as_os_str() == "panic_open" {
                    panic!("open encoder panic");
                }
                let session: Box<dyn EncoderSession> = Box::new(DemoEncoderSession {
                    frames: 0,
                    samples: 0,
                    panic_on_finish: cfg.width == 999,
                });
                Ok(session)
            },
        )?;
        host.register_viewer_overlay("test.overlay", "Test Overlay")?;
        host.register_decoder(
            DecoderSpec {
                id: "test.dec".to_owned(),
                name: "Test Decoder".to_owned(),
                extensions: vec!["tdec".to_owned()],
                priority: 0,
            },
            NoFiles,
        )?;
        Ok(())
    }

    fn command_run(&self, host: &Host, id: &str, _args_json: &str) -> Result<(), String> {
        if id == "panic" {
            panic!("command exploded");
        }
        if id == "fail" {
            return Err("command refused".to_owned());
        }
        host.storage_set("cmd", b"ran")?;
        let val = host.storage_get("cmd")?;
        assert_eq!(val.as_deref(), Some(&b"ran"[..]));
        assert_eq!(host.storage_keys()?, vec!["cmd".to_owned()]);
        host.storage_delete("cmd")?;
        assert_eq!(host.storage_get("cmd")?, None);
        assert_eq!(host.setting_get("notes.limit")?, Some(serde_json::json!(8)));
        assert_eq!(host.setting_get("absent")?, None);
        assert_eq!(host.project_snapshot()?["name"], "Demo");
        assert_eq!(host.editor_view()?["speed"], 1);
        let receipt = host.project_transact("Edit", &[serde_json::json!({"op": 1})])?;
        assert_eq!(receipt["revision"], 2);
        assert!(host.project_undo()?);
        Ok(())
    }

    fn event(&self, _host: &Host, kind: &str, event_json: &str) -> Result<EventResponse, String> {
        if event_json == "panic" {
            panic!("event handler panicked");
        }
        match kind {
            "command-proposed" => Ok(EventResponse::Refuse("too many tracks".to_owned())),
            "resolve-dropped" | "resolve-dropped-path" => {
                Ok(EventResponse::ClaimDrop(DropAction::ImportMedia))
            }
            _ => Ok(EventResponse::Pass),
        }
    }

    fn import(
        &self,
        _host: &Host,
        importer_id: &str,
        _path: &Path,
    ) -> Result<ImportResult, String> {
        if importer_id == "panic" {
            panic!("importer panicked");
        }
        Ok(ImportResult::new("[]", vec!["skipped line".to_owned()]))
    }

    fn export(
        &self,
        _host: &Host,
        exporter_id: &str,
        _project_json: &str,
        _path: &Path,
    ) -> Result<ExportReport, String> {
        if exporter_id == "panic" {
            panic!("exporter panicked");
        }
        Ok(ExportReport {
            skipped: vec!["unexported".to_owned()],
        })
    }

    fn audio_create(
        &self,
        _host: &Host,
        effect_id: &str,
        _sample_rate: u32,
    ) -> Result<Box<dyn AudioProcessor>, String> {
        if effect_id == "panic" {
            panic!("audio_create panicked");
        }
        if effect_id == "nan" {
            return Ok(Box::new(ScaleAudio { scale: f32::NAN }));
        }
        Ok(Box::new(ScaleAudio { scale: 0.5 }))
    }

    fn overlay_scene(&self, _host: &Host, id: &str) -> OverlayScene {
        if id == "panic_scene" {
            panic!("overlay scene panicked");
        }
        OverlayScene::new(vec![OverlayShape::Handle {
            id: "focus".into(),
            at: [100.0, 200.0],
            color: OverlayColor::Accent,
            look: HandleLook::Cross,
        }])
    }

    fn overlay_event(&self, host: &Host, id: &str, event: &OverlayEvent) -> Result<(), String> {
        let OverlayEvent::Drag { handle, .. } = event;
        if &**handle == "panic_event" {
            panic!("overlay event panicked");
        }
        if &**handle == "err_event" {
            return Err("overlay event refused".to_owned());
        }
        host.refresh_viewer_overlay(id)
    }
}

#[test]
fn runtime_executes_every_host_and_plugin_operation_and_catches_panics() {
    let runtime = PluginRuntime::new(TestPlugin);
    let state = Mutex::new(MockHostState::default());
    let ctx = (&raw const state) as *mut c_void;

    #[allow(unsafe_code)]
    let bound = unsafe { runtime.bind_host(&MOCK_HOST, ARIA_API_MAJOR) };
    assert!(bound);

    let mount_res = runtime.handle_mount(ctx);
    assert_eq!(mount_res.status, ARIA_STATUS_OK);
    {
        let st = state.lock();
        assert_eq!(st.commands.len(), 1);
        assert_eq!(st.subscriptions, vec!["command-proposed"]);
        assert_eq!(st.importers.len(), 1);
        assert_eq!(st.exporters.len(), 1);
        assert_eq!(st.audio_effects.len(), 1);
        assert_eq!(st.panels.len(), 1);
        assert_eq!(st.encoders.len(), 1);
        assert_eq!(st.overlays.len(), 1);
        assert_eq!(
            st.decoders,
            vec![r#"{"id":"test.dec","name":"Test Decoder","extensions":["tdec"],"priority":0}"#]
        );
    }

    let cmd_ok =
        runtime.handle_command_run(ctx, AriaStr::from_str("test.run"), AriaStr::from_str("{}"));
    assert_eq!(cmd_ok.status, ARIA_STATUS_OK);
    assert_eq!(state.lock().undos, 1);

    let cmd_err =
        runtime.handle_command_run(ctx, AriaStr::from_str("fail"), AriaStr::from_str("{}"));
    assert_eq!(cmd_err.status, ARIA_STATUS_ERR);
    #[allow(unsafe_code)]
    let err_msg = unsafe { cmd_err.value.into_string_with(free_rust_string) };
    assert_eq!(err_msg, "command refused");

    let cmd_panic =
        runtime.handle_command_run(ctx, AriaStr::from_str("panic"), AriaStr::from_str("{}"));
    assert_eq!(cmd_panic.status, ARIA_STATUS_PANIC);
    #[allow(unsafe_code)]
    let panic_msg = unsafe { cmd_panic.value.into_string_with(free_rust_string) };
    assert_eq!(panic_msg, "command exploded");

    let ev_veto = runtime.handle_event(
        ctx,
        AriaStr::from_str("command-proposed"),
        AriaStr::from_str("{}"),
    );
    assert_eq!(ev_veto.status, ARIA_STATUS_OK);
    #[allow(unsafe_code)]
    let veto_json = unsafe { ev_veto.value.into_string_with(free_rust_string) };
    assert_eq!(veto_json, r#"{"veto":"too many tracks"}"#);

    let ev_panic = runtime.handle_event(
        ctx,
        AriaStr::from_str("command-proposed"),
        AriaStr::from_str("panic"),
    );
    assert_eq!(ev_panic.status, ARIA_STATUS_PANIC);
    #[allow(unsafe_code)]
    let ev_panic_msg = unsafe { ev_panic.value.into_string_with(free_rust_string) };
    assert_eq!(ev_panic_msg, "event handler panicked");

    let imp_panic =
        runtime.handle_import(ctx, AriaStr::from_str("panic"), AriaStr::from_str("a.tst"));
    assert_eq!(imp_panic.status, ARIA_STATUS_PANIC);
    #[allow(unsafe_code)]
    let _ = unsafe { imp_panic.value.into_string_with(free_rust_string) };

    let exp_panic = runtime.handle_export(
        ctx,
        AriaStr::from_str("panic"),
        AriaStr::from_str("{}"),
        AriaStr::from_str("a.tst"),
    );
    assert_eq!(exp_panic.status, ARIA_STATUS_PANIC);
    #[allow(unsafe_code)]
    let _ = unsafe { exp_panic.value.into_string_with(free_rust_string) };

    let created = runtime.handle_audio_create(ctx, AriaStr::from_str("test.gain"), 48_000);
    assert_eq!(created.status, ARIA_STATUS_OK);
    let mut left = [1.0f32, 2.0f32];
    let mut right = [4.0f32, 6.0f32];
    let sc_l = [1.0f32, 1.0f32];
    let sc_r = [0.0f32, 0.0f32];
    let per_frame = [0.5f32, 0.25f32];
    let param = AriaAudioParam {
        constant: 0.5,
        per_frame: per_frame.as_ptr(),
        per_frame_len: per_frame.len(),
    };
    let mut block = AriaAudioBlock {
        left: left.as_mut_ptr(),
        right: right.as_mut_ptr(),
        frames: 2,
        position: 0,
        sample_rate: 48_000,
        has_sidechain: 1,
        sidechain_left: sc_l.as_ptr(),
        sidechain_right: sc_r.as_ptr(),
    };
    #[allow(unsafe_code)]
    let proc_ok = unsafe {
        runtime.handle_audio_process(ctx, created.instance, &raw const param, 1, &raw mut block)
    };
    assert_eq!(proc_ok.status, ARIA_STATUS_OK);
    assert_eq!(left, [1.0, 0.75]);
    assert_eq!(right, [2.5, 1.75]);

    let bad_param = AriaAudioParam {
        constant: -1.0,
        per_frame: std::ptr::null(),
        per_frame_len: 0,
    };
    #[allow(unsafe_code)]
    let proc_panic = unsafe {
        runtime.handle_audio_process(
            ctx,
            created.instance,
            &raw const bad_param,
            1,
            &raw mut block,
        )
    };
    assert_eq!(proc_panic.status, ARIA_STATUS_PANIC);
    #[allow(unsafe_code)]
    let proc_panic_msg = unsafe { proc_panic.value.into_string_with(free_rust_string) };
    assert_eq!(proc_panic_msg, "negative gain panic");

    let dropped = runtime.handle_audio_drop(ctx, created.instance);
    assert_eq!(dropped.status, ARIA_STATUS_OK);

    let view_res = runtime.handle_panel_view(ctx, AriaStr::from_str("test.panel"));
    assert_eq!(view_res.status, ARIA_BYTES_FOUND);
    #[allow(unsafe_code)]
    let view_bytes = unsafe { view_res.bytes.into_vec_with(free_rust_bytes) };
    let tree: UiNode = serde_json::from_slice(&view_bytes).expect("valid UiNode JSON");
    assert_eq!(
        tree,
        UiNode::column([
            UiNode::heading("Clicks 0"),
            UiNode::button("inc", "Increment"),
        ])
    );

    let ev_res = runtime.handle_panel_event(
        ctx,
        AriaStr::from_str("test.panel"),
        AriaStr::from_str(r#"{"kind":"pressed","id":"inc"}"#),
    );
    assert_eq!(ev_res.status, ARIA_STATUS_OK);
    assert_eq!(state.lock().refreshes, vec!["test.panel".to_owned()]);

    let ev_panic = runtime.handle_panel_event(
        ctx,
        AriaStr::from_str("test.panel"),
        AriaStr::from_str(r#"{"kind":"pressed","id":"panic_event"}"#),
    );
    assert_eq!(ev_panic.status, ARIA_STATUS_PANIC);
    #[allow(unsafe_code)]
    let ev_panic_msg = unsafe { ev_panic.value.into_string_with(free_rust_string) };
    assert_eq!(ev_panic_msg, "panel event panicked");

    let arm_res = runtime.handle_panel_event(
        ctx,
        AriaStr::from_str("test.panel"),
        AriaStr::from_str(r#"{"kind":"pressed","id":"arm_view_panic"}"#),
    );
    assert_eq!(arm_res.status, ARIA_STATUS_OK);
    let view_panic = runtime.handle_panel_view(ctx, AriaStr::from_str("test.panel"));
    assert_eq!(view_panic.status, ARIA_BYTES_PANIC);
    #[allow(unsafe_code)]
    let view_panic_msg = unsafe { view_panic.error.into_string_with(free_rust_string) };
    assert_eq!(view_panic_msg, "panel view panicked");
    let enc_cfg = AriaEncoderConfig {
        width: 4,
        height: 2,
        fps_num: 24,
        fps_den: 1,
        video_bitrate_kbps: 1000,
        audio_bitrate_kbps: 128,
    };
    #[allow(unsafe_code)]
    let enc_opened = unsafe {
        runtime.handle_encoder_open(
            ctx,
            AriaStr::from_str("test.enc"),
            AriaStr::from_str("out.mov"),
            &raw const enc_cfg,
        )
    };
    assert_eq!(enc_opened.status, ARIA_STATUS_OK);
    let rgba = [255u8; 32];
    let raw_frame = AriaEncoderFrame {
        rgba: rgba.as_ptr(),
        rgba_len: rgba.len(),
        width: 4,
        height: 2,
        stride: 16,
        pts_value: 0,
        pts_rate: 24,
    };
    #[allow(unsafe_code)]
    let push_ok = unsafe {
        runtime.handle_encoder_push_frame(ctx, enc_opened.instance, &raw const raw_frame)
    };
    assert_eq!(push_ok.status, ARIA_STATUS_OK);
    let samples = [0.25f32, -0.25f32];
    let raw_audio = AriaEncoderAudio {
        samples: samples.as_ptr(),
        samples_len: samples.len(),
        sample_rate: 48_000,
        channels: 2,
        pts_value: 0,
        pts_rate: 48_000,
    };
    #[allow(unsafe_code)]
    let audio_ok = unsafe {
        runtime.handle_encoder_push_audio(ctx, enc_opened.instance, &raw const raw_audio)
    };
    assert_eq!(audio_ok.status, ARIA_STATUS_OK);
    let fin_ok = runtime.handle_encoder_finish(ctx, enc_opened.instance);
    assert_eq!(fin_ok.status, ARIA_STATUS_OK);

    #[allow(unsafe_code)]
    let enc_panic_open = unsafe {
        runtime.handle_encoder_open(
            ctx,
            AriaStr::from_str("test.enc"),
            AriaStr::from_str("panic_open"),
            &raw const enc_cfg,
        )
    };
    assert_eq!(enc_panic_open.status, ARIA_STATUS_PANIC);
    #[allow(unsafe_code)]
    let open_panic_msg = unsafe { enc_panic_open.error.into_string_with(free_rust_string) };
    assert_eq!(open_panic_msg, "open encoder panic");

    let ovl_res = runtime.handle_overlay_scene(ctx, AriaStr::from_str("test.overlay"));
    assert_eq!(ovl_res.status, ARIA_BYTES_FOUND);
    #[allow(unsafe_code)]
    let ovl_bytes = unsafe { ovl_res.bytes.into_vec_with(free_rust_bytes) };
    let decoded_scene: OverlayScene =
        serde_json::from_slice(&ovl_bytes).expect("valid OverlayScene JSON");
    assert_eq!(
        decoded_scene,
        OverlayScene::new(vec![OverlayShape::Handle {
            id: "focus".into(),
            at: [100.0, 200.0],
            color: OverlayColor::Accent,
            look: HandleLook::Cross,
        }])
    );

    let ovl_ev_ok = runtime.handle_overlay_event(
        ctx,
        AriaStr::from_str("test.overlay"),
        AriaStr::from_str(r#"{"kind":"drag","handle":"focus","phase":"end","at":[120.0,240.0]}"#),
    );
    assert_eq!(ovl_ev_ok.status, ARIA_STATUS_OK);
    assert_eq!(
        state.lock().overlay_refreshes,
        vec!["test.overlay".to_owned()]
    );

    let ovl_ev_err = runtime.handle_overlay_event(
        ctx,
        AriaStr::from_str("test.overlay"),
        AriaStr::from_str(r#"{"kind":"drag","handle":"err_event","phase":"start","at":[0.0,0.0]}"#),
    );
    assert_eq!(ovl_ev_err.status, ARIA_STATUS_ERR);
    #[allow(unsafe_code)]
    let ovl_err_msg = unsafe { ovl_ev_err.value.into_string_with(free_rust_string) };
    assert_eq!(ovl_err_msg, "overlay event refused");

    let ovl_ev_panic = runtime.handle_overlay_event(
        ctx,
        AriaStr::from_str("test.overlay"),
        AriaStr::from_str(
            r#"{"kind":"drag","handle":"panic_event","phase":"move","at":[10.0,20.0]}"#,
        ),
    );
    assert_eq!(ovl_ev_panic.status, ARIA_STATUS_PANIC);
    #[allow(unsafe_code)]
    let ovl_panic_msg = unsafe { ovl_ev_panic.value.into_string_with(free_rust_string) };
    assert_eq!(ovl_panic_msg, "overlay event panicked");

    let ovl_scene_panic = runtime.handle_overlay_scene(ctx, AriaStr::from_str("panic_scene"));
    assert_eq!(ovl_scene_panic.status, ARIA_BYTES_PANIC);
    #[allow(unsafe_code)]
    let scene_panic_msg = unsafe { ovl_scene_panic.error.into_string_with(free_rust_string) };
    assert_eq!(scene_panic_msg, "overlay scene panicked");

    let unmount_res = runtime.handle_unmount(ctx);
    assert_eq!(unmount_res.status, ARIA_STATUS_OK);
}

#[test]
fn legacy_host_table_without_overlays_reports_missing_callback() {
    struct OverlayOnlyPlugin;
    impl Plugin for OverlayOnlyPlugin {
        fn mount(&self, host: &Host) -> Result<(), String> {
            host.register_viewer_overlay("legacy.overlay", "Legacy Overlay")
        }
        fn command_run(&self, host: &Host, _id: &str, _args: &str) -> Result<(), String> {
            host.refresh_viewer_overlay("legacy.overlay")
        }
    }

    let mut short_host = MOCK_HOST;
    short_host.struct_size = aria_plugin_abi::ARIA_HOST_V1_ENCODERS_SIZE;
    assert!(!short_host.has_overlays());

    let runtime = PluginRuntime::new(OverlayOnlyPlugin);
    let state = Mutex::new(MockHostState::default());
    let ctx = (&raw const state) as *mut c_void;

    #[allow(unsafe_code)]
    let bound = unsafe { runtime.bind_host(&short_host, ARIA_API_MAJOR) };
    assert!(bound);

    let mount_res = runtime.handle_mount(ctx);
    assert_eq!(mount_res.status, ARIA_STATUS_ERR);
    #[allow(unsafe_code)]
    let mount_err = unsafe { mount_res.value.into_string_with(free_rust_string) };
    assert!(
        mount_err.contains("register_viewer_overlay"),
        "expected missing callback name in error, got: {mount_err}"
    );

    let refresh_res =
        runtime.handle_command_run(ctx, AriaStr::from_str("c"), AriaStr::from_str("{}"));
    assert_eq!(refresh_res.status, ARIA_STATUS_ERR);
    #[allow(unsafe_code)]
    let refresh_err = unsafe { refresh_res.value.into_string_with(free_rust_string) };
    assert!(
        refresh_err.contains("refresh_viewer_overlay"),
        "expected missing callback name in error, got: {refresh_err}"
    );
}
