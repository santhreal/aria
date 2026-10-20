use crate::passes::{GAUSSIAN_BLUR, TransitionPass, WipeDirection, effect_passes, transition_pass};
use std::collections::HashMap;
use std::sync::LazyLock;

use super::RT_FORMAT;
use super::blend::{BLEND_MODE_COUNT, build_blend_pipelines};
use super::blit::{
    build_accumulate_blit_pipeline, build_additive_blit_pipeline, build_blit_pipeline,
    build_offset_blit_pipeline, build_replace_pipeline,
};

static EFFECT_PIPELINE_INDEX_BY_ID: LazyLock<HashMap<&'static str, usize>> = LazyLock::new(|| {
    effect_passes()
        .iter()
        .enumerate()
        .map(|(index, pass)| (pass.id, index))
        .collect()
});

/// GPU pipelines for catalog effect, transition, and matte passes.
pub(crate) struct PassRegistry {
    /// Drawn for an id with no registry row: the frame, unchanged.
    pub passthrough: wgpu::RenderPipeline,
    /// One entry per row of [`effect_passes`], in registry order; each entry
    /// holds that row's shaders built in dispatch order. Adding an effect adds
    /// a row, not a field here.
    effects: Box<[Box<[wgpu::RenderPipeline]>]>,
    pub crossfade: wgpu::RenderPipeline,
    pub dip: wgpu::RenderPipeline,
    pub wipe: wgpu::RenderPipeline,
    pub slide: wgpu::RenderPipeline,
    /// Track-matte cut: two textures in, the content cut by the stencil out.
    pub matte: wgpu::RenderPipeline,
    /// One mask combined with the stack above it: two coverages in, the
    /// combined coverage out.
    pub mask_stack: wgpu::RenderPipeline,
    pub grade: wgpu::RenderPipeline,
    pub lut: wgpu::RenderPipeline,
    pub effect_layout: wgpu::BindGroupLayout,
    pub lut_layout: wgpu::BindGroupLayout,
    pub transition_layout: wgpu::BindGroupLayout,
    pub sampler: wgpu::Sampler,
    /// Full-canvas blit of an offscreen texture (premultiplied src-over).
    pub blit_pipeline: wgpu::RenderPipeline,
    /// Full-canvas replacement blit for canvas-wide passes.
    pub replace_pipeline: wgpu::RenderPipeline,
    pub blit_layout: wgpu::BindGroupLayout,
    /// Specialized dst-sampling blend pipelines (one per BlendMode).
    pub blend_pipelines: [wgpu::RenderPipeline; BLEND_MODE_COUNT],
    pub blend_layout: wgpu::BindGroupLayout,
    /// Layer-alpha silhouette tint (shadow/glow seed).
    pub silhouette_pipeline: wgpu::RenderPipeline,
    /// Premultiplied blit with UV offset (shadow placement).
    pub offset_blit_pipeline: wgpu::RenderPipeline,
    pub offset_blit_layout: wgpu::BindGroupLayout,
    /// Additive premultiplied blit (glow).
    pub additive_blit_pipeline: wgpu::RenderPipeline,
    /// Weighted additive blit for motion-blur sample accumulation.
    pub accumulate_blit_pipeline: wgpu::RenderPipeline,
    pub accumulate_blit_layout: wgpu::BindGroupLayout,
    /// Harden blurred silhouette into an outside stroke (outline).
    pub harden_pipeline: wgpu::RenderPipeline,
    pub harden_layout: wgpu::BindGroupLayout,
    /// Pipelines, layouts, and targets for passes registered while the process
    /// runs, built the first time a frame draws each one.
    pub(super) runtime: super::registered::RuntimePipelines,
}

/// The bindings every effect pass reads: the input texture, the shared
/// sampler, and that pass's uniforms. The count is public to the pass cache,
/// whose reuse key must cover every one of them; a fourth binding here fails
/// that crate's compile-time check until the key covers it too.
pub(super) const EFFECT_BINDINGS: usize = 3;

impl PassRegistry {
    pub fn new(device: &wgpu::Device) -> Self {
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("aria.effect.sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let effect_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("aria.effect.bgl"),
            entries: &effect_bindings(),
        });

        let transition_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("aria.transition.bgl"),
            entries: &[
                tex_entry(0),
                tex_entry(1),
                sampler_entry(2),
                uniform_entry(3),
            ],
        });

        let blit_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("aria.blit.bgl"),
            entries: &[tex_entry(0), sampler_entry(1)],
        });

        let lut_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("aria.lut.bgl"),
            entries: &[
                tex_entry(0),
                sampler_entry(1),
                tex3d_entry(2),
                sampler_entry(3),
                uniform_entry(4),
            ],
        });

        let blend_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("aria.blend.bgl"),
            entries: &[
                tex_entry(0),
                tex_entry(1),
                sampler_entry(2),
                uniform_entry(3),
            ],
        });

        let offset_blit_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("aria.offset_blit.bgl"),
                entries: &[tex_entry(0), sampler_entry(1), uniform_entry(2)],
            });

        let accumulate_blit_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("aria.accumulate_blit.bgl"),
                entries: &[tex_entry(0), sampler_entry(1), uniform_entry(2)],
            });

        let harden_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("aria.harden.bgl"),
            entries: &[
                tex_entry(0),
                tex_entry(1),
                sampler_entry(2),
                uniform_entry(3),
            ],
        });

        let passthrough = build_effect_pipeline(
            device,
            "aria.passthrough",
            &effect_layout,
            include_str!("../../shaders/effect_passthrough.wgsl"),
        );
        let effects: Box<[Box<[wgpu::RenderPipeline]>]> = effect_passes()
            .iter()
            .map(|pass| {
                pass.shaders
                    .iter()
                    .enumerate()
                    .map(|(index, source)| {
                        build_effect_pipeline(
                            device,
                            &format!("aria.effect.{}.{index}", pass.id),
                            &effect_layout,
                            source,
                        )
                    })
                    .collect()
            })
            .collect();
        // One pipeline per variant kind, not per row: dip switches black/white
        // and wipe switches its four edges on the mode slot, so the sources come
        // from a representative variant of each kind.
        let crossfade = build_effect_pipeline(
            device,
            "aria.crossfade",
            &transition_layout,
            TransitionPass::Crossfade.shader(),
        );
        let wipe = build_effect_pipeline(
            device,
            "aria.wipe",
            &transition_layout,
            TransitionPass::Wipe {
                direction: WipeDirection::Left,
            }
            .shader(),
        );
        let dip = build_effect_pipeline(
            device,
            "aria.dip",
            &transition_layout,
            TransitionPass::Dip { white: false }.shader(),
        );
        let slide = build_effect_pipeline(
            device,
            "aria.slide",
            &transition_layout,
            TransitionPass::Slide.shader(),
        );
        let matte = build_effect_pipeline(
            device,
            "aria.matte",
            &transition_layout,
            include_str!("../../shaders/matte.wgsl"),
        );
        let mask_stack = build_effect_pipeline(
            device,
            "aria.mask_stack",
            &transition_layout,
            include_str!("../../shaders/mask_stack.wgsl"),
        );
        let grade = build_effect_pipeline(
            device,
            "aria.canvas_grade",
            &effect_layout,
            &format!(
                "{}\n{}",
                *crate::grade::GRADE_WGSL,
                include_str!("../../shaders/canvas_grade.wgsl")
            ),
        );
        let lut = build_effect_pipeline(
            device,
            "aria.lut",
            &lut_layout,
            include_str!("../../shaders/lut.wgsl"),
        );
        let blit_pipeline = build_blit_pipeline(device, &blit_layout);
        let replace_pipeline = build_replace_pipeline(device, &blit_layout);
        let blend_pipelines = build_blend_pipelines(device, &blend_layout);
        let silhouette_pipeline = build_effect_pipeline(
            device,
            "aria.style_silhouette",
            &effect_layout,
            include_str!("../../shaders/style_silhouette.wgsl"),
        );
        let offset_blit_pipeline = build_offset_blit_pipeline(device, &offset_blit_layout);
        let additive_blit_pipeline = build_additive_blit_pipeline(device, &blit_layout);
        let accumulate_blit_pipeline =
            build_accumulate_blit_pipeline(device, &accumulate_blit_layout);
        let harden_pipeline = build_effect_pipeline(
            device,
            "aria.style_harden",
            &harden_layout,
            include_str!("../../shaders/style_harden.wgsl"),
        );

        Self {
            passthrough,
            effects,
            crossfade,
            dip,
            wipe,
            slide,
            matte,
            mask_stack,
            grade,
            lut,
            effect_layout,
            lut_layout,
            transition_layout,
            sampler,
            blit_pipeline,
            replace_pipeline,
            blit_layout,
            blend_pipelines,
            blend_layout,
            silhouette_pipeline,
            offset_blit_pipeline,
            offset_blit_layout,
            additive_blit_pipeline,
            accumulate_blit_pipeline,
            accumulate_blit_layout,
            harden_pipeline,
            harden_layout,
            runtime: super::registered::RuntimePipelines::default(),
        }
    }

    /// Pipelines built for registered passes since this registry was created.
    pub(crate) fn registered_pipeline_builds(&self) -> u64 {
        self.runtime.builds()
    }

    /// The pipelines rendering `id`, in dispatch order; empty for an id with no
    /// registry row.
    fn effect_passes_for(&self, id: &str) -> &[wgpu::RenderPipeline] {
        EFFECT_PIPELINE_INDEX_BY_ID
            .get(id)
            .map_or(&[][..], |&row| &self.effects[row][..])
    }

    /// The first pass of `id`. An id with no row never reaches here — the chain
    /// skips it as a no-op — so the passthrough is a last resort, not a path.
    pub(super) fn effect_pipeline(&self, id: &str) -> &wgpu::RenderPipeline {
        self.effect_passes_for(id)
            .first()
            .unwrap_or(&self.passthrough)
    }

    /// The separable blur's horizontal and vertical passes. Layer styles blur
    /// with the same two passes as the effect, so both read one registry row.
    pub(super) fn blur_passes(&self) -> (&wgpu::RenderPipeline, &wgpu::RenderPipeline) {
        let passes = self.effect_passes_for(GAUSSIAN_BLUR);
        let [horizontal, vertical] = passes else {
            panic!("the '{GAUSSIAN_BLUR}' row must declare its H and V shaders");
        };
        (horizontal, vertical)
    }

    pub(crate) fn blend_pipeline(&self, mode: crate::layer::BlendMode) -> &wgpu::RenderPipeline {
        &self.blend_pipelines[mode.shader_id() as usize]
    }

    pub(super) fn transition_pipeline(&self, id: &str) -> &wgpu::RenderPipeline {
        match transition_pass(id) {
            TransitionPass::Crossfade => &self.crossfade,
            TransitionPass::Dip { .. } => &self.dip,
            TransitionPass::Wipe { .. } => &self.wipe,
            TransitionPass::Slide => &self.slide,
        }
    }
}

/// The effect layout's entries, in binding order. One list, so the count the
/// cache keys against and the layout the passes are built from cannot drift.
fn effect_bindings() -> [wgpu::BindGroupLayoutEntry; EFFECT_BINDINGS] {
    [tex_entry(0), sampler_entry(1), uniform_entry(2)]
}

fn tex_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn tex3d_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D3,
            multisampled: false,
        },
        count: None,
    }
}

fn sampler_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    }
}

fn uniform_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

pub(super) const PREMULT_OVER: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
    },
    alpha: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
    },
};

/// A full-screen pipeline of `source` (`vs` and `fs` entry points) that
/// draws over its target with [`PREMULT_OVER`].
pub(super) fn build_effect_pipeline(
    device: &wgpu::Device,
    label: &str,
    layout: &wgpu::BindGroupLayout,
    source: &str,
) -> wgpu::RenderPipeline {
    build_blended_pipeline(device, label, layout, source, Some(PREMULT_OVER))
}

/// A full-screen pipeline of `source` (`vs` and `fs` entry points) that
/// writes its target with `blend`, or replaces it when `blend` is `None`.
pub(super) fn build_blended_pipeline(
    device: &wgpu::Device,
    label: &str,
    layout: &wgpu::BindGroupLayout,
    source: &str,
    blend: Option<wgpu::BlendState>,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts: &[Some(layout)],
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs"),
            buffers: &[],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs"),
            targets: &[Some(wgpu::ColorTargetState {
                format: RT_FORMAT,
                blend,
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        multiview_mask: None,
        cache: None,
    })
}
