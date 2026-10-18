//! Pipelines and draws for passes registered while the process runs.
//!
//! Each pass of a registered effect binds its own layout: the four host
//! globals, one texture per input beyond the source frame, and for a compute
//! pass the storage texture it writes. Pipelines are built the first time a
//! frame draws the effect, inside device error scopes, so a pass the device
//! rejects is logged and recorded on the effect and the effect draws nothing
//! instead of reaching the uncaptured-error handler.
//!
//! Passes run in the order the graph was planned in. A pass other than the
//! last writes a target of its own, sized by its declared output and kept
//! across frames. The last pass writes the pool slot after the effect's input:
//! a fragment pass draws into it, a compute pass writes its own target, which
//! is then copied into the slot, so the canvas-sized pool never needs storage
//! usage.

use std::cell::{Cell, RefCell};
use std::sync::Arc;

use rustc_hash::FxHashMap;

use super::RT_FORMAT;
use super::canvas_targets::CanvasTargets;
use super::pass_cache::{Bound, PassCache};
use super::pipelines::PassRegistry;
use super::pool::OffscreenPool;
use crate::passes::PassInstance;
use crate::plugin_textures::PluginTexture;
use crate::registered_graph::{CompiledInput, CompiledPass};
use crate::registered_passes::RegisteredPass;

mod build;
mod uniform;

use build::build;
use uniform::{effect_uniform, transition_uniform};

/// Most resources one registered pass binds: four host globals, one texture
/// per input, and the storage output of a compute pass.
const MAX_PASS_BINDINGS: usize = 4 + aria_models::MAX_PASS_INPUTS + 1;

/// Pipelines, intermediate targets, and texture uploads for registered passes.
#[derive(Default)]
pub(crate) struct RuntimePipelines {
    built: RefCell<FxHashMap<u64, Built>>,
    /// Outputs of passes other than a fragment last pass, by registration
    /// serial and execution index.
    targets: RefCell<FxHashMap<(u64, usize), Target>>,
    /// Plugin textures uploaded, by texture serial.
    uploads: RefCell<FxHashMap<u64, Upload>>,
    /// Bound for a layer input whose clip has no frame: one transparent texel.
    empty: RefCell<Option<Target>>,
    /// Pipelines built since the registry was created.
    builds: Cell<u64>,
}

struct Built {
    pass: Arc<RegisteredPass>,
    /// `None` when the device refused a pass; the entry stays so the build is
    /// not retried every frame.
    passes: Option<Arc<[BuiltPass]>>,
}

struct BuiltPass {
    layout: wgpu::BindGroupLayout,
    pipeline: PassPipeline,
}

enum PassPipeline {
    Render(wgpu::RenderPipeline),
    Compute {
        pipeline: wgpu::ComputePipeline,
        workgroup: [u32; 2],
    },
}

struct Target {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    size: (u32, u32),
}

struct Upload {
    /// Held so the entry can tell when no effect reads the texture any more.
    texture: Arc<PluginTexture>,
    _gpu: wgpu::Texture,
    view: wgpu::TextureView,
}

impl RuntimePipelines {
    /// Pipelines built since the registry was created. A pass is built once
    /// per registration, however many frames draw it.
    pub(crate) fn builds(&self) -> u64 {
        self.builds.get()
    }

    /// The built passes of `pass` in execution order, or `None` when the
    /// registration was dropped or the device refused a pass.
    fn get(&self, device: &wgpu::Device, pass: &Arc<RegisteredPass>) -> Option<Arc<[BuiltPass]>> {
        if pass.is_removed() {
            self.forget(pass.serial());
            return None;
        }
        if let Some(built) = self.built.borrow().get(&pass.serial()) {
            return built.passes.clone();
        }
        let passes = build(device, pass);
        self.builds
            .set(self.builds.get() + pass.compiled().len() as u64);
        let mut built = self.built.borrow_mut();
        built.retain(|_, held| !held.pass.is_removed());
        built.insert(
            pass.serial(),
            Built {
                pass: Arc::clone(pass),
                passes: passes.clone(),
            },
        );
        self.targets
            .borrow_mut()
            .retain(|(serial, _), _| built.contains_key(serial));
        self.uploads
            .borrow_mut()
            .retain(|_, upload| Arc::strong_count(&upload.texture) > 1);
        passes
    }

    fn forget(&self, serial: u64) {
        self.built.borrow_mut().remove(&serial);
        self.targets
            .borrow_mut()
            .retain(|(held, _), _| *held != serial);
    }

    /// Make every target, upload, and the empty texture `pass` reads or
    /// writes at an effect input of `width`×`height`.
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        serial: u64,
        compiled: &[CompiledPass],
        width: u32,
        height: u32,
    ) {
        let last = compiled.len().saturating_sub(1);
        let mut targets = self.targets.borrow_mut();
        for (exec, pass) in compiled.iter().enumerate() {
            if exec == last && !pass.is_compute() {
                continue;
            }
            let size = pass.output.resolve(width, height);
            let stale = targets
                .get(&(serial, exec))
                .is_none_or(|target| target.size != size);
            if stale {
                let mut usage = wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC;
                if pass.is_compute() {
                    usage |= wgpu::TextureUsages::STORAGE_BINDING;
                }
                targets.insert(
                    (serial, exec),
                    target(device, "aria.registered.target", size, usage),
                );
            }
        }
        drop(targets);
        let mut uploads = self.uploads.borrow_mut();
        for input in compiled.iter().flat_map(|pass| pass.inputs.iter()) {
            if let CompiledInput::Texture(texture) = input
                && !uploads.contains_key(&texture.serial())
            {
                uploads.insert(texture.serial(), upload(device, queue, texture));
            }
        }
        drop(uploads);
        let mut empty = self.empty.borrow_mut();
        if empty.is_none() {
            *empty = Some(target(
                device,
                "aria.registered.empty",
                (1, 1),
                wgpu::TextureUsages::TEXTURE_BINDING,
            ));
        }
    }
}

fn target(
    device: &wgpu::Device,
    label: &'static str,
    (width, height): (u32, u32),
    usage: wgpu::TextureUsages,
) -> Target {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: RT_FORMAT,
        usage,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    Target {
        texture,
        view,
        size: (width, height),
    }
}

fn upload(device: &wgpu::Device, queue: &wgpu::Queue, texture: &Arc<PluginTexture>) -> Upload {
    let size = wgpu::Extent3d {
        width: texture.width(),
        height: texture.height(),
        depth_or_array_layers: 1,
    };
    let gpu = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("aria.plugin.texture"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: RT_FORMAT,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        gpu.as_image_copy(),
        texture.premultiplied(),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(texture.width() * 4),
            rows_per_image: Some(texture.height()),
        },
        size,
    );
    let view = gpu.create_view(&wgpu::TextureViewDescriptor::default());
    Upload {
        texture: Arc::clone(texture),
        _gpu: gpu,
        view,
    }
}

/// Clear `output` to transparent: a compute pass starts from nothing, so a
/// texel its shader skips holds no earlier frame.
fn clear(encoder: &mut wgpu::CommandEncoder, output: &wgpu::TextureView) {
    let _clear = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: None,
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: output,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
}

/// Draw or dispatch one built pass with `bounds` into `output` of `size`.
fn encode(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    cache: &PassCache,
    built: &BuiltPass,
    bounds: &[Bound<'_>],
    output: &wgpu::TextureView,
    (width, height): (u32, u32),
) {
    let bind_group = cache.bind_group(device, &built.layout, "aria.registered.bg", bounds);
    match &built.pipeline {
        PassPipeline::Render(pipeline) => {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: output,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &*bind_group, &[]);
            pass.draw(0..6, 0..1);
        }
        PassPipeline::Compute {
            pipeline,
            workgroup: [x, y],
        } => {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: None,
                timestamp_writes: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &*bind_group, &[]);
            pass.dispatch_workgroups(width.div_ceil(*x), height.div_ceil(*y), 1);
        }
    }
}

/// Run every pass of the registered effect `pass` on `input`, which is pool
/// slot `in_idx` or outside the pool, reading layer inputs from
/// `references`. Returns the slot and view holding the result, or `None` when
/// nothing was drawn and the input stands.
#[allow(clippy::too_many_arguments)]
pub(super) fn run_registered_effect<'a>(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    registry: &PassRegistry,
    pool: &'a OffscreenPool,
    references: &CanvasTargets,
    input: &'a wgpu::TextureView,
    in_idx: usize,
    effect: &PassInstance,
    pass: &Arc<RegisteredPass>,
    width: u32,
    height: u32,
) -> Option<(usize, &'a wgpu::TextureView)> {
    if pass.is_cpu() {
        return crate::cpu_effect::execute_cpu_effect(
            device, encoder, registry, pool, input, in_idx, effect, pass, width, height,
        );
    }
    let runtime = &registry.runtime;
    let built = runtime.get(device, pass)?;
    let compiled = pass.compiled();
    let cache = pool.passes();
    let serial = pass.serial();
    runtime.prepare(device, &cache.queue(), serial, compiled, width, height);
    let frame_budget = cache.frame_budget();
    let started = pass.tracks_cost(frame_budget).then(std::time::Instant::now);
    let written = (in_idx + 1) % pool.slots();
    let count = compiled.len();
    {
        let targets = runtime.targets.borrow();
        let uploads = runtime.uploads.borrow();
        let empty = runtime.empty.borrow();
        let empty = &empty.as_ref()?.view;
        let view_of = |read: &CompiledInput| -> &wgpu::TextureView {
            match read {
                CompiledInput::Source => input,
                CompiledInput::Pass(exec) => targets
                    .get(&(serial, *exec))
                    .map_or(empty, |target| &target.view),
                CompiledInput::Layer(param) => effect
                    .instance
                    .as_ref()
                    .and_then(|instance| instance.layer_params.get(&**param))
                    .and_then(|clip| references.view(*clip))
                    .unwrap_or(empty),
                CompiledInput::Texture(texture) => uploads
                    .get(&texture.serial())
                    .map_or(empty, |upload| &upload.view),
            }
        };
        for (exec, (step, built)) in compiled.iter().zip(built.iter()).enumerate() {
            let last = exec + 1 == count;
            let size = step.output.resolve(width, height);
            let output = if last && !step.is_compute() {
                pool.view(written)
            } else {
                &targets.get(&(serial, exec))?.view
            };
            let words = effect_uniform(size.0, size.1, effect.time, exec, count, &effect.params);
            let mut bounds: [Bound<'_>; MAX_PASS_BINDINGS] =
                std::array::from_fn(|_| Bound::Sampler(&registry.sampler));
            bounds[0] = Bound::Texture(step.inputs.first().map_or(input, &view_of));
            bounds[1] = Bound::Texture(input);
            bounds[3] = Bound::Uniform(bytemuck::cast_slice(&words));
            let mut used = 4;
            for read in step.inputs.iter() {
                if !matches!(read, CompiledInput::Source) {
                    bounds[used] = Bound::Texture(view_of(read));
                    used += 1;
                }
            }
            if step.is_compute() {
                bounds[used] = Bound::Texture(output);
                used += 1;
                clear(encoder, output);
            }
            encode(device, encoder, cache, built, &bounds[..used], output, size);
            if last && step.is_compute() {
                let target = targets.get(&(serial, exec))?;
                encoder.copy_texture_to_texture(
                    target.texture.as_image_copy(),
                    pool.texture(written).as_image_copy(),
                    wgpu::Extent3d {
                        width: size.0,
                        height: size.1,
                        depth_or_array_layers: 1,
                    },
                );
            }
        }
    }
    if let Some(started) = started {
        pass.record_cost(started.elapsed(), frame_budget);
    }
    Some((written, pool.view(written)))
}

/// Draw the registered transition `pass` into `output`. Returns `false` when
/// nothing was drawn, so the caller falls back to a shipped transition.
#[allow(clippy::too_many_arguments)]
pub(super) fn run_registered_transition(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    registry: &PassRegistry,
    cache: &PassCache,
    pass: &Arc<RegisteredPass>,
    outgoing: &wgpu::TextureView,
    incoming: &wgpu::TextureView,
    output: &wgpu::TextureView,
    progress: f32,
    params: &[f32],
    width: u32,
    height: u32,
) -> bool {
    let Some(built) = registry.runtime.get(device, pass) else {
        return false;
    };
    let Some(first) = built.first() else {
        return false;
    };
    let frame_budget = cache.frame_budget();
    let started = pass.tracks_cost(frame_budget).then(std::time::Instant::now);
    let param_slice = if params.is_empty() {
        pass.default_params()
    } else {
        params
    };
    let words = transition_uniform(width, height, progress, param_slice);
    encode(
        device,
        encoder,
        cache,
        first,
        &[
            Bound::Texture(outgoing),
            Bound::Texture(incoming),
            Bound::Sampler(&registry.sampler),
            Bound::Uniform(bytemuck::cast_slice(&words)),
        ],
        output,
        (width, height),
    );
    if let Some(started) = started {
        pass.record_cost(started.elapsed(), frame_budget);
    }
    true
}
