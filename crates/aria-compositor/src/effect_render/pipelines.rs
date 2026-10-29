use crate::color_pass::ReplacePipeline;
use crate::passes::{GAUSSIAN_BLUR, TransitionPass, WipeDirection, effect_passes, transition_pass};
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, OnceLock};

use super::RT_FORMAT;
use super::blend::{BLEND_MODE_COUNT, BlendShader, build_blend_pipeline, build_blend_shader};
use super::blit::{
    BLIT_WGSL, build_accumulate_blit_pipeline, build_additive_blit_pipeline, build_blit_pipeline,
    build_offset_blit_pipeline,
};

static EFFECT_PIPELINE_INDEX_BY_ID: LazyLock<HashMap<&'static str, usize>> = LazyLock::new(|| {
    effect_passes()
        .iter()
        .enumerate()
        .map(|(index, pass)| (pass.id, index))
        .collect()
});

/// Device-shared, lazily-compiled GPU pipelines and layouts for built-in
/// catalog effect, transition, blend, style, and matte passes.
pub(crate) struct SharedPassPipelines {
    device: wgpu::Device,
    /// Drawn for an id with no registry row: the frame, unchanged.
    passthrough: OnceLock<ReplacePipeline>,
    /// One entry per row of [`effect_passes`], in registry order; each entry
    /// holds that row's shaders built on first dispatch. Adding an effect adds
    /// a row, not a field here.
    effects: Box<[Box<[OnceLock<ReplacePipeline>]>]>,
    crossfade: OnceLock<ReplacePipeline>,
    dip: OnceLock<ReplacePipeline>,
    wipe: OnceLock<ReplacePipeline>,
    slide: OnceLock<ReplacePipeline>,
    /// Track-matte cut: two textures in, the content cut by the stencil out.
    matte: OnceLock<ReplacePipeline>,
    /// One mask combined with the stack above it: two coverages in, the
    /// combined coverage out.
    mask_stack: OnceLock<ReplacePipeline>,
    grade: OnceLock<ReplacePipeline>,
    lut: OnceLock<ReplacePipeline>,
    pub effect_layout: wgpu::BindGroupLayout,
    pub lut_layout: wgpu::BindGroupLayout,
    pub transition_layout: wgpu::BindGroupLayout,
    pub sampler: wgpu::Sampler,
    /// Full-canvas blit of an offscreen texture (premultiplied src-over).
    blit_pipeline: OnceLock<wgpu::RenderPipeline>,
    /// Full-canvas replacement blit for canvas-wide passes.
    replace_pipeline: OnceLock<ReplacePipeline>,
    pub blit_layout: wgpu::BindGroupLayout,
    /// Shared WGSL shader module and pipeline layout for dst-sampling blend modes.
    blend_shader: OnceLock<BlendShader>,
    /// Specialized dst-sampling blend pipelines (one per BlendMode).
    blend_pipelines: [OnceLock<ReplacePipeline>; BLEND_MODE_COUNT],
    pub blend_layout: wgpu::BindGroupLayout,
    /// Layer-alpha silhouette tint (shadow/glow seed).
    silhouette_pipeline: OnceLock<ReplacePipeline>,
    /// Premultiplied blit with UV offset (shadow placement).
    offset_blit_pipeline: OnceLock<wgpu::RenderPipeline>,
    pub offset_blit_layout: wgpu::BindGroupLayout,
    /// Additive premultiplied blit (glow).
    additive_blit_pipeline: OnceLock<wgpu::RenderPipeline>,
    /// Weighted additive blit for motion-blur sample accumulation.
    accumulate_blit_pipeline: OnceLock<wgpu::RenderPipeline>,
    pub accumulate_blit_layout: wgpu::BindGroupLayout,
    /// Harden blurred silhouette into an outside stroke (outline).
    harden_pipeline: OnceLock<wgpu::RenderPipeline>,
    pub harden_layout: wgpu::BindGroupLayout,
}

/// GPU pipelines for catalog effect, transition, and matte passes, plus
/// per-compositor runtime pass state.
pub(crate) struct PassRegistry {
    shared: Arc<SharedPassPipelines>,
    /// Pipelines, layouts, and targets for passes registered while the process
    /// runs, built the first time a frame draws each one.
    pub(super) runtime: super::registered::RuntimePipelines,
}

impl std::ops::Deref for PassRegistry {
    type Target = SharedPassPipelines;

    fn deref(&self) -> &Self::Target {
        &self.shared
    }
}

/// The bindings every effect pass reads: the input texture, the shared
/// sampler, and that pass's uniforms. The count is public to the pass cache,
/// whose reuse key must cover every one of them; a fourth binding here fails
/// that crate's compile-time check until the key covers it too.
pub(super) const EFFECT_BINDINGS: usize = 3;

impl SharedPassPipelines {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
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

        let effects: Box<[Box<[OnceLock<ReplacePipeline>]>]> = effect_passes()
            .iter()
            .map(|pass| pass.shaders.iter().map(|_| OnceLock::new()).collect())
            .collect();

        Self {
            device: device.clone(),
            passthrough: OnceLock::new(),
            effects,
            crossfade: OnceLock::new(),
            dip: OnceLock::new(),
            wipe: OnceLock::new(),
            slide: OnceLock::new(),
            matte: OnceLock::new(),
            mask_stack: OnceLock::new(),
            grade: OnceLock::new(),
            lut: OnceLock::new(),
            effect_layout,
            lut_layout,
            transition_layout,
            sampler,
            blit_pipeline: OnceLock::new(),
            replace_pipeline: OnceLock::new(),
            blit_layout,
            blend_shader: OnceLock::new(),
            blend_pipelines: std::array::from_fn(|_| OnceLock::new()),
            blend_layout,
            silhouette_pipeline: OnceLock::new(),
            offset_blit_pipeline: OnceLock::new(),
            offset_blit_layout,
            additive_blit_pipeline: OnceLock::new(),
            accumulate_blit_pipeline: OnceLock::new(),
            accumulate_blit_layout,
            harden_pipeline: OnceLock::new(),
            harden_layout,
        }
    }

    pub(crate) fn compiled_count(&self) -> usize {
        let (replacing, blending) = self.cells();
        replacing.filter(|cell| cell.get().is_some()).count()
            + blending.filter(|cell| cell.get().is_some()).count()
    }

    /// Every pipeline cell: the replacing ones, then the blending ones. The
    /// destructure names every field, so a new field does not compile until
    /// it is sorted into one of the two.
    fn cells(
        &self,
    ) -> (
        impl Iterator<Item = &OnceLock<ReplacePipeline>>,
        impl Iterator<Item = &OnceLock<wgpu::RenderPipeline>>,
    ) {
        let Self {
            device: _,
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
            effect_layout: _,
            lut_layout: _,
            transition_layout: _,
            sampler: _,
            blit_pipeline,
            replace_pipeline,
            blit_layout: _,
            blend_shader: _,
            blend_pipelines,
            blend_layout: _,
            silhouette_pipeline,
            offset_blit_pipeline,
            offset_blit_layout: _,
            additive_blit_pipeline,
            accumulate_blit_pipeline,
            accumulate_blit_layout: _,
            harden_pipeline,
            harden_layout: _,
        } = self;
        let replacing = [
            passthrough,
            crossfade,
            dip,
            wipe,
            slide,
            matte,
            mask_stack,
            grade,
            lut,
            replace_pipeline,
            silhouette_pipeline,
        ]
        .into_iter()
        .chain(effects.iter().flat_map(|row| row.iter()))
        .chain(blend_pipelines);
        let blending = [
            blit_pipeline,
            offset_blit_pipeline,
            additive_blit_pipeline,
            accumulate_blit_pipeline,
            harden_pipeline,
        ]
        .into_iter();
        (replacing, blending)
    }

    fn passthrough(&self) -> &ReplacePipeline {
        self.passthrough.get_or_init(|| {
            build_effect_pipeline(
                &self.device,
                "aria.passthrough",
                &self.effect_layout,
                include_str!("../../shaders/effect_passthrough.wgsl"),
            )
        })
    }

    fn effect_pass(&self, row: usize, index: usize) -> &ReplacePipeline {
        self.effects[row][index].get_or_init(|| {
            let pass = &effect_passes()[row];
            build_effect_pipeline(
                &self.device,
                &format!("aria.effect.{}.{index}", pass.id),
                &self.effect_layout,
                pass.shaders[index],
            )
        })
    }

    /// The first pass of `id`. An id with no row never reaches here — the chain
    /// skips it as a no-op — so the passthrough is a last resort, not a path.
    pub(super) fn effect_pipeline(&self, id: &str) -> &ReplacePipeline {
        match EFFECT_PIPELINE_INDEX_BY_ID.get(id) {
            Some(&row) if !self.effects[row].is_empty() => self.effect_pass(row, 0),
            _ => self.passthrough(),
        }
    }

    /// The separable blur's horizontal and vertical passes. Layer styles blur
    /// with the same two passes as the effect, so both read one registry row.
    pub(super) fn blur_passes(&self) -> (&ReplacePipeline, &ReplacePipeline) {
        let Some(&row) = EFFECT_PIPELINE_INDEX_BY_ID.get(GAUSSIAN_BLUR) else {
            panic!("the '{GAUSSIAN_BLUR}' row must declare its H and V shaders");
        };
        assert_eq!(
            self.effects[row].len(),
            2,
            "the '{GAUSSIAN_BLUR}' row must declare its H and V shaders"
        );
        (self.effect_pass(row, 0), self.effect_pass(row, 1))
    }

    pub(crate) fn blend_pipeline(&self, mode: crate::layer::BlendMode) -> &ReplacePipeline {
        self.blend_pipelines[mode.shader_id() as usize].get_or_init(|| {
            let shader = self
                .blend_shader
                .get_or_init(|| build_blend_shader(&self.device, &self.blend_layout));
            build_blend_pipeline(&self.device, shader, mode)
        })
    }

    pub(super) fn transition_pipeline(&self, id: &str) -> &ReplacePipeline {
        // One pipeline per variant kind, not per row: dip switches black/white
        // and wipe switches its four edges on the mode slot, so the sources come
        // from a representative variant of each kind.
        match transition_pass(id) {
            TransitionPass::Crossfade => self.crossfade.get_or_init(|| {
                build_effect_pipeline(
                    &self.device,
                    "aria.crossfade",
                    &self.transition_layout,
                    TransitionPass::Crossfade.shader(),
                )
            }),
            TransitionPass::Dip { .. } => self.dip.get_or_init(|| {
                build_effect_pipeline(
                    &self.device,
                    "aria.dip",
                    &self.transition_layout,
                    TransitionPass::Dip { white: false }.shader(),
                )
            }),
            TransitionPass::Wipe { .. } => self.wipe.get_or_init(|| {
                build_effect_pipeline(
                    &self.device,
                    "aria.wipe",
                    &self.transition_layout,
                    TransitionPass::Wipe {
                        direction: WipeDirection::Left,
                    }
                    .shader(),
                )
            }),
            TransitionPass::Slide => self.slide.get_or_init(|| {
                build_effect_pipeline(
                    &self.device,
                    "aria.slide",
                    &self.transition_layout,
                    TransitionPass::Slide.shader(),
                )
            }),
        }
    }

    pub(super) fn matte(&self) -> &ReplacePipeline {
        self.matte.get_or_init(|| {
            build_effect_pipeline(
                &self.device,
                "aria.matte",
                &self.transition_layout,
                include_str!("../../shaders/matte.wgsl"),
            )
        })
    }

    pub(super) fn mask_stack(&self) -> &ReplacePipeline {
        self.mask_stack.get_or_init(|| {
            build_effect_pipeline(
                &self.device,
                "aria.mask_stack",
                &self.transition_layout,
                include_str!("../../shaders/mask_stack.wgsl"),
            )
        })
    }

    pub(super) fn grade(&self) -> &ReplacePipeline {
        self.grade.get_or_init(|| {
            build_effect_pipeline(
                &self.device,
                "aria.canvas_grade",
                &self.effect_layout,
                &format!(
                    "{}\n{}",
                    *crate::grade::GRADE_WGSL,
                    include_str!("../../shaders/canvas_grade.wgsl")
                ),
            )
        })
    }

    pub(super) fn lut(&self) -> &ReplacePipeline {
        self.lut.get_or_init(|| {
            build_effect_pipeline(
                &self.device,
                "aria.lut",
                &self.lut_layout,
                include_str!("../../shaders/lut.wgsl"),
            )
        })
    }

    pub(super) fn blit_pipeline(&self) -> &wgpu::RenderPipeline {
        self.blit_pipeline
            .get_or_init(|| build_blit_pipeline(&self.device, &self.blit_layout))
    }

    pub(super) fn replace_pipeline(&self) -> &ReplacePipeline {
        self.replace_pipeline.get_or_init(|| {
            build_effect_pipeline(
                &self.device,
                "aria.blit_replace",
                &self.blit_layout,
                BLIT_WGSL,
            )
        })
    }

    pub(super) fn silhouette_pipeline(&self) -> &ReplacePipeline {
        self.silhouette_pipeline.get_or_init(|| {
            build_effect_pipeline(
                &self.device,
                "aria.style_silhouette",
                &self.effect_layout,
                include_str!("../../shaders/style_silhouette.wgsl"),
            )
        })
    }

    pub(super) fn offset_blit_pipeline(&self) -> &wgpu::RenderPipeline {
        self.offset_blit_pipeline
            .get_or_init(|| build_offset_blit_pipeline(&self.device, &self.offset_blit_layout))
    }

    pub(super) fn additive_blit_pipeline(&self) -> &wgpu::RenderPipeline {
        self.additive_blit_pipeline
            .get_or_init(|| build_additive_blit_pipeline(&self.device, &self.blit_layout))
    }

    pub(super) fn accumulate_blit_pipeline(&self) -> &wgpu::RenderPipeline {
        self.accumulate_blit_pipeline.get_or_init(|| {
            build_accumulate_blit_pipeline(&self.device, &self.accumulate_blit_layout)
        })
    }

    pub(super) fn harden_pipeline(&self) -> &wgpu::RenderPipeline {
        self.harden_pipeline.get_or_init(|| {
            build_blended_pipeline(
                &self.device,
                "aria.style_harden",
                &self.harden_layout,
                include_str!("../../shaders/style_harden.wgsl"),
                PREMULT_OVER,
            )
        })
    }
}

impl PassRegistry {
    pub(crate) fn from_shared(shared: Arc<SharedPassPipelines>) -> Self {
        Self {
            shared,
            runtime: super::registered::RuntimePipelines::default(),
        }
    }

    /// Pipelines built for registered passes since this registry was created.
    pub(crate) fn registered_pipeline_builds(&self) -> u64 {
        self.runtime.builds()
    }
}

/// The effect layout's entries, in binding order. One list, so the count the
/// cache keys against and the layout the passes are built from cannot drift.
fn effect_bindings() -> [wgpu::BindGroupLayoutEntry; EFFECT_BINDINGS] {
    [tex_entry(0), sampler_entry(1), uniform_entry(2)]
}

pub(crate) fn tex_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
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

pub(crate) fn sampler_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
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

/// Premultiplied src-over: the source fragment is already alpha-weighted, so
/// the color factor is `One`.
pub(crate) const PREMULT_OVER: wgpu::BlendState = wgpu::BlendState {
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
/// replaces its target.
///
/// Every caller passes a shader of this crate whose `vs` covers the target
/// with vertices `0..6` and whose `fs` never discards;
/// `tests::every_replacing_pipeline_writes_every_pixel` draws each pipeline
/// built here over two different targets.
fn build_effect_pipeline(
    device: &wgpu::Device,
    label: &str,
    layout: &wgpu::BindGroupLayout,
    source: &str,
) -> ReplacePipeline {
    let pipeline = build_full_screen_pipeline(device, label, layout, source, None);
    // SAFETY: blending is off and every channel is written below, and every
    // caller in this file passes a full-screen `vs` and an `fs` that never
    // discards.
    #[allow(unsafe_code)]
    unsafe {
        ReplacePipeline::new(pipeline)
    }
}

/// A full-screen pipeline of `source` (`vs` and `fs` entry points) that
/// writes its target with `blend`.
pub(super) fn build_blended_pipeline(
    device: &wgpu::Device,
    label: &str,
    layout: &wgpu::BindGroupLayout,
    source: &str,
    blend: wgpu::BlendState,
) -> wgpu::RenderPipeline {
    build_full_screen_pipeline(device, label, layout, source, Some(blend))
}

/// A full-screen pipeline of `source` (`vs` and `fs` entry points) that
/// writes every channel of its target with `blend`, or replaces it when
/// `blend` is `None`.
fn build_full_screen_pipeline(
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

#[cfg(test)]
#[path = "pipelines_tests.rs"]
mod tests;
