//! WGPU compositor for Aria: turns decoded [`aria_core::VideoFrame`]s and
//! generated layers into a composited RGBA canvas for preview and export.
//!
//! The decoder hands frames across the [`aria_core`] seam as either CPU
//! planes (NV12 / I420 / packed RGB) or, on the fast path, a zero-copy GPU
//! surface. The compositor uploads those planes, converts YUV→RGB on the GPU
//! using the frame's own [`aria_core::ColorSpace`] (so BT.601/709/2020 and
//! limited/full range are honored rather than assumed), places each layer on the
//! canvas, and blends them bottom-to-top with src-over alpha. Generated content
//! (text, shapes, stickers) arrives pre-rasterized as a straight-alpha
//! [`RgbaImage`] and composites through the same placement model.
//!
//! [`GpuContext::new_headless_blocking`] is the entry point for tests and export;
//! the desktop UI will instead share its device via [`GpuContext::from_parts`].
//!
//! ## Scope today
//!
//! - Inputs: 8-bit **NV12** and **I420** CPU video frames, straight-alpha
//!   **RGBA** bitmap layers (text/shape/sticker generators, stills), solid
//!   fills, and zero-copy NV12 GPU surfaces — on Apple, `CVPixelBuffer`s
//!   mapped via `CVMetalTextureCache` (see `metal_import`); on Windows,
//!   shared D3D11 textures opened on the D3D12 device as `wgpu` NV12 plane
//!   views (see `dx12_import`); on Linux, LINEAR DMA-BUF fds imported
//!   through Vulkan external memory (see `vulkan_import`). Tiled DMABuf
//!   modifiers fall back to CPU decode.
//! - 10-bit/HDR (P010, PQ/HLG tone-mapping), packed-RGB *video frames*, and
//!   GPU-surface import on Android (`AHardwareBuffer`) are recognized but not
//!   yet handled — they surface as [`CompositorError::UnsupportedFormat`].

mod color_pass;
mod compositor;
mod cpu_effect;
#[cfg(target_os = "windows")]
mod dx12_import;
mod effect_render;
mod error;
mod gpu;
mod grade;
mod layer;
mod lut;
#[cfg(target_vendor = "apple")]
mod metal_import;
mod passes;
mod passes_uniform;
mod planes;
mod plugin_textures;
mod registered_graph;
mod registered_passes;
mod scopes;
#[path = "track_feature.rs"]
mod track_feature;
#[cfg(target_os = "linux")]
mod vulkan_import;

pub use aria_core::RgbaImage;
pub use aria_shapes::{SdfParams, SdfShape, Stroke};
pub use compositor::{
    Compositor, FrameSink, ImageSink, PendingReadback, TexturePoolStats, check_frame_target,
    create_frame_target, identity_instances,
};
pub use cpu_effect::{
    CpuEffect, CpuEffectDefinition, CpuFrameIn, CpuFrameOut, EffectCostSink, EffectFault,
    EffectFaultKind, EffectFaultSink, EffectParams, EffectRender, F16_ONE, U8_TO_F16, f16_to_f32,
    f16_to_u8, f32_to_f16,
};
pub use effect_render::PassStats;
pub use error::CompositorError;
pub use gpu::GpuContext;
pub use grade::ColorGrade;
pub use layer::{
    BlendMode, CompositeLayer, CompositorConfig, CompositorLayer, CoverageMix, FULL_UV,
    GlyphInstance, GlyphsLayer, LayerBackground, LayerChromaKey, LayerContent, LayerEffects,
    LayerGlow, LayerLut, LayerMask, LayerOutline, LayerPlacement, LayerShadow, LayerStencil,
    LayerStyles, MaskOp, MatteRead, SdfLayer, StencilCoverage, StencilMask, mask_kind,
};
pub use lut::{CubeLut, LutError, MAX_CUBE_SIZE};
pub use passes::{
    PassDescriptor, PassId, PassInstance, PassParam, TransitionPass, WipeDirection,
    effect_descriptor, effect_is_noop, pass_id, renders_effect, renders_transition,
    shipped_effect_descriptors, shipped_transition_descriptors, shipped_transition_ids,
    transition_pass,
};
pub use plugin_textures::{
    MAX_TEXTURE_SIDE, PluginTexture, TextureError, TextureRegistration, plugin_texture,
    register_texture,
};
pub use registered_passes::{
    EffectDefinition, MAX_EFFECT_FLOATS, MAX_EFFECT_PASSES, PassRegistration, RegisterError,
    RegisteredPass, TransitionDefinition, register_cpu_effect, register_effect,
    register_effect_with_fault_sink, register_effect_with_sinks, register_transition,
    register_transition_with_sinks, registered_effect,
};
pub use scopes::{
    GpuScopes, MAX_SCOPE_CHANNELS, MAX_SCOPE_COLUMNS, MAX_SCOPE_GRID, ScopeChannel, ScopeCounts,
    ScopeGrid,
};
pub use track_feature::{Match as TrackMatch, Patch as TrackPatch, track_patch};
/// The `wgpu` the compositor's device and textures belong to, for a caller that
/// creates a texture on [`GpuContext`].
pub use wgpu;
