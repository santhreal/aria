use std::ffi::c_void;

#[cfg(doc)]
use super::{ARIA_BYTES_ERR, ARIA_INSTANCE_ERR, ARIA_STATUS_ERR};
use super::{
    AriaBytesResult, AriaCpuEffectCall, AriaDriverEvalCall, AriaEncoderAudio, AriaEncoderConfig,
    AriaEncoderFrame, AriaHostV1, AriaInstanceResult, AriaJobStep, AriaPluginV1, AriaResult,
    AriaScopeCall, AriaStr, AriaVideoFrameOut, AriaVideoInfo,
};

/// Smallest valid [`AriaHostV1`]: the byte size of every slot through `project_undo`.
pub const ARIA_HOST_V1_MIN_SIZE: usize = std::mem::offset_of!(AriaHostV1, register_panel);

/// Smallest valid [`AriaPluginV1`]: the byte size of every slot through `audio_drop`.
pub const ARIA_PLUGIN_V1_MIN_SIZE: usize = std::mem::offset_of!(AriaPluginV1, panel_view);

/// Byte size of an [`AriaHostV1`] through `refresh_panel`.
pub const ARIA_HOST_V1_PANELS_SIZE: usize = std::mem::offset_of!(AriaHostV1, register_encoder);

/// Byte size of an [`AriaPluginV1`] through `panel_event`.
pub const ARIA_PLUGIN_V1_PANELS_SIZE: usize = std::mem::offset_of!(AriaPluginV1, encoder_open);

/// Byte size of an [`AriaHostV1`] through `register_encoder`.
pub const ARIA_HOST_V1_ENCODERS_SIZE: usize =
    std::mem::offset_of!(AriaHostV1, register_viewer_overlay);

/// Byte size of an [`AriaPluginV1`] through `encoder_drop`.
pub const ARIA_PLUGIN_V1_ENCODERS_SIZE: usize = std::mem::offset_of!(AriaPluginV1, overlay_scene);

/// Byte size of an [`AriaHostV1`] through `refresh_viewer_overlay`.
pub const ARIA_HOST_V1_OVERLAYS_SIZE: usize = std::mem::offset_of!(AriaHostV1, start_job);

/// Byte size of an [`AriaPluginV1`] through `overlay_event`.
pub const ARIA_PLUGIN_V1_OVERLAYS_SIZE: usize = std::mem::offset_of!(AriaPluginV1, job_step);

/// Byte size of an [`AriaHostV1`] through `start_job`.
pub const ARIA_HOST_V1_JOBS_SIZE: usize = std::mem::offset_of!(AriaHostV1, register_decoder);

/// Byte size of an [`AriaPluginV1`] through `job_end`.
pub const ARIA_PLUGIN_V1_JOBS_SIZE: usize = std::mem::offset_of!(AriaPluginV1, decoder_probe);

/// Byte size of an [`AriaHostV1`] through `register_decoder`.
pub const ARIA_HOST_V1_DECODERS_SIZE: usize = std::mem::offset_of!(AriaHostV1, register_ai_tool);

/// Byte size of an [`AriaPluginV1`] through `decoder_close`.
pub const ARIA_PLUGIN_V1_DECODERS_SIZE: usize = std::mem::offset_of!(AriaPluginV1, ai_tool_call);

/// Byte size of an [`AriaHostV1`] through `register_ai_tool`.
pub const ARIA_HOST_V1_AI_TOOLS_SIZE: usize = std::mem::offset_of!(AriaHostV1, register_scope);

/// Byte size of an [`AriaPluginV1`] through `ai_tool_call`.
pub const ARIA_PLUGIN_V1_AI_TOOLS_SIZE: usize = std::mem::offset_of!(AriaPluginV1, scope_measure);

/// Byte size of an [`AriaHostV1`] through `register_scope`.
pub const ARIA_HOST_V1_SCOPES_SIZE: usize = std::mem::offset_of!(AriaHostV1, register_effect);

/// Byte size of an [`AriaPluginV1`] through `scope_measure`.
pub const ARIA_PLUGIN_V1_SCOPES_SIZE: usize =
    std::mem::offset_of!(AriaPluginV1, cpu_effect_process);

/// Byte size of an [`AriaHostV1`] through `register_analyzer`.
pub const ARIA_HOST_V1_CONTRIBUTIONS_SIZE: usize = std::mem::offset_of!(AriaHostV1, clip);

/// Byte size of an [`AriaHostV1`] through `in_out`.
pub const ARIA_HOST_V1_QUERIES_SIZE: usize = std::mem::offset_of!(AriaHostV1, register_texture);

#[allow(unsafe_code)]
unsafe extern "C" fn stub_panel_view(
    _host_ctx: *mut c_void,
    _panel_id: AriaStr,
) -> AriaBytesResult {
    AriaBytesResult::err("plugin table does not export panel_view")
}

#[allow(unsafe_code)]
unsafe extern "C" fn stub_panel_event(
    _host_ctx: *mut c_void,
    _panel_id: AriaStr,
    _event_json: AriaStr,
) -> AriaResult {
    AriaResult::err("plugin table does not export panel_event")
}
#[allow(unsafe_code)]
unsafe extern "C" fn stub_encoder_open(
    _host_ctx: *mut c_void,
    _encoder_id: AriaStr,
    _path: AriaStr,
    _config: *const AriaEncoderConfig,
) -> AriaInstanceResult {
    AriaInstanceResult::err("plugin table does not export encoder_open")
}

#[allow(unsafe_code)]
unsafe extern "C" fn stub_encoder_push_frame(
    _host_ctx: *mut c_void,
    _instance: u64,
    _frame: *const AriaEncoderFrame,
) -> AriaResult {
    AriaResult::err("plugin table does not export encoder_push_frame")
}

#[allow(unsafe_code)]
unsafe extern "C" fn stub_encoder_push_audio(
    _host_ctx: *mut c_void,
    _instance: u64,
    _audio: *const AriaEncoderAudio,
) -> AriaResult {
    AriaResult::err("plugin table does not export encoder_push_audio")
}

#[allow(unsafe_code)]
unsafe extern "C" fn stub_encoder_finish(_host_ctx: *mut c_void, _instance: u64) -> AriaResult {
    AriaResult::err("plugin table does not export encoder_finish")
}

#[allow(unsafe_code)]
unsafe extern "C" fn stub_encoder_drop(_host_ctx: *mut c_void, _instance: u64) -> AriaResult {
    AriaResult::err("plugin table does not export encoder_drop")
}

#[allow(unsafe_code)]
unsafe extern "C" fn stub_overlay_scene(
    _host_ctx: *mut c_void,
    _overlay_id: AriaStr,
) -> AriaBytesResult {
    AriaBytesResult::err("plugin table does not export overlay_scene")
}

#[allow(unsafe_code)]
unsafe extern "C" fn stub_overlay_event(
    _host_ctx: *mut c_void,
    _overlay_id: AriaStr,
    _event_json: AriaStr,
) -> AriaResult {
    AriaResult::err("plugin table does not export overlay_event")
}

#[allow(unsafe_code)]
unsafe extern "C" fn stub_job_step(
    _host_ctx: *mut c_void,
    _job: u64,
    _step: *mut AriaJobStep,
) -> AriaResult {
    AriaResult::err("plugin table does not export job_step")
}

#[allow(unsafe_code)]
unsafe extern "C" fn stub_job_end(
    _host_ctx: *mut c_void,
    _job: u64,
    _cancelled: u32,
) -> AriaResult {
    AriaResult::err("plugin table does not export job_end")
}

#[allow(unsafe_code)]
unsafe extern "C" fn stub_decoder_probe(
    _host_ctx: *mut c_void,
    _decoder_id: AriaStr,
    _path: AriaStr,
) -> AriaResult {
    AriaResult::err("plugin table does not export decoder_probe")
}

#[allow(unsafe_code)]
unsafe extern "C" fn stub_decoder_open_video(
    _host_ctx: *mut c_void,
    _decoder_id: AriaStr,
    _path: AriaStr,
    _info: *mut AriaVideoInfo,
) -> AriaInstanceResult {
    AriaInstanceResult::err("plugin table does not export decoder_open_video")
}

#[allow(unsafe_code)]
unsafe extern "C" fn stub_decoder_seek(
    _host_ctx: *mut c_void,
    _instance: u64,
    _pts: i64,
) -> AriaResult {
    AriaResult::err("plugin table does not export decoder_seek")
}

#[allow(unsafe_code)]
unsafe extern "C" fn stub_decoder_next_frame(
    _host_ctx: *mut c_void,
    _instance: u64,
    _frame: *mut AriaVideoFrameOut,
) -> AriaResult {
    AriaResult::err("plugin table does not export decoder_next_frame")
}

#[allow(unsafe_code)]
unsafe extern "C" fn stub_decoder_open_audio(
    _host_ctx: *mut c_void,
    _decoder_id: AriaStr,
    _path: AriaStr,
    _sample_rate: u32,
    _channels: u32,
) -> AriaInstanceResult {
    AriaInstanceResult::err("plugin table does not export decoder_open_audio")
}

#[allow(unsafe_code)]
unsafe extern "C" fn stub_decoder_read_audio(
    _host_ctx: *mut c_void,
    _instance: u64,
    _out: *mut f32,
    _out_len: usize,
    _written: *mut usize,
) -> AriaResult {
    AriaResult::err("plugin table does not export decoder_read_audio")
}

#[allow(unsafe_code)]
unsafe extern "C" fn stub_decoder_seek_audio(
    _host_ctx: *mut c_void,
    _instance: u64,
    _frame: i64,
) -> AriaResult {
    AriaResult::err("plugin table does not export decoder_seek_audio")
}

#[allow(unsafe_code)]
unsafe extern "C" fn stub_decoder_close(_host_ctx: *mut c_void, _instance: u64) -> AriaResult {
    AriaResult::err("plugin table does not export decoder_close")
}

#[allow(unsafe_code)]
unsafe extern "C" fn stub_ai_tool_call(
    _host_ctx: *mut c_void,
    _name: AriaStr,
    _arguments_json: AriaStr,
) -> AriaResult {
    AriaResult::err("plugin table does not export ai_tool_call")
}

#[allow(unsafe_code)]
unsafe extern "C" fn stub_scope_measure(
    _host_ctx: *mut c_void,
    _scope_id: AriaStr,
    _call: *mut AriaScopeCall,
) -> AriaResult {
    AriaResult::err("plugin table does not export scope_measure")
}

#[allow(unsafe_code)]
unsafe extern "C" fn stub_cpu_effect_process(
    _host_ctx: *mut c_void,
    _effect_id: AriaStr,
    _call: *mut AriaCpuEffectCall,
) -> AriaResult {
    AriaResult::err("plugin table does not export cpu_effect_process")
}

#[allow(unsafe_code)]
unsafe extern "C" fn stub_driver_evaluate(
    _host_ctx: *mut c_void,
    _language_id: AriaStr,
    _expr: AriaStr,
    _call: *mut AriaDriverEvalCall,
) -> AriaResult {
    AriaResult::err("plugin table does not export driver_evaluate")
}

#[allow(unsafe_code)]
pub(super) unsafe extern "C" fn stub_analyzer_open(
    _host_ctx: *mut c_void,
    _analyzer_id: AriaStr,
    _sample_rate: u32,
    _channels: u32,
) -> AriaInstanceResult {
    AriaInstanceResult::err("plugin table does not export analyzer_open")
}

#[allow(unsafe_code)]
pub(super) unsafe extern "C" fn stub_analyzer_push(
    _host_ctx: *mut c_void,
    _instance: u64,
    _samples: *const f32,
    _samples_len: usize,
) -> AriaResult {
    AriaResult::err("plugin table does not export analyzer_push")
}

#[allow(unsafe_code)]
pub(super) unsafe extern "C" fn stub_analyzer_finish(
    _host_ctx: *mut c_void,
    _instance: u64,
) -> AriaResult {
    AriaResult::err("plugin table does not export analyzer_finish")
}

#[allow(unsafe_code)]
pub(super) unsafe extern "C" fn stub_analyzer_drop(
    _host_ctx: *mut c_void,
    _instance: u64,
) -> AriaResult {
    AriaResult::err("plugin table does not export analyzer_drop")
}

impl AriaHostV1 {
    /// Whether the table holds `register_panel` and `refresh_panel`.
    pub const fn has_panels(&self) -> bool {
        self.struct_size >= ARIA_HOST_V1_PANELS_SIZE
    }

    /// Whether the table holds `register_encoder`.
    pub const fn has_encoders(&self) -> bool {
        self.struct_size >= ARIA_HOST_V1_ENCODERS_SIZE
    }

    /// Whether the table holds `register_viewer_overlay` and `refresh_viewer_overlay`.
    pub const fn has_overlays(&self) -> bool {
        self.struct_size >= ARIA_HOST_V1_OVERLAYS_SIZE
    }

    /// Whether the table holds `start_job`.
    pub const fn has_jobs(&self) -> bool {
        self.struct_size >= ARIA_HOST_V1_JOBS_SIZE
    }

    /// Whether the table holds `register_decoder`.
    pub const fn has_decoders(&self) -> bool {
        self.struct_size >= ARIA_HOST_V1_DECODERS_SIZE
    }

    /// Whether the table holds `register_ai_tool`.
    pub const fn has_ai_tools(&self) -> bool {
        self.struct_size >= ARIA_HOST_V1_AI_TOOLS_SIZE
    }

    /// Whether the table holds `register_scope`.
    pub const fn has_scopes(&self) -> bool {
        self.struct_size >= ARIA_HOST_V1_SCOPES_SIZE
    }

    /// Whether the table holds the contribution calls, `register_effect` through
    /// `register_analyzer`.
    pub const fn has_contributions(&self) -> bool {
        self.struct_size >= ARIA_HOST_V1_CONTRIBUTIONS_SIZE
    }

    /// Whether the table holds the project queries, `clip` through `in_out`.
    pub const fn has_project_queries(&self) -> bool {
        self.struct_size >= ARIA_HOST_V1_QUERIES_SIZE
    }

    /// Whether the table holds `register_texture`.
    pub const fn has_textures(&self) -> bool {
        self.struct_size >= std::mem::size_of::<Self>()
    }
}

impl AriaPluginV1 {
    /// Whether the table holds `panel_view` and `panel_event`.
    pub const fn has_panels(&self) -> bool {
        self.struct_size >= ARIA_PLUGIN_V1_PANELS_SIZE
    }

    /// Whether the table holds the encoder callbacks, `encoder_open` through
    /// `encoder_drop`.
    pub const fn has_encoders(&self) -> bool {
        self.struct_size >= ARIA_PLUGIN_V1_ENCODERS_SIZE
    }

    /// Whether the table holds `overlay_scene` and `overlay_event`.
    pub const fn has_overlays(&self) -> bool {
        self.struct_size >= ARIA_PLUGIN_V1_OVERLAYS_SIZE
    }

    /// Whether the table holds `job_step` and `job_end`.
    pub const fn has_jobs(&self) -> bool {
        self.struct_size >= ARIA_PLUGIN_V1_JOBS_SIZE
    }

    /// Whether the table holds the decoder callbacks, `decoder_probe` through
    /// `decoder_close`.
    pub const fn has_decoders(&self) -> bool {
        self.struct_size >= ARIA_PLUGIN_V1_DECODERS_SIZE
    }

    /// Whether the table holds `ai_tool_call`.
    pub const fn has_ai_tools(&self) -> bool {
        self.struct_size >= ARIA_PLUGIN_V1_AI_TOOLS_SIZE
    }

    /// Whether the table holds `scope_measure`.
    pub const fn has_scopes(&self) -> bool {
        self.struct_size >= ARIA_PLUGIN_V1_SCOPES_SIZE
    }

    /// Whether the table holds the contribution callbacks, `cpu_effect_process`,
    /// `driver_evaluate`, and the analyzer callbacks.
    pub const fn has_contributions(&self) -> bool {
        self.struct_size >= std::mem::size_of::<Self>()
    }

    /// Read a validated [`AriaPluginV1`] from `ptr`. When `struct_size` is smaller than
    /// [`AriaPluginV1`], each slot group the prefix does not cover holds stubs that
    /// return [`ARIA_STATUS_ERR`], [`ARIA_BYTES_ERR`] or [`ARIA_INSTANCE_ERR`]: the
    /// panel, encoder, overlay, job, decoder, AI tool, scope, and contribution groups.
    ///
    /// # Safety
    ///
    /// `ptr` must point to at least `struct_size` valid bytes of an `AriaPluginV1` prefix,
    /// where `struct_size >= ARIA_PLUGIN_V1_MIN_SIZE`.
    #[allow(unsafe_code)]
    pub unsafe fn copy_from_prefix(ptr: *const Self, struct_size: usize) -> Self {
        if struct_size >= std::mem::size_of::<Self>() {
            // SAFETY: The caller guarantees `ptr` holds at least `size_of::<Self>()` bytes.
            return unsafe { *ptr };
        }
        let mut fallback = std::mem::MaybeUninit::<Self>::zeroed();
        let copy_len = if struct_size >= ARIA_PLUGIN_V1_SCOPES_SIZE {
            ARIA_PLUGIN_V1_SCOPES_SIZE
        } else if struct_size >= ARIA_PLUGIN_V1_AI_TOOLS_SIZE {
            ARIA_PLUGIN_V1_AI_TOOLS_SIZE
        } else if struct_size >= ARIA_PLUGIN_V1_DECODERS_SIZE {
            ARIA_PLUGIN_V1_DECODERS_SIZE
        } else if struct_size >= ARIA_PLUGIN_V1_JOBS_SIZE {
            ARIA_PLUGIN_V1_JOBS_SIZE
        } else if struct_size >= ARIA_PLUGIN_V1_OVERLAYS_SIZE {
            ARIA_PLUGIN_V1_OVERLAYS_SIZE
        } else if struct_size >= ARIA_PLUGIN_V1_ENCODERS_SIZE {
            ARIA_PLUGIN_V1_ENCODERS_SIZE
        } else if struct_size >= ARIA_PLUGIN_V1_PANELS_SIZE {
            ARIA_PLUGIN_V1_PANELS_SIZE
        } else {
            ARIA_PLUGIN_V1_MIN_SIZE
        };
        // SAFETY: Copying the validated prefix into `fallback`, then initializing missing trailing fn pointers.
        unsafe {
            std::ptr::copy_nonoverlapping(
                ptr.cast::<u8>(),
                fallback.as_mut_ptr().cast::<u8>(),
                copy_len,
            );
            let dst = fallback.as_mut_ptr();
            if struct_size < ARIA_PLUGIN_V1_PANELS_SIZE {
                (*dst).panel_view = stub_panel_view;
                (*dst).panel_event = stub_panel_event;
            }
            if struct_size < ARIA_PLUGIN_V1_ENCODERS_SIZE {
                (*dst).encoder_open = stub_encoder_open;
                (*dst).encoder_push_frame = stub_encoder_push_frame;
                (*dst).encoder_push_audio = stub_encoder_push_audio;
                (*dst).encoder_finish = stub_encoder_finish;
                (*dst).encoder_drop = stub_encoder_drop;
            }
            if struct_size < ARIA_PLUGIN_V1_OVERLAYS_SIZE {
                (*dst).overlay_scene = stub_overlay_scene;
                (*dst).overlay_event = stub_overlay_event;
            }
            if struct_size < ARIA_PLUGIN_V1_JOBS_SIZE {
                (*dst).job_step = stub_job_step;
                (*dst).job_end = stub_job_end;
            }
            if struct_size < ARIA_PLUGIN_V1_DECODERS_SIZE {
                (*dst).decoder_probe = stub_decoder_probe;
                (*dst).decoder_open_video = stub_decoder_open_video;
                (*dst).decoder_seek = stub_decoder_seek;
                (*dst).decoder_next_frame = stub_decoder_next_frame;
                (*dst).decoder_open_audio = stub_decoder_open_audio;
                (*dst).decoder_read_audio = stub_decoder_read_audio;
                (*dst).decoder_seek_audio = stub_decoder_seek_audio;
                (*dst).decoder_close = stub_decoder_close;
            }
            if struct_size < ARIA_PLUGIN_V1_AI_TOOLS_SIZE {
                (*dst).ai_tool_call = stub_ai_tool_call;
            }
            if struct_size < ARIA_PLUGIN_V1_SCOPES_SIZE {
                (*dst).scope_measure = stub_scope_measure;
            }
            (*dst).cpu_effect_process = stub_cpu_effect_process;
            (*dst).driver_evaluate = stub_driver_evaluate;
            (*dst).analyzer_open = stub_analyzer_open;
            (*dst).analyzer_push = stub_analyzer_push;
            (*dst).analyzer_finish = stub_analyzer_finish;
            (*dst).analyzer_drop = stub_analyzer_drop;
            fallback.assume_init()
        }
    }
}
