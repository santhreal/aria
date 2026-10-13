//! Pipelines and draws for passes registered while the process runs.
//!
//! A registered pass binds against the two-texture layout transitions use: two
//! textures, the shared sampler, and a uniform sized to the pass. A pipeline is
//! built the first time a frame draws the pass, inside a device error scope, so
//! a shader the device rejects is logged and recorded on the pass and draws
//! nothing instead of reaching the uncaptured-error handler.

use std::cell::RefCell;
use std::sync::Arc;

use rustc_hash::FxHashMap;

use super::pass_cache::{Bound, PassCache};
use super::pipelines::{PassRegistry, build_effect_pipeline};
use super::pool::OffscreenPool;
use crate::passes::PassInstance;
use crate::registered_passes::{
    EFFECT_HEADER_WORDS, MAX_EFFECT_FLOATS, RegisteredPass, TRANSITION_WORDS, param_vec4s,
};

/// Words in the largest effect uniform: the header and every parameter slot.
const MAX_EFFECT_WORDS: usize = EFFECT_HEADER_WORDS + 4 * MAX_EFFECT_FLOATS.div_ceil(4);

/// Pipelines built for registered passes, keyed by registration serial.
#[derive(Default)]
pub(crate) struct RuntimePipelines {
    built: RefCell<FxHashMap<u64, Built>>,
}

struct Built {
    pass: Arc<RegisteredPass>,
    /// `None` when the device refused a pass; the entry stays so the build is
    /// not retried every frame.
    pipelines: Option<Arc<[wgpu::RenderPipeline]>>,
}

impl RuntimePipelines {
    /// The pipelines drawing `pass`, one per pass in dispatch order, or `None`
    /// when the registration was dropped or the device refused a pass.
    pub(super) fn get(
        &self,
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        pass: &Arc<RegisteredPass>,
    ) -> Option<Arc<[wgpu::RenderPipeline]>> {
        if pass.is_removed() {
            self.built.borrow_mut().remove(&pass.serial());
            return None;
        }
        if let Some(built) = self.built.borrow().get(&pass.serial()) {
            return built.pipelines.clone();
        }
        let pipelines = build(device, layout, pass);
        let mut built = self.built.borrow_mut();
        built.retain(|_, held| !held.pass.is_removed());
        built.insert(
            pass.serial(),
            Built {
                pass: Arc::clone(pass),
                pipelines: pipelines.clone(),
            },
        );
        pipelines
    }
}

fn build(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    pass: &RegisteredPass,
) -> Option<Arc<[wgpu::RenderPipeline]>> {
    let mut pipelines = Vec::with_capacity(pass.pass_count());
    for (index, source) in pass.sources().iter().enumerate() {
        let label = format!("aria.registered.{}.{index}", pass.id());
        let internal = device.push_error_scope(wgpu::ErrorFilter::Internal);
        let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let pipeline = build_effect_pipeline(device, &label, layout, source);
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
        pipelines.push(pipeline);
    }
    Some(pipelines.into())
}

/// The uniform of one effect pass, in `words`; returns the words used.
#[allow(clippy::too_many_arguments)]
fn effect_uniform(
    words: &mut [u32; MAX_EFFECT_WORDS],
    width: u32,
    height: u32,
    time: f32,
    pass_index: usize,
    pass_count: usize,
    params: &[f32],
) -> usize {
    let (w, h) = (width.max(1) as f32, height.max(1) as f32);
    words[..EFFECT_HEADER_WORDS].copy_from_slice(&[
        w.to_bits(),
        h.to_bits(),
        (1.0 / w).to_bits(),
        (1.0 / h).to_bits(),
        time.to_bits(),
        pass_index as u32,
        pass_count as u32,
        0,
    ]);
    let used = EFFECT_HEADER_WORDS + 4 * param_vec4s(params.len());
    for (slot, word) in words[EFFECT_HEADER_WORDS..used].iter_mut().enumerate() {
        *word = params.get(slot).map_or(0, |value| value.to_bits());
    }
    used
}

fn transition_uniform(width: u32, height: u32, progress: f32) -> [u32; TRANSITION_WORDS] {
    let (w, h) = (width.max(1) as f32, height.max(1) as f32);
    [
        w.to_bits(),
        h.to_bits(),
        (1.0 / w).to_bits(),
        (1.0 / h).to_bits(),
        progress.to_bits(),
        0,
        0,
        0,
    ]
}

/// Draw one registered pass: `textures` at bindings 0 and 1, the shared sampler
/// at 2, `uniform` at 3.
#[allow(clippy::too_many_arguments)]
fn draw(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    registry: &PassRegistry,
    cache: &PassCache,
    pipeline: &wgpu::RenderPipeline,
    textures: [&wgpu::TextureView; 2],
    output: &wgpu::TextureView,
    uniform: &[u8],
) {
    let bind_group = cache.bind_group(
        device,
        &registry.transition_layout,
        "aria.registered.bg",
        &[
            Bound::Texture(textures[0]),
            Bound::Texture(textures[1]),
            Bound::Sampler(&registry.sampler),
            Bound::Uniform(uniform),
        ],
    );
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("aria.registered.pass"),
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

/// Run every pass of the registered effect `pass` on `input`, which is pool
/// slot `in_idx` or outside the pool.
///
/// Passes write alternately to the two slots after `in_idx`, so the effect's
/// input stays readable as `source_tex` through the chain; more than one pass
/// needs three slots, which [`PassInstance::slot_demand`] reports. Returns the
/// slot and view holding the result, or `None` when nothing was drawn and the
/// input stands.
#[allow(clippy::too_many_arguments)]
pub(super) fn run_registered_effect<'a>(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    registry: &PassRegistry,
    pool: &'a OffscreenPool,
    input: &'a wgpu::TextureView,
    in_idx: usize,
    effect: &PassInstance,
    pass: &Arc<RegisteredPass>,
    width: u32,
    height: u32,
) -> Option<(usize, &'a wgpu::TextureView)> {
    let pipelines = registry
        .runtime
        .get(device, &registry.transition_layout, pass)?;
    let count = pipelines.len();
    let slots = pool.slots();
    if count > 1 && slots < 3 {
        tracing::error!(
            pass = %pass.id(),
            slots,
            "a multi-pass effect needs three offscreen slots; it draws nothing"
        );
        return None;
    }
    let targets = [(in_idx + 1) % slots, (in_idx + 2) % slots];
    let mut words = [0u32; MAX_EFFECT_WORDS];
    let mut previous = input;
    let mut written = in_idx;
    for (index, pipeline) in pipelines.iter().enumerate() {
        written = targets[index % 2];
        let output = pool.view(written);
        let used = effect_uniform(
            &mut words,
            width,
            height,
            effect.time,
            index,
            count,
            &effect.params,
        );
        draw(
            device,
            encoder,
            registry,
            pool.passes(),
            pipeline,
            [previous, input],
            output,
            bytemuck::cast_slice(&words[..used]),
        );
        previous = output;
    }
    Some((written, previous))
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
    width: u32,
    height: u32,
) -> bool {
    let Some(pipelines) = registry
        .runtime
        .get(device, &registry.transition_layout, pass)
    else {
        return false;
    };
    let Some(pipeline) = pipelines.first() else {
        return false;
    };
    let words = transition_uniform(width, height, progress);
    draw(
        device,
        encoder,
        registry,
        cache,
        pipeline,
        [outgoing, incoming],
        output,
        bytemuck::cast_slice(&words),
    );
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WHY: the WGSL `AriaPass` layout is size, texel, time, pass index, pass
    /// count, padding, then `params` at byte 32 in whole `vec4`s. A word in the
    /// wrong place feeds every registered effect a wrong value; this pins each
    /// word and the zero fill of a partial `vec4`. It does not check the WGSL
    /// side, which the GPU test reads back.
    #[test]
    fn effect_uniform_words_follow_the_prelude_layout() {
        let mut words = [u32::MAX; MAX_EFFECT_WORDS];
        let used = effect_uniform(&mut words, 200, 100, 1.5, 1, 3, &[0.25, 0.5, 0.75, 1.0, 2.0]);
        assert_eq!(used, EFFECT_HEADER_WORDS + 8, "five floats take two vec4s");
        let floats = |range: std::ops::Range<usize>| {
            words[range]
                .iter()
                .map(|word| f32::from_bits(*word))
                .collect::<Vec<_>>()
        };
        assert_eq!(floats(0..5), [200.0, 100.0, 0.005, 0.01, 1.5]);
        assert_eq!(&words[5..8], &[1, 3, 0]);
        assert_eq!(
            floats(8..16),
            [0.25, 0.5, 0.75, 1.0, 2.0, 0.0, 0.0, 0.0],
            "a partial vec4 is zero-filled, not left with stale words"
        );
    }

    #[test]
    fn an_effect_without_parameters_still_uploads_one_vec4() {
        let mut words = [u32::MAX; MAX_EFFECT_WORDS];
        let used = effect_uniform(&mut words, 0, 0, 0.0, 0, 1, &[]);
        assert_eq!(used, EFFECT_HEADER_WORDS + 4);
        assert_eq!(&words[EFFECT_HEADER_WORDS..used], &[0; 4]);
        // A zero-sized target reads as one pixel rather than dividing by zero.
        assert_eq!(f32::from_bits(words[2]), 1.0);
    }

    #[test]
    fn the_largest_effect_fills_the_buffer_exactly() {
        let mut words = [0u32; MAX_EFFECT_WORDS];
        let params = vec![1.0; MAX_EFFECT_FLOATS];
        assert_eq!(
            effect_uniform(&mut words, 1, 1, 0.0, 0, 1, &params),
            MAX_EFFECT_WORDS
        );
    }

    #[test]
    fn transition_uniform_carries_size_texel_and_progress() {
        let words = transition_uniform(50, 25, 0.4);
        let floats: Vec<f32> = words[..5].iter().map(|w| f32::from_bits(*w)).collect();
        assert_eq!(floats, [50.0, 25.0, 0.02, 0.04, 0.4]);
        assert_eq!(&words[5..], &[0, 0, 0]);
    }
}
