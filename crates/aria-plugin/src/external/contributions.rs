//! Effects, CPU effects, generators, transitions, clip types, driver languages,
//! presets, themes, workspaces, and textures registered by native plugins.

#![allow(unsafe_code)]

use std::ffi::c_void;
use std::panic::resume_unwind;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use aria_compositor::{
    CpuEffect, CpuFrameIn, CpuFrameOut, EffectDefinition, EffectParams, TransitionDefinition,
};
use aria_models::{ClipTypeSpec, EffectSpec, GraphPass, MarkerColor, ParamValue, TransitionSpec};
use aria_plugin_abi::{
    ARIA_PARAM_COLOR, ARIA_PARAM_RECT, ARIA_PARAM_SCALAR, ARIA_PARAM_VEC2, ARIA_PARAM_VEC3,
    ARIA_STATUS_OK, ARIA_STATUS_PANIC, AriaCpuEffectCall, AriaDriverEvalCall, AriaParamValue,
    AriaStr,
};
use aria_render::{DriverInput, DriverLanguage, DriverProgram, DriverScope};
use serde::Deserialize;

use super::{HostCallContext, NativePluginState};
use crate::analyzers::{Analyzer, AnalyzerResult, AnalyzerRun, AnalyzerSpec};
use crate::context::PluginContext;
use crate::drivers::DriverLanguageSpec;
use crate::host_wire;
use crate::workspaces::WorkspaceSpec;

#[path = "clip_type_schema.rs"]
mod clip_type_schema;

#[derive(Deserialize)]
struct RawClipTypeSpec {
    id: String,
    label: String,
    /// Empty or the type's own id for a type with a `schema`.
    #[serde(default)]
    generator: String,
    colour: MarkerColor,
    #[serde(default)]
    icon_svg: Option<String>,
    #[serde(default)]
    handles: Vec<String>,
    /// The fields and renderer of a type that draws itself.
    #[serde(default)]
    schema: Option<clip_type_schema::RawClipTypeSchema>,
}

/// One pass of a C ABI pass list: WGSL source for a fragment pass reading the
/// previous pass, or a whole [`GraphPass`] object.
#[derive(Deserialize)]
#[serde(untagged)]
enum RawPass {
    Source(String),
    Graph(GraphPass),
}

impl RawPass {
    fn into_graph(self) -> GraphPass {
        match self {
            Self::Source(source) => GraphPass::fragment(source),
            Self::Graph(pass) => pass,
        }
    }
}

/// The passes `wgsl` declares: one fragment pass of WGSL source, or a JSON
/// array of WGSL strings and [`GraphPass`] objects. No WGSL module starts with
/// `[`, so a list that fails to parse is refused rather than compiled.
fn parse_passes(wgsl: &str) -> Result<Vec<GraphPass>, String> {
    if !wgsl.trim_start().starts_with('[') {
        return Ok(vec![GraphPass::fragment(wgsl)]);
    }
    let passes: Vec<RawPass> =
        serde_json::from_str(wgsl).map_err(|err| format!("invalid pass list JSON: {err}"))?;
    Ok(passes.into_iter().map(RawPass::into_graph).collect())
}

pub(super) fn register_effect(
    ctx: &HostCallContext<'_>,
    spec_json: &str,
    wgsl: &str,
) -> Result<(), String> {
    let mut spec: EffectSpec = serde_json::from_str(spec_json)
        .map_err(|err| format!("invalid effect spec JSON: {err}"))?;
    spec.generator = false;
    let passes = parse_passes(wgsl)?;
    ctx.scoped_ctx
        .register_effect(EffectDefinition { spec, passes })
        .map(drop)
        .map_err(|err| err.to_string())
}

pub(super) fn register_cpu_effect(
    ctx: &HostCallContext<'_>,
    spec_json: &str,
) -> Result<(), String> {
    if !ctx.state.table.has_contributions() {
        return Err("plugin table does not include the cpu_effect_process callback".to_owned());
    }
    let spec: EffectSpec = serde_json::from_str(spec_json)
        .map_err(|err| format!("invalid CPU effect spec JSON: {err}"))?;
    let effect = NativeCpuEffect {
        id: spec.id.clone(),
        state: Arc::clone(ctx.state),
        scoped_ctx: ctx.scoped_ctx.clone(),
    };
    ctx.scoped_ctx
        .register_cpu_effect(spec, effect)
        .map(drop)
        .map_err(|err| err.to_string())
}

pub(super) fn register_transition(
    ctx: &HostCallContext<'_>,
    spec_json: &str,
    wgsl: &str,
) -> Result<(), String> {
    let spec: TransitionSpec = serde_json::from_str(spec_json)
        .map_err(|err| format!("invalid transition spec JSON: {err}"))?;
    ctx.scoped_ctx
        .register_transition(TransitionDefinition {
            spec,
            shader: wgsl.to_owned(),
        })
        .map(drop)
        .map_err(|err| err.to_string())
}

pub(super) fn register_generator(
    ctx: &HostCallContext<'_>,
    spec_json: &str,
    wgsl: &str,
) -> Result<(), String> {
    let mut spec: EffectSpec = serde_json::from_str(spec_json)
        .map_err(|err| format!("invalid generator spec JSON: {err}"))?;
    spec.generator = true;
    if wgsl.is_empty() {
        if !ctx.state.table.has_contributions() {
            return Err("plugin table does not include the cpu_effect_process callback".to_owned());
        }
        let effect = NativeCpuEffect {
            id: spec.id.clone(),
            state: Arc::clone(ctx.state),
            scoped_ctx: ctx.scoped_ctx.clone(),
        };
        ctx.scoped_ctx
            .register_cpu_effect(spec, effect)
            .map(drop)
            .map_err(|err| err.to_string())
    } else {
        let passes = parse_passes(wgsl)?;
        ctx.scoped_ctx
            .register_effect(EffectDefinition { spec, passes })
            .map(drop)
            .map_err(|err| err.to_string())
    }
}

pub(super) fn register_clip_type(ctx: &HostCallContext<'_>, spec_json: &str) -> Result<(), String> {
    let raw: RawClipTypeSpec = serde_json::from_str(spec_json)
        .map_err(|err| format!("invalid clip type spec JSON: {err}"))?;
    let icon = host_wire::clip_type_icon(raw.icon_svg);
    let spec = ClipTypeSpec {
        id: raw.id,
        label: raw.label,
        generator: raw.generator,
        colour: raw.colour,
        icon,
        handles: raw.handles,
    };
    match raw.schema {
        Some(schema) => clip_type_schema::define(ctx, spec, schema),
        None => ctx
            .scoped_ctx
            .register_clip_type(spec)
            .map(drop)
            .map_err(|err| err.to_string()),
    }
}

pub(super) fn register_driver_language(
    ctx: &HostCallContext<'_>,
    spec_json: &str,
) -> Result<(), String> {
    if !ctx.state.table.has_contributions() {
        return Err("plugin table does not include the driver_evaluate callback".to_owned());
    }
    let spec: DriverLanguageSpec = serde_json::from_str(spec_json)
        .map_err(|err| format!("invalid driver language spec JSON: {err}"))?;
    let language = NativeDriverLanguage {
        id: Arc::from(spec.id.as_str()),
        state: Arc::clone(ctx.state),
        scoped_ctx: ctx.scoped_ctx.clone(),
    };
    ctx.scoped_ctx
        .register_driver_language(spec, language)
        .map(drop)
        .map_err(|err| err.to_string())
}

pub(super) fn register_preset(ctx: &HostCallContext<'_>, preset_json: &str) -> Result<(), String> {
    host_wire::register_preset_json(ctx.scoped_ctx, preset_json)
}

pub(super) fn register_theme(
    ctx: &HostCallContext<'_>,
    id: &str,
    toml: &str,
) -> Result<(), String> {
    host_wire::register_theme_toml(ctx.scoped_ctx, id, toml)
}

pub(super) fn register_texture(
    ctx: &HostCallContext<'_>,
    id: &str,
    width: u32,
    height: u32,
    pixels: &[u8],
) -> Result<(), String> {
    host_wire::register_rgba_texture(ctx.scoped_ctx, id, width, height, pixels.to_vec())
}

pub(super) fn register_workspace(ctx: &HostCallContext<'_>, spec_json: &str) -> Result<(), String> {
    let spec: WorkspaceSpec = serde_json::from_str(spec_json)
        .map_err(|err| format!("invalid workspace spec JSON: {err}"))?;
    ctx.scoped_ctx
        .register_workspace(spec)
        .map(drop)
        .map_err(|err| err.to_string())
}

pub(super) fn register_analyzer(ctx: &HostCallContext<'_>, spec_json: &str) -> Result<(), String> {
    if !ctx.state.table.has_contributions() {
        return Err("plugin table does not include the analyzer callbacks".to_owned());
    }
    let spec: AnalyzerSpec = serde_json::from_str(spec_json)
        .map_err(|err| format!("invalid analyzer spec JSON: {err}"))?;
    let analyzer = NativeAnalyzer {
        id: Arc::from(spec.id.as_str()),
        state: Arc::clone(ctx.state),
        scoped_ctx: ctx.scoped_ctx.clone(),
    };
    ctx.scoped_ctx
        .register_analyzer(spec, analyzer)
        .map(drop)
        .map_err(|err| err.to_string())
}

struct NativeCpuEffect {
    id: String,
    state: Arc<NativePluginState>,
    scoped_ctx: PluginContext,
}

impl CpuEffect for NativeCpuEffect {
    fn process(
        &self,
        input: CpuFrameIn<'_>,
        output: &mut CpuFrameOut<'_>,
        params: &EffectParams<'_>,
        time: f32,
    ) -> Result<(), String> {
        if self.state.is_failed() {
            return Err(format!("plugin `{}` has faulted", self.state.id));
        }
        let width = input.width();
        let height = input.height();
        let row_elems = (width as usize) * 4;
        let pixels_len = (height as usize) * row_elems;
        let mut input_f16 = Vec::new();
        host_wire::pack_cpu_frame_in(input, &mut input_f16);
        let mut output_f16 = vec![0u16; pixels_len];
        let param_values = params.values();
        let params_json = params
            .instance()
            .and_then(|inst| serde_json::to_string(inst).ok())
            .unwrap_or_default();
        let mut call_ctx = HostCallContext {
            state: &self.state,
            scoped_ctx: &self.scoped_ctx,
            unscoped_ctx: self.scoped_ctx.scoped(None),
            in_mount: false,
        };
        let mut call = AriaCpuEffectCall {
            input: input_f16.as_ptr(),
            output: output_f16.as_mut_ptr(),
            pixels_len,
            width,
            height,
            params: param_values.as_ptr(),
            params_len: param_values.len(),
            params_json: AriaStr::from_str(&params_json),
            time,
        };
        // SAFETY: `call_ctx`, `self.id`, `input_f16`, `output_f16`, and
        // `param_values` outlive `cpu_effect_process`.
        let result = unsafe {
            (self.state.table.cpu_effect_process)(
                (&raw mut call_ctx).cast::<c_void>(),
                AriaStr::from_str(&self.id),
                &raw mut call,
            )
        };
        let text = self.state.take_guest_string(result.value);
        match result.status {
            ARIA_STATUS_OK => {
                host_wire::unpack_cpu_frame_out(&output_f16, output);
                Ok(())
            }
            ARIA_STATUS_PANIC => {
                self.state.failed.store(true, Ordering::Release);
                self.state.mounted.store(false, Ordering::Release);
                resume_unwind(Box::new(text));
            }
            _ => Err(text),
        }
    }
}

struct NativeDriverLanguage {
    id: Arc<str>,
    state: Arc<NativePluginState>,
    scoped_ctx: PluginContext,
}

impl DriverLanguage for NativeDriverLanguage {
    fn compile(&self, source: &str) -> Result<Arc<dyn DriverProgram>, String> {
        host_wire::validate_driver_source(source)?;
        Ok(Arc::new(NativeDriverProgram {
            language_id: Arc::clone(&self.id),
            expr: Arc::from(source),
            state: Arc::clone(&self.state),
            scoped_ctx: self.scoped_ctx.clone(),
        }))
    }
}

struct NativeDriverProgram {
    language_id: Arc<str>,
    expr: Arc<str>,
    state: Arc<NativePluginState>,
    scoped_ctx: PluginContext,
}

impl DriverProgram for NativeDriverProgram {
    fn inputs(&self) -> &[DriverInput] {
        &[]
    }

    fn evaluate(&self, scope: &mut dyn DriverScope) -> Result<ParamValue, String> {
        if self.state.is_failed() {
            return Err(format!("plugin `{}` has faulted", self.state.id));
        }
        let [canvas_width, canvas_height] = scope.canvas();
        let value = to_abi_param_value(scope.value());
        let mut call_ctx = HostCallContext {
            state: &self.state,
            scoped_ctx: &self.scoped_ctx,
            unscoped_ctx: self.scoped_ctx.scoped(None),
            in_mount: false,
        };
        let mut call = AriaDriverEvalCall {
            time: scope.time(),
            clip_time: scope.clip_time(),
            frame_rate: scope.frame_rate(),
            duration: scope.duration(),
            canvas_width,
            canvas_height,
            value,
            seed: scope.seed(),
            output: value,
        };
        // SAFETY: `call_ctx`, `language_id`, `expr`, and `call` outlive `driver_evaluate`.
        let result = unsafe {
            (self.state.table.driver_evaluate)(
                (&raw mut call_ctx).cast::<c_void>(),
                AriaStr::from_str(&self.language_id),
                AriaStr::from_str(&self.expr),
                &raw mut call,
            )
        };
        let text = self.state.take_guest_string(result.value);
        match result.status {
            ARIA_STATUS_OK => from_abi_param_value(call.output),
            ARIA_STATUS_PANIC => Err(format!("the driver panicked: {text}")),
            _ => Err(text),
        }
    }
}

struct NativeAnalyzer {
    id: Arc<str>,
    state: Arc<NativePluginState>,
    scoped_ctx: PluginContext,
}

impl Analyzer for NativeAnalyzer {
    fn begin(&self, sample_rate: u32, channels: u16) -> Box<dyn AnalyzerRun> {
        if self.state.is_failed() {
            return Box::new(NativeAnalyzerRun {
                instance: None,
                error: Some(format!("plugin `{}` has faulted", self.state.id)),
                finished: true,
                state: Arc::clone(&self.state),
                scoped_ctx: self.scoped_ctx.clone(),
            });
        }
        let mut call_ctx = HostCallContext {
            state: &self.state,
            scoped_ctx: &self.scoped_ctx,
            unscoped_ctx: self.scoped_ctx.scoped(None),
            in_mount: false,
        };
        // SAFETY: `call_ctx` and `self.id` outlive `analyzer_open`.
        let result = unsafe {
            (self.state.table.analyzer_open)(
                (&raw mut call_ctx).cast::<c_void>(),
                AriaStr::from_str(&self.id),
                sample_rate,
                u32::from(channels),
            )
        };
        match self.state.decode_instance(result) {
            Ok(instance) => Box::new(NativeAnalyzerRun {
                instance: Some(instance),
                error: None,
                finished: false,
                state: Arc::clone(&self.state),
                scoped_ctx: self.scoped_ctx.clone(),
            }),
            Err((true, err)) => {
                self.state.failed.store(true, Ordering::Release);
                self.state.mounted.store(false, Ordering::Release);
                resume_unwind(Box::new(err));
            }
            Err((false, err)) => Box::new(NativeAnalyzerRun {
                instance: None,
                error: Some(err),
                finished: true,
                state: Arc::clone(&self.state),
                scoped_ctx: self.scoped_ctx.clone(),
            }),
        }
    }
}

struct NativeAnalyzerRun {
    instance: Option<u64>,
    error: Option<String>,
    finished: bool,
    state: Arc<NativePluginState>,
    scoped_ctx: PluginContext,
}

impl AnalyzerRun for NativeAnalyzerRun {
    fn block(&mut self, _first_frame: i64, samples: &[f32]) -> std::ops::ControlFlow<()> {
        if self.error.is_some() {
            return std::ops::ControlFlow::Break(());
        }
        if self.state.is_failed() {
            self.error = Some(format!("plugin `{}` has faulted", self.state.id));
            return std::ops::ControlFlow::Break(());
        }
        let Some(instance) = self.instance else {
            return std::ops::ControlFlow::Break(());
        };
        let mut call_ctx = HostCallContext {
            state: &self.state,
            scoped_ctx: &self.scoped_ctx,
            unscoped_ctx: self.scoped_ctx.scoped(None),
            in_mount: false,
        };
        // SAFETY: `call_ctx` and `samples` outlive `analyzer_push`.
        let result = unsafe {
            (self.state.table.analyzer_push)(
                (&raw mut call_ctx).cast::<c_void>(),
                instance,
                samples.as_ptr(),
                samples.len(),
            )
        };
        let text = self.state.take_guest_string(result.value);
        match result.status {
            ARIA_STATUS_OK => std::ops::ControlFlow::Continue(()),
            ARIA_STATUS_PANIC => {
                self.finished = true;
                self.state.failed.store(true, Ordering::Release);
                self.state.mounted.store(false, Ordering::Release);
                resume_unwind(Box::new(text));
            }
            _ => {
                self.error = Some(text);
                std::ops::ControlFlow::Break(())
            }
        }
    }

    fn finish(mut self: Box<Self>) -> Result<AnalyzerResult, String> {
        if let Some(err) = self.error.take() {
            return Err(err);
        }
        self.finished = true;
        if self.state.is_failed() {
            return Err(format!("plugin `{}` has faulted", self.state.id));
        }
        let Some(instance) = self.instance else {
            return Err("analyzer session was not opened".to_owned());
        };
        let mut call_ctx = HostCallContext {
            state: &self.state,
            scoped_ctx: &self.scoped_ctx,
            unscoped_ctx: self.scoped_ctx.scoped(None),
            in_mount: false,
        };
        // SAFETY: `call_ctx` outlives `analyzer_finish`.
        let result = unsafe {
            (self.state.table.analyzer_finish)((&raw mut call_ctx).cast::<c_void>(), instance)
        };
        let text = self.state.take_guest_string(result.value);
        match result.status {
            ARIA_STATUS_OK => host_wire::parse_analyzer_result(&text),
            ARIA_STATUS_PANIC => {
                self.state.failed.store(true, Ordering::Release);
                self.state.mounted.store(false, Ordering::Release);
                resume_unwind(Box::new(text));
            }
            _ => Err(text),
        }
    }
}

impl Drop for NativeAnalyzerRun {
    fn drop(&mut self) {
        if self.finished || self.state.is_failed() {
            return;
        }
        let Some(instance) = self.instance else {
            return;
        };
        let mut call_ctx = HostCallContext {
            state: &self.state,
            scoped_ctx: &self.scoped_ctx,
            unscoped_ctx: self.scoped_ctx.scoped(None),
            in_mount: false,
        };
        // SAFETY: `call_ctx` outlives `analyzer_drop`.
        let result = unsafe {
            (self.state.table.analyzer_drop)((&raw mut call_ctx).cast::<c_void>(), instance)
        };
        if let Err((true, msg)) = self.state.decode_result(result) {
            self.state
                .mark_faulted(&self.scoped_ctx, format!("analyzer_drop panicked: {msg}"));
        }
    }
}

fn to_abi_param_value(value: ParamValue) -> AriaParamValue {
    match value {
        ParamValue::Scalar(x) => AriaParamValue {
            kind: ARIA_PARAM_SCALAR,
            x,
            ..AriaParamValue::default()
        },
        ParamValue::Vec2([x, y]) => AriaParamValue {
            kind: ARIA_PARAM_VEC2,
            x,
            y,
            ..AriaParamValue::default()
        },
        ParamValue::Vec3([x, y, z]) => AriaParamValue {
            kind: ARIA_PARAM_VEC3,
            x,
            y,
            z,
            ..AriaParamValue::default()
        },
        ParamValue::Color([r, g, b, a]) => AriaParamValue {
            kind: ARIA_PARAM_COLOR,
            r,
            g,
            b,
            a,
            ..AriaParamValue::default()
        },
        ParamValue::Rect([x, y, z, w]) => AriaParamValue {
            kind: ARIA_PARAM_RECT,
            x,
            y,
            z,
            w,
            ..AriaParamValue::default()
        },
    }
}

fn from_abi_param_value(raw: AriaParamValue) -> Result<ParamValue, String> {
    match raw.kind {
        ARIA_PARAM_SCALAR => Ok(ParamValue::Scalar(raw.x)),
        ARIA_PARAM_VEC2 => Ok(ParamValue::Vec2([raw.x, raw.y])),
        ARIA_PARAM_VEC3 => Ok(ParamValue::Vec3([raw.x, raw.y, raw.z])),
        ARIA_PARAM_COLOR => Ok(ParamValue::Color([raw.r, raw.g, raw.b, raw.a])),
        ARIA_PARAM_RECT => Ok(ParamValue::Rect([raw.x, raw.y, raw.z, raw.w])),
        other => Err(format!("unknown AriaParamValue kind {other}")),
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::c_void;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

    use aria_core::{Rational, RationalTime, TimeRange};
    use aria_models::Project;
    use aria_plugin_abi::{
        ARIA_API_MAJOR, AriaAudioBlock, AriaAudioParam, AriaBytesResult, AriaCpuEffectCall,
        AriaDriverEvalCall, AriaEncoderAudio, AriaEncoderConfig, AriaEncoderFrame,
        AriaInstanceResult, AriaJobStep, AriaOwnedBytes, AriaOwnedStr, AriaPluginV1, AriaResult,
        AriaScopeCall, AriaStr, AriaVideoFrameOut, AriaVideoInfo,
    };

    use super::super::{HOST_V1, NativePlugin, NativePluginState};
    use crate::analyzers::AnalyzerError;
    use crate::audio_host::AudioSource;
    use crate::context::PluginContext;
    use crate::package::Package;

    static UNMOUNT_CALLS: AtomicUsize = AtomicUsize::new(0);
    static OPEN_CALLS: AtomicUsize = AtomicUsize::new(0);
    static DROP_CALLS: AtomicUsize = AtomicUsize::new(0);
    static CANCEL_ON_OPEN: AtomicBool = AtomicBool::new(false);

    unsafe extern "C" fn free_str(s: AriaOwnedStr) {
        if !s.ptr.is_null() && s.cap > 0 {
            drop(unsafe { String::from_raw_parts(s.ptr, s.len, s.cap) });
        }
    }
    unsafe extern "C" fn free_bytes(_: AriaOwnedBytes) {}
    unsafe extern "C" fn noop_unmount(_: *mut c_void) -> AriaResult {
        UNMOUNT_CALLS.fetch_add(1, Ordering::SeqCst);
        AriaResult::ok_empty()
    }
    unsafe extern "C" fn noop_cmd(_: *mut c_void, _: AriaStr, _: AriaStr) -> AriaResult {
        AriaResult::ok_empty()
    }
    unsafe extern "C" fn noop_u64(_: *mut c_void, _: u64) -> AriaResult {
        AriaResult::ok_empty()
    }
    unsafe extern "C" fn noop_path(_: *mut c_void, _: AriaStr, _: AriaStr) -> AriaResult {
        AriaResult::ok_empty()
    }
    unsafe extern "C" fn noop_export(
        _: *mut c_void,
        _: AriaStr,
        _: AriaStr,
        _: AriaStr,
    ) -> AriaResult {
        AriaResult::ok_empty()
    }
    unsafe extern "C" fn noop_audio_create(
        _: *mut c_void,
        _: AriaStr,
        _: u32,
    ) -> AriaInstanceResult {
        AriaInstanceResult::err("unused")
    }
    unsafe extern "C" fn noop_audio_proc(
        _: *mut c_void,
        _: u64,
        _: *const AriaAudioParam,
        _: usize,
        _: *mut AriaAudioBlock,
    ) -> AriaResult {
        AriaResult::ok_empty()
    }
    unsafe extern "C" fn noop_bytes(_: *mut c_void, _: AriaStr) -> AriaBytesResult {
        AriaBytesResult::err("unused")
    }
    unsafe extern "C" fn noop_enc_open(
        _: *mut c_void,
        _: AriaStr,
        _: AriaStr,
        _: *const AriaEncoderConfig,
    ) -> AriaInstanceResult {
        AriaInstanceResult::err("unused")
    }
    unsafe extern "C" fn noop_enc_frame(
        _: *mut c_void,
        _: u64,
        _: *const AriaEncoderFrame,
    ) -> AriaResult {
        AriaResult::ok_empty()
    }
    unsafe extern "C" fn noop_enc_audio(
        _: *mut c_void,
        _: u64,
        _: *const AriaEncoderAudio,
    ) -> AriaResult {
        AriaResult::ok_empty()
    }
    unsafe extern "C" fn noop_job_step(_: *mut c_void, _: u64, _: *mut AriaJobStep) -> AriaResult {
        AriaResult::ok_empty()
    }
    unsafe extern "C" fn noop_job_end(_: *mut c_void, _: u64, _: u32) -> AriaResult {
        AriaResult::ok_empty()
    }
    unsafe extern "C" fn noop_dec_vid(
        _: *mut c_void,
        _: AriaStr,
        _: AriaStr,
        _: *mut AriaVideoInfo,
    ) -> AriaInstanceResult {
        AriaInstanceResult::err("unused")
    }
    unsafe extern "C" fn noop_dec_seek(_: *mut c_void, _: u64, _: i64) -> AriaResult {
        AriaResult::ok_empty()
    }
    unsafe extern "C" fn noop_dec_next(
        _: *mut c_void,
        _: u64,
        _: *mut AriaVideoFrameOut,
    ) -> AriaResult {
        AriaResult::ok_empty()
    }
    unsafe extern "C" fn noop_dec_aud(
        _: *mut c_void,
        _: AriaStr,
        _: AriaStr,
        _: u32,
        _: u32,
    ) -> AriaInstanceResult {
        AriaInstanceResult::err("unused")
    }
    unsafe extern "C" fn noop_dec_read(
        _: *mut c_void,
        _: u64,
        _: *mut f32,
        _: usize,
        _: *mut usize,
    ) -> AriaResult {
        AriaResult::ok_empty()
    }
    unsafe extern "C" fn noop_scope(
        _: *mut c_void,
        _: AriaStr,
        _: *mut AriaScopeCall,
    ) -> AriaResult {
        AriaResult::ok_empty()
    }
    unsafe extern "C" fn noop_cpu_fx(
        _: *mut c_void,
        _: AriaStr,
        _: *mut AriaCpuEffectCall,
    ) -> AriaResult {
        AriaResult::ok_empty()
    }
    unsafe extern "C" fn noop_driver(
        _: *mut c_void,
        _: AriaStr,
        _: AriaStr,
        _: *mut AriaDriverEvalCall,
    ) -> AriaResult {
        AriaResult::ok_empty()
    }
    unsafe extern "C" fn noop_an_push(
        _: *mut c_void,
        _: u64,
        _: *const f32,
        _: usize,
    ) -> AriaResult {
        AriaResult::ok_empty()
    }

    unsafe extern "C" fn mount_open_panic(host_ctx: *mut c_void) -> AriaResult {
        let spec = r#"{"id":"example.pan.open","label":"Open Panic","scope":"sequence"}"#;
        let res = unsafe { (HOST_V1.register_analyzer)(host_ctx, AriaStr::from_str(spec)) };
        unsafe { (HOST_V1.free_string)(res.value) };
        AriaResult::ok_empty()
    }
    unsafe extern "C" fn analyzer_open_panic(
        _: *mut c_void,
        _: AriaStr,
        _: u32,
        _: u32,
    ) -> AriaInstanceResult {
        OPEN_CALLS.fetch_add(1, Ordering::SeqCst);
        AriaInstanceResult::panic("boom in analyzer_open")
    }

    unsafe extern "C" fn mount_drop_panic(host_ctx: *mut c_void) -> AriaResult {
        let spec = r#"{"id":"example.pan.drop","label":"Drop Panic","scope":"sequence"}"#;
        let res = unsafe { (HOST_V1.register_analyzer)(host_ctx, AriaStr::from_str(spec)) };
        unsafe { (HOST_V1.free_string)(res.value) };
        AriaResult::ok_empty()
    }
    unsafe extern "C" fn analyzer_open_ok(
        _: *mut c_void,
        _: AriaStr,
        _: u32,
        _: u32,
    ) -> AriaInstanceResult {
        CANCEL_ON_OPEN.store(true, Ordering::SeqCst);
        AriaInstanceResult::ok(7)
    }
    unsafe extern "C" fn analyzer_drop_panic(_: *mut c_void, _: u64) -> AriaResult {
        DROP_CALLS.fetch_add(1, Ordering::SeqCst);
        AriaResult::panic("boom in analyzer_drop")
    }

    fn base_table() -> AriaPluginV1 {
        AriaPluginV1 {
            struct_size: std::mem::size_of::<AriaPluginV1>(),
            api_major: ARIA_API_MAJOR,
            reserved: 0,
            free_string: free_str,
            free_bytes,
            mount: mount_open_panic,
            unmount: noop_unmount,
            command_run: noop_cmd,
            event: noop_cmd,
            import_file: noop_path,
            export_file: noop_export,
            audio_create: noop_audio_create,
            audio_process: noop_audio_proc,
            audio_reset: noop_u64,
            audio_drop: noop_u64,
            panel_view: noop_bytes,
            panel_event: noop_cmd,
            encoder_open: noop_enc_open,
            encoder_push_frame: noop_enc_frame,
            encoder_push_audio: noop_enc_audio,
            encoder_finish: noop_u64,
            encoder_drop: noop_u64,
            overlay_scene: noop_bytes,
            overlay_event: noop_cmd,
            job_step: noop_job_step,
            job_end: noop_job_end,
            decoder_probe: noop_path,
            decoder_open_video: noop_dec_vid,
            decoder_seek: noop_dec_seek,
            decoder_next_frame: noop_dec_next,
            decoder_open_audio: noop_dec_aud,
            decoder_read_audio: noop_dec_read,
            decoder_seek_audio: noop_dec_seek,
            decoder_close: noop_u64,
            ai_tool_call: noop_cmd,
            scope_measure: noop_scope,
            cpu_effect_process: noop_cpu_fx,
            driver_evaluate: noop_driver,
            analyzer_open: analyzer_open_panic,
            analyzer_push: noop_an_push,
            analyzer_finish: noop_u64,
            analyzer_drop: noop_u64,
        }
    }

    fn make_plugin(id: &str, table: AriaPluginV1) -> (tempfile::TempDir, NativePlugin) {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("plugin.toml"),
            format!("[plugin]\nid = {id:?}\nname = \"Test\"\nversion = \"0.1.0\"\napi = 1\n"),
        )
        .expect("write manifest");
        let package = Package::open(dir.path()).expect("open package");
        let manifest = package.plugin_manifest();
        let state = Arc::new(NativePluginState {
            id: id.to_owned(),
            package: package.clone(),
            data_root: None,
            table,
            mounted: AtomicBool::new(false),
            failed: AtomicBool::new(false),
            next_job: AtomicU64::new(1),
            _library: None,
        });
        (
            dir,
            NativePlugin {
                package,
                manifest,
                state,
            },
        )
    }

    /// WHY: Closes the native C-ABI callback panic gap on `analyzer_open` and
    /// `analyzer_drop`: a panic in `analyzer_open` must mark `NativePluginState`
    /// failed before `ctx.fail` drops the `on_unmount` guard (so `unmount` is
    /// never called on a panicked plugin and retained handles refuse re-entry),
    /// and a panic in `analyzer_drop` on a cancelled run must fault and unmount
    /// the plugin without calling `unmount`. Does not cover WASM traps, which
    /// `aria_plugin_wasm` tests exercise.
    #[test]
    fn analyzer_open_and_drop_panics_fault_plugin_without_calling_unmount() {
        UNMOUNT_CALLS.store(0, Ordering::SeqCst);
        OPEN_CALLS.store(0, Ordering::SeqCst);
        DROP_CALLS.store(0, Ordering::SeqCst);

        let (_dir1, plugin1) = make_plugin("example.pan.open", base_table());
        let state1 = Arc::clone(&plugin1.state);
        let ctx1 = PluginContext::new();
        ctx1.mount(&plugin1).expect("mount open-panic plugin");
        let retained = ctx1.analyzer("example.pan.open").expect("registered");

        let project = Project::new("T", Rational::FPS_24);
        let range = TimeRange::new(
            RationalTime::new(0, Rational::FPS_24),
            RationalTime::new(24, Rational::FPS_24),
        )
        .expect("valid range");
        let res = ctx1.run_analyzer(
            "example.pan.open",
            &project,
            AudioSource::Sequence(None),
            range,
            &AtomicBool::new(false),
        );
        assert!(matches!(res, Err(AnalyzerError::Panicked { .. })));
        assert!(
            state1.is_failed(),
            "analyzer_open panic must mark state failed"
        );
        assert_eq!(
            UNMOUNT_CALLS.load(Ordering::SeqCst),
            0,
            "unmount must not run on a plugin that panicked in analyzer_open"
        );
        assert!(ctx1.analyzer("example.pan.open").is_none());
        let _ = retained.run_with_progress(
            &ctx1,
            &project,
            AudioSource::Sequence(None),
            range,
            &AtomicBool::new(false),
            |_, _| {},
        );
        assert_eq!(
            OPEN_CALLS.load(Ordering::SeqCst),
            1,
            "retained handle must not re-enter panicked analyzer_open"
        );

        let mut drop_table = base_table();
        drop_table.mount = mount_drop_panic;
        drop_table.analyzer_open = analyzer_open_ok;
        drop_table.analyzer_drop = analyzer_drop_panic;
        let (_dir2, plugin2) = make_plugin("example.pan.drop", drop_table);
        let state2 = Arc::clone(&plugin2.state);
        let ctx2 = PluginContext::new();
        ctx2.mount(&plugin2).expect("mount drop-panic plugin");

        CANCEL_ON_OPEN.store(false, Ordering::SeqCst);
        let res2 = ctx2.run_analyzer(
            "example.pan.drop",
            &project,
            AudioSource::Sequence(None),
            range,
            &CANCEL_ON_OPEN,
        );
        assert!(matches!(res2, Err(AnalyzerError::Cancelled)));
        assert_eq!(DROP_CALLS.load(Ordering::SeqCst), 1);
        assert!(
            state2.is_failed(),
            "analyzer_drop panic must mark state failed"
        );
        assert!(
            ctx2.analyzer("example.pan.drop").is_none(),
            "analyzer_drop panic must unmount registrations"
        );
        assert!(
            ctx2.failures()
                .iter()
                .any(|f| f.id == "example.pan.drop" && f.reason.contains("analyzer_drop panicked")),
            "failures must record analyzer_drop panic: {:?}",
            ctx2.failures()
        );
        assert_eq!(
            UNMOUNT_CALLS.load(Ordering::SeqCst),
            0,
            "unmount must not run on a plugin that panicked in analyzer_drop"
        );
    }
}
