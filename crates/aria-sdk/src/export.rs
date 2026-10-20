//! The [`export!`](crate::export) macro that builds a native plugin's entry
//! table.

/// Export a [`Plugin`](crate::Plugin) implementation from a native `cdylib` crate.
///
/// ```ignore
/// aria_sdk::export!(NotesPlugin);
/// ```
#[macro_export]
macro_rules! export {
    ($plugin_ty:ty) => {
        $crate::export!(
            $plugin_ty,
            <$plugin_ty as ::core::default::Default>::default()
        );
    };
    ($plugin_ty:ty, $init:expr) => {
        const _: () = {
            use ::core::ffi::c_void;
            use ::std::sync::LazyLock;
            use $crate::abi::{
                ARIA_API_MAJOR, AriaAudioBlock, AriaAudioParam, AriaBytesResult, AriaCpuEffectCall,
                AriaDriverEvalCall, AriaEncoderAudio, AriaEncoderConfig, AriaEncoderFrame,
                AriaHostV1, AriaInstanceResult, AriaJobStep, AriaPluginV1, AriaResult,
                AriaScopeCall, AriaStr, AriaVideoFrameOut, AriaVideoInfo, free_rust_bytes,
                free_rust_string,
            };

            static RUNTIME: LazyLock<$crate::PluginRuntime<$plugin_ty>> =
                LazyLock::new(|| $crate::PluginRuntime::new($init));
            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_mount(host_ctx: *mut c_void) -> AriaResult {
                RUNTIME.handle_mount(host_ctx)
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_unmount(host_ctx: *mut c_void) -> AriaResult {
                RUNTIME.handle_unmount(host_ctx)
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_command_run(
                host_ctx: *mut c_void,
                id: AriaStr,
                args_json: AriaStr,
            ) -> AriaResult {
                RUNTIME.handle_command_run(host_ctx, id, args_json)
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_event(
                host_ctx: *mut c_void,
                kind: AriaStr,
                event_json: AriaStr,
            ) -> AriaResult {
                RUNTIME.handle_event(host_ctx, kind, event_json)
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_import(
                host_ctx: *mut c_void,
                importer_id: AriaStr,
                path: AriaStr,
            ) -> AriaResult {
                RUNTIME.handle_import(host_ctx, importer_id, path)
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_export(
                host_ctx: *mut c_void,
                exporter_id: AriaStr,
                project_json: AriaStr,
                path: AriaStr,
            ) -> AriaResult {
                RUNTIME.handle_export(host_ctx, exporter_id, project_json, path)
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_audio_create(
                host_ctx: *mut c_void,
                effect_id: AriaStr,
                sample_rate: u32,
            ) -> AriaInstanceResult {
                RUNTIME.handle_audio_create(host_ctx, effect_id, sample_rate)
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_audio_process(
                host_ctx: *mut c_void,
                instance: u64,
                params: *const AriaAudioParam,
                params_len: usize,
                block: *mut AriaAudioBlock,
            ) -> AriaResult {
                // SAFETY: `params` and `block` are valid for the duration of the FFI call.
                unsafe {
                    RUNTIME.handle_audio_process(host_ctx, instance, params, params_len, block)
                }
            }
            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_audio_reset(
                host_ctx: *mut c_void,
                instance: u64,
            ) -> AriaResult {
                RUNTIME.handle_audio_reset(host_ctx, instance)
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_audio_drop(
                host_ctx: *mut c_void,
                instance: u64,
            ) -> AriaResult {
                RUNTIME.handle_audio_drop(host_ctx, instance)
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_panel_view(
                host_ctx: *mut c_void,
                panel_id: AriaStr,
            ) -> AriaBytesResult {
                RUNTIME.handle_panel_view(host_ctx, panel_id)
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_panel_event(
                host_ctx: *mut c_void,
                panel_id: AriaStr,
                event_json: AriaStr,
            ) -> AriaResult {
                RUNTIME.handle_panel_event(host_ctx, panel_id, event_json)
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_encoder_open(
                host_ctx: *mut c_void,
                encoder_id: AriaStr,
                path: AriaStr,
                config: *const AriaEncoderConfig,
            ) -> AriaInstanceResult {
                // SAFETY: `config` is valid for the duration of the FFI call.
                unsafe { RUNTIME.handle_encoder_open(host_ctx, encoder_id, path, config) }
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_encoder_push_frame(
                host_ctx: *mut c_void,
                instance: u64,
                frame: *const AriaEncoderFrame,
            ) -> AriaResult {
                // SAFETY: `frame` is valid for the duration of the FFI call.
                unsafe { RUNTIME.handle_encoder_push_frame(host_ctx, instance, frame) }
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_encoder_push_audio(
                host_ctx: *mut c_void,
                instance: u64,
                audio: *const AriaEncoderAudio,
            ) -> AriaResult {
                // SAFETY: `audio` is valid for the duration of the FFI call.
                unsafe { RUNTIME.handle_encoder_push_audio(host_ctx, instance, audio) }
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_encoder_finish(
                host_ctx: *mut c_void,
                instance: u64,
            ) -> AriaResult {
                RUNTIME.handle_encoder_finish(host_ctx, instance)
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_encoder_drop(
                host_ctx: *mut c_void,
                instance: u64,
            ) -> AriaResult {
                RUNTIME.handle_encoder_drop(host_ctx, instance)
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_overlay_scene(
                host_ctx: *mut c_void,
                overlay_id: AriaStr,
            ) -> AriaBytesResult {
                RUNTIME.handle_overlay_scene(host_ctx, overlay_id)
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_overlay_event(
                host_ctx: *mut c_void,
                overlay_id: AriaStr,
                event_json: AriaStr,
            ) -> AriaResult {
                RUNTIME.handle_overlay_event(host_ctx, overlay_id, event_json)
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_job_step(
                host_ctx: *mut c_void,
                job: u64,
                step: *mut AriaJobStep,
            ) -> AriaResult {
                // SAFETY: the host passes a writable `step` for the duration of the call.
                unsafe { RUNTIME.handle_job_step(host_ctx, job, step) }
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_job_end(
                host_ctx: *mut c_void,
                job: u64,
                cancelled: u32,
            ) -> AriaResult {
                RUNTIME.handle_job_end(host_ctx, job, cancelled)
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_decoder_probe(
                host_ctx: *mut c_void,
                decoder_id: AriaStr,
                path: AriaStr,
            ) -> AriaResult {
                RUNTIME.handle_decoder_probe(host_ctx, decoder_id, path)
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_decoder_open_video(
                host_ctx: *mut c_void,
                decoder_id: AriaStr,
                path: AriaStr,
                info: *mut AriaVideoInfo,
            ) -> AriaInstanceResult {
                // SAFETY: the host passes a writable `info` for the duration of the call.
                unsafe { RUNTIME.handle_decoder_open_video(host_ctx, decoder_id, path, info) }
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_decoder_seek(
                host_ctx: *mut c_void,
                instance: u64,
                pts: i64,
            ) -> AriaResult {
                RUNTIME.handle_decoder_seek(host_ctx, instance, pts)
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_decoder_next_frame(
                host_ctx: *mut c_void,
                instance: u64,
                frame: *mut AriaVideoFrameOut,
            ) -> AriaResult {
                // SAFETY: the host passes a writable `frame` and planes for the duration of the call.
                unsafe { RUNTIME.handle_decoder_next_frame(host_ctx, instance, frame) }
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_decoder_open_audio(
                host_ctx: *mut c_void,
                decoder_id: AriaStr,
                path: AriaStr,
                sample_rate: u32,
                channels: u32,
            ) -> AriaInstanceResult {
                RUNTIME.handle_decoder_open_audio(host_ctx, decoder_id, path, sample_rate, channels)
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_decoder_read_audio(
                host_ctx: *mut c_void,
                instance: u64,
                out: *mut f32,
                out_len: usize,
                written: *mut usize,
            ) -> AriaResult {
                // SAFETY: the host passes `out_len` writable samples and a writable count.
                unsafe {
                    RUNTIME.handle_decoder_read_audio(host_ctx, instance, out, out_len, written)
                }
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_decoder_seek_audio(
                host_ctx: *mut c_void,
                instance: u64,
                frame: i64,
            ) -> AriaResult {
                RUNTIME.handle_decoder_seek_audio(host_ctx, instance, frame)
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_decoder_close(
                host_ctx: *mut c_void,
                instance: u64,
            ) -> AriaResult {
                RUNTIME.handle_decoder_close(host_ctx, instance)
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_ai_tool_call(
                host_ctx: *mut c_void,
                name: AriaStr,
                arguments_json: AriaStr,
            ) -> AriaResult {
                RUNTIME.handle_ai_tool_call(host_ctx, name, arguments_json)
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_scope_measure(
                host_ctx: *mut c_void,
                scope_id: AriaStr,
                call: *mut AriaScopeCall,
            ) -> AriaResult {
                // SAFETY: the host passes a call whose frame and image buffers are valid for the call.
                unsafe { RUNTIME.handle_scope_measure(host_ctx, scope_id, call) }
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_cpu_effect_process(
                host_ctx: *mut c_void,
                effect_id: AriaStr,
                call: *mut AriaCpuEffectCall,
            ) -> AriaResult {
                // SAFETY: the host passes a call whose input, output, and params buffers are valid for the call.
                unsafe { RUNTIME.handle_cpu_effect_process(host_ctx, effect_id, call) }
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_driver_evaluate(
                host_ctx: *mut c_void,
                language_id: AriaStr,
                expr: AriaStr,
                call: *mut AriaDriverEvalCall,
            ) -> AriaResult {
                // SAFETY: the host passes a valid writable `AriaDriverEvalCall` for the duration of the call.
                unsafe { RUNTIME.handle_driver_evaluate(host_ctx, language_id, expr, call) }
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_analyzer_open(
                host_ctx: *mut c_void,
                analyzer_id: AriaStr,
                sample_rate: u32,
                channels: u32,
            ) -> AriaInstanceResult {
                RUNTIME.handle_analyzer_open(host_ctx, analyzer_id, sample_rate, channels)
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_analyzer_push(
                host_ctx: *mut c_void,
                instance: u64,
                samples: *const f32,
                samples_len: usize,
            ) -> AriaResult {
                // SAFETY: the host passes a valid `samples` buffer covering `samples_len` elements.
                unsafe { RUNTIME.handle_analyzer_push(host_ctx, instance, samples, samples_len) }
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_analyzer_finish(
                host_ctx: *mut c_void,
                instance: u64,
            ) -> AriaResult {
                RUNTIME.handle_analyzer_finish(host_ctx, instance)
            }

            #[allow(unsafe_code)]
            unsafe extern "C" fn plugin_analyzer_drop(
                host_ctx: *mut c_void,
                instance: u64,
            ) -> AriaResult {
                RUNTIME.handle_analyzer_drop(host_ctx, instance)
            }

            static TABLE: AriaPluginV1 = AriaPluginV1 {
                struct_size: ::core::mem::size_of::<AriaPluginV1>(),
                api_major: ARIA_API_MAJOR,
                reserved: 0,
                free_string: free_rust_string,
                free_bytes: free_rust_bytes,
                mount: plugin_mount,
                unmount: plugin_unmount,
                command_run: plugin_command_run,
                event: plugin_event,
                import_file: plugin_import,
                export_file: plugin_export,
                audio_create: plugin_audio_create,
                audio_process: plugin_audio_process,
                audio_reset: plugin_audio_reset,
                audio_drop: plugin_audio_drop,
                panel_view: plugin_panel_view,
                panel_event: plugin_panel_event,
                encoder_open: plugin_encoder_open,
                encoder_push_frame: plugin_encoder_push_frame,
                encoder_push_audio: plugin_encoder_push_audio,
                encoder_finish: plugin_encoder_finish,
                encoder_drop: plugin_encoder_drop,
                overlay_scene: plugin_overlay_scene,
                overlay_event: plugin_overlay_event,
                job_step: plugin_job_step,
                job_end: plugin_job_end,
                decoder_probe: plugin_decoder_probe,
                decoder_open_video: plugin_decoder_open_video,
                decoder_seek: plugin_decoder_seek,
                decoder_next_frame: plugin_decoder_next_frame,
                decoder_open_audio: plugin_decoder_open_audio,
                decoder_read_audio: plugin_decoder_read_audio,
                decoder_seek_audio: plugin_decoder_seek_audio,
                decoder_close: plugin_decoder_close,
                ai_tool_call: plugin_ai_tool_call,
                scope_measure: plugin_scope_measure,
                cpu_effect_process: plugin_cpu_effect_process,
                driver_evaluate: plugin_driver_evaluate,
                analyzer_open: plugin_analyzer_open,
                analyzer_push: plugin_analyzer_push,
                analyzer_finish: plugin_analyzer_finish,
                analyzer_drop: plugin_analyzer_drop,
            };

            #[allow(unsafe_code)]
            #[unsafe(no_mangle)]
            pub unsafe extern "C" fn aria_plugin_entry(
                host: *const AriaHostV1,
                api_major: u32,
            ) -> *const AriaPluginV1 {
                let _ = ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| {
                    // SAFETY: `host` is checked for null and size inside `bind_host`.
                    unsafe {
                        RUNTIME.bind_host(host, api_major);
                    }
                }));
                &TABLE
            }
        };
    };
}
