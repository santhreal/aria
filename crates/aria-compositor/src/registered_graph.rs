//! Compiling a WGSL effect graph: the host prelude each pass is checked
//! against, the entry-point and binding checks, and the compiled passes the
//! runtime draws.
//!
//! # Bindings
//!
//! Every effect pass binds, in order: `input_tex` (its first input),
//! `source_tex` (the effect's input frame), `aria_sampler`, the storage buffer
//! `aria: AriaPass`, then one `texture_2d<f32>` per input that is not the
//! source frame, named by [`PassInput::binding_name`]. A compute pass binds
//! `output_tex: texture_storage_2d<rgba8unorm, write>` after those. A
//! transition binds `outgoing_tex`, `incoming_tex`, `aria_sampler` and
//! `aria: AriaTransition`.

use std::fmt::Write as _;
use std::sync::Arc;

use aria_models::{
    EffectParamKind, EffectParamSpec, EffectSpec, GraphPass, OutputSize, PassInput, PassStage,
};

use crate::plugin_textures::{PluginTexture, plugin_texture};
use crate::registered_passes::{RegisterError, floats_of};

/// The four globals every effect pass binds first, by name.
pub(crate) const EFFECT_HOST: [&str; 4] = ["input_tex", "source_tex", "aria_sampler", "aria"];

/// The entry point a pass declares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Entry {
    /// `@fragment fn fs` returning `@location(0) vec4<f32>`.
    Fragment,
    /// `@compute @workgroup_size(x, y, 1) fn cs`.
    Compute([u32; 2]),
}

impl From<PassStage> for Entry {
    fn from(stage: PassStage) -> Self {
        match stage {
            PassStage::Fragment => Self::Fragment,
            PassStage::Compute { workgroup } => Self::Compute(workgroup),
        }
    }
}

/// One texture a compiled pass reads.
#[derive(Debug)]
pub(crate) enum CompiledInput {
    /// The effect's input frame.
    Source,
    /// The output of the pass at this execution index.
    Pass(usize),
    /// The frame of the clip the named layer parameter references.
    Layer(Box<str>),
    /// A registered plugin texture, held for as long as the effect is.
    Texture(Arc<PluginTexture>),
}

/// One pass as the runtime draws it.
#[derive(Debug)]
pub(crate) struct CompiledPass {
    pub(crate) name: Box<str>,
    pub(crate) stage: PassStage,
    /// Inputs in binding order; the first is also `input_tex`. Empty for a
    /// transition, which binds its two sides itself.
    pub(crate) inputs: Box<[CompiledInput]>,
    pub(crate) output: OutputSize,
    /// The plugin's WGSL followed by the host prelude.
    pub(crate) source: String,
}

impl CompiledPass {
    /// A transition pass: one fragment pass binding the transition prelude.
    pub(crate) fn transition(source: String) -> Self {
        Self {
            name: "transition".into(),
            stage: PassStage::Fragment,
            inputs: Box::new([]),
            output: OutputSize::Same,
            source,
        }
    }

    /// Textures bound beyond the four host globals, before `output_tex`.
    pub(crate) fn extra_textures(&self) -> usize {
        self.inputs
            .iter()
            .filter(|input| !matches!(input, CompiledInput::Source))
            .count()
    }

    pub(crate) fn is_compute(&self) -> bool {
        matches!(self.stage, PassStage::Compute { .. })
    }
}

/// Plan, check, and compile every pass of `passes`, in execution order.
///
/// Refused by every rule of [`aria_models::plan_graph`], by a texture input
/// naming no registered texture, and by a pass whose WGSL does not parse,
/// validate, declare its entry point, or keep to the host bindings. A WGSL
/// refusal of a pass declared with a name starts with that name.
pub(crate) fn compile_effect(
    spec: &EffectSpec,
    passes: &[GraphPass],
) -> Result<Box<[CompiledPass]>, RegisterError> {
    let refuse =
        |pass: Option<usize>, location: Option<(u32, u32)>, message: String| RegisterError {
            id: spec.id.clone(),
            pass,
            location,
            message,
        };
    let plan = aria_models::plan_graph(spec, passes)
        .map_err(|error| refuse(error.pass, None, error.message))?;
    let mut position = vec![0usize; passes.len()];
    for (exec, &declared) in plan.order.iter().enumerate() {
        position[declared] = exec;
    }
    let mut compiled = Vec::with_capacity(passes.len());
    for (index, pass) in passes.iter().enumerate() {
        let name = &plan.names[index];
        let inputs = &plan.inputs[index];
        let mut extras = Vec::with_capacity(inputs.len());
        let mut bound = Vec::with_capacity(inputs.len());
        for input in inputs {
            if let Some(binding) = input.binding_name() {
                extras.push(binding);
            }
            bound.push(match input {
                PassInput::Source => CompiledInput::Source,
                PassInput::Pass(read) => match plan.index_of(read) {
                    Some(declared) => CompiledInput::Pass(position[declared]),
                    None => {
                        return Err(refuse(
                            Some(index),
                            None,
                            format!("pass `{name}` reads `{read}`, which is no pass"),
                        ));
                    }
                },
                PassInput::Layer(param) => CompiledInput::Layer(param.as_str().into()),
                PassInput::Texture(id) => match plugin_texture(id) {
                    Some(texture) => CompiledInput::Texture(texture),
                    None => {
                        return Err(refuse(
                            Some(index),
                            None,
                            format!(
                                "pass `{name}` reads texture `{id}`, which is not registered; \
                                 register the texture before the effect"
                            ),
                        ));
                    }
                },
            });
        }
        let entry = Entry::from(pass.stage);
        let prelude = graph_prelude(&spec.params, &extras, entry);
        let mut host: Vec<&str> = EFFECT_HOST.to_vec();
        host.extend(extras.iter().map(String::as_str));
        if matches!(entry, Entry::Compute(_)) {
            host.push("output_tex");
        }
        let source = check_composed(&pass.source, &prelude, &host, entry).map_err(
            |(location, message)| {
                let message = if pass.name.is_empty() {
                    message
                } else {
                    format!("pass `{name}`: {message}")
                };
                refuse(Some(index), location, message)
            },
        )?;
        compiled.push((
            position[index],
            CompiledPass {
                name: name.as_str().into(),
                stage: pass.stage,
                inputs: bound.into_boxed_slice(),
                output: pass.output,
                source,
            },
        ));
    }
    compiled.sort_unstable_by_key(|(exec, _)| *exec);
    Ok(compiled.into_iter().map(|(_, pass)| pass).collect())
}

/// Deterministic FNV-1a digest of what the passes draw: composed sources,
/// stages, sizes, inputs, and the pixels of every plugin texture read.
pub(crate) fn digest(passes: &[CompiledPass]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0100_0000_01b3;
    let mut hash = OFFSET;
    let mut feed = |bytes: &[u8]| {
        for &byte in bytes {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(PRIME);
        }
    };
    for pass in passes {
        feed(pass.source.as_bytes());
        feed(&[0xff]);
        match pass.stage {
            PassStage::Fragment => feed(&[0]),
            PassStage::Compute { workgroup: [x, y] } => {
                feed(&[1]);
                feed(&x.to_le_bytes());
                feed(&y.to_le_bytes());
            }
        }
        match pass.output {
            OutputSize::Same => feed(&[0]),
            OutputSize::Scale([x, y]) => {
                feed(&[1]);
                feed(&x.to_bits().to_le_bytes());
                feed(&y.to_bits().to_le_bytes());
            }
            OutputSize::Fixed([w, h]) => {
                feed(&[2]);
                feed(&w.to_le_bytes());
                feed(&h.to_le_bytes());
            }
        }
        for input in &pass.inputs {
            match input {
                CompiledInput::Source => feed(&[0]),
                CompiledInput::Pass(exec) => {
                    feed(&[1]);
                    feed(&(*exec as u64).to_le_bytes());
                }
                CompiledInput::Layer(param) => {
                    feed(&[2]);
                    feed(param.as_bytes());
                    feed(&[0xff]);
                }
                CompiledInput::Texture(texture) => {
                    feed(&[3]);
                    feed(&texture.digest().to_le_bytes());
                }
            }
        }
        feed(&[0xfe]);
    }
    hash
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

/// The prelude of an effect pass whose parameters are `params`, binding
/// `extras` after the four host globals, and `output_tex` after those for a
/// compute pass.
pub(crate) fn graph_prelude(params: &[EffectParamSpec], extras: &[String], entry: Entry) -> String {
    let mut out = String::from(
        "// Aria host prelude.
struct AriaPass {
    size: vec2<f32>,
    texel: vec2<f32>,
    time: f32,
    pass_index: u32,
    pass_count: u32,
    pad: u32,
    params: array<f32>,
}

@group(0) @binding(0) var input_tex: texture_2d<f32>;
@group(0) @binding(1) var source_tex: texture_2d<f32>;
@group(0) @binding(2) var aria_sampler: sampler;
@group(0) @binding(3) var<storage, read> aria: AriaPass;
",
    );
    let first = EFFECT_HOST.len();
    for (offset, name) in extras.iter().enumerate() {
        let _ = writeln!(
            out,
            "@group(0) @binding({}) var {name}: texture_2d<f32>;",
            first + offset
        );
    }
    if matches!(entry, Entry::Compute(_)) {
        let _ = writeln!(
            out,
            "@group(0) @binding({}) var output_tex: texture_storage_2d<rgba8unorm, write>;",
            first + extras.len()
        );
    }
    out.push_str(VERTEX_STAGE);
    write_param_accessors(&mut out, params);
    out
}

/// The prelude of a transition whose parameters are `params`.
pub(crate) fn transition_prelude(params: &[EffectParamSpec]) -> String {
    let mut out = format!(
        "// Aria host prelude.
struct AriaTransition {{
    size: vec2<f32>,
    texel: vec2<f32>,
    progress: f32,
    pad0: f32,
    pad1: f32,
    pad2: f32,
    params: array<f32>,
}}

@group(0) @binding(0) var outgoing_tex: texture_2d<f32>;
@group(0) @binding(1) var incoming_tex: texture_2d<f32>;
@group(0) @binding(2) var aria_sampler: sampler;
@group(0) @binding(3) var<storage, read> aria: AriaTransition;
{VERTEX_STAGE}"
    );
    write_param_accessors(&mut out, params);
    out
}

fn write_param_accessors(out: &mut String, params: &[EffectParamSpec]) {
    let mut offset = 0usize;
    for param in params {
        let name = &param.name;
        let _ = match param.kind {
            EffectParamKind::Scalar | EffectParamKind::Angle => writeln!(
                out,
                "fn param_{name}() -> f32 {{ return aria.params[{offset}]; }}"
            ),
            EffectParamKind::Int => writeln!(
                out,
                "fn param_{name}() -> i32 {{ return i32(round(aria.params[{offset}])); }}"
            ),
            EffectParamKind::Bool => writeln!(
                out,
                "fn param_{name}() -> bool {{ return aria.params[{offset}] >= 0.5; }}"
            ),
            EffectParamKind::Choice => writeln!(
                out,
                "fn param_{name}() -> u32 {{ return u32(max(round(aria.params[{offset}]), 0.0)); }}"
            ),
            EffectParamKind::Vec2 | EffectParamKind::Size => writeln!(
                out,
                "fn param_{name}() -> vec2<f32> {{ return vec2<f32>(aria.params[{offset}], aria.params[{}]); }}",
                offset + 1
            ),
            EffectParamKind::Point3 => writeln!(
                out,
                "fn param_{name}() -> vec3<f32> {{ return vec3<f32>(aria.params[{offset}], aria.params[{}], aria.params[{}]); }}",
                offset + 1,
                offset + 2
            ),
            EffectParamKind::Color => writeln!(
                out,
                "fn param_{name}() -> vec4<f32> {{ return vec4<f32>(aria.params[{offset}], aria.params[{}], aria.params[{}], aria.params[{}]); }}",
                offset + 1,
                offset + 2,
                offset + 3
            ),
            EffectParamKind::Curve => writeln!(
                out,
                "fn param_{name}(x: f32) -> f32 {{ let u = clamp(x, 0.0, 1.0) * 63.0; let i = min(u32(floor(u)), 62u); let f = u - f32(i); return mix(aria.params[{offset}u + i], aria.params[{offset}u + i + 1u], f); }}"
            ),
            EffectParamKind::Gradient => writeln!(
                out,
                "fn param_{name}(t: f32) -> vec4<f32> {{ let u = clamp(t, 0.0, 1.0) * 63.0; let i = min(u32(floor(u)), 62u); let f = u - f32(i); let b0 = {offset}u + i * 4u; let b1 = b0 + 4u; let c0 = vec4<f32>(aria.params[b0], aria.params[b0 + 1u], aria.params[b0 + 2u], aria.params[b0 + 3u]); let c1 = vec4<f32>(aria.params[b1], aria.params[b1 + 1u], aria.params[b1 + 2u], aria.params[b1 + 3u]); return mix(c0, c1, f); }}"
            ),
            EffectParamKind::Text
            | EffectParamKind::Font
            | EffectParamKind::File
            | EffectParamKind::Layer => Ok(()),
        };
        offset += floats_of(param.kind);
    }
}

/// A refusal: one-based line and column in the plugin source, and the reason.
pub(crate) type Refusal = (Option<(u32, u32)>, String);

/// Compose `source` with `prelude` and check it against `entry` and the
/// globals in `host`, returning the composed WGSL.
pub(crate) fn check_composed(
    source: &str,
    prelude: &str,
    host: &[&str],
    entry: Entry,
) -> Result<String, Refusal> {
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
    check_interface(&module, host, entry).map_err(|message| (None, message))?;
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

/// The entry point, its inputs and output, and the bindings.
fn check_interface(module: &naga::Module, host: &[&str], entry: Entry) -> Result<(), String> {
    match entry {
        Entry::Fragment => check_fragment(module)?,
        Entry::Compute([x, y]) => {
            let cs = module
                .entry_points
                .iter()
                .find(|point| point.name == "cs" && point.stage == naga::ShaderStage::Compute)
                .ok_or_else(|| {
                    format!(
                        "no `@compute @workgroup_size({x}, {y}, 1) fn cs`; \
                         the pass is declared compute"
                    )
                })?;
            if cs.workgroup_size != [x, y, 1] {
                let [sx, sy, sz] = cs.workgroup_size;
                return Err(format!(
                    "`cs` declares @workgroup_size({sx}, {sy}, {sz}); \
                     the pass declares a {x}×{y} workgroup"
                ));
            }
        }
    }
    for (_, global) in module.global_variables.iter() {
        let name = global.name.as_deref().unwrap_or("_");
        if let Some(binding) = &global.binding
            && !host.contains(&name)
        {
            return Err(format!(
                "`{name}` is bound at @group({}) @binding({}); a pass binds only {}",
                binding.group,
                binding.binding,
                host.join(", ")
            ));
        }
        if matches!(global.space, naga::AddressSpace::Immediate) {
            return Err(format!(
                "`{name}` is an immediate; a pass reads constants from `aria`"
            ));
        }
    }
    Ok(())
}

/// `@fragment fn fs` returning `@location(0) vec4<f32>` and reading only what
/// the host vertex stage writes.
fn check_fragment(module: &naga::Module) -> Result<(), String> {
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
    Ok(())
}
