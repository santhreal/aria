//! Render-graph declarations: the passes of a WGSL effect, what each pass
//! reads, and the size each pass writes.
//!
//! Every plugin tier declares an effect as a list of [`GraphPass`]es: in-process
//! Rust, the C ABI (JSON), WIT, and package TOML all carry this shape.
//! [`plan_graph`] checks the declaration before any WGSL is parsed: pass names
//! are unique identifiers, every input names a pass, a layer parameter, a
//! texture, or the source frame, the passes form an acyclic graph whose only
//! sink is the last declared pass, and that pass writes the effect's frame at
//! the input size.
//!
//! # Defaults
//!
//! A pass with no name is named `pass<index>`. A pass with no inputs reads the
//! pass declared before it, or the source frame when it is the first pass, so
//! a plain list of fragment passes is a chain.

use serde::{Deserialize, Serialize};

use crate::effects::{EffectParamKind, EffectSpec};

/// Most passes one effect runs.
pub const MAX_GRAPH_PASSES: usize = 8;

/// Most inputs one pass reads.
pub const MAX_PASS_INPUTS: usize = 8;

/// Largest side of a pass output, in pixels.
pub const MAX_PASS_SIDE: u32 = 8192;

/// Largest scale factor a pass output may declare per axis.
pub const MAX_PASS_SCALE: f32 = 16.0;

/// Most invocations in one compute workgroup (the WebGPU default limit).
pub const MAX_WORKGROUP_INVOCATIONS: u32 = 256;

/// How a pass runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PassStage {
    /// `@fragment fn fs` drawn over the whole output, returning
    /// `@location(0) vec4<f32>`.
    #[default]
    Fragment,
    /// `@compute @workgroup_size(x, y, 1) fn cs` dispatched over the output in
    /// `workgroup`-sized tiles, writing `output_tex` with `textureStore`.
    Compute {
        /// Workgroup width and height; the shader declares the same size.
        workgroup: [u32; 2],
    },
}

/// One texture a pass reads.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PassInput {
    /// The effect's input frame, bound as `source_tex`.
    Source,
    /// The output of the named pass, bound as `pass_<name>`.
    Pass(String),
    /// The frame of the clip the named layer parameter references, rendered
    /// at the same time, bound as `layer_<param>`. A parameter naming no clip,
    /// or a clip with no frame at that time, reads transparent.
    Layer(String),
    /// The plugin texture registered under this id, bound as `texture_<id>`
    /// with every character that is not an ASCII letter, digit, or `_`
    /// replaced by `_`.
    Texture(String),
}

impl PassInput {
    /// The output of the pass named `name`.
    pub fn pass(name: impl Into<String>) -> Self {
        Self::Pass(name.into())
    }

    /// The clip the layer parameter `param` references.
    pub fn layer(param: impl Into<String>) -> Self {
        Self::Layer(param.into())
    }

    /// The plugin texture `id`.
    pub fn texture(id: impl Into<String>) -> Self {
        Self::Texture(id.into())
    }

    /// The WGSL global this input is bound to beyond `input_tex`, or `None`
    /// for [`PassInput::Source`], which is always `source_tex`.
    pub fn binding_name(&self) -> Option<String> {
        match self {
            Self::Source => None,
            Self::Pass(name) => Some(format!("pass_{name}")),
            Self::Layer(param) => Some(format!("layer_{param}")),
            Self::Texture(id) => {
                let mut out = String::with_capacity("texture_".len() + id.len());
                out.push_str("texture_");
                out.extend(id.chars().map(|c| {
                    if c.is_ascii_alphanumeric() || c == '_' {
                        c
                    } else {
                        '_'
                    }
                }));
                Some(out)
            }
        }
    }
}

/// The size a pass writes.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputSize {
    /// The effect's input size.
    #[default]
    Same,
    /// The input size times `[x, y]`, rounded, at least one pixel.
    Scale([f32; 2]),
    /// `[width, height]` pixels whatever the input size.
    Fixed([u32; 2]),
}

impl OutputSize {
    /// Pixel size of this output for an effect input of `width`×`height`.
    pub fn resolve(self, width: u32, height: u32) -> (u32, u32) {
        let scaled = |side: u32, factor: f32| -> u32 {
            let value = (side as f32 * factor).round();
            if value.is_finite() {
                (value as u32).clamp(1, MAX_PASS_SIDE)
            } else {
                1
            }
        };
        match self {
            Self::Same => (width.max(1), height.max(1)),
            Self::Scale([x, y]) => (scaled(width, x), scaled(height, y)),
            Self::Fixed([w, h]) => (w.clamp(1, MAX_PASS_SIDE), h.clamp(1, MAX_PASS_SIDE)),
        }
    }
}

/// One pass of an effect graph.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct GraphPass {
    /// Name other passes read this pass by; empty names it `pass<index>`.
    #[serde(default)]
    pub name: String,
    /// Fragment or compute.
    #[serde(default)]
    pub stage: PassStage,
    /// Textures read, in binding order; the first is also bound as
    /// `input_tex`. Empty reads the previous pass, or the source frame for the
    /// first pass.
    #[serde(default)]
    pub inputs: Vec<PassInput>,
    /// The size this pass writes. The last pass writes the effect's frame and
    /// uses [`OutputSize::Same`].
    #[serde(default)]
    pub output: OutputSize,
    /// WGSL source.
    pub source: String,
}

impl GraphPass {
    /// A fragment pass declaring `@fragment fn fs`.
    pub fn fragment(source: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            ..Self::default()
        }
    }

    /// A compute pass declaring `@compute @workgroup_size(x, y, 1) fn cs`.
    pub fn compute(source: impl Into<String>, workgroup: [u32; 2]) -> Self {
        Self {
            stage: PassStage::Compute { workgroup },
            source: source.into(),
            ..Self::default()
        }
    }

    /// This pass named `name`.
    #[must_use]
    pub fn named(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// This pass reading `inputs`, in binding order.
    #[must_use]
    pub fn reading(mut self, inputs: impl IntoIterator<Item = PassInput>) -> Self {
        self.inputs = inputs.into_iter().collect();
        self
    }

    /// This pass writing `output`.
    #[must_use]
    pub fn sized(mut self, output: OutputSize) -> Self {
        self.output = output;
        self
    }
}

impl From<&str> for GraphPass {
    /// A fragment pass with default name, inputs, and size.
    fn from(source: &str) -> Self {
        Self::fragment(source)
    }
}

impl From<String> for GraphPass {
    /// A fragment pass with default name, inputs, and size.
    fn from(source: String) -> Self {
        Self::fragment(source)
    }
}

/// Why a graph declaration was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphError {
    /// Declaration index of the refused pass, or `None` for the whole graph.
    pub pass: Option<usize>,
    pub message: String,
}

impl GraphError {
    fn graph(message: impl Into<String>) -> Self {
        Self {
            pass: None,
            message: message.into(),
        }
    }

    fn pass(index: usize, message: impl Into<String>) -> Self {
        Self {
            pass: Some(index),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for GraphError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(pass) = self.pass {
            write!(f, "pass {pass}: ")?;
        }
        f.write_str(&self.message)
    }
}

impl std::error::Error for GraphError {}

/// A checked graph: effective names and inputs, and the execution order.
#[derive(Debug, Clone, PartialEq)]
pub struct GraphPlan {
    /// Effective name of each declared pass.
    pub names: Vec<String>,
    /// Effective inputs of each declared pass, defaults filled in.
    pub inputs: Vec<Vec<PassInput>>,
    /// Declaration indices in execution order. Every pass appears once and
    /// the last entry is the last declared pass, which writes the frame.
    pub order: Vec<usize>,
}

impl GraphPlan {
    /// Declaration index of the pass named `name`.
    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.names.iter().position(|n| n == name)
    }
}

fn is_identifier(name: &str) -> bool {
    !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

fn is_texture_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
}

/// Check `passes` against `spec` and order them for execution.
///
/// Refused, naming the pass where one is at fault: no passes or more than
/// [`MAX_GRAPH_PASSES`]; a name that is not an identifier or repeats; an input
/// naming no pass, a pass reading itself, a layer input naming no
/// [`EffectParamKind::Layer`] parameter, a texture id with characters other
/// than ASCII letters, digits, `_`, `.`, `-`; two inputs of one pass bound to
/// one name; more than [`MAX_PASS_INPUTS`] inputs; a workgroup with a zero side
/// or more than [`MAX_WORKGROUP_INVOCATIONS`] invocations; a scale that is not
/// finite, not positive, or above [`MAX_PASS_SCALE`]; a fixed size with a zero
/// side or above [`MAX_PASS_SIDE`]; a cycle; a pass the last pass does not
/// read, directly or through other passes; and a last pass whose output is
/// not [`OutputSize::Same`].
pub fn plan_graph(spec: &EffectSpec, passes: &[GraphPass]) -> Result<GraphPlan, GraphError> {
    if passes.is_empty() {
        return Err(GraphError::graph(
            "an effect needs at least one pass; the last pass writes the frame",
        ));
    }
    if passes.len() > MAX_GRAPH_PASSES {
        return Err(GraphError::graph(format!(
            "{} passes; an effect runs at most {MAX_GRAPH_PASSES}",
            passes.len()
        )));
    }
    let names: Vec<String> = passes
        .iter()
        .enumerate()
        .map(|(index, pass)| {
            if pass.name.is_empty() {
                format!("pass{index}")
            } else {
                pass.name.clone()
            }
        })
        .collect();
    for (index, name) in names.iter().enumerate() {
        if !is_identifier(name) {
            return Err(GraphError::pass(
                index,
                format!("pass name `{name}` is not an identifier (ASCII letters, digits, `_`)"),
            ));
        }
        if let Some(first) = names[..index].iter().position(|n| n == name) {
            return Err(GraphError::pass(
                index,
                format!("pass `{name}` repeats the name of pass {first}"),
            ));
        }
    }
    let mut inputs: Vec<Vec<PassInput>> = Vec::with_capacity(passes.len());
    for (index, pass) in passes.iter().enumerate() {
        let name = &names[index];
        let effective = if pass.inputs.is_empty() {
            vec![match index {
                0 => PassInput::Source,
                _ => PassInput::Pass(names[index - 1].clone()),
            }]
        } else {
            pass.inputs.clone()
        };
        if effective.len() > MAX_PASS_INPUTS {
            return Err(GraphError::pass(
                index,
                format!(
                    "pass `{name}` reads {} inputs; a pass reads at most {MAX_PASS_INPUTS}",
                    effective.len()
                ),
            ));
        }
        for (slot, input) in effective.iter().enumerate() {
            match input {
                PassInput::Source => {}
                PassInput::Pass(read) if read == name => {
                    return Err(GraphError::pass(
                        index,
                        format!("pass `{name}` reads itself; the graph has a cycle"),
                    ));
                }
                PassInput::Pass(read) => {
                    if !names.contains(read) {
                        return Err(GraphError::pass(
                            index,
                            format!("pass `{name}` reads pass `{read}`, which is not declared"),
                        ));
                    }
                }
                PassInput::Layer(param) => {
                    if spec.param(param).map(|p| p.kind) != Some(EffectParamKind::Layer) {
                        return Err(GraphError::pass(
                            index,
                            format!(
                                "pass `{name}` reads layer `{param}`, which is not a layer \
                                 parameter of `{}`",
                                spec.id
                            ),
                        ));
                    }
                }
                PassInput::Texture(id) => {
                    if !is_texture_id(id) {
                        return Err(GraphError::pass(
                            index,
                            format!(
                                "pass `{name}` reads texture `{id}`; a texture id is ASCII \
                                 letters, digits, `_`, `.`, and `-`"
                            ),
                        ));
                    }
                }
            }
            let binding = input.binding_name();
            if binding.is_some()
                && effective[..slot]
                    .iter()
                    .any(|earlier| earlier.binding_name() == binding)
            {
                return Err(GraphError::pass(
                    index,
                    format!(
                        "pass `{name}` binds `{}` twice",
                        binding.unwrap_or_default()
                    ),
                ));
            }
        }
        if let PassStage::Compute { workgroup: [x, y] } = pass.stage
            && (x == 0 || y == 0 || x.saturating_mul(y) > MAX_WORKGROUP_INVOCATIONS)
        {
            return Err(GraphError::pass(
                index,
                format!(
                    "pass `{name}` declares workgroup {x}x{y}; each side is at least 1 and \
                     a workgroup runs at most {MAX_WORKGROUP_INVOCATIONS} invocations"
                ),
            ));
        }
        match pass.output {
            OutputSize::Same => {}
            OutputSize::Scale([x, y]) => {
                let valid = |f: f32| f.is_finite() && f > 0.0 && f <= MAX_PASS_SCALE;
                if !valid(x) || !valid(y) {
                    return Err(GraphError::pass(
                        index,
                        format!(
                            "pass `{name}` scales its output by {x}x{y}; each factor is above \
                             0 and at most {MAX_PASS_SCALE}"
                        ),
                    ));
                }
            }
            OutputSize::Fixed([w, h]) => {
                if w == 0 || h == 0 || w > MAX_PASS_SIDE || h > MAX_PASS_SIDE {
                    return Err(GraphError::pass(
                        index,
                        format!(
                            "pass `{name}` writes {w}x{h}; each side is 1 to {MAX_PASS_SIDE} pixels"
                        ),
                    ));
                }
            }
        }
        inputs.push(effective);
    }

    let order = topological_order(&names, &inputs)?;

    let last = passes.len() - 1;
    if passes[last].output != OutputSize::Same {
        return Err(GraphError::pass(
            last,
            format!(
                "the last pass `{}` writes the effect's frame and uses output size `same`",
                names[last]
            ),
        ));
    }
    let mut feeds_last = vec![false; passes.len()];
    let mut stack = vec![last];
    while let Some(index) = stack.pop() {
        if std::mem::replace(&mut feeds_last[index], true) {
            continue;
        }
        for input in &inputs[index] {
            if let PassInput::Pass(read) = input
                && let Some(read) = names.iter().position(|n| n == read)
            {
                stack.push(read);
            }
        }
    }
    if let Some(dead) = feeds_last.iter().position(|reached| !reached) {
        return Err(GraphError::pass(
            dead,
            format!(
                "pass `{}` feeds nothing: the last pass `{}` does not read it",
                names[dead], names[last]
            ),
        ));
    }

    Ok(GraphPlan {
        names,
        inputs,
        order,
    })
}

/// Kahn's algorithm, taking the lowest ready declaration index first so the
/// order is the declaration order wherever the graph allows it. A cycle is
/// reported at the lowest-indexed pass on it, with the cycle spelled out.
fn topological_order(
    names: &[String],
    inputs: &[Vec<PassInput>],
) -> Result<Vec<usize>, GraphError> {
    let count = names.len();
    let reads = |index: usize| {
        inputs[index].iter().filter_map(|input| match input {
            PassInput::Pass(read) => names.iter().position(|n| n == read),
            _ => None,
        })
    };
    let mut pending: Vec<usize> = (0..count).map(|index| reads(index).count()).collect();
    let mut done = vec![false; count];
    let mut order = Vec::with_capacity(count);
    while order.len() < count {
        let Some(next) = (0..count).find(|&index| !done[index] && pending[index] == 0) else {
            break;
        };
        done[next] = true;
        order.push(next);
        for reader in 0..count {
            if !done[reader] {
                pending[reader] -= reads(reader).filter(|&read| read == next).count();
            }
        }
    }
    if order.len() == count {
        return Ok(order);
    }
    // Every pass left over reads another pass left over, so walking first
    // reads from any of them revisits a pass: that loop is a cycle.
    let start = (0..count).find(|&index| !done[index]).unwrap_or(0);
    let mut path = vec![start];
    let mut current = start;
    loop {
        let next = reads(current).find(|&read| !done[read]).unwrap_or(start);
        if let Some(at) = path.iter().position(|&seen| seen == next) {
            let cycle = &path[at..];
            let first = *cycle.iter().min().unwrap_or(&next);
            let mut spelled: Vec<&str> = cycle.iter().map(|&i| names[i].as_str()).collect();
            spelled.push(names[cycle[0]].as_str());
            return Err(GraphError::pass(
                first,
                format!(
                    "pass `{}` is on a cycle: {}",
                    names[first],
                    spelled.join(" reads ")
                ),
            ));
        }
        path.push(next);
        current = next;
    }
}

#[cfg(test)]
#[path = "effect_graph_tests.rs"]
mod tests;
