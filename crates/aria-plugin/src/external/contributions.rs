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
use aria_models::{
    ClipTypeIcon, ClipTypeSpec, EffectSpec, GraphPass, MarkerColor, ParamValue, Preset,
    TransitionSpec,
};
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
    let icon = raw.icon_svg.map(|svg| ClipTypeIcon {
        svg: Arc::from(svg),
        key: 0,
    });
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
    let preset: Preset =
        serde_json::from_str(preset_json).map_err(|err| format!("invalid preset JSON: {err}"))?;
    ctx.scoped_ctx
        .register_preset(preset)
        .map(drop)
        .map_err(|err| err.to_string())
}

pub(super) fn register_theme(
    ctx: &HostCallContext<'_>,
    id: &str,
    toml: &str,
) -> Result<(), String> {
    ctx.scoped_ctx
        .register_theme(id, toml)
        .map(drop)
        .map_err(|err| err.to_string())
}

pub(super) fn register_texture(
    ctx: &HostCallContext<'_>,
    id: &str,
    width: u32,
    height: u32,
    pixels: &[u8],
) -> Result<(), String> {
    let image = aria_core::RgbaImage::try_new(width, height, pixels.to_vec())
        .map_err(|err| format!("texture `{id}`: {err}"))?;
    ctx.scoped_ctx
        .register_texture(id, &image)
        .map(drop)
        .map_err(|err| err.to_string())
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
        let mut input_f16 = Vec::with_capacity(pixels_len);
        for y in 0..height {
            input_f16.extend_from_slice(input.row(y));
        }
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
                for (y, row) in output_f16.chunks_exact(row_elems).enumerate() {
                    output.row_mut(y as u32).copy_from_slice(row);
                }
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
        if source.trim().is_empty() {
            return Err("driver expression is empty".to_owned());
        }
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
        let err = self.state.take_guest_string(result.error);
        match result.status {
            ARIA_STATUS_OK => Box::new(NativeAnalyzerRun {
                instance: Some(result.instance),
                error: None,
                finished: false,
                state: Arc::clone(&self.state),
                scoped_ctx: self.scoped_ctx.clone(),
            }),
            ARIA_STATUS_PANIC => {
                resume_unwind(Box::new(err));
            }
            _ => Box::new(NativeAnalyzerRun {
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
            ARIA_STATUS_OK => serde_json::from_str(&text)
                .map_err(|err| format!("invalid analyzer result JSON: {err}")),
            ARIA_STATUS_PANIC => {
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
        let _ = self.state.take_guest_string(result.value);
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
