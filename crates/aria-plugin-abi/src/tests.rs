use std::mem::{offset_of, size_of};

use super::*;

#[test]
fn both_tables_start_with_the_table_header() {
    let header = [
        offset_of!(AriaTableHeader, struct_size),
        offset_of!(AriaTableHeader, api_major),
        offset_of!(AriaTableHeader, reserved),
    ];
    let host = [
        offset_of!(AriaHostV1, struct_size),
        offset_of!(AriaHostV1, api_major),
        offset_of!(AriaHostV1, reserved),
    ];
    let plugin = [
        offset_of!(AriaPluginV1, struct_size),
        offset_of!(AriaPluginV1, api_major),
        offset_of!(AriaPluginV1, reserved),
    ];
    assert_eq!(host, header);
    assert_eq!(plugin, header);
    assert_eq!(
        offset_of!(AriaHostV1, free_string),
        size_of::<AriaTableHeader>()
    );
    assert_eq!(
        offset_of!(AriaPluginV1, free_string),
        size_of::<AriaTableHeader>()
    );
}

#[test]
fn owned_string_and_bytes_round_trip_and_free() {
    let s = AriaOwnedStr::from_string("hello native abi".to_owned());
    assert!(!s.is_null());
    #[allow(unsafe_code)]
    // SAFETY: `s` was created by `AriaOwnedStr::from_string` in this crate.
    let recovered = unsafe { s.into_string_with(free_rust_string) };
    assert_eq!(recovered, "hello native abi");

    let empty = AriaOwnedStr::empty();
    assert!(empty.is_null());
    #[allow(unsafe_code)]
    // SAFETY: `empty` is `AriaOwnedStr::empty()`.
    let empty_str = unsafe { empty.into_string_with(free_rust_string) };
    assert_eq!(empty_str, "");

    let b = AriaOwnedBytes::from_vec(vec![1, 2, 3, 4]);
    #[allow(unsafe_code)]
    // SAFETY: `b` was created by `AriaOwnedBytes::from_vec` in this crate.
    let recovered_bytes = unsafe { b.into_vec_with(free_rust_bytes) };
    assert_eq!(recovered_bytes, vec![1, 2, 3, 4]);
}

#[test]
fn table_validation_refuses_wrong_api_major_and_short_tables() {
    assert_eq!(
        validate_plugin_table(ARIA_API_MAJOR, size_of::<AriaPluginV1>()),
        Ok(())
    );
    // A v1 table without the trailing panel callbacks is still accepted.
    assert_eq!(
        validate_plugin_table(ARIA_API_MAJOR, ARIA_PLUGIN_V1_MIN_SIZE),
        Ok(())
    );
    for wrong in [0, 2, 99] {
        let err = validate_plugin_table(wrong, size_of::<AriaPluginV1>())
            .expect_err("wrong API major must be refused");
        assert!(err.contains(&wrong.to_string()), "{err}");
        assert!(err.contains(&ARIA_API_MAJOR.to_string()), "{err}");
    }
    let short = ARIA_PLUGIN_V1_MIN_SIZE - 8;
    let err = validate_plugin_table(ARIA_API_MAJOR, short)
        .expect_err("short plugin table must be refused");
    assert!(err.contains(&short.to_string()), "{err}");
    assert!(err.contains(&ARIA_PLUGIN_V1_MIN_SIZE.to_string()), "{err}");
}

#[test]
fn prefix_size_predicates_and_copy_from_prefix_fill_trailing_stubs() {
    assert!(ARIA_HOST_V1_MIN_SIZE < ARIA_HOST_V1_PANELS_SIZE);
    assert!(ARIA_HOST_V1_PANELS_SIZE < ARIA_HOST_V1_ENCODERS_SIZE);
    assert!(ARIA_HOST_V1_ENCODERS_SIZE < ARIA_HOST_V1_OVERLAYS_SIZE);
    assert!(ARIA_HOST_V1_OVERLAYS_SIZE < ARIA_HOST_V1_JOBS_SIZE);
    assert!(ARIA_HOST_V1_JOBS_SIZE < ARIA_HOST_V1_DECODERS_SIZE);
    assert!(ARIA_HOST_V1_DECODERS_SIZE < ARIA_HOST_V1_AI_TOOLS_SIZE);
    assert!(ARIA_HOST_V1_AI_TOOLS_SIZE < ARIA_HOST_V1_SCOPES_SIZE);
    assert!(ARIA_HOST_V1_SCOPES_SIZE < ARIA_HOST_V1_CONTRIBUTIONS_SIZE);
    assert!(ARIA_HOST_V1_CONTRIBUTIONS_SIZE < ARIA_HOST_V1_QUERIES_SIZE);
    assert!(ARIA_HOST_V1_QUERIES_SIZE < size_of::<AriaHostV1>());

    assert!(ARIA_PLUGIN_V1_MIN_SIZE < ARIA_PLUGIN_V1_PANELS_SIZE);
    assert!(ARIA_PLUGIN_V1_PANELS_SIZE < ARIA_PLUGIN_V1_ENCODERS_SIZE);
    assert!(ARIA_PLUGIN_V1_ENCODERS_SIZE < ARIA_PLUGIN_V1_OVERLAYS_SIZE);
    assert!(ARIA_PLUGIN_V1_OVERLAYS_SIZE < ARIA_PLUGIN_V1_JOBS_SIZE);
    assert!(ARIA_PLUGIN_V1_JOBS_SIZE < ARIA_PLUGIN_V1_DECODERS_SIZE);
    assert!(ARIA_PLUGIN_V1_DECODERS_SIZE < ARIA_PLUGIN_V1_AI_TOOLS_SIZE);
    assert!(ARIA_PLUGIN_V1_AI_TOOLS_SIZE < ARIA_PLUGIN_V1_SCOPES_SIZE);
    assert!(ARIA_PLUGIN_V1_SCOPES_SIZE < size_of::<AriaPluginV1>());

    #[allow(unsafe_code)]
    unsafe extern "C" fn noop_ok(_ctx: *mut c_void) -> AriaResult {
        AriaResult::ok_empty()
    }
    #[allow(unsafe_code)]
    unsafe extern "C" fn noop_cmd(_ctx: *mut c_void, _id: AriaStr, _args: AriaStr) -> AriaResult {
        AriaResult::ok_empty()
    }
    #[allow(unsafe_code)]
    unsafe extern "C" fn noop_export(
        _ctx: *mut c_void,
        _id: AriaStr,
        _proj: AriaStr,
        _path: AriaStr,
    ) -> AriaResult {
        AriaResult::ok_empty()
    }
    #[allow(unsafe_code)]
    unsafe extern "C" fn noop_audio_create(
        _ctx: *mut c_void,
        _id: AriaStr,
        _sr: u32,
    ) -> AriaInstanceResult {
        AriaInstanceResult::ok(1)
    }
    #[allow(unsafe_code)]
    unsafe extern "C" fn noop_audio_proc(
        _ctx: *mut c_void,
        _inst: u64,
        _params: *const AriaAudioParam,
        _len: usize,
        _block: *mut AriaAudioBlock,
    ) -> AriaResult {
        AriaResult::ok_empty()
    }
    #[allow(unsafe_code)]
    unsafe extern "C" fn noop_u64(_ctx: *mut c_void, _inst: u64) -> AriaResult {
        AriaResult::ok_empty()
    }
    #[allow(unsafe_code)]
    unsafe extern "C" fn custom_panel_view(_ctx: *mut c_void, _id: AriaStr) -> AriaBytesResult {
        AriaBytesResult::found(b"panel-ok".to_vec())
    }
    #[allow(unsafe_code)]
    unsafe extern "C" fn custom_encoder_open(
        _ctx: *mut c_void,
        _id: AriaStr,
        _path: AriaStr,
        _cfg: *const AriaEncoderConfig,
    ) -> AriaInstanceResult {
        AriaInstanceResult::ok(77)
    }
    #[allow(unsafe_code)]
    unsafe extern "C" fn custom_encoder_frame(
        _ctx: *mut c_void,
        _inst: u64,
        _frame: *const AriaEncoderFrame,
    ) -> AriaResult {
        AriaResult::ok_empty()
    }
    #[allow(unsafe_code)]
    unsafe extern "C" fn custom_encoder_audio(
        _ctx: *mut c_void,
        _inst: u64,
        _audio: *const AriaEncoderAudio,
    ) -> AriaResult {
        AriaResult::ok_empty()
    }
    #[allow(unsafe_code)]
    unsafe extern "C" fn custom_overlay_scene(_ctx: *mut c_void, _id: AriaStr) -> AriaBytesResult {
        AriaBytesResult::found(b"overlay-ok".to_vec())
    }
    #[allow(unsafe_code)]
    unsafe extern "C" fn custom_job_step(
        _ctx: *mut c_void,
        job: u64,
        step: *mut AriaJobStep,
    ) -> AriaResult {
        // SAFETY: the caller passes a valid, writable `AriaJobStep`.
        unsafe {
            (*step).progress = 0.5;
            (*step).done = 1;
        }
        AriaResult::ok(format!("job {job}"))
    }
    #[allow(unsafe_code)]
    unsafe extern "C" fn noop_job_end(_ctx: *mut c_void, _job: u64, _cancelled: u32) -> AriaResult {
        AriaResult::ok_empty()
    }
    #[allow(unsafe_code)]
    unsafe extern "C" fn custom_decoder_probe(
        _ctx: *mut c_void,
        id: AriaStr,
        _path: AriaStr,
    ) -> AriaResult {
        // SAFETY: the caller passes a valid UTF-8 `AriaStr`.
        AriaResult::unsupported(format!("not {}", unsafe { id.as_str() }.unwrap_or("?")))
    }
    #[allow(unsafe_code)]
    unsafe extern "C" fn noop_decoder_open_video(
        _ctx: *mut c_void,
        _id: AriaStr,
        _path: AriaStr,
        _info: *mut AriaVideoInfo,
    ) -> AriaInstanceResult {
        AriaInstanceResult::ok(5)
    }
    #[allow(unsafe_code)]
    unsafe extern "C" fn noop_decoder_seek(_ctx: *mut c_void, _inst: u64, _at: i64) -> AriaResult {
        AriaResult::ok_empty()
    }
    #[allow(unsafe_code)]
    unsafe extern "C" fn noop_decoder_next_frame(
        _ctx: *mut c_void,
        _inst: u64,
        _frame: *mut AriaVideoFrameOut,
    ) -> AriaResult {
        AriaResult::ok_empty()
    }
    #[allow(unsafe_code)]
    unsafe extern "C" fn noop_decoder_open_audio(
        _ctx: *mut c_void,
        _id: AriaStr,
        _path: AriaStr,
        _rate: u32,
        _channels: u32,
    ) -> AriaInstanceResult {
        AriaInstanceResult::ok(6)
    }
    #[allow(unsafe_code)]
    unsafe extern "C" fn noop_decoder_read_audio(
        _ctx: *mut c_void,
        _inst: u64,
        _out: *mut f32,
        _len: usize,
        _written: *mut usize,
    ) -> AriaResult {
        AriaResult::ok_empty()
    }

    #[allow(unsafe_code)]
    unsafe extern "C" fn custom_ai_tool_call(
        _ctx: *mut c_void,
        _name: AriaStr,
        _arguments: AriaStr,
    ) -> AriaResult {
        AriaResult::ok("tool-ok".to_owned())
    }
    #[allow(unsafe_code)]
    unsafe extern "C" fn custom_scope_measure(
        _ctx: *mut c_void,
        _scope_id: AriaStr,
        call: *mut AriaScopeCall,
    ) -> AriaResult {
        // SAFETY: the test passes a call whose buffers hold at least one pixel each.
        unsafe {
            let call = &*call;
            std::ptr::copy_nonoverlapping(call.frame, call.image, 4);
        }
        AriaResult::ok("scope-ok".to_owned())
    }
    #[allow(unsafe_code)]
    unsafe extern "C" fn custom_cpu_effect_process(
        _ctx: *mut c_void,
        _effect_id: AriaStr,
        call: *mut AriaCpuEffectCall,
    ) -> AriaResult {
        // SAFETY: the test passes a call whose buffers hold at least one pixel each.
        unsafe {
            let call = &*call;
            std::ptr::copy_nonoverlapping(call.input, call.output, 4);
        }
        AriaResult::ok_empty()
    }
    #[allow(unsafe_code)]
    unsafe extern "C" fn custom_driver_evaluate(
        _ctx: *mut c_void,
        _language_id: AriaStr,
        _expr: AriaStr,
        call: *mut AriaDriverEvalCall,
    ) -> AriaResult {
        // SAFETY: the test passes a valid writable `AriaDriverEvalCall`.
        unsafe {
            let call = &mut *call;
            call.output = call.value;
            call.output.x *= 2.0;
        }
        AriaResult::ok_empty()
    }
    let scope_measure = |table: &AriaPluginV1| {
        let frame = [10_u8, 20, 30, 255];
        let mut image = [0_u8; 4];
        let mut call = AriaScopeCall {
            frame: frame.as_ptr(),
            frame_len: frame.len(),
            frame_width: 1,
            frame_height: 1,
            image: image.as_mut_ptr(),
            image_len: image.len(),
            image_width: 1,
            image_height: 1,
        };
        #[allow(unsafe_code)]
        // SAFETY: `scope_measure` is the table's own or a stub; `call` outlives it.
        let result = unsafe {
            (table.scope_measure)(
                std::ptr::null_mut(),
                AriaStr::from_str("meter"),
                &raw mut call,
            )
        };
        #[allow(unsafe_code)]
        // SAFETY: the value was allocated by this crate's `AriaOwnedStr::from_string`.
        let text = unsafe { result.value.into_string_with(free_rust_string) };
        (result.status, text, image)
    };
    let tool_call = |table: &AriaPluginV1| {
        #[allow(unsafe_code)]
        // SAFETY: `ai_tool_call` is the table's own or a stub; both take any strings.
        let result = unsafe {
            (table.ai_tool_call)(
                std::ptr::null_mut(),
                AriaStr::from_str("echo"),
                AriaStr::from_str("{}"),
            )
        };
        #[allow(unsafe_code)]
        // SAFETY: the value was allocated by this crate's `AriaOwnedStr::from_string`.
        let text = unsafe { result.value.into_string_with(free_rust_string) };
        (result.status, text)
    };

    let full = AriaPluginV1 {
        struct_size: size_of::<AriaPluginV1>(),
        api_major: ARIA_API_MAJOR,
        reserved: 0,
        free_string: free_rust_string,
        free_bytes: free_rust_bytes,
        mount: noop_ok,
        unmount: noop_ok,
        command_run: noop_cmd,
        event: noop_cmd,
        import_file: noop_cmd,
        export_file: noop_export,
        audio_create: noop_audio_create,
        audio_process: noop_audio_proc,
        audio_reset: noop_u64,
        audio_drop: noop_u64,
        panel_view: custom_panel_view,
        panel_event: noop_cmd,
        encoder_open: custom_encoder_open,
        encoder_push_frame: custom_encoder_frame,
        encoder_push_audio: custom_encoder_audio,
        encoder_finish: noop_u64,
        encoder_drop: noop_u64,
        overlay_scene: custom_overlay_scene,
        overlay_event: noop_cmd,
        job_step: custom_job_step,
        job_end: noop_job_end,
        decoder_probe: custom_decoder_probe,
        decoder_open_video: noop_decoder_open_video,
        decoder_seek: noop_decoder_seek,
        decoder_next_frame: noop_decoder_next_frame,
        decoder_open_audio: noop_decoder_open_audio,
        decoder_read_audio: noop_decoder_read_audio,
        decoder_seek_audio: noop_decoder_seek,
        decoder_close: noop_u64,
        ai_tool_call: custom_ai_tool_call,
        scope_measure: custom_scope_measure,
        cpu_effect_process: custom_cpu_effect_process,
        driver_evaluate: custom_driver_evaluate,
        analyzer_open: stub_analyzer_open,
        analyzer_push: stub_analyzer_push,
        analyzer_finish: stub_analyzer_finish,
        analyzer_drop: stub_analyzer_drop,
    };
    let probe = |table: &AriaPluginV1| {
        #[allow(unsafe_code)]
        // SAFETY: `decoder_probe` is the table's own or a stub; both take any strings.
        let result = unsafe {
            (table.decoder_probe)(
                std::ptr::null_mut(),
                AriaStr::from_str("tiff"),
                AriaStr::from_str("/a.tif"),
            )
        };
        #[allow(unsafe_code)]
        // SAFETY: the value was allocated by this crate's `AriaOwnedStr::from_string`.
        let text = unsafe { result.value.into_string_with(free_rust_string) };
        (result.status, text)
    };

    // 1. MIN_SIZE prefix: no panels, no encoders, no overlays; all three stubbed.
    let mut min_table = full;
    min_table.struct_size = ARIA_PLUGIN_V1_MIN_SIZE;
    assert!(!min_table.has_panels());
    assert!(!min_table.has_encoders());
    assert!(!min_table.has_overlays());
    #[allow(unsafe_code)]
    let copied_min =
        unsafe { AriaPluginV1::copy_from_prefix(&raw const min_table, min_table.struct_size) };
    #[allow(unsafe_code)]
    let min_panel =
        unsafe { (copied_min.panel_view)(std::ptr::null_mut(), AriaStr::from_str("p")) };
    assert_eq!(min_panel.status, ARIA_BYTES_ERR);
    #[allow(unsafe_code)]
    let min_panel_err = unsafe { min_panel.error.into_string_with(free_rust_string) };
    assert!(min_panel_err.contains("panel_view"), "{min_panel_err}");

    // 2. PANELS_SIZE prefix: panels preserved, encoders and overlays stubbed.
    let mut panels_table = full;
    panels_table.struct_size = ARIA_PLUGIN_V1_PANELS_SIZE;
    assert!(panels_table.has_panels());
    assert!(!panels_table.has_encoders());
    assert!(!panels_table.has_overlays());
    #[allow(unsafe_code)]
    let copied_panels = unsafe {
        AriaPluginV1::copy_from_prefix(&raw const panels_table, panels_table.struct_size)
    };
    #[allow(unsafe_code)]
    let ok_panel =
        unsafe { (copied_panels.panel_view)(std::ptr::null_mut(), AriaStr::from_str("p")) };
    assert_eq!(ok_panel.status, ARIA_BYTES_FOUND);
    #[allow(unsafe_code)]
    let _ = unsafe { ok_panel.bytes.into_vec_with(free_rust_bytes) };
    #[allow(unsafe_code)]
    let stub_enc = unsafe {
        (copied_panels.encoder_open)(
            std::ptr::null_mut(),
            AriaStr::from_str("e"),
            AriaStr::from_str("p"),
            std::ptr::null(),
        )
    };
    assert_eq!(stub_enc.status, ARIA_STATUS_ERR);
    #[allow(unsafe_code)]
    let stub_enc_err = unsafe { stub_enc.error.into_string_with(free_rust_string) };
    assert!(stub_enc_err.contains("encoder_open"), "{stub_enc_err}");

    // 3. ENCODERS_SIZE prefix: panels and encoders preserved, overlays stubbed.
    let mut encoders_table = full;
    encoders_table.struct_size = ARIA_PLUGIN_V1_ENCODERS_SIZE;
    assert!(encoders_table.has_panels());
    assert!(encoders_table.has_encoders());
    assert!(!encoders_table.has_overlays());
    #[allow(unsafe_code)]
    let copied_encoders = unsafe {
        AriaPluginV1::copy_from_prefix(&raw const encoders_table, encoders_table.struct_size)
    };
    #[allow(unsafe_code)]
    let ok_enc = unsafe {
        (copied_encoders.encoder_open)(
            std::ptr::null_mut(),
            AriaStr::from_str("e"),
            AriaStr::from_str("p"),
            std::ptr::null(),
        )
    };
    assert_eq!(ok_enc.status, ARIA_STATUS_OK);
    assert_eq!(ok_enc.instance, 77);
    #[allow(unsafe_code)]
    let stub_ovl_scene =
        unsafe { (copied_encoders.overlay_scene)(std::ptr::null_mut(), AriaStr::from_str("ovl")) };
    assert_eq!(stub_ovl_scene.status, ARIA_BYTES_ERR);
    #[allow(unsafe_code)]
    let stub_ovl_scene_err = unsafe { stub_ovl_scene.error.into_string_with(free_rust_string) };
    assert!(
        stub_ovl_scene_err.contains("overlay_scene"),
        "{stub_ovl_scene_err}"
    );
    #[allow(unsafe_code)]
    let stub_ovl_ev = unsafe {
        (copied_encoders.overlay_event)(
            std::ptr::null_mut(),
            AriaStr::from_str("ovl"),
            AriaStr::from_str("{}"),
        )
    };
    assert_eq!(stub_ovl_ev.status, ARIA_STATUS_ERR);
    #[allow(unsafe_code)]
    let stub_ovl_ev_err = unsafe { stub_ovl_ev.value.into_string_with(free_rust_string) };
    assert!(
        stub_ovl_ev_err.contains("overlay_event"),
        "{stub_ovl_ev_err}"
    );

    // 4. OVERLAYS_SIZE prefix: overlays preserved, jobs stubbed.
    let mut overlays_table = full;
    overlays_table.struct_size = ARIA_PLUGIN_V1_OVERLAYS_SIZE;
    assert!(overlays_table.has_overlays());
    assert!(!overlays_table.has_jobs());
    #[allow(unsafe_code)]
    let copied_overlays = unsafe {
        AriaPluginV1::copy_from_prefix(&raw const overlays_table, overlays_table.struct_size)
    };
    #[allow(unsafe_code)]
    let kept_ovl =
        unsafe { (copied_overlays.overlay_scene)(std::ptr::null_mut(), AriaStr::from_str("ovl")) };
    assert_eq!(kept_ovl.status, ARIA_BYTES_FOUND);
    #[allow(unsafe_code)]
    let kept_bytes = unsafe { kept_ovl.bytes.into_vec_with(free_rust_bytes) };
    assert_eq!(kept_bytes, b"overlay-ok");
    let mut step = AriaJobStep {
        progress: -1.0,
        done: 0,
        commit: 0,
    };
    #[allow(unsafe_code)]
    let stub_step = unsafe { (copied_overlays.job_step)(std::ptr::null_mut(), 3, &raw mut step) };
    assert_eq!(stub_step.status, ARIA_STATUS_ERR);
    assert_eq!(step.done, 0, "a stubbed job_step leaves the step untouched");
    #[allow(unsafe_code)]
    let stub_step_err = unsafe { stub_step.value.into_string_with(free_rust_string) };
    assert!(stub_step_err.contains("job_step"), "{stub_step_err}");
    #[allow(unsafe_code)]
    let stub_end = unsafe { (copied_overlays.job_end)(std::ptr::null_mut(), 3, 1) };
    assert_eq!(stub_end.status, ARIA_STATUS_ERR);
    #[allow(unsafe_code)]
    let stub_end_err = unsafe { stub_end.value.into_string_with(free_rust_string) };
    assert!(stub_end_err.contains("job_end"), "{stub_end_err}");
    let (status, text) = probe(&copied_overlays);
    assert_eq!(status, ARIA_STATUS_ERR);
    assert!(text.contains("decoder_probe"), "{text}");

    // 5. JOBS_SIZE prefix: jobs preserved, decoders stubbed.
    let mut jobs_table = full;
    jobs_table.struct_size = ARIA_PLUGIN_V1_JOBS_SIZE;
    assert!(jobs_table.has_jobs());
    assert!(!jobs_table.has_decoders());
    #[allow(unsafe_code)]
    let copied_jobs =
        unsafe { AriaPluginV1::copy_from_prefix(&raw const jobs_table, jobs_table.struct_size) };
    let mut step = AriaJobStep {
        progress: -1.0,
        done: 0,
        commit: 0,
    };
    #[allow(unsafe_code)]
    let kept_step = unsafe { (copied_jobs.job_step)(std::ptr::null_mut(), 4, &raw mut step) };
    assert_eq!(kept_step.status, ARIA_STATUS_OK);
    #[allow(unsafe_code)]
    let kept_detail = unsafe { kept_step.value.into_string_with(free_rust_string) };
    assert_eq!(kept_detail, "job 4");
    let (status, text) = probe(&copied_jobs);
    assert_eq!(status, ARIA_STATUS_ERR);
    assert!(text.contains("decoder_probe"), "{text}");
    #[allow(unsafe_code)]
    let stub_close = unsafe { (copied_jobs.decoder_close)(std::ptr::null_mut(), 5) };
    assert_eq!(stub_close.status, ARIA_STATUS_ERR);
    #[allow(unsafe_code)]
    let stub_close_err = unsafe { stub_close.value.into_string_with(free_rust_string) };
    assert!(stub_close_err.contains("decoder_close"), "{stub_close_err}");
    let (status, text) = tool_call(&copied_jobs);
    assert_eq!(status, ARIA_STATUS_ERR);
    assert!(text.contains("ai_tool_call"), "{text}");

    // 6. DECODERS_SIZE prefix: decoders preserved, the AI tool and scope
    // calls stubbed.
    let mut decoders_table = full;
    decoders_table.struct_size = ARIA_PLUGIN_V1_DECODERS_SIZE;
    assert!(decoders_table.has_decoders());
    assert!(!decoders_table.has_ai_tools());
    #[allow(unsafe_code)]
    let copied_decoders = unsafe {
        AriaPluginV1::copy_from_prefix(&raw const decoders_table, decoders_table.struct_size)
    };
    assert_eq!(
        probe(&copied_decoders),
        (ARIA_STATUS_UNSUPPORTED, "not tiff".to_owned())
    );
    let (status, text) = tool_call(&copied_decoders);
    assert_eq!(status, ARIA_STATUS_ERR);
    assert!(text.contains("ai_tool_call"), "{text}");
    let (status, text, image) = scope_measure(&copied_decoders);
    assert_eq!(status, ARIA_STATUS_ERR);
    assert!(text.contains("scope_measure"), "{text}");
    assert_eq!(image, [0; 4], "a stub draws nothing");

    // 7. AI_TOOLS_SIZE prefix: the AI tool call preserved, the scope call
    // stubbed.
    let mut tools_table = full;
    tools_table.struct_size = ARIA_PLUGIN_V1_AI_TOOLS_SIZE;
    assert!(tools_table.has_ai_tools());
    assert!(!tools_table.has_scopes());
    #[allow(unsafe_code)]
    let copied_tools =
        unsafe { AriaPluginV1::copy_from_prefix(&raw const tools_table, tools_table.struct_size) };
    assert_eq!(
        tool_call(&copied_tools),
        (ARIA_STATUS_OK, "tool-ok".to_owned())
    );
    let (status, text, _) = scope_measure(&copied_tools);
    assert_eq!(status, ARIA_STATUS_ERR);
    assert!(text.contains("scope_measure"), "{text}");

    // 7b. SCOPES_SIZE prefix: scope_measure preserved, cpu_effect_process and driver_evaluate stubbed.
    let mut scopes_table = full;
    scopes_table.struct_size = ARIA_PLUGIN_V1_SCOPES_SIZE;
    assert!(scopes_table.has_scopes());
    assert!(!scopes_table.has_contributions());
    #[allow(unsafe_code)]
    let copied_scopes = unsafe {
        AriaPluginV1::copy_from_prefix(&raw const scopes_table, scopes_table.struct_size)
    };
    assert_eq!(
        scope_measure(&copied_scopes),
        (ARIA_STATUS_OK, "scope-ok".to_owned(), [10, 20, 30, 255])
    );
    #[allow(unsafe_code)]
    let stub_cpu = unsafe {
        (copied_scopes.cpu_effect_process)(
            std::ptr::null_mut(),
            AriaStr::from_str("fx"),
            std::ptr::null_mut(),
        )
    };
    assert_eq!(stub_cpu.status, ARIA_STATUS_ERR);
    #[allow(unsafe_code)]
    let stub_cpu_err = unsafe { stub_cpu.value.into_string_with(free_rust_string) };
    assert!(
        stub_cpu_err.contains("cpu_effect_process"),
        "{stub_cpu_err}"
    );
    #[allow(unsafe_code)]
    let stub_drv = unsafe {
        (copied_scopes.driver_evaluate)(
            std::ptr::null_mut(),
            AriaStr::from_str("lang"),
            AriaStr::from_str("x"),
            std::ptr::null_mut(),
        )
    };
    assert_eq!(stub_drv.status, ARIA_STATUS_ERR);
    #[allow(unsafe_code)]
    let stub_drv_err = unsafe { stub_drv.value.into_string_with(free_rust_string) };
    assert!(stub_drv_err.contains("driver_evaluate"), "{stub_drv_err}");
    // 8. Full table: every tier present and every callback is the plugin's own.
    assert!(full.has_panels());
    assert!(full.has_encoders());
    assert!(full.has_overlays());
    assert!(full.has_jobs());
    assert!(full.has_decoders());
    assert!(full.has_ai_tools());
    assert!(full.has_scopes());
    assert!(full.has_contributions());
    #[allow(unsafe_code)]
    let copied_full = unsafe { AriaPluginV1::copy_from_prefix(&raw const full, full.struct_size) };
    #[allow(unsafe_code)]
    let ok_ovl =
        unsafe { (copied_full.overlay_scene)(std::ptr::null_mut(), AriaStr::from_str("ovl")) };
    assert_eq!(ok_ovl.status, ARIA_BYTES_FOUND);
    #[allow(unsafe_code)]
    let ovl_bytes = unsafe { ok_ovl.bytes.into_vec_with(free_rust_bytes) };
    assert_eq!(ovl_bytes, b"overlay-ok");
    let mut step = AriaJobStep {
        progress: -1.0,
        done: 0,
        commit: 0,
    };
    #[allow(unsafe_code)]
    let ok_step = unsafe { (copied_full.job_step)(std::ptr::null_mut(), 9, &raw mut step) };
    assert_eq!(ok_step.status, ARIA_STATUS_OK);
    assert_eq!((step.progress, step.done, step.commit), (0.5, 1, 0));
    #[allow(unsafe_code)]
    let detail = unsafe { ok_step.value.into_string_with(free_rust_string) };
    assert_eq!(detail, "job 9");
    assert_eq!(
        probe(&copied_full),
        (ARIA_STATUS_UNSUPPORTED, "not tiff".to_owned())
    );
    assert_eq!(
        tool_call(&copied_full),
        (ARIA_STATUS_OK, "tool-ok".to_owned())
    );
    assert_eq!(
        scope_measure(&copied_full),
        (ARIA_STATUS_OK, "scope-ok".to_owned(), [10, 20, 30, 255])
    );
    let cpu_in = [0x4400u16, 0xc000, 0x3800, 0x3c00];
    let mut cpu_out = [0u16; 4];
    let mut cpu_call = AriaCpuEffectCall {
        input: cpu_in.as_ptr(),
        output: cpu_out.as_mut_ptr(),
        pixels_len: 4,
        width: 1,
        height: 1,
        params: std::ptr::null(),
        params_len: 0,
        params_json: AriaStr::from_str(""),
        time: 0.0,
    };
    #[allow(unsafe_code)]
    let ok_cpu = unsafe {
        (copied_full.cpu_effect_process)(
            std::ptr::null_mut(),
            AriaStr::from_str("fx"),
            &raw mut cpu_call,
        )
    };
    assert_eq!(ok_cpu.status, ARIA_STATUS_OK);
    assert_eq!(cpu_out, cpu_in);
}
