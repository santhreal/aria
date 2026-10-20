use super::pipelines::{PassRegistry, build_blended_pipeline, build_effect_pipeline};
use super::{Bound, PassCache};
use crate::color_pass;

/// A full-screen quad that samples `tex` at its UV: the source of the plain,
/// additive, and replacing blits.
const BLIT_WGSL: &str = r#"
struct VsOut { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> }
@vertex fn vs(@builtin(vertex_index) vi: u32) -> VsOut {
    var positions = array<vec2<f32>, 6>(
        vec2(-1.0,-1.0), vec2(1.0,-1.0), vec2(-1.0,1.0),
        vec2(-1.0,1.0), vec2(1.0,-1.0), vec2(1.0,1.0));
    var uvs = array<vec2<f32>, 6>(
        vec2(0.0,1.0), vec2(1.0,1.0), vec2(0.0,0.0),
        vec2(0.0,0.0), vec2(1.0,1.0), vec2(1.0,0.0));
    var o: VsOut;
    o.pos = vec4(positions[vi], 0.0, 1.0);
    o.uv = uvs[vi];
    return o;
}
@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@fragment fn fs(in: VsOut) -> @location(0) vec4<f32> {
    return textureSample(tex, samp, in.uv);
}
"#;

/// Alpha-over a full-canvas offscreen texture onto `canvas_view`.
pub(crate) fn blit_premultiplied_to_canvas(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    registry: &PassRegistry,
    cache: &PassCache,
    source: &wgpu::TextureView,
    canvas_view: &wgpu::TextureView,
) {
    let bind_group = cache.bind_group(
        device,
        &registry.blit_layout,
        "aria.blit.bg",
        &[Bound::Texture(source), Bound::Sampler(&registry.sampler)],
    );
    color_pass::draw_quad(
        encoder,
        canvas_view,
        color_pass::KEEP,
        registry.blit_pipeline(),
        &bind_group,
    );
}

/// Replace a full-canvas render target with `source`.
pub(crate) fn blit_replace(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    registry: &PassRegistry,
    cache: &PassCache,
    source: &wgpu::TextureView,
    output: &wgpu::TextureView,
) {
    let bind_group = cache.bind_group(
        device,
        &registry.blit_layout,
        "aria.blit_replace.bg",
        &[Bound::Texture(source), Bound::Sampler(&registry.sampler)],
    );
    color_pass::draw_quad(
        encoder,
        output,
        color_pass::CLEAR,
        registry.replace_pipeline(),
        &bind_group,
    );
}

pub(super) fn build_blit_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    build_effect_pipeline(device, "aria.blit", layout, BLIT_WGSL)
}

/// Premultiplied blit with UV offset (shadow placement onto the canvas).
pub(super) fn build_offset_blit_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    build_effect_pipeline(
        device,
        "aria.offset_blit",
        layout,
        include_str!("../../shaders/style_offset_blit.wgsl"),
    )
}

/// Weighted additive blit for motion-blur accumulation: `accum += src * w`
/// with One/One for both color and alpha into a cleared target.
pub(crate) fn blit_weighted_additive(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    registry: &PassRegistry,
    cache: &PassCache,
    source: &wgpu::TextureView,
    accum: &wgpu::TextureView,
    weight: f32,
) {
    let bind_group = cache.bind_group(
        device,
        &registry.accumulate_blit_layout,
        "aria.accumulate.bg",
        &[
            Bound::Texture(source),
            Bound::Sampler(&registry.sampler),
            Bound::Uniform(bytemuck::bytes_of(&[weight, 0.0f32, 0.0, 0.0])),
        ],
    );
    color_pass::draw_quad(
        encoder,
        accum,
        color_pass::KEEP,
        registry.accumulate_blit_pipeline(),
        &bind_group,
    );
}

/// Additive premultiplied blit (glow): color One/One, alpha One/OneMinusSrcAlpha.
pub(super) fn build_additive_blit_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    const ADDITIVE: wgpu::BlendState = wgpu::BlendState {
        color: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        },
        alpha: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
            operation: wgpu::BlendOperation::Add,
        },
    };
    build_blended_pipeline(
        device,
        "aria.additive_blit",
        layout,
        BLIT_WGSL,
        Some(ADDITIVE),
    )
}

pub(super) fn build_accumulate_blit_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    const ACCUM: wgpu::BlendState = wgpu::BlendState {
        color: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        },
        alpha: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        },
    };
    build_blended_pipeline(
        device,
        "aria.accumulate_blit",
        layout,
        include_str!("../../shaders/accumulate_blit.wgsl"),
        Some(ACCUM),
    )
}

pub(super) fn build_replace_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    build_blended_pipeline(device, "aria.blit_replace", layout, BLIT_WGSL, None)
}
