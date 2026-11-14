//! Regression tests for [`PluginRuntime`] unmount cleanup and synchronous
//! host registration rollback.
//!
//! WHY: `PluginRuntime` lives in a process-lifetime `OnceLock` inside native
//! plugin `cdylib`s, so `handle_unmount` must drop every per-mount registry and
//! active stream/session table (`decoders`, `decoder_instances`,
//! `cpu_effect_specs`, `analyzers`, and `analyzer_instances`), and
//! `Host::register_panel` / `Host::register_encoder` must bind handlers before
//! notifying the host and roll back on host refusal. Does not cover `dlopen`
//! library unloading, which `aria_plugin::external::tests` exercises.

use parking_lot::Mutex;
use std::ffi::c_void;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use aria_plugin_abi::{
    ARIA_API_MAJOR, ARIA_BYTES_ERR, ARIA_BYTES_FOUND, ARIA_STATUS_ERR, ARIA_STATUS_OK,
    AriaCpuEffectCall, AriaEncoderConfig, AriaHostV1, AriaResult, AriaStr, AriaVideoInfo,
    free_rust_bytes, free_rust_string,
};

use crate::{
    Analyzer, AnalyzerResult, AnalyzerRun, AnalyzerScope, AnalyzerSpec, CpuFrameIn, CpuFrameOut,
    DecodeFailure, Decoder, DecoderProbe, DecoderSpec, EffectParamSpec, EffectSpec, EncoderAudio,
    EncoderConfig, EncoderFrame, EncoderSession, EncoderSpec, FrameColor, FrameFormat, FramePlane,
    Host, Panel, PanelLocation, PanelSpec, Plugin, PluginRuntime, UiNode, VideoInfo, VideoStream,
};

use super::{MOCK_HOST, MockHostState};

struct NoopEncoderSession;

impl EncoderSession for NoopEncoderSession {
    fn push_frame(&mut self, _frame: &EncoderFrame<'_>) -> Result<(), String> {
        Ok(())
    }

    fn push_audio(&mut self, _audio: &EncoderAudio<'_>) -> Result<(), String> {
        Ok(())
    }

    fn finish(self: Box<Self>) -> Result<(), String> {
        Ok(())
    }
}

#[test]
fn unmount_drops_decoders_analyzers_streams_and_cpu_effect_specs() {
    struct TrackedVideo {
        drops: Arc<AtomicUsize>,
    }
    impl Drop for TrackedVideo {
        fn drop(&mut self) {
            self.drops.fetch_add(1, Ordering::SeqCst);
        }
    }
    impl VideoStream for TrackedVideo {
        fn seek(&mut self, _pts: i64) -> Result<(), String> {
            Ok(())
        }
        fn next_frame(&mut self, _planes: &mut [FramePlane<'_>]) -> Result<Option<i64>, String> {
            Ok(None)
        }
    }

    struct TrackedDecoder {
        drops: Arc<AtomicUsize>,
    }
    impl Decoder for TrackedDecoder {
        fn probe(&self, _host: &Host, _path: &Path) -> Result<DecoderProbe, DecodeFailure> {
            Ok(DecoderProbe {
                width: 2,
                height: 2,
                frame_rate_num: 24,
                frame_rate_den: 1,
                ..DecoderProbe::default()
            })
        }
        fn open_video(
            &self,
            _host: &Host,
            _path: &Path,
        ) -> Result<(VideoInfo, Box<dyn VideoStream>), DecodeFailure> {
            Ok((
                VideoInfo {
                    width: 2,
                    height: 2,
                    format: FrameFormat::Nv12,
                    color: FrameColor::Bt709,
                    frame_rate_num: 24,
                    frame_rate_den: 1,
                    time_base_num: 1,
                    time_base_den: 24,
                    duration: Some(24),
                },
                Box::new(TrackedVideo {
                    drops: Arc::clone(&self.drops),
                }),
            ))
        }
    }

    struct TrackedAnalyzerRun {
        drops: Arc<AtomicUsize>,
    }
    impl Drop for TrackedAnalyzerRun {
        fn drop(&mut self) {
            self.drops.fetch_add(1, Ordering::SeqCst);
        }
    }
    impl AnalyzerRun for TrackedAnalyzerRun {
        fn push(&mut self, _samples: &[f32]) -> Result<(), String> {
            Ok(())
        }
        fn finish(self: Box<Self>) -> Result<AnalyzerResult, String> {
            Ok(AnalyzerResult::default())
        }
    }

    struct TrackedAnalyzer {
        drops: Arc<AtomicUsize>,
    }
    impl Analyzer for TrackedAnalyzer {
        fn open(
            &self,
            _host: &Host,
            _sample_rate: u32,
            _channels: u16,
        ) -> Result<Box<dyn AnalyzerRun>, String> {
            Ok(Box::new(TrackedAnalyzerRun {
                drops: Arc::clone(&self.drops),
            }))
        }
    }

    struct UnmountPlugin {
        video_drops: Arc<AtomicUsize>,
        analyzer_drops: Arc<AtomicUsize>,
    }

    impl Plugin for UnmountPlugin {
        fn mount(&self, host: &Host) -> Result<(), String> {
            host.register_decoder(
                DecoderSpec {
                    id: "u.dec".to_owned(),
                    name: "Unmount Decoder".to_owned(),
                    extensions: vec!["udec".to_owned()],
                    priority: 0,
                },
                TrackedDecoder {
                    drops: Arc::clone(&self.video_drops),
                },
            )?;
            host.register_analyzer(
                AnalyzerSpec::new("u.ana", "Unmount Analyzer", AnalyzerScope::Clip),
                TrackedAnalyzer {
                    drops: Arc::clone(&self.analyzer_drops),
                },
            )?;
            host.register_cpu_effect_spec(
                &EffectSpec::new("u.fx", "Unmount CPU Effect")
                    .with_params(vec![EffectParamSpec::scalar("gain", "Gain", 1.0, 0.0, 2.0)]),
            )?;
            Ok(())
        }

        fn cpu_effect_process(
            &self,
            _host: &Host,
            _id: &str,
            input: &CpuFrameIn<'_>,
            output: &mut CpuFrameOut<'_>,
        ) -> Result<(), String> {
            output.pixels[0] = if input.spec.is_some() { 100 } else { 200 };
            Ok(())
        }
    }

    let video_drops = Arc::new(AtomicUsize::new(0));
    let analyzer_drops = Arc::new(AtomicUsize::new(0));
    let runtime = PluginRuntime::new(UnmountPlugin {
        video_drops: Arc::clone(&video_drops),
        analyzer_drops: Arc::clone(&analyzer_drops),
    });
    let state = Mutex::new(MockHostState::default());
    let ctx = (&raw const state) as *mut c_void;
    #[allow(unsafe_code)]
    let bound = unsafe { runtime.bind_host(&MOCK_HOST, ARIA_API_MAJOR) };
    assert!(bound);
    assert_eq!(runtime.handle_mount(ctx).status, ARIA_STATUS_OK);

    let mut info = AriaVideoInfo::default();
    #[allow(unsafe_code)]
    let vid = unsafe {
        runtime.handle_decoder_open_video(
            ctx,
            AriaStr::from_str("u.dec"),
            AriaStr::from_str("clip.udec"),
            &raw mut info,
        )
    };
    assert_eq!(vid.status, ARIA_STATUS_OK);

    let ana = runtime.handle_analyzer_open(ctx, AriaStr::from_str("u.ana"), 48_000, 2);
    assert_eq!(ana.status, ARIA_STATUS_OK);

    let input_px = [0u16; 4];
    let mut output_px = [0u16; 4];
    let params = [1.5f32];
    let mut fx_call = AriaCpuEffectCall {
        input: input_px.as_ptr(),
        output: output_px.as_mut_ptr(),
        pixels_len: 4,
        width: 1,
        height: 1,
        params: params.as_ptr(),
        params_len: 1,
        params_json: AriaStr::from_str("{}"),
        time: 0.0,
    };
    #[allow(unsafe_code)]
    let fx_before = unsafe {
        runtime.handle_cpu_effect_process(ctx, AriaStr::from_str("u.fx"), &raw mut fx_call)
    };
    assert_eq!(fx_before.status, ARIA_STATUS_OK);
    assert_eq!(output_px[0], 100);
    assert_eq!(video_drops.load(Ordering::SeqCst), 0);
    assert_eq!(analyzer_drops.load(Ordering::SeqCst), 0);

    assert_eq!(runtime.handle_unmount(ctx).status, ARIA_STATUS_OK);
    assert_eq!(video_drops.load(Ordering::SeqCst), 1);
    assert_eq!(analyzer_drops.load(Ordering::SeqCst), 1);

    let seek_after = runtime.handle_decoder_seek(ctx, vid.instance, 0);
    assert_eq!(seek_after.status, ARIA_STATUS_ERR);
    #[allow(unsafe_code)]
    drop(unsafe { seek_after.value.into_string_with(free_rust_string) });

    let samples = [0.0f32; 2];
    #[allow(unsafe_code)]
    let push_after =
        unsafe { runtime.handle_analyzer_push(ctx, ana.instance, samples.as_ptr(), samples.len()) };
    assert_eq!(push_after.status, ARIA_STATUS_ERR);
    #[allow(unsafe_code)]
    drop(unsafe { push_after.value.into_string_with(free_rust_string) });

    let probe_after = runtime.handle_decoder_probe(
        ctx,
        AriaStr::from_str("u.dec"),
        AriaStr::from_str("clip.udec"),
    );
    assert_eq!(probe_after.status, ARIA_STATUS_ERR);
    #[allow(unsafe_code)]
    let probe_err = unsafe { probe_after.value.into_string_with(free_rust_string) };
    assert!(probe_err.contains("unknown decoder: u.dec"), "{probe_err}");

    let ana_after = runtime.handle_analyzer_open(ctx, AriaStr::from_str("u.ana"), 48_000, 2);
    assert_eq!(ana_after.status, ARIA_STATUS_ERR);
    #[allow(unsafe_code)]
    let ana_err = unsafe { ana_after.error.into_string_with(free_rust_string) };
    assert!(ana_err.contains("no analyzer u.ana"), "{ana_err}");

    #[allow(unsafe_code)]
    let fx_after = unsafe {
        runtime.handle_cpu_effect_process(ctx, AriaStr::from_str("u.fx"), &raw mut fx_call)
    };
    assert_eq!(fx_after.status, ARIA_STATUS_OK);
    assert_eq!(
        output_px[0], 200,
        "cpu_effect_specs must clear on unmount so stale specs do not leak across mounts"
    );
}

#[test]
fn panel_and_encoder_handlers_are_live_during_host_registration_and_roll_back_on_error() {
    struct SyncRegPanel {
        label: &'static str,
    }
    impl Panel for SyncRegPanel {
        fn view(&self, _host: &Host) -> UiNode {
            UiNode::heading(self.label)
        }
    }

    struct SyncMountPlugin;
    impl Plugin for SyncMountPlugin {
        fn mount(&self, host: &Host) -> Result<(), String> {
            host.register_panel(
                PanelSpec::new("sync.panel", "Sync Panel", PanelLocation::Sidebar),
                SyncRegPanel { label: "first" },
            )?;
            let panel_dup = host.register_panel(
                PanelSpec::new("sync.panel", "Duplicate Panel", PanelLocation::Sidebar),
                SyncRegPanel { label: "replaced" },
            );
            assert!(panel_dup.is_err());
            let panel_refused = host.register_panel(
                PanelSpec::new("refuse.panel", "Refused Panel", PanelLocation::Sidebar),
                SyncRegPanel { label: "refused" },
            );
            assert!(panel_refused.is_err());

            host.register_encoder(
                EncoderSpec {
                    id: "sync.enc".to_owned(),
                    name: "Sync Encoder".to_owned(),
                    extension: "mov".to_owned(),
                    audio: false,
                },
                |_host: &Host, _path: &Path, _cfg: &EncoderConfig| {
                    Ok(Box::new(NoopEncoderSession) as Box<dyn EncoderSession>)
                },
            )?;
            let enc_refused = host.register_encoder(
                EncoderSpec {
                    id: "refuse.enc".to_owned(),
                    name: "Refused Encoder".to_owned(),
                    extension: "mov".to_owned(),
                    audio: false,
                },
                |_host: &Host, _path: &Path, _cfg: &EncoderConfig| {
                    Ok(Box::new(NoopEncoderSession) as Box<dyn EncoderSession>)
                },
            );
            assert!(enc_refused.is_err());
            Ok(())
        }
    }

    struct CallbackCtx {
        runtime: *const PluginRuntime<SyncMountPlugin>,
        panel_registered: bool,
    }

    #[allow(unsafe_code)]
    unsafe extern "C" fn sync_register_panel(ctx: *mut c_void, spec: AriaStr) -> AriaResult {
        // SAFETY: Test passes `&mut CallbackCtx` as `ctx` and valid UTF-8 `spec`.
        let cb = unsafe { &mut *(ctx as *mut CallbackCtx) };
        // SAFETY: Validated by test harness.
        let raw = unsafe { spec.as_str() }.unwrap();
        if raw.contains("refuse.panel") {
            return AriaResult::err("panel refused by host");
        }
        if raw.contains("sync.panel") && cb.panel_registered {
            return AriaResult::err("duplicate panel id");
        }
        cb.panel_registered = true;
        // SAFETY: `cb.runtime` points to the live `PluginRuntime` under test.
        let view = unsafe { (*cb.runtime).handle_panel_view(ctx, AriaStr::from_str("sync.panel")) };
        assert_eq!(
            view.status, ARIA_BYTES_FOUND,
            "panel handler must be bound before host register_panel runs"
        );
        // SAFETY: Allocated by `PluginRuntime` with Rust allocator.
        drop(unsafe { view.bytes.into_vec_with(free_rust_bytes) });
        AriaResult::ok_empty()
    }

    #[allow(unsafe_code)]
    unsafe extern "C" fn sync_register_encoder(ctx: *mut c_void, spec: AriaStr) -> AriaResult {
        // SAFETY: Test passes `&mut CallbackCtx` as `ctx` and valid UTF-8 `spec`.
        let cb = unsafe { &*(ctx as *const CallbackCtx) };
        // SAFETY: Validated by test harness.
        let raw = unsafe { spec.as_str() }.unwrap();
        if raw.contains("refuse.enc") {
            return AriaResult::err("encoder refused by host");
        }
        let cfg = AriaEncoderConfig {
            width: 2,
            height: 2,
            fps_num: 24,
            fps_den: 1,
            video_bitrate_kbps: 0,
            audio_bitrate_kbps: 0,
        };
        // SAFETY: `cb.runtime` and `cfg` are live for the call.
        let opened = unsafe {
            (*cb.runtime).handle_encoder_open(
                ctx,
                AriaStr::from_str("sync.enc"),
                AriaStr::from_str("probe.mov"),
                &raw const cfg,
            )
        };
        assert_eq!(
            opened.status, ARIA_STATUS_OK,
            "encoder handler must be bound before host register_encoder runs"
        );
        // SAFETY: `cb.runtime` is live for the call.
        let dropped = unsafe { (*cb.runtime).handle_encoder_drop(ctx, opened.instance) };
        assert_eq!(dropped.status, ARIA_STATUS_OK);
        AriaResult::ok_empty()
    }

    static SYNC_HOST: AriaHostV1 = {
        let mut h = MOCK_HOST;
        h.register_panel = sync_register_panel;
        h.register_encoder = sync_register_encoder;
        h
    };

    let runtime = PluginRuntime::new(SyncMountPlugin);
    let mut cb_ctx = CallbackCtx {
        runtime: &raw const runtime,
        panel_registered: false,
    };
    let ctx = (&raw mut cb_ctx).cast::<c_void>();
    #[allow(unsafe_code)]
    let bound = unsafe { runtime.bind_host(&SYNC_HOST, ARIA_API_MAJOR) };
    assert!(bound);

    let mount_res = runtime.handle_mount(ctx);
    assert_eq!(mount_res.status, ARIA_STATUS_OK);

    let kept_view = runtime.handle_panel_view(ctx, AriaStr::from_str("sync.panel"));
    assert_eq!(kept_view.status, ARIA_BYTES_FOUND);
    #[allow(unsafe_code)]
    let kept_bytes = unsafe { kept_view.bytes.into_vec_with(free_rust_bytes) };
    let kept_tree: UiNode = serde_json::from_slice(&kept_bytes).expect("UiNode JSON");
    assert_eq!(kept_tree, UiNode::heading("first"));

    let refused_view = runtime.handle_panel_view(ctx, AriaStr::from_str("refuse.panel"));
    assert_eq!(refused_view.status, ARIA_BYTES_ERR);
    #[allow(unsafe_code)]
    drop(unsafe { refused_view.error.into_string_with(free_rust_string) });

    let cfg = AriaEncoderConfig {
        width: 2,
        height: 2,
        fps_num: 24,
        fps_den: 1,
        video_bitrate_kbps: 0,
        audio_bitrate_kbps: 0,
    };
    #[allow(unsafe_code)]
    let refused_enc = unsafe {
        runtime.handle_encoder_open(
            ctx,
            AriaStr::from_str("refuse.enc"),
            AriaStr::from_str("out.mov"),
            &raw const cfg,
        )
    };
    assert_eq!(refused_enc.status, ARIA_STATUS_ERR);
    #[allow(unsafe_code)]
    drop(unsafe { refused_enc.error.into_string_with(free_rust_string) });
}

/// WHY: `Host::register_cpu_effect_spec`, `Host::register_cpu_generator_spec`,
/// and `Host::define_cpu_clip_type_spec` previously inserted into
/// `cpu_effect_specs` after the host table callback returned, leaving
/// `CpuFrameIn` without parameter metadata during synchronous host probes and
/// lacking rollback symmetry. `CpuFrameIn::{int,boolean,choice}` also cast
/// `NaN` or negative floats to `0` instead of parameter defaults. Closes all
/// three CPU spec registration paths and non-finite/out-of-range parameter
/// fallbacks. Does not cover GPU WGSL compilation.
#[test]
fn cpu_specs_are_live_during_host_registration_and_non_finite_params_use_defaults() {
    use crate::{ClipTypeSchema, ClipTypeSpec, EffectChoiceOption, MarkerColor};

    struct CpuSyncPlugin;
    impl Plugin for CpuSyncPlugin {
        fn mount(&self, host: &Host) -> Result<(), String> {
            let fx_spec = EffectSpec::new("sync.fx", "Sync FX").with_params(vec![
                EffectParamSpec::int("count", "Count", 7, 0, 10),
                EffectParamSpec::boolean("enabled", "Enabled", true),
                EffectParamSpec::choice(
                    "mode",
                    "Mode",
                    vec![
                        EffectChoiceOption::new("fast", "Fast"),
                        EffectChoiceOption::new("hq", "HQ"),
                    ],
                    "hq",
                ),
            ]);
            host.register_cpu_effect_spec(&fx_spec)?;
            assert!(
                host.register_cpu_effect_spec(&EffectSpec::new("sync.fx", "Dup FX"))
                    .is_err()
            );
            assert!(
                host.register_cpu_effect_spec(&EffectSpec::new("refuse.fx", "Refused FX"))
                    .is_err()
            );

            let gen_spec =
                EffectSpec::new("sync.gen", "Sync Gen").with_params(vec![EffectParamSpec::scalar(
                    "level", "Level", 0.5, 0.0, 1.0,
                )]);
            host.register_cpu_generator_spec(&gen_spec)?;
            assert!(
                host.register_cpu_generator_spec(&EffectSpec::new("refuse.gen", "Refused Gen"))
                    .is_err()
            );

            let clip_spec =
                ClipTypeSpec::new("sync.clip", "Sync Clip", "sync.clip", MarkerColor::Blue);
            let clip_schema =
                ClipTypeSchema::new(vec![EffectParamSpec::int("seed", "Seed", 42, 0, 100)]);
            host.define_cpu_clip_type_spec(&clip_spec, &clip_schema)?;
            let refused_clip = ClipTypeSpec::new(
                "refuse.clip",
                "Refused Clip",
                "refuse.clip",
                MarkerColor::Red,
            );
            assert!(
                host.define_cpu_clip_type_spec(&refused_clip, &clip_schema)
                    .is_err()
            );
            Ok(())
        }

        fn cpu_effect_process(
            &self,
            _host: &Host,
            id: &str,
            input: &CpuFrameIn<'_>,
            output: &mut CpuFrameOut<'_>,
        ) -> Result<(), String> {
            output.pixels[0] = match id {
                "sync.fx" => {
                    let count = input.int("count").unwrap_or(-1);
                    let enabled = input.boolean("enabled").unwrap_or(false);
                    let mode = input.choice("mode").unwrap_or_default();
                    match (count, enabled, mode.as_str()) {
                        (7, true, "hq") => 311,
                        _ => 900,
                    }
                }
                "sync.gen" => {
                    if input.scalar("level").is_some() {
                        312
                    } else {
                        901
                    }
                }
                "sync.clip" => {
                    if input.int("seed") == Some(42) {
                        313
                    } else {
                        902
                    }
                }
                _ => {
                    if input.spec.is_some() {
                        999
                    } else {
                        404
                    }
                }
            };
            Ok(())
        }
    }

    struct CpuSyncCtx {
        runtime: *const PluginRuntime<CpuSyncPlugin>,
        fx_registered: bool,
    }

    fn probe_cpu(
        runtime: &PluginRuntime<CpuSyncPlugin>,
        ctx: *mut c_void,
        id: &str,
        params: &[f32],
    ) -> u16 {
        let input_px = [0u16; 4];
        let mut output_px = [0u16; 4];
        let mut call = AriaCpuEffectCall {
            input: input_px.as_ptr(),
            output: output_px.as_mut_ptr(),
            pixels_len: 4,
            width: 1,
            height: 1,
            params: params.as_ptr(),
            params_len: params.len(),
            params_json: AriaStr::from_str("{}"),
            time: 0.0,
        };
        #[allow(unsafe_code)]
        let res =
            unsafe { runtime.handle_cpu_effect_process(ctx, AriaStr::from_str(id), &raw mut call) };
        assert_eq!(res.status, ARIA_STATUS_OK);
        output_px[0]
    }

    #[allow(unsafe_code)]
    unsafe extern "C" fn sync_reg_cpu_fx(ctx: *mut c_void, spec: AriaStr) -> AriaResult {
        let cb = unsafe { &mut *(ctx as *mut CpuSyncCtx) };
        let raw = unsafe { spec.as_str() }.unwrap();
        if raw.contains("refuse.fx") {
            return AriaResult::err("refused cpu fx");
        }
        if raw.contains("sync.fx") && cb.fx_registered {
            return AriaResult::err("duplicate cpu fx");
        }
        cb.fx_registered = true;
        let nan_params = [f32::NAN, f32::NAN, -3.0];
        let code = probe_cpu(unsafe { &*cb.runtime }, ctx, "sync.fx", &nan_params);
        assert_eq!(
            code, 311,
            "spec and default fallbacks must be live during register_cpu_effect"
        );
        AriaResult::ok_empty()
    }

    #[allow(unsafe_code)]
    unsafe extern "C" fn sync_reg_gen(
        ctx: *mut c_void,
        spec: AriaStr,
        _wgsl: AriaStr,
    ) -> AriaResult {
        let cb = unsafe { &*(ctx as *const CpuSyncCtx) };
        let raw = unsafe { spec.as_str() }.unwrap();
        if raw.contains("refuse.gen") {
            return AriaResult::err("refused gen");
        }
        let code = probe_cpu(unsafe { &*cb.runtime }, ctx, "sync.gen", &[0.75]);
        assert_eq!(
            code, 312,
            "generator spec must be live during register_generator"
        );
        AriaResult::ok_empty()
    }

    #[allow(unsafe_code)]
    unsafe extern "C" fn sync_reg_clip(ctx: *mut c_void, spec: AriaStr) -> AriaResult {
        let cb = unsafe { &*(ctx as *const CpuSyncCtx) };
        let raw = unsafe { spec.as_str() }.unwrap();
        if raw.contains("refuse.clip") {
            return AriaResult::err("refused clip");
        }
        let code = probe_cpu(unsafe { &*cb.runtime }, ctx, "sync.clip", &[f32::INFINITY]);
        assert_eq!(
            code, 313,
            "clip type CPU spec must be live during register_clip_type"
        );
        AriaResult::ok_empty()
    }

    static CPU_SYNC_HOST: AriaHostV1 = {
        let mut h = MOCK_HOST;
        h.register_cpu_effect = sync_reg_cpu_fx;
        h.register_generator = sync_reg_gen;
        h.register_clip_type = sync_reg_clip;
        h
    };

    let runtime = PluginRuntime::new(CpuSyncPlugin);
    let mut cb_ctx = CpuSyncCtx {
        runtime: &raw const runtime,
        fx_registered: false,
    };
    let ctx = (&raw mut cb_ctx).cast::<c_void>();
    #[allow(unsafe_code)]
    let bound = unsafe { runtime.bind_host(&CPU_SYNC_HOST, ARIA_API_MAJOR) };
    assert!(bound);
    assert_eq!(runtime.handle_mount(ctx).status, ARIA_STATUS_OK);

    assert_eq!(
        probe_cpu(&runtime, ctx, "sync.fx", &[f32::NAN, f32::NAN, f32::NAN]),
        311
    );
    assert_eq!(probe_cpu(&runtime, ctx, "refuse.fx", &[]), 404);
    assert_eq!(probe_cpu(&runtime, ctx, "refuse.gen", &[]), 404);
    assert_eq!(probe_cpu(&runtime, ctx, "refuse.clip", &[]), 404);
}

/// WHY: `Host::render_frame` previously freed only `res.bytes` on
/// `ARIA_BYTES_FOUND` and only `res.error` otherwise, leaking whichever buffer
/// was uninspected, and returned `RenderedFrame` without checking `width > 0`,
/// `height > 0`, or `width * height * 4 == data.len()`. Closes buffer leaks
/// and malformed RGBA8 frame construction across the C ABI. Does not cover
/// host-side GPU readback.
#[test]
fn host_render_frame_reclaims_both_buffers_and_validates_dimensions() {
    use crate::{FrameSize, FrameSource, Rational, RationalTime};
    use aria_plugin_abi::{AriaBytesResult, AriaFrameRequest, AriaOwnedBytes, AriaOwnedStr};

    static FREED_BYTES: AtomicUsize = AtomicUsize::new(0);
    static FREED_STRS: AtomicUsize = AtomicUsize::new(0);

    #[allow(unsafe_code)]
    unsafe extern "C" fn tracked_free_bytes(bytes: AriaOwnedBytes) {
        if !bytes.ptr.is_null() {
            FREED_BYTES.fetch_add(1, Ordering::SeqCst);
        }
        unsafe { free_rust_bytes(bytes) };
    }

    #[allow(unsafe_code)]
    unsafe extern "C" fn tracked_free_str(s: AriaOwnedStr) {
        if !s.is_null() {
            FREED_STRS.fetch_add(1, Ordering::SeqCst);
        }
        unsafe { free_rust_string(s) };
    }

    #[allow(unsafe_code)]
    unsafe extern "C" fn mock_render_frame(
        _ctx: *mut c_void,
        req: *mut AriaFrameRequest,
    ) -> AriaBytesResult {
        let req = unsafe { &mut *req };
        match req.source_id {
            1 => {
                req.width = 2;
                req.height = 2;
                AriaBytesResult {
                    status: ARIA_BYTES_FOUND,
                    bytes: AriaOwnedBytes::from_vec(vec![9u8; 16]),
                    error: AriaOwnedStr::from_string("diagnostic note".to_owned()),
                }
            }
            2 => AriaBytesResult {
                status: ARIA_BYTES_ERR,
                bytes: AriaOwnedBytes::from_vec(vec![1u8; 8]),
                error: AriaOwnedStr::from_string("render failed".to_owned()),
            },
            3 => {
                req.width = 2;
                req.height = 2;
                AriaBytesResult {
                    status: ARIA_BYTES_FOUND,
                    bytes: AriaOwnedBytes::from_vec(vec![7u8; 12]),
                    error: AriaOwnedStr::empty(),
                }
            }
            _ => {
                req.width = 0;
                req.height = 2;
                AriaBytesResult {
                    status: ARIA_BYTES_FOUND,
                    bytes: AriaOwnedBytes::empty(),
                    error: AriaOwnedStr::empty(),
                }
            }
        }
    }

    static FRAME_HOST: AriaHostV1 = {
        let mut h = MOCK_HOST;
        h.free_bytes = tracked_free_bytes;
        h.free_string = tracked_free_str;
        h.render_frame = mock_render_frame;
        h
    };

    FREED_BYTES.store(0, Ordering::SeqCst);
    FREED_STRS.store(0, Ordering::SeqCst);
    #[allow(unsafe_code)]
    let host = unsafe { Host::from_raw(&FRAME_HOST, std::ptr::dangling_mut::<c_void>()) };
    let t = RationalTime::new(0, Rational::FPS_24);

    let ok = host
        .render_frame(FrameSource::Clip(1), t, FrameSize::Canvas)
        .expect("valid 2x2 RGBA8 frame");
    assert_eq!((ok.width, ok.height, ok.data.len()), (2, 2, 16));
    assert_eq!(
        (
            FREED_BYTES.load(Ordering::SeqCst),
            FREED_STRS.load(Ordering::SeqCst)
        ),
        (1, 1)
    );

    let err = host
        .render_frame(FrameSource::Clip(2), t, FrameSize::Canvas)
        .expect_err("status err must fail");
    assert_eq!(err, "render failed");
    assert_eq!(
        (
            FREED_BYTES.load(Ordering::SeqCst),
            FREED_STRS.load(Ordering::SeqCst)
        ),
        (2, 2)
    );

    let short_err = host
        .render_frame(FrameSource::Clip(3), t, FrameSize::Canvas)
        .expect_err("truncated RGBA8 frame must fail");
    assert!(
        short_err.contains("invalid 2x2 frame with 12 bytes"),
        "{short_err}"
    );
    assert_eq!(FREED_BYTES.load(Ordering::SeqCst), 3);

    let zero_err = host
        .render_frame(FrameSource::Clip(4), t, FrameSize::Canvas)
        .expect_err("zero-width frame must fail");
    assert!(zero_err.contains("invalid 0x2 frame"), "{zero_err}");
}
