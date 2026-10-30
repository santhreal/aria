//! Effects and transitions whose WGSL arrives while the process runs.
//!
//! A plugin supplies a catalog spec and WGSL source. Registration parses and
//! validates every pass with naga against the host prelude, adds the spec to the
//! model catalog, and adds the pass here; the returned [`PassRegistration`]
//! removes both when dropped. The compositor builds a pipeline for an entry the
//! first time a frame draws it.
//!
//! # Source layout
//!
//! A pass compiles as the plugin's WGSL followed by a host prelude. WGSL module
//! declarations are order-independent, so plugin code uses prelude names freely,
//! and a line number in a refusal is a line of the plugin's own source.
//!
//! An effect is a graph of passes ([`GraphPass`]): each pass names the textures
//! it reads and the size it writes, and runs as a fragment pass declaring
//! `@fragment fn fs` or a compute pass declaring `@compute fn cs`. A pass binds
//! `input_tex` (its first input), `source_tex` (the effect's input),
//! `aria_sampler`, the storage buffer `aria: AriaPass`, and one texture per
//! further input, named by [`aria_models::PassInput::binding_name`]; a compute
//! pass also binds `output_tex`. A transition binds `outgoing_tex`,
//! `incoming_tex`, `aria_sampler` and `aria: AriaTransition`. Every pass gets
//! `struct VsOut { pos, uv }` and the vertex stage `vs`. Textures hold
//! premultiplied RGBA, and a pass writes premultiplied RGBA.
//!
//! Each effect parameter gets an accessor named `param_<name>`: `f32` for a
//! scalar, `vec2<f32>` for a vec2, `vec4<f32>` RGBA in 0..1 for a colour.

use std::ops::Range;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use aria_models::{
    EffectBounds, EffectParamKind, EffectParamSpec, EffectSpec, GraphPass, TransitionSpec,
};
use parking_lot::{Mutex, RwLock};
use rustc_hash::FxHashMap;

use crate::cpu_effect::{
    CpuEffect, CpuEffectDefinition, CpuFrameIn, CpuFrameOut, EffectCostSink, EffectFault,
    EffectFaultKind, EffectFaultSink, EffectParams, EffectRender, RegisteredParam,
};
use crate::registered_graph::{
    CompiledInput, CompiledPass, Entry, Refusal, check_composed, compile_effect, digest,
    transition_prelude,
};

/// Most floats the parameters of one registered effect flatten to.
pub const MAX_EFFECT_FLOATS: usize = 256;

/// Most passes one registered effect runs.
pub const MAX_EFFECT_PASSES: usize = aria_models::MAX_GRAPH_PASSES;

/// Words before the parameters in an effect pass uniform: size, texel, time,
/// pass index, pass count, padding.
pub(crate) const EFFECT_HEADER_WORDS: usize = 8;

/// Words in a transition uniform: size, texel, progress, padding.
pub(crate) const TRANSITION_WORDS: usize = 8;

/// An effect a plugin renders with its own WGSL.
#[derive(Debug, Clone, PartialEq)]
pub struct EffectDefinition {
    /// The catalog entry: a namespaced id, a label, and the parameters in the
    /// order they flatten into `aria.params`.
    pub spec: EffectSpec,
    /// The passes, the last of which writes the effect's frame. A plain
    /// string converts to a fragment pass reading the pass before it.
    pub passes: Vec<GraphPass>,
}

impl EffectDefinition {
    /// An effect or generator rendered on the CPU by `effect`.
    pub fn cpu(spec: EffectSpec, effect: impl CpuEffect + 'static) -> CpuEffectDefinition {
        CpuEffectDefinition::new(spec, effect)
    }
}

/// A transition a plugin renders with its own WGSL.
#[derive(Debug, Clone, PartialEq)]
pub struct TransitionDefinition {
    /// The catalog entry: a namespaced id and a label.
    pub spec: TransitionSpec,
    /// WGSL declaring `fs`.
    pub shader: String,
}

/// Why a definition was not registered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisterError {
    /// The id the definition declared.
    pub id: String,
    /// Index of the refused pass, or `None` when the spec was refused.
    pub pass: Option<usize>,
    /// One-based line and column in that pass's source, when the refusal points
    /// at one.
    pub location: Option<(u32, u32)>,
    pub message: String,
}

impl std::fmt::Display for RegisterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.id)?;
        if let Some(pass) = self.pass {
            write!(f, " pass {pass}")?;
        }
        if let Some((line, column)) = self.location {
            write!(f, " at {line}:{column}")?;
        }
        write!(f, ": {}", self.message)
    }
}

impl std::error::Error for RegisterError {}

enum PassBackend {
    /// Compiled passes in execution order.
    Shader(Box<[CompiledPass]>),
    /// CPU callback invoked off the GPU queue.
    Cpu {
        effect: Arc<dyn CpuEffect>,
        params: Box<[RegisteredParam]>,
        generator: bool,
    },
}

/// A registered pass as the compositor draws it.
///
/// When a cost sink is attached, CPU effects record the callback plus `u8 <-> f16`
/// staging conversion and texture upload, and WGSL effects/transitions record
/// CPU-side uniform preparation, bind-group lookup, and render-pass encoding
/// across the pass list (the compositor device does not enable GPU timestamp
/// queries).
pub struct RegisteredPass {
    serial: u64,
    id: String,
    flags: PassFlags,
    digest: u64,
    backend: PassBackend,
    on_fault: Option<EffectFaultSink>,
    on_cost: Option<EffectCostSink>,
    removed: AtomicBool,
    failure: Mutex<Option<String>>,
}

impl std::fmt::Debug for RegisteredPass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RegisteredPass")
            .field("id", &self.id)
            .field("digest", &self.digest)
            .field("stateful", &self.flags.stateful)
            .field("time_dependent", &self.flags.time_dependent)
            .field("reads_outside", &self.flags.reads_outside)
            .field("bounds", &self.flags.bounds)
            .field("passes", &self.pass_count())
            .field("cpu", &self.is_cpu())
            .field("float_count", &self.flags.float_count)
            .finish_non_exhaustive()
    }
}

impl RegisteredPass {
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Passes the effect runs; 1 for a CPU effect or a transition.
    pub fn pass_count(&self) -> usize {
        match &self.backend {
            PassBackend::Shader(passes) => passes.len(),
            PassBackend::Cpu { .. } => 1,
        }
    }

    /// Whether this effect renders through a [`CpuEffect`] callback rather
    /// than WGSL passes.
    pub fn is_cpu(&self) -> bool {
        matches!(self.backend, PassBackend::Cpu { .. })
    }

    /// Whether this effect holds state across frames (`spec.stateful == true`).
    pub fn is_stateful(&self) -> bool {
        self.flags.stateful
    }

    /// Whether this effect reads the time (`spec.time_dependent == true`). An
    /// effect that does not is handed time 0, so its frames at equal
    /// parameters are equal.
    pub fn is_time_dependent(&self) -> bool {
        self.flags.time_dependent
    }

    /// Whether this effect samples its input outside `0.0..=1.0`
    /// (`spec.reads_outside == true`).
    pub fn reads_outside(&self) -> bool {
        self.flags.reads_outside
    }

    /// How far this effect draws past its layer's quad, in pixels
    /// (`spec.bounds`); zero for an effect that stays inside it.
    pub fn bounds(&self) -> EffectBounds {
        self.flags.bounds
    }

    /// Whether a pass of this effect binds the frame of the clip the layer
    /// parameter `param` references.
    pub fn reads_layer(&self, param: &str) -> bool {
        self.compiled().iter().any(|pass| {
            pass.inputs
                .iter()
                .any(|input| matches!(input, CompiledInput::Layer(name) if &**name == param))
        })
    }

    /// Deterministic 64-bit FNV-1a digest of what the passes draw: sources,
    /// stages, sizes, inputs, and texture pixels (`0` for CPU passes).
    pub fn digest(&self) -> u64 {
        self.digest
    }

    /// Whether this CPU effect is a generator (`spec.generator == true`).
    pub fn is_generator(&self) -> bool {
        matches!(
            self.backend,
            PassBackend::Cpu {
                generator: true,
                ..
            }
        )
    }

    /// Floats the instance parameters flatten to; 0 for a transition.
    pub fn float_count(&self) -> usize {
        self.flags.float_count
    }

    /// Default flattened float parameters for this pass.
    pub fn default_params(&self) -> &[f32] {
        &self.flags.default_params
    }

    /// Float slot ranges in [`crate::PassInstance::params`] for parameters
    /// declared with `unit = "px"`.
    pub fn canvas_px_float_ranges(&self) -> &[Range<usize>] {
        &self.flags.px_float_ranges
    }

    /// Whether the registration was dropped. A removed pass draws nothing.
    pub fn is_removed(&self) -> bool {
        self.removed.load(Ordering::Acquire)
    }

    /// The device or panic error that kept the pass from drawing, if one did.
    pub fn failure(&self) -> Option<String> {
        self.failure.lock().clone()
    }

    pub(crate) fn serial(&self) -> u64 {
        self.serial
    }

    /// The compiled passes in execution order; empty for a CPU effect.
    pub(crate) fn compiled(&self) -> &[CompiledPass] {
        match &self.backend {
            PassBackend::Shader(passes) => passes,
            PassBackend::Cpu { .. } => &[],
        }
    }

    pub(crate) fn record_failure(&self, message: String) {
        *self.failure.lock() = Some(message);
    }

    /// Run the CPU callback on `input` and `output`. Returns `true` when
    /// `output` was written; on `Err` or panic reports the fault to `on_fault`
    /// and returns `false` so the caller leaves the input frame as passthrough.
    pub fn run_cpu(
        &self,
        input: CpuFrameIn<'_>,
        output: &mut CpuFrameOut<'_>,
        values: &[f32],
        time: f32,
    ) -> bool {
        self.run_cpu_with_instance(input, output, values, None, time)
    }

    /// Run the CPU callback on `input` and `output` with an optional
    /// [`aria_models::EffectInstance`] for non-float parameter kinds.
    pub fn run_cpu_with_instance(
        &self,
        input: CpuFrameIn<'_>,
        output: &mut CpuFrameOut<'_>,
        values: &[f32],
        instance: Option<&aria_models::EffectInstance>,
        time: f32,
    ) -> bool {
        if self.is_removed() {
            return false;
        }
        let PassBackend::Cpu { effect, params, .. } = &self.backend else {
            return false;
        };
        let effect_params = EffectParams::with_instance(params, values, instance);
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            effect.process(input, output, &effect_params, time)
        }));
        match outcome {
            Ok(Ok(())) => true,
            Ok(Err(message)) => {
                let fault = EffectFault {
                    effect_id: self.id.clone(),
                    kind: EffectFaultKind::Failed(message),
                };
                tracing::warn!(%fault, "cpu effect returned error; passing frame through");
                if let Some(sink) = &self.on_fault {
                    sink(&fault);
                }
                false
            }
            Err(payload) => {
                self.removed.store(true, Ordering::Release);
                let message = panic_message(payload.as_ref());
                let fault = EffectFault {
                    effect_id: self.id.clone(),
                    kind: EffectFaultKind::Panicked(message.clone()),
                };
                tracing::error!(
                    %fault,
                    "cpu effect panicked; failing plugin and passing frame through"
                );
                self.record_failure(format!("panic: {message}"));
                if let Some(sink) = &self.on_fault {
                    sink(&fault);
                }
                false
            }
        }
    }

    /// Whether this pass records per-frame cost or enforces `frame_budget`.
    pub(crate) fn tracks_cost(&self, frame_budget: Option<Duration>) -> bool {
        self.on_cost.is_some() || (frame_budget.is_some() && self.on_fault.is_some())
    }

    /// Record `took` to `on_cost` and, when `took` exceeds `frame_budget`,
    /// report an [`EffectFaultKind::MissedDeadline`] fault to `on_fault` without
    /// unmounting the plugin.
    pub(crate) fn record_cost(&self, took: Duration, frame_budget: Option<Duration>) {
        if let Some(sink) = &self.on_cost {
            sink(took);
        }
        if let Some(budget) = frame_budget
            && took > budget
            && let Some(sink) = &self.on_fault
        {
            let fault = EffectFault {
                effect_id: self.id.clone(),
                kind: EffectFaultKind::MissedDeadline { budget, took },
            };
            sink(&fault);
        }
    }
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|text| (*text).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "a panic with no message".to_owned())
}

type Table = RwLock<FxHashMap<String, Arc<RegisteredPass>>>;

static EFFECTS: LazyLock<Table> = LazyLock::new(Table::default);
static TRANSITIONS: LazyLock<Table> = LazyLock::new(Table::default);
static NEXT_SERIAL: AtomicU64 = AtomicU64::new(1);

fn lookup(table: &Table, id: &str) -> Option<Arc<RegisteredPass>> {
    table.read().get(id).cloned()
}

/// The registered effect `id`, or `None`.
pub fn registered_effect(id: &str) -> Option<Arc<RegisteredPass>> {
    lookup(&EFFECTS, id)
}

/// The registered transition `id`, or `None`.
pub(crate) fn registered_transition(id: &str) -> Option<Arc<RegisteredPass>> {
    lookup(&TRANSITIONS, id)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Effect,
    Transition,
}

impl Kind {
    fn table(self) -> &'static Table {
        match self {
            Kind::Effect => &EFFECTS,
            Kind::Transition => &TRANSITIONS,
        }
    }

    /// The globals the prelude binds, by name.
    fn host_bindings(self) -> [&'static str; 4] {
        match self {
            Kind::Effect => ["input_tex", "source_tex", "aria_sampler", "aria"],
            Kind::Transition => ["outgoing_tex", "incoming_tex", "aria_sampler", "aria"],
        }
    }
}

/// A registered effect or transition. Dropping it removes the pass and its
/// catalog entry; instances already in a project draw nothing until an entry
/// with the same id is registered again.
#[must_use = "dropping the registration removes the entry"]
pub struct PassRegistration {
    pass: Arc<RegisteredPass>,
    kind: Kind,
    _catalog: aria_models::Registered,
}

impl std::fmt::Debug for PassRegistration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PassRegistration")
            .field("kind", &self.kind)
            .field("pass", &self.pass)
            .finish()
    }
}

impl PassRegistration {
    pub fn id(&self) -> &str {
        self.pass.id()
    }

    pub fn pass(&self) -> &Arc<RegisteredPass> {
        &self.pass
    }
}

impl Drop for PassRegistration {
    fn drop(&mut self) {
        self.pass.removed.store(true, Ordering::Release);
        let mut table = self.kind.table().write();
        if table
            .get(self.pass.id())
            .is_some_and(|held| held.serial == self.pass.serial)
        {
            table.remove(self.pass.id());
        }
    }
}

/// Validate `definition`, add it to the effect catalog, and make it renderable
/// until the returned registration drops.
///
/// Refused when a parameter name is not a WGSL identifier; by every rule of
/// [`aria_models::plan_graph`] (no passes, more than [`MAX_EFFECT_PASSES`], an
/// input naming nothing, a cycle, a pass the last pass does not read); when a
/// pass reads a texture that is not registered; when a pass does not parse or
/// validate, does not declare its entry point (`@fragment fn fs` returning
/// `@location(0) vec4<f32>`, or `@compute fn cs` with the declared workgroup
/// size), reads a vertex output the host does not write, or declares a binding
/// of its own; and by every rule of [`aria_models::register_effect`].
pub fn register_effect(
    definition: impl Into<EffectRender>,
) -> Result<PassRegistration, RegisterError> {
    register_effect_with_fault_sink(definition, None)
}

/// Register a [`CpuEffect`] with its catalog `spec` and optional `on_fault`
/// sink.
pub fn register_cpu_effect(
    spec: EffectSpec,
    effect: impl CpuEffect + 'static,
    on_fault: Option<EffectFaultSink>,
) -> Result<PassRegistration, RegisterError> {
    register_effect_with_fault_sink(CpuEffectDefinition::new(spec, effect), on_fault)
}

/// Validate `definition` (WGSL passes or CPU callback), add its spec to the
/// effect catalog, and make it renderable until the returned registration
/// drops.
pub fn register_effect_with_fault_sink(
    definition: impl Into<EffectRender>,
    on_fault: Option<EffectFaultSink>,
) -> Result<PassRegistration, RegisterError> {
    register_effect_with_sinks(definition, on_fault, None)
}

/// Validate `definition` (WGSL passes or CPU callback), add its spec to the
/// effect catalog, and make it renderable until the returned registration
/// drops, reporting faults to `on_fault` and per-frame costs to `on_cost`.
pub fn register_effect_with_sinks(
    definition: impl Into<EffectRender>,
    on_fault: Option<EffectFaultSink>,
    on_cost: Option<EffectCostSink>,
) -> Result<PassRegistration, RegisterError> {
    match definition.into() {
        EffectRender::Shader(EffectDefinition { spec, passes }) => {
            let refuse = |message: String| RegisterError {
                id: spec.id.clone(),
                pass: None,
                location: None,
                message,
            };
            if let Some(param) = spec.params.iter().find(|p| !is_identifier(&p.name)) {
                return Err(refuse(format!(
                    "parameter `{}` is not a WGSL identifier (ASCII letters, digits, `_`)",
                    param.name
                )));
            }
            let compiled = compile_effect(&spec, &passes)?;
            let id = spec.id.clone();
            let flags = PassFlags::of(&spec);
            let (catalog, _) =
                aria_models::register_effect(spec).map_err(|message| RegisterError {
                    id: id.clone(),
                    pass: None,
                    location: None,
                    message,
                })?;
            Ok(insert(
                Kind::Effect,
                id,
                flags,
                PassBackend::Shader(compiled),
                on_fault,
                on_cost,
                catalog,
            ))
        }
        EffectRender::Cpu(CpuEffectDefinition { spec, effect }) => {
            let params = registered_params(&spec.params);
            let generator = spec.generator;
            let id = spec.id.clone();
            let flags = PassFlags::of(&spec);
            let (catalog, _) =
                aria_models::register_effect(spec).map_err(|message| RegisterError {
                    id: id.clone(),
                    pass: None,
                    location: None,
                    message,
                })?;
            Ok(insert(
                Kind::Effect,
                id,
                flags,
                PassBackend::Cpu {
                    effect,
                    params,
                    generator,
                },
                on_fault,
                on_cost,
                catalog,
            ))
        }
    }
}

fn registered_params(params: &[EffectParamSpec]) -> Box<[RegisteredParam]> {
    let mut offset = 0usize;
    params
        .iter()
        .map(|param| {
            let slot = RegisteredParam {
                name: param.name.clone().into_boxed_str(),
                kind: param.kind,
                offset,
                choices: param
                    .choices
                    .iter()
                    .map(|c| c.id.clone().into_boxed_str())
                    .collect(),
                default_text: param.default_text.clone().into_boxed_str(),
                default_curve: param.default_curve_points().into_boxed_slice(),
                default_gradient: param.default_gradient_stops().into_boxed_slice(),
            };
            offset += floats_of(param.kind);
            slot
        })
        .collect()
}

/// Validate `definition`, add it to the transition catalog, and make it
/// renderable until the returned registration drops. Refused by the shader
/// rules of [`register_effect`] and by [`aria_models::register_transition`].
pub fn register_transition(
    definition: TransitionDefinition,
) -> Result<PassRegistration, RegisterError> {
    register_transition_with_sinks(definition, None, None)
}

/// Validate `definition`, add it to the transition catalog, and make it
/// renderable until the returned registration drops, reporting deadline faults
/// to `on_fault` and per-frame costs to `on_cost`.
pub fn register_transition_with_sinks(
    definition: TransitionDefinition,
    on_fault: Option<EffectFaultSink>,
    on_cost: Option<EffectCostSink>,
) -> Result<PassRegistration, RegisterError> {
    let TransitionDefinition { spec, shader } = definition;
    if let Some(param) = spec.params.iter().find(|p| !is_identifier(&p.name)) {
        return Err(RegisterError {
            id: spec.id.clone(),
            pass: None,
            location: None,
            message: format!(
                "parameter `{}` is not a WGSL identifier (ASCII letters, digits, `_`)",
                param.name
            ),
        });
    }
    let prelude = transition_prelude(&spec.params);
    let source =
        check_pass(&shader, &prelude, Kind::Transition).map_err(|(location, message)| {
            RegisterError {
                id: spec.id.clone(),
                pass: Some(0),
                location,
                message,
            }
        })?;
    let id = spec.id.clone();
    let flags = PassFlags::transition(&spec);
    let (catalog, _) = aria_models::register_transition(spec).map_err(|message| RegisterError {
        id: id.clone(),
        pass: None,
        location: None,
        message,
    })?;
    Ok(insert(
        Kind::Transition,
        id,
        flags,
        PassBackend::Shader(Box::new([CompiledPass::transition(source)])),
        on_fault,
        on_cost,
        catalog,
    ))
}

/// What a registered effect declares about its frames beyond its passes.
#[derive(Debug, Clone, Default)]
struct PassFlags {
    float_count: usize,
    default_params: Box<[f32]>,
    px_float_ranges: Box<[Range<usize>]>,
    stateful: bool,
    time_dependent: bool,
    reads_outside: bool,
    bounds: EffectBounds,
}

impl PassFlags {
    fn from_params(params: &[EffectParamSpec]) -> Self {
        Self {
            float_count: params.iter().map(|p| floats_of(p.kind)).sum(),
            default_params: aria_models::default_flattened_param_floats(params).into_boxed_slice(),
            px_float_ranges: aria_models::canvas_px_float_ranges(params).collect(),
            ..Self::default()
        }
    }

    fn of(spec: &EffectSpec) -> Self {
        Self {
            stateful: spec.stateful,
            time_dependent: spec.time_dependent,
            reads_outside: spec.reads_outside,
            bounds: spec.bounds,
            ..Self::from_params(&spec.params)
        }
    }

    fn transition(spec: &TransitionSpec) -> Self {
        Self::from_params(&spec.params)
    }
}

fn insert(
    kind: Kind,
    id: String,
    flags: PassFlags,
    backend: PassBackend,
    on_fault: Option<EffectFaultSink>,
    on_cost: Option<EffectCostSink>,
    catalog: aria_models::Registered,
) -> PassRegistration {
    let digest = match &backend {
        PassBackend::Shader(passes) => digest(passes),
        PassBackend::Cpu { .. } => 0,
    };
    let pass = Arc::new(RegisteredPass {
        serial: NEXT_SERIAL.fetch_add(1, Ordering::Relaxed),
        id: id.clone(),
        flags,
        digest,
        backend,
        on_fault,
        on_cost,
        removed: AtomicBool::new(false),
        failure: Mutex::new(None),
    });
    kind.table().write().insert(id, Arc::clone(&pass));
    PassRegistration {
        pass,
        kind,
        _catalog: catalog,
    }
}

fn is_identifier(name: &str) -> bool {
    !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

pub(crate) fn floats_of(kind: EffectParamKind) -> usize {
    kind.shader_floats()
}

/// `vec4` slots in an effect's `aria.params`; at least one, since WGSL has no
/// empty array.
pub(crate) fn param_vec4s(float_count: usize) -> usize {
    float_count.div_ceil(4).max(1)
}

/// Compose `source` with `prelude` and check it as a fragment pass of `kind`,
/// returning the composed WGSL.
fn check_pass(source: &str, prelude: &str, kind: Kind) -> Result<String, Refusal> {
    check_composed(source, prelude, &kind.host_bindings(), Entry::Fragment)
}

#[cfg(test)]
#[path = "registered_passes_tests.rs"]
mod tests;
