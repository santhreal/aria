//! Compositor failures, kept backend-agnostic.

use thiserror::Error;

/// Something went wrong setting up the GPU or compositing a frame.
#[derive(Debug, Error)]
pub enum CompositorError {
    /// No GPU adapter satisfied the request (no hardware, no driver).
    #[error("no GPU adapter available: {0}")]
    NoAdapter(String),

    /// The adapter was found but a logical device couldn't be created.
    #[error("failed to request GPU device: {0}")]
    RequestDevice(String),

    /// Canvas dimensions were zero (nothing to render into).
    #[error("invalid canvas dimensions: {width}x{height}")]
    InvalidDimensions { width: u32, height: u32 },

    /// A frame's pixel format isn't handled by the compositor yet (e.g. 10-bit
    /// P010, which needs a 16-bit sampling path).
    #[error("unsupported frame format: {0}")]
    UnsupportedFormat(String),

    /// A frame's planes don't match its declared geometry (a plane buffer is
    /// too small for its `stride × rows`, or the wrong plane count).
    #[error("malformed frame: {0}")]
    MalformedFrame(String),

    /// Both deferred readback slots are awaiting consumption.
    #[error("both GPU readback slots are pending; finish one frame before submitting another")]
    ReadbackBusy,

    /// Reading the rendered canvas back to the CPU failed.
    #[error("GPU buffer map failed")]
    MapFailed,
    /// Effect parameters did not match the expected count or exceeded the uniform capacity.
    #[error("invalid effect parameters for '{effect}': expected {expected}, got {actual}")]
    InvalidEffectParams {
        effect: String,
        expected: usize,
        actual: usize,
    },

    /// A layer shows a nested sequence that no
    /// [`CompositorLayer::Nest`](crate::CompositorLayer::Nest) of the frame
    /// draws.
    #[error(
        "a layer shows nest {0}, which no nest item of this frame draws; submit the nest \
         in the same frame, before the layers that show it"
    )]
    UndrawnNest(u64),

    /// A [`StencilMask`](crate::StencilMask) reads a coverage image its
    /// [`LayerStencil`](crate::LayerStencil) does not hold.
    #[error(
        "a stencil mask reads coverage image {index}, but its stencil holds {images}; \
         add the image to `LayerStencil::images` or fix the mask's index"
    )]
    StencilImage { index: usize, images: usize },

    /// A caller texture cannot receive a composited frame.
    #[error(
        "a frame target must be an Rgba8Unorm texture with COPY_DST usage and size \
         {width}x{height}; this one is {format:?} with {usage:?} at {target_width}x{target_height}"
    )]
    FrameTarget {
        width: u32,
        height: u32,
        format: wgpu::TextureFormat,
        usage: wgpu::TextureUsages,
        target_width: u32,
        target_height: u32,
    },

    /// A scope pass was handed a frame it cannot read.
    #[error(
        "a scope pass reads an Rgba8Unorm texture with TEXTURE_BINDING usage; this one is \
         {format:?} with {usage:?}"
    )]
    ScopeSource {
        format: wgpu::TextureFormat,
        usage: wgpu::TextureUsages,
    },

    /// A scope pass was asked for a grid outside its bounds.
    #[error(
        "a scope grid needs 1 to 1024 waveform columns, a vectorscope edge of 1 to 512, a \
         stride of at least 1, and 1 to 3 waveform channels; got {columns} columns, edge \
         {size}, stride {stride}, {channels} channels"
    )]
    ScopeGrid {
        columns: u32,
        size: u32,
        stride: u32,
        channels: usize,
    },
}

impl From<wgpu::BufferAsyncError> for CompositorError {
    fn from(_: wgpu::BufferAsyncError) -> Self {
        Self::MapFailed
    }
}
