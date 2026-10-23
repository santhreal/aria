//! Dst-sampling blend composite pass (non-`Normal` layer blend modes).

use bytemuck::{Pod, Zeroable};

use crate::color_pass::{QuadPipeline, ReplacePipeline};
use crate::compositor::TargetPass;
use crate::layer::BlendMode;

use super::pipelines::PassRegistry;
use super::{Bound, PassCache, RT_FORMAT};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct BlendUniforms {
    mode: u32,
    _pad: [u32; 3],
}

/// Composite premultiplied `src_layer` over a canvas `dst_snapshot` onto
/// `target` using `mode`. Fixed-function blending is disabled; the shader
/// writes the final premultiplied result.
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_blend_composite_to_target<'a>(
    device: &wgpu::Device,
    registry: &'a PassRegistry,
    cache: &PassCache,
    dst_snapshot: &wgpu::TextureView,
    src_layer: &wgpu::TextureView,
    slot_mask: u8,
    target: &mut TargetPass<'a>,
    mode: BlendMode,
) {
    let bind_group = cache
        .bind_group(
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
        )
        .clone();

    target.push_quad(
        QuadPipeline::Replace(registry.blend_pipeline(mode)),
        bind_group,
        slot_mask,
    );
}

pub(super) const BLEND_MODE_COUNT: usize = BlendMode::ALL.len();

/// The dst-sampling blend shader and its pipeline layout, shared by the
/// pipeline of every blend mode.
pub(super) struct BlendShader {
    shader: wgpu::ShaderModule,
    layout: wgpu::PipelineLayout,
}

pub(super) fn build_blend_shader(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
) -> BlendShader {
    let source = include_str!("../../shaders/blend_composite.wgsl");
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("aria.blend_composite"),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("aria.blend_composite"),
        bind_group_layouts: &[Some(layout)],
        immediate_size: 0,
    });
    BlendShader { shader, layout }
}

pub(super) fn build_blend_pipeline(
    device: &wgpu::Device,
    blend: &BlendShader,
    mode: BlendMode,
) -> ReplacePipeline {
    let constants = [("0", mode.shader_id() as f64)];
    let compilation_options = wgpu::PipelineCompilationOptions {
        constants: &constants,
        zero_initialize_workgroup_memory: true,
    };
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(&format!("aria.blend_composite.{mode:?}")),
        layout: Some(&blend.layout),
        vertex: wgpu::VertexState {
            module: &blend.shader,
            entry_point: Some("vs"),
            buffers: &[],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &blend.shader,
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
    });
    // SAFETY: blending is off and every channel is written, and
    // `blend_composite.wgsl`, the one shader a `BlendShader` holds, draws the
    // full-screen quad from vertices `0..6` and never discards.
    unsafe { ReplacePipeline::new(pipeline) }
}
