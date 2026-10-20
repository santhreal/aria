//! Dst-sampling blend composite pass (non-`Normal` layer blend modes).

use bytemuck::{Pod, Zeroable};

use crate::color_pass;
use crate::layer::BlendMode;

use super::pipelines::PassRegistry;
use super::{Bound, PassCache, RT_FORMAT};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct BlendUniforms {
    mode: u32,
    _pad: [u32; 3],
}

/// Composite premultiplied `src` over a canvas `dst` snapshot into `output`
/// using `mode`. Fixed-function blending is disabled — the shader writes the
/// final premultiplied result.
pub(crate) fn run_blend_composite(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    registry: &PassRegistry,
    cache: &PassCache,
    dst_snapshot: &wgpu::TextureView,
    src_layer: &wgpu::TextureView,
    output: &wgpu::TextureView,
    mode: BlendMode,
) {
    let bind_group = cache.bind_group(
        device,
        &registry.blend_layout,
        "aria.blend.bg",
        &[
            Bound::Texture(dst_snapshot),
            Bound::Texture(src_layer),
            Bound::Sampler(&registry.sampler),
            Bound::Uniform(bytemuck::bytes_of(&BlendUniforms {
                mode: mode.shader_id(),
                _pad: [0; 3],
            })),
        ],
    );

    // Every pixel is written; Load preserves unread edges if any.
    color_pass::draw_quad(
        encoder,
        output,
        color_pass::KEEP,
        registry.blend_pipeline(mode),
        &bind_group,
    );
}

pub(super) const BLEND_MODE_COUNT: usize = BlendMode::ALL.len();

pub(super) fn build_blend_shader(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
) -> (wgpu::ShaderModule, wgpu::PipelineLayout) {
    let source = include_str!("../../shaders/blend_composite.wgsl");
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("aria.blend_composite"),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("aria.blend_composite"),
        bind_group_layouts: &[Some(layout)],
        immediate_size: 0,
    });
    (shader, pipeline_layout)
}

pub(super) fn build_blend_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    pipeline_layout: &wgpu::PipelineLayout,
    mode: BlendMode,
) -> wgpu::RenderPipeline {
    let constants = [("0", mode.shader_id() as f64)];
    let compilation_options = wgpu::PipelineCompilationOptions {
        constants: &constants,
        zero_initialize_workgroup_memory: true,
    };
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(&format!("aria.blend_composite.{mode:?}")),
        layout: Some(pipeline_layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs"),
            buffers: &[],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs"),
            targets: &[Some(wgpu::ColorTargetState {
                format: RT_FORMAT,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options,
        }),
        multiview_mask: None,
        cache: None,
    })
}
