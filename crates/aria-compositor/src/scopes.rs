//! Scope counts of a frame target, measured on the device that rendered it.
//!
//! A Viewer frame presented as a texture is never read back whole. The scopes
//! read it where it is: one compute pass counts the red, green, blue, and luma
//! histograms, the waveform of one to three channels, and the vectorscope, and
//! only those counts are copied to the CPU, a few hundred kilobytes where the
//! frame is megabytes.
//!
//! The counts are the ones `aria_analysis::scopes::measure` takes from the
//! same frame read back: the shader runs the same fixed-point arithmetic over
//! the same pixels, so a scope reads one frame the same way on either path.

use std::sync::mpsc::sync_channel;

use crate::error::CompositorError;

/// Bins per channel: one per 8-bit code value.
const BINS: usize = 256;

/// Counts ahead of the waveform: four histograms and the sampled count.
const HEAD: usize = 4 * BINS + 1;

/// Invocations per workgroup edge, as the shader declares.
const WORKGROUP: u32 = 16;

/// Largest waveform column count a pass takes.
pub const MAX_SCOPE_COLUMNS: u32 = 1024;

/// Largest vectorscope edge a pass takes. The vectorscope's fixed point fits
/// `u32` up to this edge.
pub const MAX_SCOPE_GRID: u32 = 512;

/// Most waveform channels one pass counts: a parade of three.
pub const MAX_SCOPE_CHANNELS: usize = 3;

/// One quantity a waveform column counts, by 8-bit code value, numbered as
/// the shader reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeChannel {
    /// BT.709 luma.
    Luma = 0,
    /// The red code value.
    Red = 1,
    /// The green code value.
    Green = 2,
    /// The blue code value.
    Blue = 3,
    /// BT.709 Cb as a code value, 128 for neutral.
    Cb = 4,
    /// BT.709 Cr as a code value, 128 for neutral.
    Cr = 5,
}

/// Which pixels a scope pass reads and how finely it counts them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScopeGrid {
    /// Waveform columns the frame's width is folded into, 1 to
    /// [`MAX_SCOPE_COLUMNS`].
    pub columns: u32,
    /// Edge of the square vectorscope grid, 1 to [`MAX_SCOPE_GRID`].
    pub size: u32,
    /// Every `stride`-th pixel of every `stride`-th row is read, from the
    /// first. At least 1.
    pub stride: u32,
    /// The channels the waveform counts, 1 to [`MAX_SCOPE_CHANNELS`], each a
    /// block of `columns` columns in this order.
    pub waveform: &'static [ScopeChannel],
}

impl ScopeGrid {
    fn valid(self) -> bool {
        (1..=MAX_SCOPE_COLUMNS).contains(&self.columns)
            && (1..=MAX_SCOPE_GRID).contains(&self.size)
            && self.stride >= 1
            && (1..=MAX_SCOPE_CHANNELS).contains(&self.waveform.len())
    }

    /// Counts in the waveform blocks.
    fn waveform_len(self) -> usize {
        self.waveform.len() * self.columns as usize * BINS
    }

    /// Counts a pass under this grid writes.
    fn len(self) -> usize {
        HEAD + self.waveform_len() + (self.size as usize).pow(2)
    }
}

/// What one scope pass counted.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScopeCounts {
    /// Red, green, blue, and BT.709 luma, 256 counts each.
    pub histogram: [Vec<u32>; 4],
    /// For each channel of the grid's waveform in order, `columns` columns of
    /// 256 counts, column after column.
    pub waveform: Vec<u32>,
    /// `size` rows of `size` chroma counts, top row first.
    pub vectorscope: Vec<u32>,
    /// Pixels read that were not fully transparent.
    pub sampled: u32,
}

/// The count buffers for one grid size.
struct Counts {
    len: usize,
    storage: wgpu::Buffer,
    staging: wgpu::Buffer,
}

/// A compute pass that counts scopes over a premultiplied `Rgba8Unorm` frame
/// target.
pub struct GpuScopes {
    device: wgpu::Device,
    queue: wgpu::Queue,
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::ComputePipeline,
    grid: wgpu::Buffer,
    counts: Option<Counts>,
}

impl std::fmt::Debug for GpuScopes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GpuScopes")
            .field("counts", &self.counts.as_ref().map(|counts| counts.len))
            .finish_non_exhaustive()
    }
}

impl GpuScopes {
    /// The pass on `device`, submitting to `queue`: the device and queue the
    /// frames it measures were rendered on.
    ///
    /// `None` when the device runs no compute shaders, as a GLES 3.0 adapter
    /// does not, or rejects the pass. The caller reads the frame back and
    /// counts it on the CPU instead.
    #[must_use]
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Option<Self> {
        let limits = device.limits();
        if limits.max_compute_invocations_per_workgroup < WORKGROUP * WORKGROUP
            || limits.max_compute_workgroup_size_x < WORKGROUP
            || limits.max_compute_workgroup_size_y < WORKGROUP
            || limits.max_compute_workgroup_storage_size < (HEAD * 4) as u32
            || limits.max_storage_buffers_per_shader_stage < 1
        {
            return None;
        }
        let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let label = Some("aria.scopes");
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label,
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label,
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/scopes.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label,
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label,
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some("cs"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        });
        let grid = device.create_buffer(&wgpu::BufferDescriptor {
            label,
            size: 48,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        if let Some(error) = pollster::block_on(validation.pop()) {
            tracing::warn!(%error, "the scope pass was rejected; scopes read frames back");
            return None;
        }
        Some(Self {
            device: device.clone(),
            queue: queue.clone(),
            layout,
            pipeline,
            grid,
            counts: None,
        })
    }

    /// Count `frame` under `grid`.
    ///
    /// Blocks until the counts are on the CPU. The frame must be a
    /// premultiplied `Rgba8Unorm` texture with `TEXTURE_BINDING` usage on this
    /// pass's device, as [`crate::create_frame_target`] makes; fully
    /// transparent pixels are not counted.
    ///
    /// # Errors
    ///
    /// [`CompositorError::ScopeSource`] for a frame of another format or
    /// usage, [`CompositorError::InvalidDimensions`] for an empty one,
    /// [`CompositorError::ScopeGrid`] for a grid outside its bounds, and
    /// [`CompositorError::MapFailed`] when the counts cannot be read back.
    pub fn measure(
        &mut self,
        frame: &wgpu::Texture,
        grid: ScopeGrid,
    ) -> Result<ScopeCounts, CompositorError> {
        if frame.format() != wgpu::TextureFormat::Rgba8Unorm
            || !frame.usage().contains(wgpu::TextureUsages::TEXTURE_BINDING)
        {
            return Err(CompositorError::ScopeSource {
                format: frame.format(),
                usage: frame.usage(),
            });
        }
        let wgpu::Extent3d { width, height, .. } = frame.size();
        if width == 0 || height == 0 {
            return Err(CompositorError::InvalidDimensions { width, height });
        }
        if !grid.valid() {
            return Err(CompositorError::ScopeGrid {
                columns: grid.columns,
                size: grid.size,
                stride: grid.stride,
                channels: grid.waveform.len(),
            });
        }
        let len = grid.len();
        let bytes = (len * 4) as u64;
        if self.counts.as_ref().is_none_or(|counts| counts.len != len) {
            let buffer = |usage| {
                self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("aria.scopes.counts"),
                    size: bytes,
                    usage,
                    mapped_at_creation: false,
                })
            };
            self.counts = Some(Counts {
                len,
                // Cleared at the start of every run, so a reused buffer counts
                // one frame only.
                storage: buffer(
                    wgpu::BufferUsages::STORAGE
                        | wgpu::BufferUsages::COPY_SRC
                        | wgpu::BufferUsages::COPY_DST,
                ),
                staging: buffer(wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ),
            });
        }
        let Some(counts) = self.counts.as_ref() else {
            return Err(CompositorError::MapFailed);
        };
        let mut channels = [0u32; MAX_SCOPE_CHANNELS];
        for (slot, channel) in channels.iter_mut().zip(grid.waveform) {
            *slot = *channel as u32;
        }
        let [first, second, third] = channels;
        let uniform = [
            width,
            height,
            grid.stride,
            grid.columns,
            grid.size,
            grid.waveform.len() as u32,
            first,
            second,
            third,
            0,
            0,
            0,
        ];
        self.queue
            .write_buffer(&self.grid, 0, bytemuck::cast_slice(&uniform));
        let view = frame.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("aria.scopes"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: self.grid.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: counts.storage.as_entire_binding(),
                },
            ],
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        encoder.clear_buffer(&counts.storage, 0, None);
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: None,
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(
                width.div_ceil(grid.stride).div_ceil(WORKGROUP),
                height.div_ceil(grid.stride).div_ceil(WORKGROUP),
                1,
            );
        }
        encoder.copy_buffer_to_buffer(&counts.storage, 0, &counts.staging, 0, bytes);
        self.queue.submit(std::iter::once(encoder.finish()));
        let slice = counts.staging.slice(..);
        let (sender, receiver) = sync_channel(1);
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|_| CompositorError::MapFailed)?;
        receiver.recv().map_err(|_| CompositorError::MapFailed)??;
        let read = {
            let mapped = slice.get_mapped_range();
            let all: &[u32] = bytemuck::cast_slice(&mapped[..]);
            let (head, rest) = all.split_at(HEAD);
            let (waveform, vectorscope) = rest.split_at(grid.waveform_len());
            ScopeCounts {
                histogram: [
                    head[..BINS].to_vec(),
                    head[BINS..2 * BINS].to_vec(),
                    head[2 * BINS..3 * BINS].to_vec(),
                    head[3 * BINS..4 * BINS].to_vec(),
                ],
                waveform: waveform.to_vec(),
                vectorscope: vectorscope.to_vec(),
                sampled: head[4 * BINS],
            }
        };
        counts.staging.unmap();
        Ok(read)
    }
}
