use std::collections::{HashMap, hash_map::Entry};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};

use aria_compositor::{CubeLut, LayerLut, RgbaImage};
use aria_core::{RationalTime, VideoDecoder, VideoFrame};
use aria_decoder::OutputMode;
use aria_models::{MediaId, Project};

use crate::error::RenderError;
use crate::scene::{LayerSource, Scene, SceneLayer, SceneLut};

use super::{Renderer, SeekPolicy};

/// A `.cube` path the renderer has seen: loading on a worker, parsed, or failed
/// (missing or malformed file; grades nothing and logs once).
pub(super) enum CubeLutState {
    Loading(Receiver<CubeLutLoad>),
    Loaded(Box<CubeLut>),
    Failed,
}

pub(super) enum CubeLutLoad {
    Loaded(Box<CubeLut>),
    Failed,
}

fn load_cube_lut(path: &str) -> CubeLutLoad {
    match std::fs::read_to_string(path) {
        Ok(text) => match CubeLut::parse(&text) {
            Ok(cube) => CubeLutLoad::Loaded(Box::new(cube)),
            Err(error) => {
                tracing::warn!("LUT '{path}' failed to parse: {error}");
                CubeLutLoad::Failed
            }
        },
        Err(error) => {
            tracing::warn!("LUT '{path}' failed to read: {error}");
            CubeLutLoad::Failed
        }
    }
}

fn start_cube_lut_load(path: String) -> Receiver<CubeLutLoad> {
    start_cube_lut_load_with(path, load_cube_lut)
}

fn start_cube_lut_load_with(path: String, load: fn(&str) -> CubeLutLoad) -> Receiver<CubeLutLoad> {
    let (send, receive) = mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("aria-lut-load".into())
        .spawn(move || {
            let _ = send.send(load(&path));
        })
        .expect("the LUT loader thread must start");
    receive
}

impl CubeLutState {
    fn finish_loading(&mut self) -> bool {
        let loaded = match self {
            Self::Loading(receive) => receive.recv().unwrap_or(CubeLutLoad::Failed),
            Self::Loaded(_) => return true,
            Self::Failed => return false,
        };
        *self = match loaded {
            CubeLutLoad::Loaded(cube) => Self::Loaded(cube),
            CubeLutLoad::Failed => Self::Failed,
        };
        matches!(self, Self::Loaded(_))
    }
}

/// Borrow the parsed table behind a realized LUT reference for the
/// compositor's [`LayerLut`] (keyed uploads; see `aria-compositor`).
/// Realize only emits references [`Renderer::resolve_scene_lut`] loaded.
pub(super) fn layer_lut<'a>(
    lut: &'a Option<SceneLut>,
    luts: &'a HashMap<String, CubeLutState>,
) -> Option<LayerLut<'a>> {
    let lut = lut.as_ref()?;
    let CubeLutState::Loaded(cube) = &luts[&lut.path] else {
        unreachable!("realized LUT reference without a loaded table")
    };
    Some(LayerLut {
        key: &lut.path,
        lut: cube,
        intensity: lut.intensity,
    })
}

fn queue_cube_lut(luts: &mut HashMap<String, CubeLutState>, path: &str) {
    if let Entry::Vacant(entry) = luts.entry(path.to_owned()) {
        entry.insert(CubeLutState::Loading(start_cube_lut_load(path.to_owned())));
    }
}

fn queue_layer_luts(luts: &mut HashMap<String, CubeLutState>, layer: &SceneLayer) {
    if let Some(lut) = &layer.lut {
        queue_cube_lut(luts, &lut.path);
    }
    match &layer.source {
        LayerSource::Transition {
            outgoing, incoming, ..
        } => {
            queue_layer_luts(luts, outgoing);
            queue_layer_luts(luts, incoming);
        }
        LayerSource::Matte { layers, .. } => {
            queue_layer_luts(luts, &layers[0]);
            queue_layer_luts(luts, &layers[1]);
        }
        LayerSource::CanvasPass
        | LayerSource::Media { .. }
        | LayerSource::Still { .. }
        | LayerSource::Sticker { .. }
        | LayerSource::Lottie { .. }
        | LayerSource::Text { .. }
        | LayerSource::Solid(_)
        | LayerSource::Shape { .. }
        | LayerSource::PathShape { .. } => {}
    }
}

/// Per-asset Lottie frame-cache budget. At the 512 px render cap
/// (~1 MB/frame) this holds ~32 frames — a full loop of a typical short
/// sticker, so steady-state playback rasterizes nothing.
pub(super) const LOTTIE_CACHE_BYTES: usize = 32 << 20;

/// A Lottie path the renderer has seen: parsed and playable, or failed
/// (missing/unsupported file — draws nothing, logged once at load).
pub(super) enum LottieState {
    Loaded(Box<LottiePlayer>),
    Failed,
}

pub(super) struct LottiePlayer {
    pub(super) animation: aria_decoder::LottieAnimation,
    /// Rasterized frames keyed by sampled-frame index, stamped with the
    /// scene counter of their last use (the LRU key).
    pub(super) frames: HashMap<usize, (RgbaImage, u64)>,
}

/// Make one sampled frame resident while keeping cache memory bounded.
///
/// An evicted frame's pixel vector becomes the next render target. Once the
/// byte budget is warm, playback therefore replaces cached frames without a
/// new frame-sized allocation.
fn cache_lottie_frame(
    player: &mut LottiePlayer,
    index: usize,
    stamp: u64,
    budget: usize,
) -> Result<(), aria_core::DecodeError> {
    if let Some((_, used)) = player.frames.get_mut(&index) {
        *used = stamp;
        return Ok(());
    }

    let required = player.animation.frame_buffer_len();
    let mut total: usize = player
        .frames
        .values()
        .map(|(frame, _)| frame.pixels.len())
        .sum();
    let mut recycled = Vec::new();
    while total.saturating_add(required) > budget {
        let Some((&victim, _)) = player
            .frames
            .iter()
            .filter(|(_, (_, used))| *used != stamp)
            .min_by_key(|(_, (_, used))| *used)
        else {
            break;
        };
        let (evicted, _) = player.frames.remove(&victim).expect("victim exists");
        total -= evicted.pixels.len();
        if evicted.pixels.capacity() > recycled.capacity() {
            recycled = evicted.pixels;
        }
    }

    let image = player.animation.render_frame_reusing(index, recycled)?;
    player.frames.insert(index, (image, stamp));
    Ok(())
}

/// A decoded sticker asset: every frame up front plus cumulative frame ends,
/// so per-composite frame selection is logarithmic and decode-free.
pub(super) struct StickerSequence {
    pub(super) frames: Vec<RgbaImage>,
    /// Exclusive display end for each frame in milliseconds.
    pub(super) ends_ms: Vec<u64>,
    /// End of the final frame.
    pub(super) total_ms: u64,
}

pub(super) fn sticker_frame_index(ends_ms: &[u64], ms: u64) -> (usize, u32) {
    let mut low = 0;
    let mut high = ends_ms.len();
    let mut comparisons = 0;
    while low < high {
        comparisons += 1;
        let middle = low + (high - low) / 2;
        if ends_ms[middle] <= ms {
            low = middle + 1;
        } else {
            high = middle;
        }
    }
    (low.min(ends_ms.len().saturating_sub(1)), comparisons)
}

impl StickerSequence {
    pub(super) fn new(frames: Vec<RgbaImage>, delays_ms: &[u32]) -> Self {
        debug_assert_eq!(frames.len(), delays_ms.len());
        let mut elapsed = 0u64;
        let ends_ms = delays_ms
            .iter()
            .map(|delay| {
                elapsed = elapsed.saturating_add(u64::from(*delay));
                elapsed
            })
            .collect();
        Self {
            frames,
            ends_ms,
            total_ms: elapsed,
        }
    }

    /// Index of the frame on screen at `local_time` seconds, looping over
    /// the sequence. Static stickers (one frame) always show frame 0.
    pub(super) fn frame_at(&self, local_time: f64) -> usize {
        if self.frames.len() <= 1 || self.total_ms == 0 {
            return 0;
        }
        let ms = (local_time.max(0.0) * 1000.0) as u64 % self.total_ms;
        sticker_frame_index(&self.ends_ms, ms).0
    }
}

/// The renderer's starting decode mode: zero-copy GPU surfaces on Apple,
/// Windows, and Linux (with a CPU fallback in [`Renderer::render_scene`]).
/// Android stays on CPU planes until AHardwareBuffer import exists.
#[cfg(any(target_vendor = "apple", target_os = "windows", target_os = "linux"))]
pub(super) fn default_decode_mode() -> OutputMode {
    OutputMode::Gpu
}

#[cfg(not(any(target_vendor = "apple", target_os = "windows", target_os = "linux")))]
pub(super) fn default_decode_mode() -> OutputMode {
    OutputMode::Cpu
}

/// Open the platform's native decoder for `path` in `mode`. Apple, Windows,
/// and Linux honor GPU output; Android always decodes to CPU planes.
#[cfg(target_vendor = "apple")]
pub(super) fn open_decoder(
    path: &Path,
    mode: OutputMode,
) -> Result<Box<dyn VideoDecoder>, RenderError> {
    Ok(Box::new(aria_decoder::AvfDecoder::open(path, mode)?))
}

#[cfg(target_os = "windows")]
pub(super) fn open_decoder(
    path: &Path,
    mode: OutputMode,
) -> Result<Box<dyn VideoDecoder>, RenderError> {
    Ok(Box::new(aria_decoder::WmfDecoder::open(path, mode)?))
}

#[cfg(target_os = "android")]
pub(super) fn open_decoder(
    path: &Path,
    _mode: OutputMode,
) -> Result<Box<dyn VideoDecoder>, RenderError> {
    Ok(Box::new(aria_decoder::MediaCodecDecoder::open(
        path,
        OutputMode::Cpu,
    )?))
}

#[cfg(target_os = "linux")]
pub(super) fn open_decoder(
    path: &Path,
    mode: OutputMode,
) -> Result<Box<dyn VideoDecoder>, RenderError> {
    Ok(Box::new(aria_decoder::GstDecoder::open(path, mode)?))
}

#[cfg(not(any(
    target_vendor = "apple",
    target_os = "windows",
    target_os = "android",
    target_os = "linux"
)))]
pub(super) fn open_decoder(
    _path: &Path,
    _mode: OutputMode,
) -> Result<Box<dyn VideoDecoder>, RenderError> {
    Err(RenderError::unsupported(
        "no native video decoder for this platform",
    ))
}

impl Renderer {
    /// Decode the frame of `media` at `source_time`, opening (and caching) a
    /// decoder for this `(media, slot)` use on first sight — over the
    /// media's proxy file when one is registered (see
    /// [`set_proxy`](Self::set_proxy)). Under [`SeekPolicy::NearestSync`]
    /// the frame may be the sync point before `source_time` rather than the
    /// exact frame (see [`SeekPolicy`]).
    pub(super) fn decode(
        &mut self,
        project: &Project,
        media: MediaId,
        slot: u32,
        source_time: RationalTime,
        policy: SeekPolicy,
    ) -> Result<VideoFrame, RenderError> {
        let mode = self.decode_mode;
        let proxy = self.use_proxies.then(|| self.proxies.get(&media)).flatten();
        let decoder = match self.decoders.entry((media, slot)) {
            std::collections::hash_map::Entry::Occupied(e) => e.into_mut(),
            std::collections::hash_map::Entry::Vacant(e) => {
                let src = project
                    .media(media)
                    .ok_or(RenderError::MissingMedia(media))?;
                let path = proxy.map(PathBuf::as_path).unwrap_or_else(|| src.path());
                e.insert(open_decoder(path, mode)?)
            }
        };
        let mut frame = match policy {
            SeekPolicy::Exact => decoder.frame_at(source_time)?,
            SeekPolicy::NearestSync => decoder.frame_at_nearest(source_time)?,
        };
        // An exact target at the media's very end can overshoot EOF: the
        // pool trusts the container's frame count, which routinely
        // over-reports by one, and proxies additionally run a frame short
        // by construction (generation drops that untrustworthy tail). Show
        // the nearest decodable frame instead of failing the whole
        // composite — transitions pin the outgoing side at the final
        // source frame, so a miss here would error every frame of the
        // window.
        if frame.is_none() {
            frame = decoder.frame_at_nearest(source_time)?;
        }
        frame.ok_or(RenderError::NoFrame {
            media,
            time: source_time,
        })
    }

    /// Decode `media`'s single still frame into the cache on first use.
    pub(super) fn ensure_still(
        &mut self,
        project: &Project,
        media: MediaId,
    ) -> Result<(), RenderError> {
        if self.stills.contains_key(&media) {
            return Ok(());
        }
        let src = project
            .media(media)
            .ok_or(RenderError::MissingMedia(media))?;
        let image = aria_decoder::decode_image(src.path())?;
        self.stills.insert(media, image);
        Ok(())
    }

    /// Decode a sticker's whole frame sequence into the cache on first use
    /// (mirrors [`ensure_still`](Self::ensure_still)).
    pub(super) fn ensure_sticker(
        &mut self,
        spec: &aria_models::StickerSpec,
    ) -> Result<(), RenderError> {
        if self.stickers.contains_key(spec.id.as_str()) {
            return Ok(());
        }
        let decoded = aria_decoder::decode_animation(&spec.bytes)?;
        let delays_ms: Vec<u32> = decoded.iter().map(|f| f.delay_ms).collect();
        self.stickers.insert(
            spec.id.clone(),
            StickerSequence::new(
                decoded.into_iter().map(|frame| frame.image).collect(),
                &delays_ms,
            ),
        );
        Ok(())
    }

    /// Make the Lottie frame for `local_time` resident (parse the file on
    /// first sight, rasterize the frame unless cached) and return its
    /// sampled-frame index. `None` — draw nothing — for files that are
    /// missing, unparseable, or fail to rasterize; the failure is
    /// remembered and logged once, not re-probed per frame.
    pub(super) fn ensure_lottie_frame(&mut self, path: &str, local_time: f64) -> Option<usize> {
        let stamp = self.lottie_stamp;
        let state = self.lottie.entry(path.to_owned()).or_insert_with(|| {
            match aria_decoder::LottieAnimation::load(std::path::Path::new(path)) {
                Ok(animation) => LottieState::Loaded(Box::new(LottiePlayer {
                    animation,
                    frames: HashMap::new(),
                })),
                Err(e) => {
                    tracing::warn!("lottie '{path}' failed to load: {e}");
                    LottieState::Failed
                }
            }
        });
        let LottieState::Loaded(player) = state else {
            return None;
        };

        let index = player.animation.frame_index_at(local_time);
        if let Err(e) = cache_lottie_frame(player, index, stamp, LOTTIE_CACHE_BYTES) {
            tracing::warn!("lottie '{path}' frame {index} failed to render: {e}");
            return None;
        }
        Some(index)
    }

    /// Start loading every LUT referenced by `scene` before layer realization.
    ///
    /// Workers run concurrently with media decode and rasterization. Realization
    /// waits only if it reaches a LUT before that worker has completed.
    pub(super) fn preload_scene_luts(&mut self, scene: &Scene) {
        for layer in &scene.layers {
            queue_layer_luts(&mut self.luts, layer);
        }
    }

    /// Return a scene LUT only after its background load has completed.
    pub(super) fn resolve_scene_lut(&mut self, lut: &Option<SceneLut>) -> Option<SceneLut> {
        let lut = lut.as_ref()?;
        queue_cube_lut(&mut self.luts, &lut.path);
        self.luts
            .get_mut(&lut.path)
            .expect("queued LUT state exists")
            .finish_loading()
            .then(|| lut.clone())
    }
}

#[cfg(test)]
mod lottie_cache_tests {
    use super::*;

    const RED_RECT: &str = r#"{
      "v": "5.7.0", "fr": 30, "ip": 0, "op": 60, "w": 100, "h": 100,
      "layers": [{
        "ty": 4, "ip": 0, "op": 60, "st": 0,
        "ks": {"o": {"a": 0, "k": 100}, "p": {"a": 0, "k": [50, 50]},
               "a": {"a": 0, "k": [0, 0, 0]}, "s": {"a": 0, "k": [100, 100, 100]},
               "r": {"a": 0, "k": 0}},
        "shapes": [
          {"ty": "rc", "p": {"a": 0, "k": [0, 0]}, "s": {"a": 0, "k": [80, 80]}, "r": {"a": 0, "k": 0}},
          {"ty": "fl", "c": {"a": 0, "k": [1, 0, 0, 1]}, "o": {"a": 0, "k": 100}}
        ]
      }]
    }"#;

    /// The bounded production cache must hand an evicted image allocation to
    /// the next rasterization rather than freeing and reallocating it.
    #[test]
    fn lottie_cache_recycles_evicted_frame_storage() {
        let animation = aria_decoder::LottieAnimation::parse(RED_RECT).unwrap();
        let budget = animation.frame_buffer_len();
        let mut player = LottiePlayer {
            animation,
            frames: HashMap::new(),
        };

        cache_lottie_frame(&mut player, 0, 1, budget).unwrap();
        let pointer = player.frames[&0].0.pixels.as_ptr();
        cache_lottie_frame(&mut player, 1, 2, budget).unwrap();

        assert!(!player.frames.contains_key(&0));
        assert_eq!(player.frames[&1].0.pixels.as_ptr(), pointer);
        assert_eq!(player.frames[&1].0.pixels.len(), budget);
    }
}

#[cfg(test)]
mod lut_loading_tests {
    use super::*;
    use std::sync::{LazyLock, Mutex};
    use std::thread::ThreadId;

    static LOADER_THREAD: LazyLock<Mutex<Option<ThreadId>>> = LazyLock::new(|| Mutex::new(None));

    fn record_loader_thread(_: &str) -> CubeLutLoad {
        *LOADER_THREAD.lock().expect("loader thread record") = Some(std::thread::current().id());
        CubeLutLoad::Failed
    }

    #[test]
    fn cube_lut_io_runs_off_the_realization_thread() {
        let realization_thread = std::thread::current().id();
        let result = start_cube_lut_load_with("unused.cube".into(), record_loader_thread)
            .recv()
            .expect("loader result");
        assert!(matches!(result, CubeLutLoad::Failed));
        assert_ne!(
            LOADER_THREAD
                .lock()
                .expect("loader thread record")
                .expect("loader ran"),
            realization_thread
        );
    }
}
