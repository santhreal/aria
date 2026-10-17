//! `decoder_*`, `ai_tool_call`, and `scope_measure` entries for a hand-built
//! `AriaPluginV1` that registers no decoder, AI tool, or scope: each refuses.

use std::ffi::c_void;

use aria_plugin_abi::{
    AriaCpuEffectCall, AriaDriverEvalCall, AriaInstanceResult, AriaResult, AriaScopeCall, AriaStr,
    AriaVideoFrameOut, AriaVideoInfo,
};

pub(crate) unsafe extern "C" fn probe(
    _ctx: *mut c_void,
    _id: AriaStr,
    _path: AriaStr,
) -> AriaResult {
    AriaResult::unsupported("no decoder")
}

pub(crate) unsafe extern "C" fn open_video(
    _ctx: *mut c_void,
    _id: AriaStr,
    _path: AriaStr,
    _info: *mut AriaVideoInfo,
) -> AriaInstanceResult {
    AriaInstanceResult::unsupported("no decoder")
}

pub(crate) unsafe extern "C" fn seek(_ctx: *mut c_void, _instance: u64, _at: i64) -> AriaResult {
    AriaResult::unsupported("no decoder")
}

pub(crate) unsafe extern "C" fn next_frame(
    _ctx: *mut c_void,
    _instance: u64,
    _frame: *mut AriaVideoFrameOut,
) -> AriaResult {
    AriaResult::unsupported("no decoder")
}

pub(crate) unsafe extern "C" fn open_audio(
    _ctx: *mut c_void,
    _id: AriaStr,
    _path: AriaStr,
    _sample_rate: u32,
    _channels: u32,
) -> AriaInstanceResult {
    AriaInstanceResult::unsupported("no decoder")
}

pub(crate) unsafe extern "C" fn read_audio(
    _ctx: *mut c_void,
    _instance: u64,
    _out: *mut f32,
    _out_len: usize,
    _written: *mut usize,
) -> AriaResult {
    AriaResult::unsupported("no decoder")
}

pub(crate) unsafe extern "C" fn close(_ctx: *mut c_void, _instance: u64) -> AriaResult {
    AriaResult::ok_empty()
}

pub(crate) unsafe extern "C" fn ai_tool_call(
    _ctx: *mut c_void,
    _name: AriaStr,
    _arguments_json: AriaStr,
) -> AriaResult {
    AriaResult::err("no AI tool")
}

pub(crate) unsafe extern "C" fn scope_measure(
    _ctx: *mut c_void,
    _scope_id: AriaStr,
    _call: *mut AriaScopeCall,
) -> AriaResult {
    AriaResult::err("no scope")
}

pub(crate) unsafe extern "C" fn cpu_effect_process(
    _ctx: *mut c_void,
    _effect_id: AriaStr,
    _call: *mut AriaCpuEffectCall,
) -> AriaResult {
    AriaResult::err("no CPU effect")
}

pub(crate) unsafe extern "C" fn driver_evaluate(
    _ctx: *mut c_void,
    _language_id: AriaStr,
    _expr: AriaStr,
    _call: *mut AriaDriverEvalCall,
) -> AriaResult {
    AriaResult::err("no driver language")
}

pub(crate) unsafe extern "C" fn analyzer_open(
    _ctx: *mut c_void,
    _analyzer_id: AriaStr,
    _sample_rate: u32,
    _channels: u32,
) -> AriaInstanceResult {
    AriaInstanceResult::err("no analyzer")
}

pub(crate) unsafe extern "C" fn analyzer_push(
    _ctx: *mut c_void,
    _instance: u64,
    _samples: *const f32,
    _samples_len: usize,
) -> AriaResult {
    AriaResult::err("no analyzer")
}

pub(crate) unsafe extern "C" fn analyzer_finish(_ctx: *mut c_void, _instance: u64) -> AriaResult {
    AriaResult::err("no analyzer")
}

pub(crate) unsafe extern "C" fn analyzer_drop(_ctx: *mut c_void, _instance: u64) -> AriaResult {
    AriaResult::err("no analyzer")
}
