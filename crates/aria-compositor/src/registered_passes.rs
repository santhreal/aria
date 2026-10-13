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
//! An effect pass binds `input_tex` (the previous pass's output, or the effect's
//! input for the first pass), `source_tex` (the effect's input), `aria_sampler`
//! and the uniform `aria: AriaPass`. A transition binds `outgoing_tex`,
//! `incoming_tex`, `aria_sampler` and `aria: AriaTransition`. Both kinds get
//! `struct VsOut { pos, uv }` and the vertex stage `vs`; the plugin writes
//! `@fragment fn fs` returning `@location(0) vec4<f32>`. Textures hold
//! premultiplied RGBA, and `fs` returns premultiplied RGBA.
//!
//! Each effect parameter gets an accessor named `param_<name>`: `f32` for a
//! scalar, `vec2<f32>` for a vec2, `vec4<f32>` RGBA in 0..1 for a colour.

use std::fmt::Write as _;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, PoisonError, RwLock};

use aria_models::{EffectParamKind, EffectParamSpec, EffectSpec, TransitionSpec};
use rustc_hash::FxHashMap;

/// Most floats the parameters of one registered effect flatten to.
pub const MAX_EFFECT_FLOATS: usize = 256;

/// Most passes one registered effect runs.
pub const MAX_EFFECT_PASSES: usize = 8;

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
    /// WGSL for each pass, in dispatch order. Each one declares `fs`.
    pub passes: Vec<String>,
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

/// A registered pass as the compositor draws it.
pub struct RegisteredPass {
    serial: u64,
    id: String,
    float_count: usize,
    /// Plugin source plus host prelude, one per pass.
    sources: Box<[String]>,
    removed: AtomicBool,
    failure: Mutex<Option<String>>,
}

impl std::fmt::Debug for RegisteredPass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RegisteredPass")
            .field("id", &self.id)
            .field("serial", &self.serial)
            .field("passes", &self.sources.len())
            .field("float_count", &self.float_count)
            .finish_non_exhaustive()
    }
}

impl RegisteredPass {
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Passes the effect runs; 1 for a transition.
    pub fn pass_count(&self) -> usize {
        self.sources.len()
    }

    /// Floats the instance parameters flatten to; 0 for a transition.
    pub fn float_count(&self) -> usize {
        self.float_count
    }

    /// Whether the registration was dropped. A removed pass draws nothing.
    pub fn is_removed(&self) -> bool {
        self.removed.load(Ordering::Acquire)
    }

    /// The device error that kept a pipeline from building, if one did.
    pub fn failure(&self) -> Option<String> {
        self.failure
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub(crate) fn serial(&self) -> u64 {
        self.serial
    }

    pub(crate) fn sources(&self) -> &[String] {
        &self.sources
    }

    pub(crate) fn record_failure(&self, message: String) {
        *self.failure.lock().unwrap_or_else(PoisonError::into_inner) = Some(message);
    }
}

type Table = RwLock<FxHashMap<String, Arc<RegisteredPass>>>;

static EFFECTS: LazyLock<Table> = LazyLock::new(Table::default);
static TRANSITIONS: LazyLock<Table> = LazyLock::new(Table::default);
static NEXT_SERIAL: AtomicU64 = AtomicU64::new(1);

fn lookup(table: &Table, id: &str) -> Option<Arc<RegisteredPass>> {
    table
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .get(id)
        .cloned()
}

/// The registered effect `id`, or `None`.
pub(crate) fn registered_effect(id: &str) -> Option<Arc<RegisteredPass>> {
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
        let mut table = self
            .kind
            .table()
            .write()
            .unwrap_or_else(PoisonError::into_inner);
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
/// Refused when a pass does not parse or validate, does not declare
/// `@fragment fn fs` returning `@location(0) vec4<f32>`, reads a vertex output
/// the host does not write, or declares a binding of its own; when a parameter
/// name is not a WGSL identifier; when there are no passes or more than
/// [`MAX_EFFECT_PASSES`]; when the parameters flatten to more than
/// [`MAX_EFFECT_FLOATS`]; and by every rule of [`aria_models::register_effect`].
pub fn register_effect(definition: EffectDefinition) -> Result<PassRegistration, RegisterError> {
    let EffectDefinition { spec, passes } = definition;
    let refuse = |message: String| RegisterError {
        id: spec.id.clone(),
        pass: None,
        location: None,
        message,
    };
    if passes.is_empty() {
        return Err(refuse("an effect needs at least one pass".into()));
    }
    if passes.len() > MAX_EFFECT_PASSES {
        return Err(refuse(format!(
            "{} passes; an effect runs at most {MAX_EFFECT_PASSES}",
            passes.len()
        )));
    }
    if let Some(param) = spec.params.iter().find(|p| !is_identifier(&p.name)) {
        return Err(refuse(format!(
            "parameter `{}` is not a WGSL identifier (ASCII letters, digits, `_`)",
            param.name
        )));
    }
    let float_count: usize = spec.params.iter().map(|p| floats_of(p.kind)).sum();
    if float_count > MAX_EFFECT_FLOATS {
        return Err(refuse(format!(
            "the parameters flatten to {float_count} floats; an effect holds at most \
             {MAX_EFFECT_FLOATS}"
        )));
    }
    let prelude = effect_prelude(&spec.params, float_count);
    let sources = passes
        .iter()
        .enumerate()
        .map(|(index, source)| {
            check_pass(source, &prelude, Kind::Effect).map_err(|(location, message)| {
                RegisterError {
                    id: spec.id.clone(),
                    pass: Some(index),
                    location,
                    message,
                }
            })
        })
        .collect::<Result<Box<[String]>, _>>()?;
    let id = spec.id.clone();
    let (catalog, _) = aria_models::register_effect(spec).map_err(|message| RegisterError {
        id: id.clone(),
        pass: None,
        location: None,
        message,
    })?;
    Ok(insert(Kind::Effect, id, float_count, sources, catalog))
}

/// Validate `definition`, add it to the transition catalog, and make it
/// renderable until the returned registration drops. Refused by the shader
/// rules of [`register_effect`] and by [`aria_models::register_transition`].
pub fn register_transition(
    definition: TransitionDefinition,
) -> Result<PassRegistration, RegisterError> {
    let TransitionDefinition { spec, shader } = definition;
    let source = check_pass(&shader, &transition_prelude(), Kind::Transition).map_err(
        |(location, message)| RegisterError {
            id: spec.id.clone(),
            pass: Some(0),
            location,
            message,
        },
    )?;
    let id = spec.id.clone();
    let (catalog, _) = aria_models::register_transition(spec).map_err(|message| RegisterError {
        id: id.clone(),
        pass: None,
        location: None,
        message,
    })?;
    Ok(insert(Kind::Transition, id, 0, Box::new([source]), catalog))
}

fn insert(
    kind: Kind,
    id: String,
    float_count: usize,
    sources: Box<[String]>,
    catalog: aria_models::Registered,
) -> PassRegistration {
    let pass = Arc::new(RegisteredPass {
        serial: NEXT_SERIAL.fetch_add(1, Ordering::Relaxed),
        id: id.clone(),
        float_count,
        sources,
        removed: AtomicBool::new(false),
        failure: Mutex::new(None),
    });
    kind.table()
        .write()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(id, Arc::clone(&pass));
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
    match kind {
        EffectParamKind::Scalar => 1,
        EffectParamKind::Vec2 => 2,
        EffectParamKind::Color => 4,
    }
}

/// `vec4` slots in an effect's `aria.params`; at least one, since WGSL has no
/// empty array.
pub(crate) fn param_vec4s(float_count: usize) -> usize {
    float_count.div_ceil(4).max(1)
}

const VERTEX_STAGE: &str = "
struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs(@builtin(vertex_index) vi: u32) -> VsOut {
    var positions = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(-1.0, 1.0),
        vec2<f32>(-1.0, 1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
    );
    var uvs = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 1.0), vec2<f32>(0.0, 0.0),
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 1.0), vec2<f32>(1.0, 0.0),
    );
    var out: VsOut;
    out.pos = vec4<f32>(positions[vi], 0.0, 1.0);
    out.uv = uvs[vi];
    return out;
}
";

/// The prelude of an effect whose parameters are `params`.
fn effect_prelude(params: &[EffectParamSpec], float_count: usize) -> String {
    let mut out = format!(
        "// Aria host prelude.
struct AriaPass {{
    size: vec2<f32>,
    texel: vec2<f32>,
    time: f32,
    pass_index: u32,
    pass_count: u32,
    pad: u32,
    params: array<vec4<f32>, {}>,
}}

@group(0) @binding(0) var input_tex: texture_2d<f32>;
@group(0) @binding(1) var source_tex: texture_2d<f32>;
@group(0) @binding(2) var aria_sampler: sampler;
@group(0) @binding(3) var<uniform> aria: AriaPass;
{VERTEX_STAGE}",
        param_vec4s(float_count)
    );
    let component = |index: usize| {
        let lane = ["x", "y", "z", "w"][index % 4];
        format!("aria.params[{}].{lane}", index / 4)
    };
    let mut offset = 0usize;
    for param in params {
        let name = &param.name;
        let _ = match param.kind {
            EffectParamKind::Scalar => writeln!(
                out,
                "fn param_{name}() -> f32 {{ return {}; }}",
                component(offset)
            ),
            EffectParamKind::Vec2 => writeln!(
                out,
                "fn param_{name}() -> vec2<f32> {{ return vec2<f32>({}, {}); }}",
                component(offset),
                component(offset + 1)
            ),
            EffectParamKind::Color => writeln!(
                out,
                "fn param_{name}() -> vec4<f32> {{ return vec4<f32>({}, {}, {}, {}); }}",
                component(offset),
                component(offset + 1),
                component(offset + 2),
                component(offset + 3)
            ),
        };
        offset += floats_of(param.kind);
    }
    out
}

fn transition_prelude() -> String {
    format!(
        "// Aria host prelude.
struct AriaTransition {{
    size: vec2<f32>,
    texel: vec2<f32>,
    progress: f32,
    pad0: f32,
    pad1: f32,
    pad2: f32,
}}

@group(0) @binding(0) var outgoing_tex: texture_2d<f32>;
@group(0) @binding(1) var incoming_tex: texture_2d<f32>;
@group(0) @binding(2) var aria_sampler: sampler;
@group(0) @binding(3) var<uniform> aria: AriaTransition;
{VERTEX_STAGE}"
    )
}

type Refusal = (Option<(u32, u32)>, String);

/// Compose `source` with `prelude` and check it, returning the composed WGSL.
fn check_pass(source: &str, prelude: &str, kind: Kind) -> Result<String, Refusal> {
    let composed = format!("{source}\n{prelude}");
    let source_lines = u32::try_from(source.lines().count()).unwrap_or(u32::MAX);
    let place = |location: Option<naga::SourceLocation>, message: String| -> Refusal {
        match location {
            Some(at) if at.line_number <= source_lines => {
                (Some((at.line_number, at.line_position)), message)
            }
            Some(_) => (None, format!("in the host prelude: {message}")),
            None => (None, message),
        }
    };
    let module = naga::front::wgsl::parse_str(&composed)
        .map_err(|error| place(error.location(&composed), error.to_string()))?;
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .map_err(|error| place(error.location(&composed), error_chain(&error)))?;
    check_interface(&module, kind).map_err(|message| (None, message))?;
    Ok(composed)
}

/// The error and every cause beneath it, joined.
fn error_chain(error: &dyn std::error::Error) -> String {
    let mut message = error.to_string();
    let mut cause = error.source();
    while let Some(next) = cause {
        let _ = write!(message, ": {next}");
        cause = next.source();
    }
    message
}

/// The fragment entry, its inputs and output, and the bindings.
fn check_interface(module: &naga::Module, kind: Kind) -> Result<(), String> {
    let fs = module
        .entry_points
        .iter()
        .find(|entry| entry.name == "fs" && entry.stage == naga::ShaderStage::Fragment)
        .ok_or("no `@fragment fn fs`; the host draws a pass with `vs` and `fs`")?;
    let is_vector = |ty: naga::Handle<naga::Type>, size: naga::VectorSize| {
        matches!(
            module.types[ty].inner,
            naga::TypeInner::Vector { size: s, scalar } if s == size && scalar == naga::Scalar::F32
        )
    };
    let returns_colour = fs.function.result.as_ref().is_some_and(|result| {
        matches!(
            result.binding,
            Some(naga::Binding::Location { location: 0, .. })
        ) && is_vector(result.ty, naga::VectorSize::Quad)
    });
    if !returns_colour {
        return Err("`fs` must return `@location(0) vec4<f32>`".into());
    }
    let vs_out = module
        .types
        .iter()
        .find(|(_, ty)| ty.name.as_deref() == Some("VsOut"))
        .map(|(handle, _)| handle);
    for argument in &fs.function.arguments {
        let reads_host_output = match &argument.binding {
            None => Some(argument.ty) == vs_out,
            Some(naga::Binding::BuiltIn(_)) => true,
            Some(naga::Binding::Location { location: 0, .. }) => {
                is_vector(argument.ty, naga::VectorSize::Bi)
            }
            Some(naga::Binding::Location { .. }) => false,
        };
        if !reads_host_output {
            return Err(format!(
                "`fs` argument `{}` reads a value the host vertex stage does not write; \
                 take `in: VsOut`, or `@location(0) uv: vec2<f32>` and builtins",
                argument.name.as_deref().unwrap_or("_")
            ));
        }
    }
    let host = kind.host_bindings();
    for (_, global) in module.global_variables.iter() {
        let name = global.name.as_deref().unwrap_or("_");
        if let Some(binding) = &global.binding {
            if !host.contains(&name) {
                return Err(format!(
                    "`{name}` is bound at @group({}) @binding({}); a pass binds only {}",
                    binding.group,
                    binding.binding,
                    host.join(", ")
                ));
            }
        }
        if matches!(global.space, naga::AddressSpace::Immediate) {
            return Err(format!(
                "`{name}` is an immediate; a pass reads constants from `aria`"
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "registered_passes_tests.rs"]
mod tests;
