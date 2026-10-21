//! Offscreen targets, pass pipelines, and effect, transition, and matte
//! execution.

use bytemuck::{Pod, Zeroable};

use crate::color_pass::{self, ReplacePipeline};
use crate::error::CompositorError;
use crate::grade::ColorGrade;
use crate::layer::{CoverageMix, MaskOp, MatteRead};
use crate::passes::{GAUSSIAN_BLUR, PassId, PassInstance, transition_pass};
use crate::registered_passes::registered_transition;

mod blend;
mod blit;
mod canvas_targets;
mod lut;
mod pass_cache;
#[cfg(test)]
mod pass_census;
mod pipelines;
mod pool;
mod registered;
mod styles;

pub(crate) use blend::run_blend_composite;
pub(crate) use blit::{blit_premultiplied_to_canvas, blit_replace, blit_weighted_additive};
pub(crate) use canvas_targets::CanvasTargets;
pub(crate) use lut::run_lut_pass;
pub use pass_cache::PassStats;
pub(crate) use pass_cache::{Bound, PassCache};
pub(crate) use pipelines::{
    PREMULT_OVER, PassRegistry, SharedPassPipelines, sampler_entry, tex_entry,
};
pub(crate) use pool::OffscreenPool;
pub(crate) use styles::{background_plate_layer, composite_layer_outline, composite_layer_styles};

const RT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct TexelUniforms {
    texel_size: [f32; 4],
}

/// Max flattened floats uploaded per effect pass (`4 × vec4`).
const EFFECT_PARAM_FLOATS: usize = 16;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct EffectUniforms {
    texel_size: [f32; 4],
    /// Catalog-ordered flattened params packed as `array<vec4, 4>` in WGSL.
    params: [[f32; 4]; 4],
}

/// Validate and pack flattened effect parameters into the GPU uniform block (16 floats).
///
/// Returns [`CompositorError::InvalidEffectParams`] naming the descriptor's
/// expected parameter count whenever the effect is registered, and the uniform
/// capacity only for an id the catalog does not describe. A registered effect
/// always wants fewer floats than the block holds, so reporting the block size
/// to a caller who asked for a one-parameter blur would name a limit that is not
/// the one it broke.
pub(crate) fn validate_and_pack_effect_params(
    effect_id: &str,
    params: &[f32],
) -> Result<[[f32; 4]; 4], CompositorError> {
    if let Some(desc) = crate::passes::effect_descriptor(effect_id) {
        if params.len() != desc.float_count() {
            return Err(CompositorError::InvalidEffectParams {
                effect: effect_id.to_string(),
                expected: desc.float_count(),
                actual: params.len(),
            });
        }
    } else if params.len() > EFFECT_PARAM_FLOATS {
        return Err(CompositorError::InvalidEffectParams {
            effect: effect_id.to_string(),
            expected: EFFECT_PARAM_FLOATS,
            actual: params.len(),
        });
    }
    let mut out = [[0.0f32; 4]; 4];
    for (i, &v) in params.iter().enumerate() {
        out[i / 4][i % 4] = v;
    }
    Ok(out)
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct TransitionUniforms {
    texel_size: [f32; 4],
    params: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GradeUniforms {
    /// Packed grade lanes, the same layout the layer shaders are fed.
    grade: [[f32; 4]; ColorGrade::LANES],
}

/// Run an effect chain on `input` view, ping-ponging through `pool`. A
/// registered pass reading a layer parameter reads that clip's frame from
/// `references`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_effect_chain<'a>(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    registry: &PassRegistry,
    pool: &'a OffscreenPool,
    references: &CanvasTargets,
    input_view: &'a wgpu::TextureView,
    effects: &[PassInstance],
    width: u32,
    height: u32,
) -> Result<&'a wgpu::TextureView, CompositorError> {
    let mut read_idx = 0usize;
    let mut current_input = input_view;

    let cache = pool.passes();
    for effect in effects {
        let shipped = match &effect.id {
            PassId::Shipped(id) => *id,
            PassId::Registered(pass) => {
                if pass.float_count() != effect.params.len() {
                    return Err(CompositorError::InvalidEffectParams {
                        effect: pass.id().to_string(),
                        expected: pass.float_count(),
                        actual: effect.params.len(),
                    });
                }
                if effect.is_noop() {
                    continue;
                }
                let in_idx = pool.index_of(current_input).unwrap_or(read_idx);
                if let Some((written, view)) = registered::run_registered_effect(
                    device,
                    encoder,
                    registry,
                    pool,
                    references,
                    current_input,
                    in_idx,
                    effect,
                    pass,
                    width,
                    height,
                ) {
                    read_idx = written;
                    current_input = view;
                }
                continue;
            }
        };
        validate_and_pack_effect_params(shipped, &effect.params)?;
        if effect.is_noop() {
            continue;
        }
        let in_idx = pool.index_of(current_input).unwrap_or(read_idx);

        match shipped {
            GAUSSIAN_BLUR => {
                let radius = effect.params[0];
                let blur_params = [radius];
                let mid_idx = (in_idx + 1) % pool.slots();
                let out_idx = (in_idx + 2) % pool.slots();
                let mid_view = pool.view(mid_idx);
                let out_view = pool.view(out_idx);
                let (horizontal, vertical) = registry.blur_passes();
                draw_effect_pass(
                    device,
                    encoder,
                    registry,
                    cache,
                    horizontal,
                    current_input,
                    mid_view,
                    width,
                    height,
                    GAUSSIAN_BLUR,
                    &blur_params,
                )?;
                draw_effect_pass(
                    device,
                    encoder,
                    registry,
                    cache,
                    vertical,
                    mid_view,
                    out_view,
                    width,
                    height,
                    GAUSSIAN_BLUR,
                    &blur_params,
                )?;
                read_idx = out_idx;
                current_input = out_view;
                continue;
            }
            _ => {
                let write_idx = (in_idx + 1) % pool.slots();
                let write_view = pool.view(write_idx);
                let pipeline = registry.effect_pipeline(shipped);
                draw_effect_pass(
                    device,
                    encoder,
                    registry,
                    cache,
                    pipeline,
                    current_input,
                    write_view,
                    width,
                    height,
                    shipped,
                    &effect.params,
                )?;
                read_idx = write_idx;
                current_input = pool.view(read_idx);
            }
        }
    }

    Ok(current_input)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn run_transition_pass(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    registry: &PassRegistry,
    cache: &PassCache,
    outgoing: &wgpu::TextureView,
    incoming: &wgpu::TextureView,
    output: &wgpu::TextureView,
    transition_id: &str,
    progress: f32,
    params: &[f32],
    width: u32,
    height: u32,
) {
    if !crate::passes::is_shipped_transition(transition_id)
        && let Some(pass) = registered_transition(transition_id)
        && registered::run_registered_transition(
            device, encoder, registry, cache, &pass, outgoing, incoming, output, progress, params,
            width, height,
        )
    {
        return;
    }
    let pipeline = registry.transition_pipeline(transition_id);
    let feather = params.first().copied().unwrap_or(0.0);
    let uniforms = TransitionUniforms {
        texel_size: texel_size(width, height),
        params: [
            progress,
            transition_pass(transition_id).mode(),
            feather,
            0.0,
        ],
    };
    let bind_group = cache.bind_group(
        device,
        &registry.transition_layout,
        "aria.transition.bg",
        &[
            Bound::Texture(outgoing),
            Bound::Texture(incoming),
            Bound::Sampler(&registry.sampler),
            Bound::Uniform(bytemuck::bytes_of(&uniforms)),
        ],
    );

    color_pass::replace_quad(encoder, output, pipeline, &bind_group);
}

/// Cut `content` by `stencil` into `output`, the stencil's coverage remapped
/// by `mix`. The stencil is read, never drawn, so the pass is a two-texture
/// blend like a transition rather than a layer.
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_matte_pass(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    registry: &PassRegistry,
    cache: &PassCache,
    content: &wgpu::TextureView,
    stencil: &wgpu::TextureView,
    output: &wgpu::TextureView,
    read: MatteRead,
    mix: CoverageMix,
    softness: f32,
    width: u32,
    height: u32,
) {
    let uniforms = TransitionUniforms {
        texel_size: texel_size(width, height),
        params: [read.mode(), mix.offset, softness.clamp(0.0, 1.0), mix.scale],
    };
    let bind_group = cache.bind_group(
        device,
        &registry.transition_layout,
        "aria.matte.bg",
        &[
            Bound::Texture(content),
            Bound::Texture(stencil),
            Bound::Sampler(&registry.sampler),
            Bound::Uniform(bytemuck::bytes_of(&uniforms)),
        ],
    );

    color_pass::replace_quad(encoder, output, registry.matte(), &bind_group);
}

/// One mask of a stack, as [`run_mask_stack_pass`] combines it.
pub(crate) struct StackStep {
    pub op: MaskOp,
    pub opacity: f32,
    pub softness: f32,
    pub invert: bool,
    /// The coverage the stack starts from, read in place of `stack` for the
    /// stack's first mask.
    pub start: f32,
}

/// Combine `mask`'s coverage with the coverage of the stack above it into
/// `output`. `stack` is `None` for the stack's first mask, which combines
/// with `step.start` instead.
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_mask_stack_pass(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    registry: &PassRegistry,
    cache: &PassCache,
    stack: Option<&wgpu::TextureView>,
    mask: &wgpu::TextureView,
    output: &wgpu::TextureView,
    step: &StackStep,
    width: u32,
    height: u32,
) {
    let [texel_x, texel_y, _, _] = texel_size(width, height);
    let uniforms = TransitionUniforms {
        texel_size: [
            texel_x,
            texel_y,
            step.start,
            if stack.is_some() { 1.0 } else { 0.0 },
        ],
        params: [
            step.op.id() as f32,
            step.opacity.clamp(0.0, 1.0),
            step.softness.clamp(0.0, 1.0),
            if step.invert { 1.0 } else { 0.0 },
        ],
    };
    let bind_group = cache.bind_group(
        device,
        &registry.transition_layout,
        "aria.mask_stack.bg",
        &[
            // The first mask reads no stack; its own coverage stands in for
            // the binding and the shader selects the start over it.
            Bound::Texture(stack.unwrap_or(mask)),
            Bound::Texture(mask),
            Bound::Sampler(&registry.sampler),
            Bound::Uniform(bytemuck::bytes_of(&uniforms)),
        ],
    );

    color_pass::replace_quad(encoder, output, registry.mask_stack(), &bind_group);
}

pub(crate) fn run_grade_pass<'a>(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    registry: &PassRegistry,
    cache: &PassCache,
    input: &'a wgpu::TextureView,
    output: &'a wgpu::TextureView,
    grade: ColorGrade,
) -> &'a wgpu::TextureView {
    let bind_group = cache.bind_group(
        device,
        &registry.effect_layout,
        "aria.canvas_grade.bg",
        &[
            Bound::Texture(input),
            Bound::Sampler(&registry.sampler),
            Bound::Uniform(bytemuck::bytes_of(&GradeUniforms {
                grade: grade.lanes(grade.shader_enabled()),
            })),
        ],
    );

    color_pass::replace_quad(encoder, output, registry.grade(), &bind_group);
    output
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_effect_pass(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    registry: &PassRegistry,
    cache: &PassCache,
    pipeline: &ReplacePipeline,
    input: &wgpu::TextureView,
    output: &wgpu::TextureView,
    width: u32,
    height: u32,
    effect_id: &str,
    params: &[f32],
) -> Result<(), CompositorError> {
    let packed = validate_and_pack_effect_params(effect_id, params)?;
    let uniforms = EffectUniforms {
        texel_size: texel_size(width, height),
        params: packed,
    };
    let bind_group = cache.bind_group(
        device,
        &registry.effect_layout,
        "aria.effect.bg",
        &[
            Bound::Texture(input),
            Bound::Sampler(&registry.sampler),
            Bound::Uniform(bytemuck::bytes_of(&uniforms)),
        ],
    );
    color_pass::replace_quad(encoder, output, pipeline, &bind_group);
    Ok(())
}
fn texel_size(width: u32, height: u32) -> [f32; 4] {
    [
        1.0 / width.max(1) as f32,
        1.0 / height.max(1) as f32,
        0.0,
        0.0,
    ]
}

/// Draw one layer to an offscreen (or canvas) target.
///
/// `instances` is `Some((buffer, count))` for instanced glyph draws; `None`
/// draws a single unit-quad like the rest of the layer pipelines.
pub(crate) fn draw_layer_to_offscreen(
    pass: &mut wgpu::RenderPass<'_>,
    pipeline: &wgpu::RenderPipeline,
    bind_group: &wgpu::BindGroup,
    instances: Option<(&wgpu::Buffer, u32)>,
) {
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, bind_group, &[]);
    if let Some((buffer, count)) = instances {
        pass.set_vertex_buffer(0, buffer.slice(..));
        pass.draw(0..6, 0..count);
    } else {
        pass.draw(0..6, 0..1);
    }
}

/// Returns `true` when the effect chain is empty or all passes are no-ops.
pub(crate) fn effects_need_offscreen(effects: &[PassInstance]) -> bool {
    !effects.is_empty() && effects.iter().any(|e| !e.is_noop())
}

#[allow(dead_code)]
pub(crate) fn check_dimensions(width: u32, height: u32) -> Result<(), CompositorError> {
    if width == 0 || height == 0 {
        return Err(CompositorError::InvalidDimensions { width, height });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{EFFECT_PARAM_FLOATS, validate_and_pack_effect_params};
    use crate::error::CompositorError;
    #[test]
    fn validate_and_pack_effect_params_rejects_count_mismatch() {
        // Vignette expects 1 scalar float (amount).
        let err = validate_and_pack_effect_params("vignette", &[0.5, 1.0])
            .expect_err("should reject 2 params for vignette");
        match err {
            CompositorError::InvalidEffectParams {
                effect,
                expected,
                actual,
            } => {
                assert_eq!(effect, "vignette");
                assert_eq!(expected, 1);
                assert_eq!(actual, 2);
            }
            other => panic!("unexpected error variant: {other:?}"),
        }
    }

    #[test]
    fn validate_and_pack_effect_params_rejects_overflow() {
        let oversized = vec![0.0f32; 17];
        let err = validate_and_pack_effect_params("custom", &oversized)
            .expect_err("should reject >16 params");
        match err {
            CompositorError::InvalidEffectParams {
                effect,
                expected,
                actual,
            } => {
                assert_eq!(effect, "custom");
                assert_eq!(expected, 16);
                assert_eq!(actual, 17);
            }
            other => panic!("unexpected error variant: {other:?}"),
        }
    }

    /// WHY: `validate_and_pack_effect_params` writes into a fixed
    /// `[[f32; 4]; 4]`, and a registered effect skips the capacity branch
    /// because its descriptor arity is the stricter answer. A catalog row that
    /// declared more floats than the block holds would therefore reach the pack
    /// loop and index out of bounds at draw time rather than returning an
    /// error. The bound is derived from `EFFECT_PARAM_FLOATS` and the row list
    /// is read from the registry, so adding an effect that overflows the block
    /// goes red here. `no_effect_overflows_the_uniform_its_shader_declares`
    /// checks the same arity against the *shader's* declared slot count, which
    /// is a separate number and would not catch a shader that declared more.
    #[test]
    fn no_catalog_effect_declares_more_floats_than_the_pack_buffer_holds() {
        let descriptors = crate::passes::shipped_effect_descriptors();
        assert!(!descriptors.is_empty(), "descriptors must be populated");
        for desc in descriptors {
            assert!(
                desc.float_count() <= EFFECT_PARAM_FLOATS,
                "effect '{}' declares {} floats, past the {EFFECT_PARAM_FLOATS} \
                 the uniform pack buffer holds",
                desc.id,
                desc.float_count()
            );
        }
    }

    #[test]
    fn validate_and_pack_effect_params_accepts_matching_catalog_params() {
        // Color overlay expects color (4) + offset (2) + amount (1).
        let valid = [1.0, 0.0, 0.0, 1.0, 0.25, -0.5, 0.5];
        let packed = validate_and_pack_effect_params("color_overlay", &valid)
            .expect("matching params should succeed");
        assert_eq!(packed[0], [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(packed[1], [0.25, -0.5, 0.5, 0.0]);
    }

    #[test]
    fn every_catalog_effect_accepts_exact_arity_and_rejects_short_and_long() {
        let descriptors = crate::passes::shipped_effect_descriptors();
        assert!(!descriptors.is_empty(), "descriptors must be populated");
        for desc in descriptors {
            let expected = desc.float_count();

            // Exact arity succeeds
            let exact = vec![0.5f32; expected];
            let res = validate_and_pack_effect_params(desc.id, &exact);
            assert!(
                res.is_ok(),
                "effect '{}' with exact params length {expected} failed: {res:?}",
                desc.id
            );

            // Short input (empty or count - 1) is rejected
            if expected > 0 {
                let empty: Vec<f32> = Vec::new();
                let err_empty = validate_and_pack_effect_params(desc.id, &empty)
                    .expect_err(&format!("effect '{}' should reject empty params", desc.id));
                match err_empty {
                    CompositorError::InvalidEffectParams {
                        ref effect,
                        expected: exp,
                        actual,
                    } => {
                        assert_eq!(effect, desc.id);
                        assert_eq!(exp, expected);
                        assert_eq!(actual, 0);
                    }
                    other => panic!("unexpected error variant: {other:?}"),
                }

                let short = vec![0.5f32; expected - 1];
                let err_short = validate_and_pack_effect_params(desc.id, &short)
                    .expect_err(&format!("effect '{}' should reject short params", desc.id));
                match err_short {
                    CompositorError::InvalidEffectParams {
                        ref effect,
                        expected: exp,
                        actual,
                    } => {
                        assert_eq!(effect, desc.id);
                        assert_eq!(exp, expected);
                        assert_eq!(actual, expected - 1);
                    }
                    other => panic!("unexpected error variant: {other:?}"),
                }
            }

            // Long input (count + 1) is rejected
            let long = vec![0.5f32; expected + 1];
            let err_long = validate_and_pack_effect_params(desc.id, &long)
                .expect_err(&format!("effect '{}' should reject long params", desc.id));
            match err_long {
                CompositorError::InvalidEffectParams {
                    ref effect,
                    expected: exp,
                    actual,
                } => {
                    assert_eq!(effect, desc.id);
                    assert_eq!(exp, expected);
                    assert_eq!(actual, expected + 1);
                }
                other => panic!("unexpected error variant: {other:?}"),
            }
        }
    }
}
