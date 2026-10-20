//! `plugin_table!(RUNTIME)`: an `AriaPluginV1` whose every entry forwards to a
//! static `aria_sdk::PluginRuntime`, as `aria_sdk::export!` builds it, for
//! loading an SDK plugin in process through `from_table`.

macro_rules! plugin_table {
    ($runtime:ident) => {{
        use ::aria_plugin_abi::{
            AriaAudioBlock, AriaAudioParam, AriaBytesResult, AriaCpuEffectCall, AriaDriverEvalCall,
            AriaEncoderAudio, AriaEncoderConfig, AriaEncoderFrame, AriaInstanceResult, AriaJobStep,
            AriaPluginV1, AriaResult, AriaScopeCall, AriaStr, AriaVideoFrameOut, AriaVideoInfo,
        };
        use ::core::ffi::c_void;

        unsafe extern "C" fn mount(host_ctx: *mut c_void) -> AriaResult {
            $runtime.handle_mount(host_ctx)
        }
        unsafe extern "C" fn unmount(host_ctx: *mut c_void) -> AriaResult {
            $runtime.handle_unmount(host_ctx)
        }
        unsafe extern "C" fn command_run(
            host_ctx: *mut c_void,
            id: AriaStr,
            args_json: AriaStr,
        ) -> AriaResult {
            $runtime.handle_command_run(host_ctx, id, args_json)
        }
        unsafe extern "C" fn event(
            host_ctx: *mut c_void,
            kind: AriaStr,
            event_json: AriaStr,
        ) -> AriaResult {
            $runtime.handle_event(host_ctx, kind, event_json)
        }
        unsafe extern "C" fn import_file(
            host_ctx: *mut c_void,
            importer_id: AriaStr,
            path: AriaStr,
        ) -> AriaResult {
            $runtime.handle_import(host_ctx, importer_id, path)
        }
        unsafe extern "C" fn export_file(
            host_ctx: *mut c_void,
            exporter_id: AriaStr,
            project_json: AriaStr,
            path: AriaStr,
        ) -> AriaResult {
            $runtime.handle_export(host_ctx, exporter_id, project_json, path)
        }
        unsafe extern "C" fn audio_create(
            host_ctx: *mut c_void,
            effect_id: AriaStr,
            sample_rate: u32,
        ) -> AriaInstanceResult {
            $runtime.handle_audio_create(host_ctx, effect_id, sample_rate)
        }
        unsafe extern "C" fn audio_process(
            host_ctx: *mut c_void,
            inst: u64,
            params: *const AriaAudioParam,
            params_len: usize,
            block: *mut AriaAudioBlock,
        ) -> AriaResult {
            // SAFETY: `params` and `block` are valid for the duration of `audio_process`.
            unsafe { $runtime.handle_audio_process(host_ctx, inst, params, params_len, block) }
        }
        unsafe extern "C" fn audio_reset(host_ctx: *mut c_void, inst: u64) -> AriaResult {
            $runtime.handle_audio_reset(host_ctx, inst)
        }
        unsafe extern "C" fn audio_drop(host_ctx: *mut c_void, inst: u64) -> AriaResult {
            $runtime.handle_audio_drop(host_ctx, inst)
        }
        unsafe extern "C" fn panel_view(
            host_ctx: *mut c_void,
            panel_id: AriaStr,
        ) -> AriaBytesResult {
            $runtime.handle_panel_view(host_ctx, panel_id)
        }
        unsafe extern "C" fn panel_event(
            host_ctx: *mut c_void,
            panel_id: AriaStr,
            event_json: AriaStr,
        ) -> AriaResult {
            $runtime.handle_panel_event(host_ctx, panel_id, event_json)
        }
        unsafe extern "C" fn encoder_open(
            host_ctx: *mut c_void,
            encoder_id: AriaStr,
            path: AriaStr,
            config: *const AriaEncoderConfig,
        ) -> AriaInstanceResult {
            // SAFETY: `config` is valid for the duration of `encoder_open`.
            unsafe { $runtime.handle_encoder_open(host_ctx, encoder_id, path, config) }
        }
        unsafe extern "C" fn encoder_push_frame(
            host_ctx: *mut c_void,
            inst: u64,
            frame: *const AriaEncoderFrame,
        ) -> AriaResult {
            // SAFETY: `frame` is valid for the duration of `encoder_push_frame`.
            unsafe { $runtime.handle_encoder_push_frame(host_ctx, inst, frame) }
        }
        unsafe extern "C" fn encoder_push_audio(
            host_ctx: *mut c_void,
            inst: u64,
            audio: *const AriaEncoderAudio,
        ) -> AriaResult {
            // SAFETY: `audio` is valid for the duration of `encoder_push_audio`.
            unsafe { $runtime.handle_encoder_push_audio(host_ctx, inst, audio) }
        }
        unsafe extern "C" fn encoder_finish(host_ctx: *mut c_void, inst: u64) -> AriaResult {
            $runtime.handle_encoder_finish(host_ctx, inst)
        }
        unsafe extern "C" fn encoder_drop(host_ctx: *mut c_void, inst: u64) -> AriaResult {
            $runtime.handle_encoder_drop(host_ctx, inst)
        }
        unsafe extern "C" fn overlay_scene(
            host_ctx: *mut c_void,
            overlay_id: AriaStr,
        ) -> AriaBytesResult {
            $runtime.handle_overlay_scene(host_ctx, overlay_id)
        }
        unsafe extern "C" fn overlay_event(
            host_ctx: *mut c_void,
            overlay_id: AriaStr,
            event_json: AriaStr,
        ) -> AriaResult {
            $runtime.handle_overlay_event(host_ctx, overlay_id, event_json)
        }
        unsafe extern "C" fn job_step(
            host_ctx: *mut c_void,
            job: u64,
            step: *mut AriaJobStep,
        ) -> AriaResult {
            // SAFETY: `step` is valid for the duration of `job_step`.
            unsafe { $runtime.handle_job_step(host_ctx, job, step) }
        }
        unsafe extern "C" fn job_end(
            host_ctx: *mut c_void,
            job: u64,
            cancelled: u32,
        ) -> AriaResult {
            $runtime.handle_job_end(host_ctx, job, cancelled)
        }
        unsafe extern "C" fn decoder_probe(
            host_ctx: *mut c_void,
            decoder_id: AriaStr,
            path: AriaStr,
        ) -> AriaResult {
            $runtime.handle_decoder_probe(host_ctx, decoder_id, path)
        }
        unsafe extern "C" fn decoder_open_video(
            host_ctx: *mut c_void,
            decoder_id: AriaStr,
            path: AriaStr,
            info: *mut AriaVideoInfo,
        ) -> AriaInstanceResult {
            // SAFETY: `info` is valid for the duration of `decoder_open_video`.
            unsafe { $runtime.handle_decoder_open_video(host_ctx, decoder_id, path, info) }
        }
        unsafe extern "C" fn decoder_seek(
            host_ctx: *mut c_void,
            instance: u64,
            pts: i64,
        ) -> AriaResult {
            $runtime.handle_decoder_seek(host_ctx, instance, pts)
        }
        unsafe extern "C" fn decoder_next_frame(
            host_ctx: *mut c_void,
            instance: u64,
            frame: *mut AriaVideoFrameOut,
        ) -> AriaResult {
            // SAFETY: `frame` and its planes are valid for the duration of `decoder_next_frame`.
            unsafe { $runtime.handle_decoder_next_frame(host_ctx, instance, frame) }
        }
        unsafe extern "C" fn decoder_open_audio(
            host_ctx: *mut c_void,
            decoder_id: AriaStr,
            path: AriaStr,
            sample_rate: u32,
            channels: u32,
        ) -> AriaInstanceResult {
            $runtime.handle_decoder_open_audio(host_ctx, decoder_id, path, sample_rate, channels)
        }
        unsafe extern "C" fn decoder_read_audio(
            host_ctx: *mut c_void,
            instance: u64,
            out: *mut f32,
            out_len: usize,
            written: *mut usize,
        ) -> AriaResult {
            // SAFETY: `out` and `written` are valid for the duration of `decoder_read_audio`.
            unsafe { $runtime.handle_decoder_read_audio(host_ctx, instance, out, out_len, written) }
        }
        unsafe extern "C" fn decoder_seek_audio(
            host_ctx: *mut c_void,
            instance: u64,
            frame: i64,
        ) -> AriaResult {
            $runtime.handle_decoder_seek_audio(host_ctx, instance, frame)
        }
        unsafe extern "C" fn decoder_close(host_ctx: *mut c_void, instance: u64) -> AriaResult {
            $runtime.handle_decoder_close(host_ctx, instance)
        }
        unsafe extern "C" fn ai_tool_call(
            host_ctx: *mut c_void,
            name: AriaStr,
            arguments_json: AriaStr,
        ) -> AriaResult {
            $runtime.handle_ai_tool_call(host_ctx, name, arguments_json)
        }
        unsafe extern "C" fn scope_measure(
            host_ctx: *mut c_void,
            scope_id: AriaStr,
            call: *mut AriaScopeCall,
        ) -> AriaResult {
            // SAFETY: `call` and its buffers are valid for the duration of `scope_measure`.
            unsafe { $runtime.handle_scope_measure(host_ctx, scope_id, call) }
        }
        unsafe extern "C" fn cpu_effect_process(
            host_ctx: *mut c_void,
            effect_id: AriaStr,
            call: *mut AriaCpuEffectCall,
        ) -> AriaResult {
            // SAFETY: `call` and its buffers are valid for the duration of `cpu_effect_process`.
            unsafe { $runtime.handle_cpu_effect_process(host_ctx, effect_id, call) }
        }
        unsafe extern "C" fn driver_evaluate(
            host_ctx: *mut c_void,
            language_id: AriaStr,
            expr: AriaStr,
            call: *mut AriaDriverEvalCall,
        ) -> AriaResult {
            // SAFETY: `call` is valid for the duration of `driver_evaluate`.
            unsafe { $runtime.handle_driver_evaluate(host_ctx, language_id, expr, call) }
        }
        unsafe extern "C" fn analyzer_open(
            host_ctx: *mut c_void,
            analyzer_id: AriaStr,
            sample_rate: u32,
            channels: u32,
        ) -> AriaInstanceResult {
            $runtime.handle_analyzer_open(host_ctx, analyzer_id, sample_rate, channels)
        }
        unsafe extern "C" fn analyzer_push(
            host_ctx: *mut c_void,
            instance: u64,
            samples: *const f32,
            samples_len: usize,
        ) -> AriaResult {
            // SAFETY: `samples` covers `samples_len` elements for the duration of `analyzer_push`.
            unsafe { $runtime.handle_analyzer_push(host_ctx, instance, samples, samples_len) }
        }
        unsafe extern "C" fn analyzer_finish(host_ctx: *mut c_void, instance: u64) -> AriaResult {
            $runtime.handle_analyzer_finish(host_ctx, instance)
        }
        unsafe extern "C" fn analyzer_drop(host_ctx: *mut c_void, instance: u64) -> AriaResult {
            $runtime.handle_analyzer_drop(host_ctx, instance)
        }
        AriaPluginV1 {
            struct_size: ::std::mem::size_of::<AriaPluginV1>(),
            api_major: ::aria_plugin_abi::ARIA_API_MAJOR,
            reserved: 0,
            free_string: ::aria_plugin_abi::free_rust_string,
            free_bytes: ::aria_plugin_abi::free_rust_bytes,
            mount,
            unmount,
            command_run,
            event,
            import_file,
            export_file,
            audio_create,
            audio_process,
            audio_reset,
            audio_drop,
            panel_view,
            panel_event,
            encoder_open,
            encoder_push_frame,
            encoder_push_audio,
            encoder_finish,
            encoder_drop,
            overlay_scene,
            overlay_event,
            job_step,
            job_end,
            decoder_probe,
            decoder_open_video,
            decoder_seek,
            decoder_next_frame,
            decoder_open_audio,
            decoder_read_audio,
            decoder_seek_audio,
            decoder_close,
            ai_tool_call,
            scope_measure,
            cpu_effect_process,
            driver_evaluate,
            analyzer_open,
            analyzer_push,
            analyzer_finish,
            analyzer_drop,
        }
    }};
}
