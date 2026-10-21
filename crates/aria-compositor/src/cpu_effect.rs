//! CPU-evaluated effects and generators (`RGBA16F` callback path).
//!
//! A plugin effect that cannot be written as WGSL implements [`CpuEffect`].
//! When an effect chain reaches a CPU pass, the compositor reads the current
//! offscreen texture into a reused staging buffer, converts it to premultiplied
//! `RGBA16F` (`[u16; 4]` IEEE-754 binary16 per pixel), invokes
//! [`CpuEffect::process`] off the GPU queue, and uploads the written output
//! into the next offscreen slot. A generator (`spec.generator == true`)
//! receives a transparent frame of the output dimensions.

use std::sync::Arc;

use aria_models::EffectSpec;

use crate::registered_passes::EffectDefinition;

#[path = "cpu_effect_frame.rs"]
mod frame;
#[path = "cpu_effect_params.rs"]
mod params;

pub use frame::{CpuFrameIn, CpuFrameOut, F16_ONE, U8_TO_F16, f16_to_f32, f16_to_u8, f32_to_f16};
pub use params::EffectParams;
pub(crate) use params::RegisteredParam;

/// A video effect or generator evaluated on the CPU.
///
/// The input and output frames hold premultiplied `RGBA16F` pixels (four
/// IEEE-754 binary16 `u16` channels `[R, G, B, A]` per pixel). For a generator
/// (`spec.generator == true`), `input` is a transparent frame of the output
/// dimensions.
///
/// Returning `Err(reason)` leaves the frame as passthrough for that frame and
/// reports a recoverable plugin fault. A panic is caught, leaves the frame as
/// passthrough, and fails the plugin for the session.
pub trait CpuEffect: Send + Sync {
    /// Process `input` into `output` at clip-local `time` (in seconds) with
    /// evaluated `params`.
    fn process(
        &self,
        input: CpuFrameIn<'_>,
        output: &mut CpuFrameOut<'_>,
        params: &EffectParams<'_>,
        time: f32,
    ) -> Result<(), String>;
}

impl<F> CpuEffect for F
where
    F: Fn(CpuFrameIn<'_>, &mut CpuFrameOut<'_>, &EffectParams<'_>, f32) -> Result<(), String>
        + Send
        + Sync,
{
    fn process(
        &self,
        input: CpuFrameIn<'_>,
        output: &mut CpuFrameOut<'_>,
        params: &EffectParams<'_>,
        time: f32,
    ) -> Result<(), String> {
        self(input, output, params, time)
    }
}

/// An effect or generator a plugin renders on the CPU.
#[derive(Clone)]
pub struct CpuEffectDefinition {
    /// The catalog entry: a namespaced id, a label, and the parameters in the
    /// order they flatten into [`EffectParams`].
    pub spec: EffectSpec,
    /// The callback that processes each frame.
    pub effect: Arc<dyn CpuEffect>,
}

impl std::fmt::Debug for CpuEffectDefinition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CpuEffectDefinition")
            .field("spec", &self.spec)
            .finish_non_exhaustive()
    }
}

impl CpuEffectDefinition {
    /// Build a CPU effect definition from a catalog `spec` and a callback.
    pub fn new(spec: EffectSpec, effect: impl CpuEffect + 'static) -> Self {
        Self {
            spec,
            effect: Arc::new(effect),
        }
    }
}

/// How a registered effect renders: WGSL fragment passes or a CPU callback.
#[derive(Debug, Clone)]
pub enum EffectRender {
    /// WGSL fragment passes, in dispatch order.
    Shader(EffectDefinition),
    /// A CPU callback.
    Cpu(CpuEffectDefinition),
}

impl EffectRender {
    /// The catalog spec of the effect.
    pub fn spec(&self) -> &EffectSpec {
        match self {
            Self::Shader(def) => &def.spec,
            Self::Cpu(def) => &def.spec,
        }
    }
}

impl From<EffectDefinition> for EffectRender {
    fn from(definition: EffectDefinition) -> Self {
        Self::Shader(definition)
    }
}

impl From<CpuEffectDefinition> for EffectRender {
    fn from(definition: CpuEffectDefinition) -> Self {
        Self::Cpu(definition)
    }
}

/// Why a registered effect or transition pass raised a fault.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EffectFaultKind {
    /// The callback returned `Err(message)`; the frame passes through and the
    /// plugin stays mounted.
    Failed(String),
    /// The callback panicked with `message`; the frame passes through and the
    /// plugin is failed for the session.
    Panicked(String),
    /// Rendering the effect took `took`, exceeding the sequence or preview
    /// frame `budget` (`1 / fps`); the plugin stays mounted and the fault is
    /// reported.
    MissedDeadline {
        budget: std::time::Duration,
        took: std::time::Duration,
    },
}

/// A fault raised by a registered effect or transition pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectFault {
    pub effect_id: String,
    pub kind: EffectFaultKind,
}

impl std::fmt::Display for EffectFault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.kind {
            EffectFaultKind::Failed(message) => {
                write!(f, "effect {} failed: {message}", self.effect_id)
            }
            EffectFaultKind::Panicked(message) => {
                write!(f, "effect {} panicked: {message}", self.effect_id)
            }
            EffectFaultKind::MissedDeadline { budget, took } => {
                write!(
                    f,
                    "effect {} took {:.1} ms, over the {:.1} ms frame budget",
                    self.effect_id,
                    took.as_secs_f64() * 1_000.0,
                    budget.as_secs_f64() * 1_000.0,
                )
            }
        }
    }
}

/// Receives faults raised by a registered effect or transition pass.
pub type EffectFaultSink = Arc<dyn Fn(&EffectFault) + Send + Sync>;

/// Receives the measured per-frame execution cost of a registered effect or
/// transition pass.
pub type EffectCostSink = Arc<dyn Fn(std::time::Duration) + Send + Sync>;

/// Run one registered [`CpuEffect`] pass on `input` (in `pool` slot `in_idx` or
/// outside `pool`), writing the output into the next offscreen slot and
/// returning `(out_idx, output_view)` or `None` on passthrough.
#[allow(clippy::too_many_arguments)]
pub(crate) fn execute_cpu_effect<'a>(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    registry: &crate::effect_render::PassRegistry,
    pool: &'a crate::effect_render::OffscreenPool,
    input: &'a wgpu::TextureView,
    in_idx: usize,
    effect: &crate::passes::PassInstance,
    pass: &crate::registered_passes::RegisteredPass,
    width: u32,
    height: u32,
) -> Option<(usize, &'a wgpu::TextureView)> {
    if width == 0 || height == 0 || pass.is_removed() {
        return None;
    }
    let slots = pool.slots();
    if slots < 2 {
        return None;
    }
    let out_idx = (in_idx + 1) % slots;
    let src_texture = if let Some(slot) = pool.index_of(input) {
        pool.texture(slot)
    } else {
        crate::effect_render::run_grade_pass(
            device,
            encoder,
            registry,
            pool.passes(),
            input,
            pool.view(out_idx),
            crate::ColorGrade::default(),
        );
        pool.texture(out_idx)
    };
    let mut staging = pool.passes().take_cpu_staging(device, width, height);
    let size = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: src_texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &staging.readback_buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(staging.padded_bytes_per_row),
                rows_per_image: Some(height),
            },
        },
        size,
    );
    let next_encoder =
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    let submitted = std::mem::replace(encoder, next_encoder).finish();
    let queue = pool.passes().queue();
    queue.submit(std::iter::once(submitted));

    staging
        .map_state
        .store(0, std::sync::atomic::Ordering::Release);
    let flag = Arc::clone(&staging.map_state);
    let slice = staging.readback_buffer.slice(..);
    slice.map_async(wgpu::MapMode::Read, move |res| {
        let code = if res.is_ok() { 1 } else { 2 };
        flag.store(code, std::sync::atomic::Ordering::Release);
    });
    let _ = device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: None,
    });
    if staging.map_state.load(std::sync::atomic::Ordering::Acquire) != 1 {
        pool.passes().return_cpu_staging(staging);
        return None;
    }

    let frame_budget = pool.passes().frame_budget();
    let started = pass.tracks_cost(frame_budget).then(std::time::Instant::now);
    let row_bytes = (width as usize) * 4;
    let padded_row = staging.padded_bytes_per_row as usize;
    {
        let mapped = slice.get_mapped_range();
        for y in 0..(height as usize) {
            let src_row = &mapped[y * padded_row..y * padded_row + row_bytes];
            let dst_row = &mut staging.input_f16[y * row_bytes..(y + 1) * row_bytes];
            for (dst, &byte) in dst_row.iter_mut().zip(src_row.iter()) {
                *dst = U8_TO_F16[usize::from(byte)];
            }
        }
    }
    staging.readback_buffer.unmap();

    staging.output_f16.fill(0);
    let input_frame = CpuFrameIn::new(&staging.input_f16, width, height, row_bytes);
    let mut output_frame = CpuFrameOut::new(&mut staging.output_f16, width, height, row_bytes);
    let succeeded = pass.run_cpu_with_instance(
        input_frame,
        &mut output_frame,
        &effect.params,
        effect.instance.as_ref(),
        effect.time,
    );
    if !succeeded {
        pool.passes().return_cpu_staging(staging);
        return None;
    }

    for (dst, &half) in staging
        .upload_rgba8
        .iter_mut()
        .zip(staging.output_f16.iter())
    {
        *dst = f16_to_u8(half);
    }
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: pool.texture(out_idx),
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &staging.upload_rgba8,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width * 4),
            rows_per_image: Some(height),
        },
        size,
    );
    if let Some(started) = started {
        pass.record_cost(started.elapsed(), frame_budget);
    }
    pool.passes().return_cpu_staging(staging);
    Some((out_idx, pool.view(out_idx)))
}
