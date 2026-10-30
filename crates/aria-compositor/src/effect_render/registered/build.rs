//! Bind group layouts and pipelines for one registered effect.

use aria_models::PassStage;

use super::super::RT_FORMAT;
use super::{BuiltPass, PassPipeline};
use crate::color_pass::ReplacePipeline;
use crate::registered_graph::CompiledPass;
use crate::registered_passes::RegisteredPass;
use std::sync::Arc;

const STAGES: wgpu::ShaderStages = wgpu::ShaderStages::FRAGMENT.union(wgpu::ShaderStages::COMPUTE);

fn texture_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: STAGES,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

/// The layout of `pass`: the host globals, one texture per input beyond the
/// source frame, and a compute pass's storage output.
fn layout_entries(pass: &CompiledPass) -> Vec<wgpu::BindGroupLayoutEntry> {
    let mut entries = vec![
        texture_entry(0),
        texture_entry(1),
        wgpu::BindGroupLayoutEntry {
            binding: 2,
            visibility: STAGES,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        },
        wgpu::BindGroupLayoutEntry {
            binding: 3,
            visibility: STAGES,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        },
    ];
    let extras = pass.extra_textures() as u32;
    entries.extend((0..extras).map(|offset| texture_entry(4 + offset)));
    if pass.is_compute() {
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: 4 + extras,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::StorageTexture {
                access: wgpu::StorageTextureAccess::WriteOnly,
                format: RT_FORMAT,
                view_dimension: wgpu::TextureViewDimension::D2,
            },
            count: None,
        });
    }
    entries
}

pub(super) fn build(device: &wgpu::Device, pass: &RegisteredPass) -> Option<Arc<[BuiltPass]>> {
    let mut built = Vec::with_capacity(pass.compiled().len());
    for (index, compiled) in pass.compiled().iter().enumerate() {
        let label = format!("aria.registered.{}.{}", pass.id(), compiled.name);
        let internal = device.push_error_scope(wgpu::ErrorFilter::Internal);
        let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some(&label),
            entries: &layout_entries(compiled),
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(&label),
            source: wgpu::ShaderSource::Wgsl(compiled.source.as_str().into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some(&label),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = if let PassStage::Compute { workgroup } = compiled.stage {
            PassPipeline::Compute {
                pipeline: device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some(&label),
                    layout: Some(&pipeline_layout),
                    module: &module,
                    entry_point: Some("cs"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    cache: None,
                }),
                workgroup,
            }
        } else {
            let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(&label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs"),
                    buffers: &[],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some("fs"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: RT_FORMAT,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                multiview_mask: None,
                cache: None,
            });
            if compiled.discards {
                PassPipeline::Cleared(pipeline)
            } else {
                // SAFETY: blending is off and every channel is written above,
                // `vs` is the host prelude's, which covers the target with
                // vertices `0..6`, and `check_composed` found no `discard` in
                // the module.
                #[allow(unsafe_code)]
                let pipeline = unsafe { ReplacePipeline::new(pipeline) };
                PassPipeline::Replace(pipeline)
            }
        };
        let invalid = pollster::block_on(validation.pop());
        let failed = pollster::block_on(internal.pop());
        if let Some(error) = invalid.or(failed) {
            let message = format!("pass {index}: {error}");
            tracing::error!(
                pass = %pass.id(),
                %message,
                "the device refused a registered pass; it draws nothing"
            );
            pass.record_failure(message);
            return None;
        }
        built.push(BuiltPass { layout, pipeline });
    }
    Some(built.into())
}
