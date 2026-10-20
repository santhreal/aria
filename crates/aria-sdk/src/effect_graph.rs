//! The render graph of a WGSL effect: passes, the textures each reads, and the
//! size each writes. The types serialize to the JSON the host reads from
//! [`Host::register_effect_graph`](crate::Host::register_effect_graph).

use serde::{Deserialize, Serialize};

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
    /// The frame of the clip the named layer parameter references, bound as
    /// `layer_<param>`.
    Layer(String),
    /// The texture registered under this id with
    /// [`Host::register_texture`](crate::Host::register_texture), bound as
    /// `texture_<id>` with every character that is not an ASCII letter,
    /// digit, or `_` replaced by `_`.
    Texture(String),
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

/// Pixel margins an effect draws past its layer's quad. Positive values
/// expand, negative values shrink.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct EffectBounds {
    /// Margin past the left edge, in pixels.
    #[serde(default)]
    pub left: f32,
    /// Margin past the top edge, in pixels.
    #[serde(default)]
    pub top: f32,
    /// Margin past the right edge, in pixels.
    #[serde(default)]
    pub right: f32,
    /// Margin past the bottom edge, in pixels.
    #[serde(default)]
    pub bottom: f32,
}

impl EffectBounds {
    /// The same margin on all four sides.
    pub const fn uniform(margin: f32) -> Self {
        Self {
            left: margin,
            top: margin,
            right: margin,
            bottom: margin,
        }
    }
}
