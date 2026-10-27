//! The GPU renderer: realize a [`Scene`] into a composited [`RgbaImage`].
//!
//! [`Renderer`] owns the expensive, reusable pieces — a `wgpu` device, the
//! compositor pipelines, a text rasterizer, and a per-media decoder cache — so
//! a single instance renders many frames (preview scrub, export) without
//! re-initializing the GPU or re-opening decoders.

mod api;
mod effects;
mod look_ahead;
mod measure;
mod media_cache;
mod realize;
pub(crate) mod text_anim;

/// Hot-path helpers exposed for Criterion benches — not a stable API.
#[doc(hidden)]
pub mod text_anim_bench {
    pub use super::text_anim::{cluster_deltas, place_clusters};
    pub use crate::animation::LookDelta;
}

#[cfg(test)]
mod media_sources_tests;
#[cfg(test)]
mod playback_rate;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use aria_compositor::{Compositor, GpuContext, PendingReadback, RgbaImage};
use aria_core::RationalTime;
use aria_decoder::OutputMode;
use aria_models::MediaId;
use aria_shapes::PathRaster;
use aria_text::TextRenderer;

use crate::render_files::RenderFiles;
use look_ahead::{DecoderKey, DecoderSlot};
pub use measure::{LayerCaret, drawn_bounds, path_style};
use media_cache::{CubeLutState, LottieState, StickerSequence};

/// A composited frame slower than this logs its stage breakdown at `info`
/// (default-visible): interactive preview budgets a few frames of latency,
/// and anything past this is worth attributing to decode vs GPU work.
pub(super) const SLOW_FRAME_LOG_MS: f64 = 150.0;

/// Per-stage timing of the most recent successful frame render.
///
/// Callers that adapt render *resolution* to cost (the preview quality
/// ladder) need the split, not the total: decode runs at the source's native
/// size no matter how small the output canvas is, so only
/// [`scaled_cost_ms`](Self::scaled_cost_ms) responds to rendering smaller.
#[derive(Debug, Clone, Copy, Default)]
pub struct FrameStats {
    /// Media decode time summed across layers (resolution-independent).
    pub decode_ms: f64,
    /// Text/shape/still realize time (raster caches, still decodes).
    pub raster_ms: f64,
    /// GPU composite + readback — scales with output pixels.
    pub composite_ms: f64,
    /// Effect passes the composite drew, across every layer.
    pub effect_passes: u32,
    /// Bind groups those passes needed built, rather than reusing one already
    /// describing that exact pass. Zero on a repeated frame.
    pub effect_pass_bind_groups: u32,
}

impl FrameStats {
    /// The portion of the frame cost that shrinks with output resolution
    /// (composite + raster) — what a quality ladder can actually buy back.
    pub fn scaled_cost_ms(&self) -> f64 {
        self.raster_ms + self.composite_ms
    }

    /// Whole-frame cost (decode + raster + composite).
    pub fn total_ms(&self) -> f64 {
        self.decode_ms + self.raster_ms + self.composite_ms
    }
}

/// How media decoders are positioned when realizing a frame.
///
/// `Exact` is correctness (export, settled preview); `NearestSync` is the
/// scrub-latency escape hatch: on long-GOP sources an exact mid-GOP target
/// costs a keyframe-prefix walk of hundreds of decodes, where the nearest
/// sync frame costs one. Frames rendered under `NearestSync` may show
/// content up to a GOP *before* the requested time — callers own the
/// follow-up exact render and must never cache snapped output under an
/// exact key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SeekPolicy {
    /// Decode the exact frame covering each layer's source time.
    #[default]
    Exact,
    /// Snap to the cheapest frame near the target: one decode from the
    /// sync point at/before it (or a short exact roll when the target is
    /// just ahead of the decoder's position).
    NearestSync,
}

/// The files one renderer decodes each media id from, at one moment: the
/// proxies registered and whether they are in use.
///
/// A second renderer that adopts it ([`Renderer::adopt_media_sources`])
/// decodes every media id from the same file as the first. Taking one costs a
/// reference count.
#[derive(Debug, Clone)]
pub struct MediaSources {
    generation: u64,
    proxies: Arc<HashMap<MediaId, PathBuf>>,
    use_proxies: bool,
}

/// A media-source generation no renderer in this process has used, so two
/// renderers never hold one generation for different files.
fn next_media_generation() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

pub(crate) struct PendingFrame {
    readback: Option<PendingReadback>,
    scratch: Option<realize::RealizeScratch>,
    decode_ms: f64,
    raster_ms: f64,
    submitted_at: std::time::Instant,
    effect_passes: u32,
    effect_pass_bind_groups: u32,
    layers: usize,
    width: u32,
    height: u32,
}

impl PendingFrame {
    pub(crate) const fn is_passthrough(&self) -> bool {
        self.readback.is_none()
    }
}

/// Renders project frames on a headless (or shared) GPU.
pub struct Renderer {
    gpu: GpuContext,
    compositor: Compositor,
    text: TextRenderer,
    /// Pen-path rasterizer (memoized, like `text`). Parametric shapes never
    /// touch it — they realize as GPU SDF layers.
    paths: PathRaster,
    /// One decode-ahead decoder per use of a media source on screen, keyed by
    /// the media and the clip showing it (see `look_ahead`). Two clips of one
    /// file sit at different source times, so each keeps its own cursor.
    decoders: HashMap<DecoderKey, DecoderSlot>,
    /// Decode-once cache for still images: one straight-alpha RGBA bitmap per
    /// media source, reused for every frame the still is on screen. Bounded by
    /// the project's still count, with each entry capped at
    /// [`aria_decoder::image::MAX_DECODE_DIMENSION`] on the long side. Shared,
    /// so the compositor hashes each still once rather than on every frame.
    stills: HashMap<MediaId, Arc<RgbaImage>>,
    /// Decode-once cache for bundled stickers, keyed by catalog id: the whole
    /// frame sequence (a static sticker is one frame) plus per-frame delays.
    /// Bounded by the catalog (small, embedded assets); frame lookup on the
    /// hot path is O(frames) over the delay table, no decode.
    stickers: HashMap<String, StickerSequence>,
    /// File-backed Lottie animations, keyed by path: parsed composition +
    /// LRU of rasterized frames (capped-fps sampling, per-asset byte budget
    /// — never the pre-render-everything sticker strategy; see
    /// `docs/lottie-design.md`). Failed loads are remembered so a missing
    /// file logs once and draws nothing instead of re-probing every frame.
    lottie: HashMap<String, LottieState>,
    /// Monotonic per-scene stamp. The Lottie frame LRU never evicts a frame
    /// the scene being composed touched, so two clips of one asset cannot
    /// alias mid-frame, and a decoder no recent scene used drops its held
    /// frames.
    scene_stamp: u64,
    /// The `scene_stamp` of the last [`Renderer::note_scene_media`] check, or
    /// `u64::MAX` before any scene check runs.
    shared_check_stamp: u64,
    /// Whether the scene at `shared_check_stamp` draws more than one video
    /// layer of the same media at the same source time.
    scene_shares_frames: bool,
    /// The tick of the last timeline frame rendered, which the next one's
    /// look-ahead reads its playback step from.
    last_tick: Option<RationalTime>,
    /// Reusable look-ahead storage: media uses and entrance ticks.
    look_uses: Vec<(DecoderKey, RationalTime)>,
    look_ticks: Vec<i64>,
    /// Parsed `.cube` LUTs, keyed by path. The GPU texture lives in the
    /// compositor's own cache (same key); this holds the CPU parse so a
    /// re-render never re-reads the file. Failed loads are remembered so a
    /// missing file logs once and grades nothing instead of re-probing
    /// every frame.
    luts: HashMap<String, CubeLutState>,
    /// Preferred decoder output mode. Apple and Windows start in
    /// [`OutputMode::Gpu`] so hardware-decoded surfaces (`CVPixelBuffer` /
    /// shared D3D11 NV12 textures) import into the compositor with no CPU
    /// copy; if a produced surface can't be imported (e.g. 10-bit/HDR, or a
    /// GPU without NV12 texture support), the renderer permanently falls back
    /// to [`OutputMode::Cpu`] and retries.
    decode_mode: OutputMode,
    /// Runtime-only substitute decode paths (preview proxies): when present
    /// (and [`use_proxies`](Self::use_proxies) holds), decoders for a media
    /// id open this file instead of the project's. Session state — never
    /// serialized, cleared when the session's media-id space changes
    /// (open/load/relink). Content must match the original frame-for-frame;
    /// only resolution/GOP may differ, so placement geometry (driven by the
    /// model's dimensions) stays valid and normalized UVs sample correctly.
    proxies: Arc<HashMap<MediaId, PathBuf>>,
    /// Whether [`decode`](Self::decode) honors `proxies`. Preview leaves
    /// this on; full-quality paths sharing this renderer (the engine's
    /// export command) flip it off for the pass. Toggling drops the
    /// proxied media's open decoders so no cursor outlives its file.
    use_proxies: bool,
    /// Changes whenever a media id may decode from a different file than
    /// before: a media-source reset or invalidation, a proxy registered or
    /// cleared, or proxies switched on or off. See [`MediaSources`].
    media_generation: u64,
    /// When true, export-quality motion blur runs: clips with motion blur
    /// enabled and an animated transform are supersampled across the shutter.
    /// Interactive preview leaves this false (N× layer draws is too costly
    /// for scrub). Flipped on for the duration of [`crate::export_observed`].
    pub(crate) apply_motion_blur: bool,
    /// Reusable CPU-side containers for layer realization.
    realize_scratch: realize::RealizeScratch,
    /// Reusable scene and pool for preview frame entry points so steady
    /// playback resolves without per-frame `Vec` or `String` allocations.
    resolve_scene: crate::scene::Scene,
    resolve_pool: crate::resolve::ResolvePool,
    /// Reusable scene and pool for [`Self::look_ahead`] so steady look-ahead
    /// resolves without per-frame `Vec` or `String` allocations.
    look_ahead_scene: crate::scene::Scene,
    look_ahead_pool: crate::resolve::ResolvePool,
    /// Memo of static text bitmaps keyed by [`text_anim::static_raster_key`],
    /// avoiding `ShapeKey::new` (`String`) and `RgbaImage::clone` (`Vec<u8>`)
    /// on steady frames when the same static text layer is realized. Shared,
    /// so the compositor hashes each bitmap once.
    text_memo: HashMap<u64, Arc<RgbaImage>>,
    /// A stop the next frame may honour, set by
    /// [`cancel_next_frame`](Renderer::cancel_next_frame) and taken by the
    /// realize pass. Frame-scoped by construction: a token left behind cannot
    /// cancel a later frame because the frame that read it took it away.
    cancel: aria_core::Cancel,
    /// Stage timings of the last successful render (see [`FrameStats`]).
    last_stats: FrameStats,
    /// Optional content-addressed render-files store consulted on preview
    /// entry points when no [`crate::ResolveOverrides`] are active.
    render_files: Option<Arc<RenderFiles>>,
    /// Reusable buffer for raw QOI bytes read from `render_files`.
    render_file_bytes: Vec<u8>,
    /// Reusable buffer for decoded RGBA8 pixels read from `render_files`.
    render_file_image: RgbaImage,
}

impl std::fmt::Debug for Renderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Renderer")
            .field("decoders", &self.decoders.len())
            .field("stills", &self.stills.len())
            .field("stickers", &self.stickers.len())
            .field("lottie", &self.lottie.len())
            .field("luts", &self.luts.len())
            .field("decode_mode", &self.decode_mode)
            .field("use_proxies", &self.use_proxies)
            .field("apply_motion_blur", &self.apply_motion_blur)
            .finish_non_exhaustive()
    }
}

/// Three fit-sized RGBA images for a zero-drift preview transform gesture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GestureFrames {
    pub below: RgbaImage,
    pub sprite: RgbaImage,
    pub above: Option<RgbaImage>,
}
