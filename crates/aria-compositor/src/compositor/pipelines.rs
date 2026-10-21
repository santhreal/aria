use std::sync::{Arc, OnceLock};

use super::glyphs::GlyphInstanceGpu;
use crate::effect_render::{
    PREMULT_OVER, PassRegistry, SharedPassPipelines, sampler_entry, tex_entry,
};
use crate::gpu::GpuContext;

use super::{CANVAS_FORMAT, Compositor, LayerPipeline};

use crate::grade::GRADE_WGSL;

/// Straight-alpha src-over: the source fragment carries un-premultiplied color,
/// so the source color is weighted by its own alpha. Used by the solid, SDF,
/// and YUV pipelines.
const STRAIGHT_OVER: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::SrcAlpha,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
    },
    alpha: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
    },
};

pub(crate) struct SharedLayerPipelines {
    device: wgpu::Device,
    pub(super) yuv_layout: wgpu::BindGroupLayout,
    pub(super) yuv_fx_layout: wgpu::BindGroupLayout,
    pub(super) rgba_layout: wgpu::BindGroupLayout,
    pub(super) rgba_fx_layout: wgpu::BindGroupLayout,
    pub(super) solid_layout: wgpu::BindGroupLayout,
    pub(super) sdf_layout: wgpu::BindGroupLayout,
    pub(super) glyphs_layout: wgpu::BindGroupLayout,
    pub(super) sampler: wgpu::Sampler,
    yuv_pipeline: OnceLock<wgpu::RenderPipeline>,
    yuv_cover_pipeline: OnceLock<wgpu::RenderPipeline>,
    yuv_fx_pipeline: OnceLock<wgpu::RenderPipeline>,
    rgba_pipeline: OnceLock<wgpu::RenderPipeline>,
    rgba_fx_pipeline: OnceLock<wgpu::RenderPipeline>,
    solid_pipeline: OnceLock<wgpu::RenderPipeline>,
    solid_cover_pipeline: OnceLock<wgpu::RenderPipeline>,
    sdf_pipeline: OnceLock<wgpu::RenderPipeline>,
    glyphs_pipeline: OnceLock<wgpu::RenderPipeline>,
    pub(super) pass_pipelines: Arc<SharedPassPipelines>,
}

impl SharedLayerPipelines {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let yuv_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("aria.yuv.bgl"),
            entries: &[
                tex_entry(0),
                tex_entry(1),
                tex_entry(2),
                sampler_entry(3),
                layer_uniform_entry(4),
            ],
        });

        let yuv_fx_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("aria.yuv_fx.bgl"),
            entries: &[
                tex_entry(0),
                tex_entry(1),
                tex_entry(2),
                sampler_entry(3),
                layer_uniform_entry(4),
                layer_uniform_entry(5),
            ],
        });

        let rgba_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("aria.rgba.bgl"),
            entries: &[tex_entry(0), sampler_entry(1), layer_uniform_entry(2)],
        });

        let rgba_fx_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("aria.rgba_fx.bgl"),
            entries: &[
                tex_entry(0),
                sampler_entry(1),
                layer_uniform_entry(2),
                layer_uniform_entry(3),
            ],
        });

        let solid_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("aria.solid.bgl"),
            entries: &[layer_uniform_entry(0)],
        });

        let sdf_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("aria.sdf.bgl"),
            entries: &[layer_uniform_entry(0)],
        });

        let glyphs_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("aria.glyphs.bgl"),
            entries: &[tex_entry(0), sampler_entry(1), layer_uniform_entry(2)],
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("aria.sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        Self {
            device: device.clone(),
            yuv_layout,
            yuv_fx_layout,
            rgba_layout,
            rgba_fx_layout,
            solid_layout,
            sdf_layout,
            glyphs_layout,
            sampler,
            yuv_pipeline: OnceLock::new(),
            yuv_cover_pipeline: OnceLock::new(),
            yuv_fx_pipeline: OnceLock::new(),
            rgba_pipeline: OnceLock::new(),
            rgba_fx_pipeline: OnceLock::new(),
            solid_pipeline: OnceLock::new(),
            solid_cover_pipeline: OnceLock::new(),
            sdf_pipeline: OnceLock::new(),
            glyphs_pipeline: OnceLock::new(),
            pass_pipelines: Arc::new(SharedPassPipelines::new(device)),
        }
    }

    pub(super) fn layer_pipeline(&self, kind: &LayerPipeline) -> &wgpu::RenderPipeline {
        match kind {
            LayerPipeline::Yuv => self
                .yuv_pipeline
                .get_or_init(|| self.yuv("aria.yuv", Some(STRAIGHT_OVER))),
            LayerPipeline::YuvCover => self
                .yuv_cover_pipeline
                .get_or_init(|| self.yuv("aria.yuv.cover", None)),
            LayerPipeline::YuvFx => self.yuv_fx_pipeline.get_or_init(|| {
                let shader = self
                    .device
                    .create_shader_module(wgpu::ShaderModuleDescriptor {
                        label: Some("aria.yuv_fx.wgsl"),
                        source: wgpu::ShaderSource::Wgsl(
                            format!(
                                "{}\n{}{}",
                                *GRADE_WGSL,
                                include_str!("../../shaders/mask.wgsl"),
                                include_str!("../../shaders/yuv_fx.wgsl"),
                            )
                            .into(),
                        ),
                    });
                build_pipeline(
                    &self.device,
                    "aria.yuv_fx",
                    &self.yuv_fx_layout,
                    &shader,
                    Some(STRAIGHT_OVER),
                )
            }),
            LayerPipeline::Rgba => self.rgba_pipeline.get_or_init(|| {
                let shader = self
                    .device
                    .create_shader_module(wgpu::ShaderModuleDescriptor {
                        label: Some("aria.rgba.wgsl"),
                        source: wgsl_with_grade(include_str!("../../shaders/rgba.wgsl")),
                    });
                build_pipeline(
                    &self.device,
                    "aria.rgba",
                    &self.rgba_layout,
                    &shader,
                    Some(PREMULT_OVER),
                )
            }),
            LayerPipeline::RgbaFx => self.rgba_fx_pipeline.get_or_init(|| {
                let shader = self
                    .device
                    .create_shader_module(wgpu::ShaderModuleDescriptor {
                        label: Some("aria.rgba_fx.wgsl"),
                        source: wgpu::ShaderSource::Wgsl(
                            format!(
                                "{}\n{}{}",
                                *GRADE_WGSL,
                                include_str!("../../shaders/mask.wgsl"),
                                include_str!("../../shaders/rgba_fx.wgsl"),
                            )
                            .into(),
                        ),
                    });
                build_pipeline(
                    &self.device,
                    "aria.rgba_fx",
                    &self.rgba_fx_layout,
                    &shader,
                    Some(PREMULT_OVER),
                )
            }),
            LayerPipeline::Solid => self
                .solid_pipeline
                .get_or_init(|| self.solid("aria.solid", Some(STRAIGHT_OVER))),
            LayerPipeline::SolidCover => self
                .solid_cover_pipeline
                .get_or_init(|| self.solid("aria.solid.cover", None)),
            LayerPipeline::Sdf => self.sdf_pipeline.get_or_init(|| {
                let shader = self
                    .device
                    .create_shader_module(wgpu::ShaderModuleDescriptor {
                        label: Some("aria.shape.wgsl"),
                        source: wgsl_with_grade(include_str!("../../shaders/shape.wgsl")),
                    });
                build_pipeline(
                    &self.device,
                    "aria.sdf",
                    &self.sdf_layout,
                    &shader,
                    Some(STRAIGHT_OVER),
                )
            }),
            LayerPipeline::Glyphs => self.glyphs_pipeline.get_or_init(|| {
                let shader = self
                    .device
                    .create_shader_module(wgpu::ShaderModuleDescriptor {
                        label: Some("aria.glyph.wgsl"),
                        source: wgsl_with_grade(include_str!("../../shaders/glyph.wgsl")),
                    });
                build_instanced_pipeline(
                    &self.device,
                    "aria.glyphs",
                    &self.glyphs_layout,
                    &shader,
                    Some(PREMULT_OVER),
                    &[GlyphInstanceGpu::LAYOUT],
                )
            }),
        }
    }

    /// The YUV pipeline, blending with `blend` or replacing its target.
    fn yuv(&self, label: &str, blend: Option<wgpu::BlendState>) -> wgpu::RenderPipeline {
        let shader = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("aria.yuv.wgsl"),
                source: wgsl_with_grade(include_str!("../../shaders/yuv.wgsl")),
            });
        build_pipeline(&self.device, label, &self.yuv_layout, &shader, blend)
    }

    /// The solid-fill pipeline, blending with `blend` or replacing its target.
    fn solid(&self, label: &str, blend: Option<wgpu::BlendState>) -> wgpu::RenderPipeline {
        let shader = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("aria.solid.wgsl"),
                source: wgsl_with_grade(include_str!("../../shaders/solid.wgsl")),
            });
        build_pipeline(&self.device, label, &self.solid_layout, &shader, blend)
    }

    pub(super) fn compiled_count(&self) -> usize {
        let layers = [
            &self.yuv_pipeline,
            &self.yuv_cover_pipeline,
            &self.yuv_fx_pipeline,
            &self.rgba_pipeline,
            &self.rgba_fx_pipeline,
            &self.solid_pipeline,
            &self.solid_cover_pipeline,
            &self.sdf_pipeline,
            &self.glyphs_pipeline,
        ]
        .into_iter()
        .filter(|cell| cell.get().is_some())
        .count();
        layers + self.pass_pipelines.compiled_count()
    }
}

impl Compositor {
    /// Build a compositor backed by `gpu`'s shared, lazily-compiled pipelines.
    pub fn new(gpu: &GpuContext) -> Self {
        let pipelines = Arc::clone(
            gpu.pipelines
                .get_or_init(|| Arc::new(SharedLayerPipelines::new(&gpu.device))),
        );
        Self::with_shared_pipelines(gpu, pipelines)
    }

    pub(crate) fn with_shared_pipelines(
        gpu: &GpuContext,
        pipelines: Arc<SharedLayerPipelines>,
    ) -> Self {
        let yuv_layout = pipelines.yuv_layout.clone();
        let yuv_fx_layout = pipelines.yuv_fx_layout.clone();
        let rgba_layout = pipelines.rgba_layout.clone();
        let rgba_fx_layout = pipelines.rgba_fx_layout.clone();
        let solid_layout = pipelines.solid_layout.clone();
        let sdf_layout = pipelines.sdf_layout.clone();
        let glyphs_layout = pipelines.glyphs_layout.clone();
        let sampler = pipelines.sampler.clone();
        let pass_registry = PassRegistry::from_shared(Arc::clone(&pipelines.pass_pipelines));

        Self {
            pipelines,
            yuv_layout,
            yuv_fx_layout,
            rgba_layout,
            rgba_fx_layout,
            solid_layout,
            sdf_layout,
            glyphs_layout,
            sampler,
            target: None,
            frame_target_views: Vec::new(),
            pass_registry,
            offscreen: None,
            references: crate::effect_render::CanvasTargets::new("aria.reference"),
            nests: crate::effect_render::CanvasTargets::new("aria.nest"),
            layer_passes: crate::effect_render::PassCache::new(gpu.queue.clone()),
            plane_textures: std::cell::RefCell::new(super::upload::PlaneTexturePool::default()),
            rgba_textures: std::cell::RefCell::new(super::upload::RgbaTextureCache::default()),
            lut_cache: super::lru::FrameLru::default(),
            glyph_atlases: std::cell::RefCell::new(super::lru::FrameLru::default()),
            lut_upload_scratch: Vec::new(),
            premul_scratch: std::cell::RefCell::new(Vec::new()),
            glyph_instances: std::cell::RefCell::new(Vec::new()),
            readback_count: 0,
            #[cfg(target_vendor = "apple")]
            metal_import: crate::metal_import::MetalSurfaceImporter::new(gpu),
            #[cfg(target_os = "windows")]
            dx12_import: crate::dx12_import::Dx12SurfaceImporter::new(gpu),
            #[cfg(target_os = "linux")]
            vulkan_import: crate::vulkan_import::VulkanSurfaceImporter::new(gpu),
        }
    }
}

/// A uniform buffer the vertex stage reads too: a layer pipeline places its
/// quad from it.
fn layer_uniform_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn build_pipeline(
    device: &wgpu::Device,
    label: &str,
    layout: &wgpu::BindGroupLayout,
    shader: &wgpu::ShaderModule,
    blend: Option<wgpu::BlendState>,
) -> wgpu::RenderPipeline {
    build_instanced_pipeline(device, label, layout, shader, blend, &[])
}

fn build_instanced_pipeline(
    device: &wgpu::Device,
    label: &str,
    layout: &wgpu::BindGroupLayout,
    shader: &wgpu::ShaderModule,
    blend: Option<wgpu::BlendState>,
    vertex_buffers: &[wgpu::VertexBufferLayout<'_>],
) -> wgpu::RenderPipeline {
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts: &[Some(layout)],
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs"),
            buffers: vertex_buffers,
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs"),
            targets: &[Some(wgpu::ColorTargetState {
                format: CANVAS_FORMAT,
                blend,
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        multiview_mask: None,
        cache: None,
    })
}

fn wgsl_with_grade(shader: &str) -> wgpu::ShaderSource<'static> {
    wgpu::ShaderSource::Wgsl(std::borrow::Cow::Owned(format!(
        "{}\n{shader}",
        *GRADE_WGSL
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer::{CompositeLayer, CompositorConfig, LayerPlacement};

    /// WHY: `Compositor::new` and `PassRegistry::new` previously compiled all 7
    /// layer pipelines and all 50 built-in effect, transition, blend, style, and
    /// blit pipelines eagerly before a single frame was drawn, spending >70% of
    /// editor startup CPU cycles in Naga + LLVM JIT (`create_render_pipeline`)
    /// and ~60 MiB of idle RSS. Built-in pipelines now compile lazily on first
    /// draw and are shared across `Compositor` instances on the same `GpuContext`.
    ///
    /// Does NOT catch: driver-internal shader cache disk I/O on first draw.
    #[test]
    fn compositor_compiles_builtin_pipelines_lazily_and_shares_across_handles() {
        let gpu = GpuContext::new_headless_blocking().expect("headless GPU");
        let shared = Arc::new(SharedLayerPipelines::new(&gpu.device));
        let mut first = Compositor::with_shared_pipelines(&gpu, Arc::clone(&shared));
        assert_eq!(
            first.builtin_pipeline_builds(),
            0,
            "new Compositor must not compile built-in pipelines before rendering"
        );

        let cfg = CompositorConfig::new(16, 16);
        let solid = CompositeLayer::solid([255, 0, 0, 255], LayerPlacement::full_canvas(&cfg));
        first.render(&gpu, &cfg, &[solid]).expect("solid render");
        assert_eq!(
            first.builtin_pipeline_builds(),
            1,
            "rendering a solid layer must compile only the solid pipeline"
        );

        let mut second = Compositor::with_shared_pipelines(&gpu, Arc::clone(&shared));
        assert_eq!(
            second.builtin_pipeline_builds(),
            1,
            "second Compositor sharing pipelines must reuse the compiled solid pipeline"
        );
        second
            .render(&gpu, &cfg, &[solid])
            .expect("second solid render");
        assert_eq!(second.builtin_pipeline_builds(), 1);
    }
}
