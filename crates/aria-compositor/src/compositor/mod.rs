//! The composite pass: scene description in, RGBA canvas out.
//!
//! [`Compositor::render`] clears the canvas to the config background, then draws
//! each layer's placed quad bottom-to-top and reads the result back to an
//! [`RgbaImage`]. Three layer kinds share one placement model and target:
//!
//! - **video frames** — uploaded as planes and converted YUV→RGB on the GPU
//!   using the frame's own [`aria_core::ColorSpace`]; straight-alpha src-over.
//! - **RGBA bitmaps** — pre-rasterized generators (text/shapes/stickers) or
//!   stills; premultiplied on upload and composited premultiplied src-over so
//!   anti-aliased edges stay clean.
//! - **solid fills** — a fast no-texture path; straight-alpha src-over.

mod canvas_pass;
mod draw_layer;
mod frame_target;
mod glyphs;
mod items;
mod layers;
mod lru;
mod pairs;
pub(crate) mod pipelines;
mod readback;
mod reference;
mod render;
mod samples;
mod scene;
mod stencil;
mod upload;

use std::cell::RefCell;

use bytemuck::{Pod, Zeroable};

use aria_core::RgbaImage;

use crate::effect_render::{
    Bound, CanvasTargets, OffscreenPool, PassCache, PassRegistry, PassStats,
};
use crate::grade::ColorGrade;
use crate::planes::PlaneSet;

pub use frame_target::{check_frame_target, create_frame_target};
use glyphs::{CachedAtlas, GlyphInstanceGpu};
use lru::FrameLru;
pub use readback::PendingReadback;
use readback::ReadbackPool;
pub use upload::TexturePoolStats;
use upload::{PlaneTexturePool, RgbaTextureCache};

pub use glyphs::identity_instances;

/// Canvas pixel format. Plain (non-sRGB) `Unorm`: the YUV shader already emits
/// gamma-encoded R'G'B', so we store those bytes verbatim (display-ready SDR)
/// and read them straight back into PNG/export without an extra transfer.
pub(super) const CANVAS_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// Caller-provided destination for a composited frame's pixels, so a render
/// can write its readback rows straight into storage the caller owns (a UI
/// framebuffer) instead of allocating an [`RgbaImage`] and copying again.
pub trait FrameSink {
    /// Called **once per successful render**, after the GPU work and buffer
    /// map succeeded, with the output dimensions. Must return exactly
    /// `width * height * 4` bytes; the compositor fills them with tightly
    /// packed row-major RGBA. A failed render never calls this.
    fn pixels(&mut self, width: u32, height: u32) -> &mut [u8];

    /// Optional replacement [`crate::gpu::GpuContext`] to adopt before encoding
    /// the frame (e.g. when the window shares or recovers its `wgpu` device).
    fn adopt_gpu(&mut self) -> Option<crate::gpu::GpuContext> {
        None
    }

    /// Optional GPU texture destination on this renderer's device for a
    /// `width`×`height` frame. When `Some`, the compositor copies the canvas
    /// texture into that texture on the GPU and skips [`pixels`](Self::pixels),
    /// `copy_texture_to_buffer`, and buffer mapping.
    fn texture(&mut self, _width: u32, _height: u32) -> Option<&wgpu::Texture> {
        None
    }

    /// Record the compositor's cumulative CPU readback count after the frame.
    fn record_readbacks(&mut self, _count: u64) {}
}

/// The [`FrameSink`] behind the [`RgbaImage`]-returning render paths: it
/// allocates the image at the reported size and hands out its bytes.
#[derive(Default)]
pub struct ImageSink(Option<RgbaImage>);

impl std::fmt::Debug for ImageSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never dump pixels. A 4K frame is 33 MB of hex in a panic payload.
        f.debug_tuple("ImageSink")
            .field(&self.0.as_ref().map(|img| (img.width, img.height)))
            .finish()
    }
}

impl ImageSink {
    /// The rendered image; `None` until a render succeeded into this sink.
    pub fn into_image(self) -> Option<RgbaImage> {
        self.0
    }
}

impl Compositor {
    /// What the effect passes of the last composite cost.
    pub fn pass_stats(&self) -> PassStats {
        self.offscreen
            .as_ref()
            .map(|pool| pool.passes().stats())
            .unwrap_or_default()
    }

    /// What base-layer bindings of the last composite cost.
    pub fn layer_pass_stats(&self) -> PassStats {
        self.layer_passes.stats()
    }

    /// Pipelines built for registered effect passes since this compositor was
    /// created. A pass builds once per registration, however many frames draw
    /// it.
    pub fn registered_pipeline_builds(&self) -> u64 {
        self.pass_registry.registered_pipeline_builds()
    }

    /// Built-in layer and pass pipelines compiled on this compositor's device.
    pub fn builtin_pipeline_builds(&self) -> usize {
        self.pipelines.compiled_count()
    }

    /// CPU video-plane texture allocations and retained storage from the last composite.
    pub fn plane_pool_stats(&self) -> TexturePoolStats {
        self.plane_textures.borrow().stats()
    }

    /// Static RGBA texture allocations and retained images from the last composite.
    pub fn rgba_cache_stats(&self) -> TexturePoolStats {
        self.rgba_textures.borrow().stats()
    }

    /// Cumulative number of GPU-to-CPU frame readbacks (`copy_texture_to_buffer`
    /// + `map_async`) submitted by this compositor.
    pub const fn readback_count(&self) -> u64 {
        self.readback_count
    }

    /// Rebuild device-bound pipelines and caches on `gpu` while preserving the
    /// cumulative [`readback_count`](Self::readback_count).
    pub fn reset_gpu(&mut self, gpu: &crate::gpu::GpuContext) {
        let readback_count = self.readback_count;
        *self = Self {
            readback_count,
            ..Self::new(gpu)
        };
    }

    /// Build one bind group per pass instead of reusing by descriptor.
    /// Diagnostic: it exists so a measurement can report both numbers from one
    /// process, and no shipped path turns reuse off.
    pub fn set_pass_reuse(&self, reuse: bool) {
        self.layer_passes.set_reuse(reuse);
        if let Some(pool) = self.offscreen.as_ref() {
            pool.passes().set_reuse(reuse);
        }
    }

    fn layer_bind_group(
        &self,
        gpu: &crate::gpu::GpuContext,
        layout: &wgpu::BindGroupLayout,
        label: &'static str,
        bounds: &[Bound<'_>],
    ) -> wgpu::BindGroup {
        self.layer_passes
            .bind_group(&gpu.device, layout, label, bounds)
            .clone()
    }
}

impl FrameSink for ImageSink {
    fn pixels(&mut self, width: u32, height: u32) -> &mut [u8] {
        let image = self.0.insert(RgbaImage::new(
            width,
            height,
            vec![0; (width as usize) * (height as usize) * 4],
        ));
        &mut image.pixels
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(super) struct YuvUniforms {
    linear: [f32; 4],
    trans_opacity: [f32; 4],
    uv_rect: [f32; 4],
    coeffs: [f32; 4],
    /// Packed grade lanes; the shader reads a slider by name through the
    /// accessors generated with the row list.
    grade: [[f32; 4]; ColorGrade::LANES],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(super) struct SolidUniforms {
    color: [f32; 4],
    linear: [f32; 4],
    trans_opacity: [f32; 4],
    grade: [[f32; 4]; ColorGrade::LANES],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(super) struct RgbaUniforms {
    linear: [f32; 4],
    trans_opacity: [f32; 4],
    uv_rect: [f32; 4],
    grade: [[f32; 4]; ColorGrade::LANES],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(super) struct FxEffectsUniform {
    /// mask_kind, mask_feather, mask_invert, mask_enabled.
    mask: [f32; 4],
    /// chroma r/g/b (normalized), chroma_enabled.
    chroma: [f32; 4],
    /// chroma_strength, chroma_shadow, pad, pad.
    chroma_params: [f32; 4],
    /// quad half-extents (x, y), mask rotation_rad (z), mask roundness (w).
    half: [f32; 4],
    /// mask center xy (fraction of layer size), mask size xy (fraction).
    mask_geo: [f32; 4],
    /// Coverage remap offset and scale, mask expansion px, pad.
    mask_mix: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(super) struct SdfUniforms {
    fill: [f32; 4],
    stroke_color: [f32; 4],
    linear: [f32; 4],
    /// Clip translation (x, y), layer opacity (z), stroke width px (w).
    trans_opacity: [f32; 4],
    /// Shape half-extents px (x, y), shape kind id (z), corner radius px (w).
    geo: [f32; 4],
    /// Star points (x), inner fraction (y); quad half-extents px (z, w).
    star: [f32; 4],
    grade: [[f32; 4]; ColorGrade::LANES],
}

/// Which pipeline draws a built layer.
#[derive(Clone, Copy)]
pub(super) enum LayerPipeline {
    Yuv,
    YuvFx,
    Rgba,
    RgbaFx,
    Solid,
    Sdf,
    Glyphs,
}

/// Instanced draw payload (glyph atlas quads).
pub(super) struct InstanceDraw {
    pub buffer: wgpu::Buffer,
    pub count: u32,
}

/// GPU resources backing one layer for the duration of a render. The bind group
/// retains its textures/buffer, but we hold them too so the lifetimes are
/// unmistakable across the pass.
pub(super) struct LayerGpu {
    pipeline: LayerPipeline,
    bind_group: wgpu::BindGroup,
    /// Plane textures this layer samples, or `None` for the texture-free
    /// pipelines (solid fills, SDF shapes).
    _planes: Option<PlaneSet>,
    /// Platform keep-alive for zero-copy imported surfaces (e.g. the Apple
    /// `CVMetalTexture` wrappers a GPU frame's planes are mapped through). Held
    /// until the layer's GPU resources drop, i.e. after the pass is submitted
    /// and waited on. `None` for CPU-uploaded and generated layers.
    _keep_alive: Option<Box<dyn core::any::Any + Send>>,
    /// Instanced draw (glyphs). `None` for single-quad layers.
    instances: Option<InstanceDraw>,
}

/// Ceiling on retained LUT textures outside the current frame's working set.
pub(super) const LUT_CACHE_MAX: usize = 16;

/// One uploaded `.cube` table: the 3D texture plus the sampling parameters
/// captured at upload time.
pub(super) struct LutTexture {
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
    size: u32,
    domain_min: [f32; 3],
    domain_max: [f32; 3],
}

/// Canvas texture plus two readback buffers reused while dimensions are stable.
pub(super) struct TargetCache {
    width: u32,
    height: u32,
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    readbacks: RefCell<ReadbackPool>,
    /// Row pitch of each readback buffer (256-byte copy alignment).
    padded_bytes_per_row: u32,
}

/// A WGPU alpha-over compositor. Build once (pipelines are reused), render many.
pub struct Compositor {
    pipelines: std::sync::Arc<pipelines::SharedLayerPipelines>,
    yuv_layout: wgpu::BindGroupLayout,
    yuv_fx_layout: wgpu::BindGroupLayout,
    rgba_layout: wgpu::BindGroupLayout,
    rgba_fx_layout: wgpu::BindGroupLayout,
    solid_layout: wgpu::BindGroupLayout,
    sdf_layout: wgpu::BindGroupLayout,
    glyphs_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    target: Option<TargetCache>,
    /// Cached texture views for caller-owned `FrameTarget` textures rendered
    /// directly without going through `target`.
    frame_target_views: Vec<(wgpu::Texture, wgpu::TextureView)>,
    pass_registry: PassRegistry,
    offscreen: Option<OffscreenPool>,
    /// Frames of clips that registered effects read through layer parameters.
    references: CanvasTargets,
    /// Pictures of nested sequences that layers sample as their content.
    nests: CanvasTargets,
    /// Layer bindings and uniforms persist independently of canvas-size
    /// offscreens, so generated layers keep their descriptors across resizes.
    layer_passes: PassCache,
    /// Dimension/format-matched upload textures retained across decoded frames.
    plane_textures: RefCell<PlaneTexturePool>,
    /// Premultiplied static bitmaps retained by a digest of their authored pixels.
    rgba_textures: RefCell<RgbaTextureCache>,
    /// Uploaded `.cube` 3D textures keyed by [`LayerLut::key`] (source path).
    /// Inactive entries leave one at a time by least-recent use.
    lut_cache: FrameLru<String, LutTexture>,
    /// Packed glyph atlases keyed by [`crate::layer::GlyphsLayer::atlas_key`].
    /// `RefCell` permits recency updates while [`Self::build_layer`] borrows `self`.
    glyph_atlases: RefCell<FrameLru<u64, CachedAtlas>>,
    /// Reusable CPU staging for quantized 3D LUT uploads.
    lut_upload_scratch: Vec<u8>,
    /// Straight→premultiplied conversion buffer for RGBA bitmap layers, reused
    /// across renders. A 1080p title bitmap is 8 MB; allocating and freeing it
    /// per layer per frame was the compositor's largest CPU allocation.
    premul_scratch: RefCell<Vec<u8>>,
    /// Glyph instance staging buffer, reused across renders: animated text
    /// rebuilds one entry per visible cluster every frame.
    glyph_instances: RefCell<Vec<GlyphInstanceGpu>>,
    /// Cumulative number of GPU-to-CPU frame readbacks submitted.
    readback_count: u64,
    /// Maps Apple `CVPixelBuffer` GPU surfaces into `wgpu` textures with no CPU
    /// copy. `None` when the device isn't a Metal device (e.g. a software
    /// adapter) — GPU frames then fall back to [`CompositorError::UnsupportedFormat`]
    /// and the caller can decode in CPU mode instead.
    #[cfg(target_vendor = "apple")]
    metal_import: Option<crate::metal_import::MetalSurfaceImporter>,
    /// Maps Windows shared D3D11 NV12 textures into `wgpu` textures with no
    /// CPU copy. `None` when the device isn't the D3D12 backend or lacks
    /// `TEXTURE_FORMAT_NV12` — GPU frames then fall back to
    /// [`CompositorError::UnsupportedFormat`] and the caller can decode in CPU
    /// mode instead.
    #[cfg(target_os = "windows")]
    dx12_import: Option<crate::dx12_import::Dx12SurfaceImporter>,
    /// Maps Linux LINEAR DMA-BUF NV12 surfaces into `wgpu` textures with no
    /// CPU copy. `None` when the device isn't Vulkan, lacks
    /// `TEXTURE_FORMAT_NV12`, or did not enable `VK_KHR_external_memory_fd`.
    /// Tiled modifiers still fail import and fall back to CPU decode.
    #[cfg(target_os = "linux")]
    vulkan_import: Option<crate::vulkan_import::VulkanSurfaceImporter>,
}

impl std::fmt::Debug for Compositor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Compositor")
            .field("has_target", &self.target.is_some())
            .field("has_offscreen", &self.offscreen.is_some())
            .finish_non_exhaustive()
    }
}
