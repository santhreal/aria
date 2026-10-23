//! The preview worker: the one thread that renders preview pictures.
//!
//! Started once per session by [`PreviewWorker::spawn`] and stopped when the
//! session drops its handle. It renders the Viewer's frames, the pool and
//! source-monitor pictures, and, during playback, up to [`RENDER_AHEAD`] frames
//! past the playhead into a [`FrameCache`]. Each Viewer frame is converted to a
//! display texture here and, while the scope strip is open, measured here, so
//! the window receives finished images and swaps pointers.
//!
//! Work runs in three lanes. Viewer frames, scope measurements and playback
//! control run first; render-ahead frames next; pool and source-monitor
//! pictures last. [`PreviewWorker::clear`] cancels the render-ahead frame in
//! flight. A panic costs the job it happened in: the worker logs it, drops the
//! job's reply, and serves the next job.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use aria_analysis::WaveformTrace;
use aria_compositor::{FrameSink, GpuContext, GpuScopes, create_frame_target, wgpu};
use aria_core::{Cancel, Rational, RgbaImage};
use aria_engine::{Engine, EngineError};
use aria_models::RationalTime;
use aria_plugin::PluginContext;
use aria_render::{DropCause, FrameStats, SeekPolicy};
use crossbeam_channel::{Receiver, Sender, TryRecvError};
use gpui::{
    BackgroundExecutor, DevicePixels, GpuDevice, GpuTexture, RenderImage, SharedString, Task, size,
};
use parking_lot::Mutex;

use crate::app::scope_view::{ScopeBuffers, ScopeTextures, measure, measure_texture};
use crate::handoff::{Wake, Wakes, wakes};

#[path = "preview_worker_cache.rs"]
mod cache;
#[path = "preview_worker_probe.rs"]
mod probe;
pub use cache::{Cadence, FrameCache, Presented};
pub use probe::{ProbeAsk, Probed, read_texels};

/// Frames rendered past the playhead during playback.
pub const RENDER_AHEAD: i64 = 4;

/// Bytes the frame cache holds at most: the frame due next and the
/// [`RENDER_AHEAD`] frames after it, each as RGBA pixels plus a BGRA texture,
/// at 3840x2160.
pub const CACHE_BYTES: usize = (RENDER_AHEAD as usize + 1) * 2 * 3840 * 2160 * 4;

/// Pooled GPU textures retained on the worker: [`RENDER_AHEAD`] cached + 1 shown + 1 in flight.
pub const TEXTURE_POOL_SLOTS: usize = RENDER_AHEAD as usize + 2;

/// Most render-ahead frames measured for the scopes per second of playback.
pub const SCOPE_RATE_HZ: i64 = 30;

/// Playback slots per scope measurement at `rate`: the fewest that keeps
/// measurements at or under [`SCOPE_RATE_HZ`]. A render-ahead frame is measured
/// when its slot is a multiple of this.
pub fn scope_pace(rate: Rational) -> i64 {
    if !rate.is_valid() {
        return 1;
    }
    // Both terms are positive for a valid rate, so this is the ceiling; the
    // signed `div_ceil` is unstable.
    let per = SCOPE_RATE_HZ * i64::from(rate.den);
    ((i64::from(rate.num) + per - 1) / per).max(1)
}

/// The worker thread's name, as `/proc/<pid>/task/*/comm` shows it.
pub const THREAD_NAME: &str = "aria-preview";

/// A Viewer frame, converted and measured, ready to put on screen.
#[derive(Clone)]
pub struct PreviewFrame {
    /// The sequence tick the frame shows.
    pub tick: i64,
    /// The frame's straight-alpha RGBA8 pixels.
    pub rgba: Arc<RgbaImage>,
    /// The display texture, or `None` for a frame that is zero-sized or whose
    /// buffer does not match its dimensions.
    pub image: Option<Arc<RenderImage>>,
    /// The GPU texture on the window's shared `wgpu` device, when rendered with
    /// no CPU readback.
    pub texture: Option<GpuTexture>,
    /// What the render cost, by stage.
    pub stats: FrameStats,
    /// Wall time from taking the engine to the finished texture and scopes.
    pub took: Duration,
    /// The scopes measured from the frame, when the strip was open and the
    /// frame was due a measurement: every frame outside playback, and every
    /// [`scope_pace`]-th slot during it.
    pub scopes: Option<ScopeTextures>,
}

impl PreviewFrame {
    /// Bytes held: the RGBA pixels and the BGRA texture of the same size.
    fn bytes(&self) -> usize {
        if self.texture.is_some() && self.rgba.pixels.is_empty() {
            (self.rgba.width as usize)
                .saturating_mul(self.rgba.height as usize)
                .saturating_mul(4)
        } else {
            self.rgba.pixels.len().saturating_mul(2)
        }
    }
}

impl std::fmt::Debug for PreviewFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreviewFrame")
            .field("tick", &self.tick)
            .field("size", &(self.rgba.width, self.rgba.height))
            .field("texture", &(self.image.is_some() || self.texture.is_some()))
            .field("scopes", &self.scopes.is_some())
            .field("took", &self.took)
            .finish()
    }
}

/// What the session looked like when a frame was rendered: the engine revision
/// and the proxy revision. Every input to a frame other than its tick moves one
/// of the two, except a live override. A frame rendered under one is held like
/// any other: the cache holds no frame more than [`RENDER_AHEAD`] slots out, so
/// a gesture shows on screen at most that many frames late.
type Stamp = (u64, u64);

/// Render-ahead work: keep the [`RENDER_AHEAD`] slots after `now` rendered,
/// each fit within `fit` (see [`crate::resolution`]), or at canvas size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Plan {
    cadence: Cadence,
    now: i64,
    fit: Option<(u32, u32)>,
}

impl Plan {
    /// The next slot with no frame held and none failed, nearest first.
    fn next(&self, cache: &FrameCache, failed: &[i64]) -> Option<i64> {
        (1..=RENDER_AHEAD)
            .map(|ahead| self.cadence.tick(self.now.saturating_add(ahead)))
            .take_while(|tick| self.cadence.renders(*tick))
            .find(|tick| !cache.contains(*tick) && !failed.contains(tick))
    }

    /// Whether the frame at `tick` is measured for the scopes: a slot ahead of
    /// `now` that shows it is a multiple of [`scope_pace`]. Slots come due at
    /// the sequence's rate and no two frames share a slot, so at most
    /// [`SCOPE_RATE_HZ`] frames a second are measured at any speed.
    fn scoped(&self, tick: i64) -> bool {
        let pace = scope_pace(self.cadence.rate);
        (1..=RENDER_AHEAD)
            .map(|ahead| self.now.saturating_add(ahead))
            .any(|slot| self.cadence.tick(slot) == tick && slot.rem_euclid(pace) == 0)
    }
}

/// What the worker leaves for the window without a request waiting on it.
#[derive(Debug, Default)]
struct Mailbox {
    /// Scopes measured for the frame on screen when the strip opened.
    measured: Option<ScopeTextures>,
    /// Why a render-ahead frame failed, reported once per failure.
    failed: Option<String>,
    /// The colour read under the pointer for the Info panel.
    probed: Option<Probed>,
}

#[derive(Debug)]
struct Shared {
    cache: Mutex<FrameCache>,
    /// Whether Viewer frames are measured for the scope strip.
    scopes: AtomicBool,
    /// What the waveform counts, as its index in [`WaveformTrace::ALL`].
    waveform: AtomicU8,
    /// Stops the render-ahead frame in flight. Replaced by every clear.
    ahead: Mutex<Cancel>,
    /// The stage split of the most recent render, for the cause of a drop.
    stats: Mutex<FrameStats>,
    mailbox: Mutex<Mailbox>,
    wake: Wake<()>,
    /// The window's shared `wgpu` device handle, or `None` when CPU readback is used.
    gpu_device: Mutex<Option<GpuDevice>>,
    /// The adapter name of `gpu_device`, read once when the device is adopted.
    window_gpu_name: Mutex<Option<SharedString>>,
    /// The adapter name of the session's own device, which renders when the
    /// window shares none.
    session_gpu_name: Option<SharedString>,
    /// The GPU texture of the frame currently shown in the Viewer.
    shown_texture: Mutex<Option<GpuTexture>>,
    /// Cumulative GPU-to-CPU frame readbacks observed on the session renderer.
    readbacks: AtomicU64,
    /// The latest pixel the Info panel asked for and the worker has not read.
    /// A newer ask replaces it, so a fast pointer costs one read per job.
    probe: Mutex<Option<ProbeAsk>>,
}

impl Shared {
    /// What the waveform counts, as [`PreviewWorker::set_waveform`] last set.
    fn waveform(&self) -> WaveformTrace {
        let index = usize::from(self.waveform.load(Ordering::Relaxed));
        WaveformTrace::ALL.get(index).copied().unwrap_or_default()
    }
}

/// Work in the first lane.
enum Job {
    /// Render the Viewer frame at a time, fit within a size or at canvas
    /// size, and send it back.
    Frame {
        time: RationalTime,
        fit: Option<(u32, u32)>,
        cancel: Cancel,
        reply: async_channel::Sender<Result<PreviewFrame, EngineError>>,
    },
    /// Measure a frame already on screen for the scope strip.
    Measure { frame: Arc<RgbaImage>, tick: i64 },
    /// Render at `time` and measure scopes when the strip opens over a GPU-only frame.
    MeasureAt {
        time: RationalTime,
        fit: Option<(u32, u32)>,
    },
    /// Keep the frames after a slot rendered.
    Ahead(Plan),
    /// Stop rendering ahead and drop the cache.
    Clear,
    /// Read the pixel in [`Shared::probe`] out of the frame on screen.
    Probe,
}

/// Work in the last lane: a pool or source-monitor picture.
type Picture = Box<dyn FnOnce() + Send>;

/// The session's handle on the preview worker. Dropping it stops the thread
/// once the job in hand finishes.
#[derive(Debug)]
pub struct PreviewWorker {
    first_lane: Sender<Job>,
    pictures: Sender<Picture>,
    shared: Arc<Shared>,
    wakes: Wakes<()>,
}

impl PreviewWorker {
    /// Start the worker for the session rendering through `engine`, measuring
    /// the scopes `plugins` registers beside the built-in three.
    pub fn spawn(engine: Arc<Mutex<Engine>>, plugins: PluginContext) -> std::io::Result<Self> {
        // The hardware decode device opens while the window builds, so the
        // first clip's decoder finds it open.
        #[cfg(target_os = "linux")]
        aria_decoder::warm_up();
        let (first_lane, queue) = crossbeam_channel::unbounded();
        let (pictures, picture_queue) = crossbeam_channel::unbounded();
        let (wake, wakes) = wakes();
        let shared = Arc::new(Shared {
            cache: Mutex::new(FrameCache::default()),
            scopes: AtomicBool::new(false),
            waveform: AtomicU8::new(0),
            ahead: Mutex::new(Cancel::new()),
            stats: Mutex::new(FrameStats::default()),
            mailbox: Mutex::new(Mailbox::default()),
            wake,
            gpu_device: Mutex::new(None),
            window_gpu_name: Mutex::new(None),
            session_gpu_name: plugins
                .try_get::<GpuContext>()
                .map(|gpu| SharedString::from(gpu.adapter.get_info().name)),
            shown_texture: Mutex::new(None),
            readbacks: AtomicU64::new(0),
            probe: Mutex::new(None),
        });
        let worker = Arc::clone(&shared);
        std::thread::Builder::new()
            .name(THREAD_NAME.to_owned())
            .spawn(move || work(&engine, plugins, &queue, &picture_queue, &worker))?;
        Ok(Self {
            first_lane,
            pictures,
            shared,
            wakes,
        })
    }

    /// Render the Viewer frame at `time`, fit within `fit` or at canvas size.
    /// Resolves to `None` when the render panicked or the worker is gone.
    pub fn frame(
        &self,
        executor: &BackgroundExecutor,
        time: RationalTime,
        fit: Option<(u32, u32)>,
        cancel: Cancel,
    ) -> Task<Option<Result<PreviewFrame, EngineError>>> {
        let (reply, answer) = async_channel::bounded(1);
        // A send to a stopped worker drops the job and with it the reply, which
        // resolves the wait to `None`.
        let _ = self.first_lane.send(Job::Frame {
            time,
            fit,
            cancel,
            reply,
        });
        crate::handoff::reply(executor, answer)
    }

    /// Run `work`, a pool or source-monitor picture, on the worker once no
    /// Viewer frame and no render-ahead frame is waiting. Resolves to `None`
    /// when `work` panicked or the worker is gone.
    pub fn run<T: Send + 'static>(
        &self,
        executor: &BackgroundExecutor,
        work: impl FnOnce() -> T + Send + 'static,
    ) -> Task<Option<T>> {
        let (reply, answer) = async_channel::bounded(1);
        let _ = self.pictures.send(Box::new(move || {
            let _ = reply.try_send(work());
        }));
        crate::handoff::reply(executor, answer)
    }

    /// Measure `frame`, the frame on screen at `tick`, for the scope strip. The
    /// result arrives through [`Self::take_measured`] after a wake.
    pub fn measure(&self, frame: Arc<RgbaImage>, tick: i64) {
        let _ = self.first_lane.send(Job::Measure { frame, tick });
    }

    /// Read one texel of the frame on screen for the Info panel. The colour
    /// arrives through [`Self::take_probed`] after a wake. An ask made while
    /// another waits replaces it.
    pub fn probe(&self, ask: ProbeAsk) {
        let waiting = self.shared.probe.lock().replace(ask).is_some();
        if !waiting {
            let _ = self.first_lane.send(Job::Probe);
        }
    }

    /// The colour the latest probe read, once.
    pub fn take_probed(&self) -> Option<Probed> {
        self.shared.mailbox.lock().probed.take()
    }

    /// Keep the [`RENDER_AHEAD`] slots after `now` rendered, fit within `fit`
    /// or at canvas size. A frame already held at another size is still shown.
    pub fn render_ahead(&self, cadence: Cadence, now: i64, fit: Option<(u32, u32)>) {
        let _ = self.first_lane.send(Job::Ahead(Plan { cadence, now, fit }));
    }

    /// Stop rendering ahead, cancel the render-ahead frame in flight, and drop
    /// every cached frame. For a seek, a stop, and anything that changes the
    /// picture.
    pub fn clear(&self) {
        // Cancel before the cache lock: the worker checks the token under that
        // lock before it holds a frame.
        let fresh = Cancel::new();
        std::mem::replace(&mut *self.shared.ahead.lock(), fresh).cancel();
        self.shared.cache.lock().clear();
        let _ = self.first_lane.send(Job::Clear);
    }

    /// Measure Viewer frames for the scopes, or stop measuring them. Returns
    /// whether the setting changed. The frame already on screen is not
    /// measured here: the caller has its tick and asks for it with
    /// [`Self::measure`] or [`Self::measure_at`].
    pub fn set_scopes(&self, open: bool) -> bool {
        let was_open = self.shared.scopes.swap(open, Ordering::Relaxed);
        if open && !was_open {
            self.shared.cache.lock().drop_texture_only();
        }
        open != was_open
    }

    /// Count `trace` in the waveform of every measurement from now on.
    pub fn set_waveform(&self, trace: WaveformTrace) {
        let index = WaveformTrace::ALL
            .iter()
            .position(|known| *known == trace)
            .unwrap_or(0);
        self.shared
            .waveform
            .store(u8::try_from(index).unwrap_or(0), Ordering::Relaxed);
    }

    /// What the waveform counts.
    pub fn waveform(&self) -> WaveformTrace {
        self.shared.waveform()
    }

    /// Render and measure scopes at `time` (fit within `fit`) on the worker,
    /// delivering the result through [`Self::take_measured`].
    pub fn measure_at(&self, time: RationalTime, fit: Option<(u32, u32)>) {
        let _ = self.first_lane.send(Job::MeasureAt { time, fit });
    }

    /// Share the window's `wgpu` device with the preview worker, or fall back to
    /// CPU readback when `device` is `None`. When the device identity changes
    /// (initial adoption or recovery after device loss), cached frames and the
    /// shown GPU texture from the previous device are dropped; returns `true`
    /// when the device changed.
    pub fn set_gpu_device(&self, device: Option<GpuDevice>) -> bool {
        let valid = device.filter(|dev| {
            dev.downcast_ref::<(wgpu::Instance, wgpu::Adapter, wgpu::Device, wgpu::Queue)>()
                .is_some()
        });
        {
            let mut current = self.shared.gpu_device.lock();
            if *current == valid {
                return false;
            }
            *self.shared.window_gpu_name.lock() = valid
                .as_ref()
                .and_then(|dev| {
                    dev.downcast_ref::<(wgpu::Instance, wgpu::Adapter, wgpu::Device, wgpu::Queue)>()
                })
                .map(|(_, adapter, _, _)| SharedString::from(adapter.get_info().name));
            *current = valid;
        }
        *self.shared.shown_texture.lock() = None;
        self.clear();
        true
    }

    /// The name of the adapter the Viewer renders on: the window's when it
    /// shares one, the session's otherwise, and `None` with no GPU at all.
    pub fn device_name(&self) -> Option<SharedString> {
        self.shared
            .window_gpu_name
            .lock()
            .clone()
            .or_else(|| self.shared.session_gpu_name.clone())
    }

    /// The window's shared `wgpu` device currently adopted by the worker.
    pub fn gpu_device(&self) -> Option<GpuDevice> {
        self.shared.gpu_device.lock().clone()
    }

    /// Store the GPU texture of the frame currently on screen in the Viewer.
    pub fn set_shown_texture(&self, texture: Option<GpuTexture>) {
        *self.shared.shown_texture.lock() = texture;
    }

    /// The GPU texture of the frame currently on screen in the Viewer.
    pub fn shown_texture(&self) -> Option<GpuTexture> {
        self.shared.shown_texture.lock().clone()
    }

    /// Cumulative GPU-to-CPU frame readbacks observed on the session renderer.
    pub fn readback_count(&self) -> u64 {
        self.shared.readbacks.load(Ordering::Relaxed)
    }

    /// Take what is due at slot `now`; see [`FrameCache::present`]. A frame not
    /// rendered in time is charged to the larger stage of the most recent
    /// render.
    pub fn present(&self, cadence: Cadence, shown: i64, now: i64) -> Presented {
        let not_ready = DropCause::not_ready(&self.shared.stats.lock());
        self.shared
            .cache
            .lock()
            .present(cadence, shown, now, not_ready)
    }

    /// Scopes measured since the last call, for the strip that just opened.
    pub fn take_measured(&self) -> Option<ScopeTextures> {
        self.shared.mailbox.lock().measured.take()
    }

    /// Why the most recent failed render-ahead frame failed, once.
    pub fn take_failure(&self) -> Option<String> {
        self.shared.mailbox.lock().failed.take()
    }

    /// The queue of wakes for deliveries made through the mailbox.
    pub fn wakes(&self) -> Wakes<()> {
        self.wakes.clone()
    }

    /// A sender half for waking the window when background work completes.
    pub fn clone_wake(&self) -> Wake<()> {
        self.shared.wake.clone()
    }

    /// Put `work` on the worker's last lane, for tests that hold the worker
    /// busy.
    #[cfg(test)]
    pub(crate) fn occupy(&self, work: impl FnOnce() + Send + 'static) {
        let _ = self.pictures.send(Box::new(work));
    }

    /// Frames cached.
    #[cfg(test)]
    pub fn cached(&self) -> usize {
        self.shared.cache.lock().len()
    }

    /// Whether a frame for `tick` is cached.
    pub fn has_cached(&self, tick: i64) -> bool {
        self.shared.cache.lock().contains(tick)
    }
}

/// The worker thread: serve the first lane, then render ahead, then draw
/// pictures, then wait for any of them.
fn work(
    engine: &Mutex<Engine>,
    plugins: PluginContext,
    queue: &Receiver<Job>,
    pictures: &Receiver<Picture>,
    shared: &Shared,
) {
    let mut images = ScopeBuffers::with_plugins(plugins);
    let mut textures = TexturePool::default();
    let mut plan: Option<Plan> = None;
    let mut failed: Vec<i64> = Vec::new();
    loop {
        match queue.try_recv() {
            Ok(job) => {
                serve(
                    engine,
                    shared,
                    job,
                    &mut plan,
                    &mut failed,
                    &mut images,
                    &mut textures,
                );
                continue;
            }
            Err(TryRecvError::Disconnected) => return,
            Err(TryRecvError::Empty) => {}
        }
        if let Some(held) = plan {
            if !ahead(
                engine,
                shared,
                held,
                &mut failed,
                &mut images,
                &mut textures,
            ) {
                plan = None;
            }
            continue;
        }
        match pictures.try_recv() {
            Ok(picture) => {
                let _ = contained("a pool picture", picture);
                continue;
            }
            Err(TryRecvError::Disconnected) => return,
            Err(TryRecvError::Empty) => {}
        }
        // Wait without taking anything, so the next turn of the loop takes
        // work in lane order even when both lanes filled during the wait.
        let mut waiting = crossbeam_channel::Select::new();
        waiting.recv(queue);
        waiting.recv(pictures);
        waiting.ready();
    }
}

/// One job from the first lane.
fn serve(
    engine: &Mutex<Engine>,
    shared: &Shared,
    job: Job,
    plan: &mut Option<Plan>,
    failed: &mut Vec<i64>,
    images: &mut ScopeBuffers,
    textures: &mut TexturePool,
) {
    match job {
        Job::Frame {
            time,
            fit,
            cancel,
            reply,
        } => {
            let scopes = shared.scopes.load(Ordering::Relaxed);
            let rendered = contained("a preview frame", || {
                render(engine, shared, textures, time, fit, cancel, scopes, images)
            });
            if let Some(rendered) = rendered {
                let rendered = rendered.map(|(frame, _)| {
                    *shared.stats.lock() = frame.stats;
                    frame
                });
                let _ = reply.try_send(rendered);
            }
        }
        Job::Measure { frame, tick } => {
            let measured = contained("a scope measurement", || {
                measure(&frame, tick, shared.waveform(), images)
            })
            .flatten();
            if let Some(measured) = measured {
                shared.mailbox.lock().measured = Some(measured);
                shared.wake.wake(());
            }
        }
        Job::MeasureAt { time, fit } => {
            let rendered = contained("a scope frame measurement", || {
                render(
                    engine,
                    shared,
                    textures,
                    time,
                    fit,
                    Cancel::never(),
                    true,
                    images,
                )
            });
            if let Some(Ok((frame, _))) = rendered
                && let Some(measured) = frame.scopes
            {
                shared.mailbox.lock().measured = Some(measured);
                shared.wake.wake(());
            }
        }
        Job::Ahead(next) => {
            if plan.is_none_or(|held| held.cadence != next.cadence) {
                failed.clear();
            }
            *plan = Some(next);
        }
        Job::Clear => {
            *plan = None;
            failed.clear();
        }
        Job::Probe => {
            let ask = shared.probe.lock().take();
            let device = shared.gpu_device.lock().clone();
            let (Some(ask), Some(device)) = (ask, device) else {
                return;
            };
            let read = contained("a pixel probe", || {
                read_texels(&device, &ask.texture, ask.texel, [1, 1])
            })
            .flatten();
            if let Some(image) = read {
                shared.mailbox.lock().probed = Some(Probed {
                    at: ask.at,
                    tick: ask.tick,
                    rgba: image.pixel(0, 0),
                });
                shared.wake.wake(());
            }
        }
    }
}

/// Render the next frame `plan` wants into the cache. False when there is
/// nothing left to render for this plan.
fn ahead(
    engine: &Mutex<Engine>,
    shared: &Shared,
    plan: Plan,
    failed: &mut Vec<i64>,
    images: &mut ScopeBuffers,
    textures: &mut TexturePool,
) -> bool {
    let Some(tick) = plan.next(&shared.cache.lock(), failed) else {
        return false;
    };
    let cancel = shared.ahead.lock().clone();
    let scopes = shared.scopes.load(Ordering::Relaxed) && plan.scoped(tick);
    let time = RationalTime::new(tick, plan.cadence.rate);
    let rendered = contained("a render-ahead frame", || {
        render(
            engine,
            shared,
            textures,
            time,
            plan.fit,
            cancel.clone(),
            scopes,
            images,
        )
    });
    match rendered {
        Some(Ok((frame, stamp))) => {
            *shared.stats.lock() = frame.stats;
            let near = plan.cadence.tick(plan.now);
            // Checked under the cache lock, which a clear takes after it
            // cancels: a frame rendered before an edit and finished after it
            // is either refused here or emptied by that clear.
            let mut cache = shared.cache.lock();
            if !cancel.is_cancelled() {
                cache.insert(stamp, frame, near);
            }
            true
        }
        Some(Err(err)) if err.is_cancelled() => true,
        Some(Err(err)) => {
            failed.push(tick);
            shared.mailbox.lock().failed = Some(err.to_string());
            shared.wake.wake(());
            true
        }
        None => {
            failed.push(tick);
            true
        }
    }
}

struct PooledSlot {
    raw: wgpu::Texture,
    gpu: GpuTexture,
}

#[derive(Default)]
struct TexturePool {
    device: Option<GpuDevice>,
    size: (u32, u32),
    slots: Vec<PooledSlot>,
    cursor: usize,
    /// The scope pass on `device`: `None` until scopes first read a texture
    /// frame, `Some(None)` once the device has shown it cannot run the pass.
    scopes: Option<Option<GpuScopes>>,
}

impl TexturePool {
    fn sync_device(&mut self, target_device: Option<&GpuDevice>) -> Option<GpuContext> {
        match target_device {
            Some(dev) if self.device.as_ref() != Some(dev) => {
                let (instance, adapter, device, queue) =
                    dev.downcast_ref::<(wgpu::Instance, wgpu::Adapter, wgpu::Device, wgpu::Queue)>(
                    )?;
                self.slots.clear();
                self.size = (0, 0);
                self.cursor = 0;
                self.scopes = None;
                self.device = Some(dev.clone());
                Some(GpuContext::from_parts(
                    instance.clone(),
                    adapter.clone(),
                    device.clone(),
                    queue.clone(),
                ))
            }
            None if self.device.is_some() => {
                self.slots.clear();
                self.size = (0, 0);
                self.cursor = 0;
                self.scopes = None;
                self.device = None;
                None
            }
            _ => None,
        }
    }

    /// The scope pass on the pool's device, built the first time it is
    /// asked for. `None` without a device, or on one that runs no compute
    /// pass or has failed one.
    fn scopes(&mut self) -> Option<&mut GpuScopes> {
        let (_, _, device, queue) =
            self.device
                .as_ref()?
                .downcast_ref::<(wgpu::Instance, wgpu::Adapter, wgpu::Device, wgpu::Queue)>()?;
        self.scopes
            .get_or_insert_with(|| GpuScopes::new(device, queue))
            .as_mut()
    }

    fn acquire(
        &mut self,
        width: u32,
        height: u32,
        shared: &Shared,
    ) -> Option<(&wgpu::Texture, GpuTexture)> {
        let (_, _, device, _) =
            self.device
                .as_ref()?
                .downcast_ref::<(wgpu::Instance, wgpu::Adapter, wgpu::Device, wgpu::Queue)>()?;
        if width == 0 || height == 0 {
            return None;
        }
        if self.size != (width, height) {
            self.slots.clear();
            self.size = (width, height);
            self.cursor = 0;
        }
        let shown = shared.shown_texture.lock().clone();
        let cache = shared.cache.lock();
        let len = self.slots.len();
        for offset in 0..len {
            let idx = (self.cursor + offset) % len;
            let candidate = &self.slots[idx].gpu;
            if shown.as_ref() != Some(candidate) && !cache.holds_texture(candidate) {
                self.cursor = (idx + 1) % len.max(1);
                let slot = &self.slots[idx];
                return Some((&slot.raw, slot.gpu.clone()));
            }
        }
        drop(cache);
        if self.slots.len() < TEXTURE_POOL_SLOTS.max(len + 1) {
            let raw = create_frame_target(device, width, height);
            let view = raw.create_view(&wgpu::TextureViewDescriptor::default());
            let gpu = GpuTexture::new(
                (raw.clone(), view),
                size(DevicePixels(width as i32), DevicePixels(height as i32)),
            );
            self.slots.push(PooledSlot { raw, gpu });
            let idx = self.slots.len() - 1;
            self.cursor = (idx + 1) % self.slots.len();
            let slot = &self.slots[idx];
            return Some((&slot.raw, slot.gpu.clone()));
        }
        None
    }
}

struct PreviewSink<'a> {
    shared: &'a Shared,
    pool: &'a mut TexturePool,
    adopt: Option<GpuContext>,
    use_texture: bool,
    acquired: Option<(wgpu::Texture, GpuTexture)>,
    rgba: Option<RgbaImage>,
}

impl FrameSink for PreviewSink<'_> {
    fn pixels(&mut self, width: u32, height: u32) -> &mut [u8] {
        let image = self.rgba.insert(RgbaImage::new(
            width,
            height,
            vec![0; (width as usize) * (height as usize) * 4],
        ));
        &mut image.pixels
    }

    fn adopt_gpu(&mut self) -> Option<GpuContext> {
        self.adopt.take()
    }

    fn texture(&mut self, width: u32, height: u32) -> Option<&wgpu::Texture> {
        if !self.use_texture {
            return None;
        }
        let (raw, gpu) = self.pool.acquire(width, height, self.shared)?;
        Some(&self.acquired.insert((raw.clone(), gpu)).0)
    }

    fn record_readbacks(&mut self, count: u64) {
        self.shared.readbacks.store(count, Ordering::Relaxed);
    }
}

/// One Viewer frame: rendered under the engine lock, fit within `fit` or at
/// canvas size, into a pooled GPU texture when the window shares its `wgpu`
/// device, or read back to CPU otherwise. With `scopes`, a texture frame is
/// measured on the device; a frame a plugin scope reads, or one on a device
/// that runs no scope pass, is read back and measured on the CPU.
#[allow(clippy::too_many_arguments)]
fn render(
    engine: &Mutex<Engine>,
    shared: &Shared,
    textures: &mut TexturePool,
    time: RationalTime,
    fit: Option<(u32, u32)>,
    cancel: Cancel,
    scopes: bool,
    images: &mut ScopeBuffers,
) -> Result<(PreviewFrame, Stamp), EngineError> {
    let started = Instant::now();
    let target_device = shared.gpu_device.lock().clone();
    let adopt = textures.sync_device(target_device.as_ref());
    let use_texture = target_device.is_some()
        && (!scopes || (!images.plugins.any() && textures.scopes().is_some()));
    let mut sink = PreviewSink {
        shared,
        pool: textures,
        adopt,
        use_texture,
        acquired: None,
        rgba: None,
    };
    let (stats, stamp, transparent) = {
        // Token and frame under one lock: a token set while another caller
        // held the engine would be spent by that caller's frame.
        let mut engine = engine.lock();
        // The grid is laid under the frame on the CPU, so a transparent frame
        // is read back rather than presented as a texture.
        let transparent = engine.preview_transparency();
        sink.use_texture &= !transparent;
        engine.cancel_next_frame(cancel);
        match fit {
            Some((width, height)) => {
                engine.get_frame_fit_into(time, width, height, SeekPolicy::Exact, &mut sink)?;
            }
            None => {
                engine.get_frame_into(time, SeekPolicy::Exact, &mut sink)?;
            }
        }
        (
            engine.last_frame_stats(),
            (engine.revision(), engine.proxy_revision()),
            transparent,
        )
    };
    if let Some((raw, texture)) = sink.acquired {
        let pass = if scopes { sink.pool.scopes() } else { None };
        let measured = match pass {
            Some(pass) => {
                match measure_texture(pass, &raw, time.value, shared.waveform(), images) {
                    Ok(measured) => measured,
                    Err(error) => {
                        // The next frame is read back and measured on the CPU.
                        tracing::warn!(%error, "the scope pass failed; scopes read frames back");
                        sink.pool.scopes = Some(None);
                        None
                    }
                }
            }
            None => None,
        };
        let wgpu::Extent3d { width, height, .. } = raw.size();
        return Ok((
            PreviewFrame {
                tick: time.value,
                rgba: Arc::new(RgbaImage::new(width, height, Vec::new())),
                image: None,
                texture: Some(texture),
                stats,
                took: started.elapsed(),
                scopes: measured,
            },
            stamp,
        ));
    }
    let rgba = sink.rgba.unwrap_or_else(|| RgbaImage::transparent(0, 0));
    let image = if transparent {
        crate::frame::rgba_over_grid_to_render(rgba.width, rgba.height, &rgba.pixels)
    } else {
        crate::frame::rgba_to_render(&rgba)
    };
    let scopes = scopes
        .then(|| measure(&rgba, time.value, shared.waveform(), images))
        .flatten();
    Ok((
        PreviewFrame {
            tick: time.value,
            rgba: Arc::new(rgba),
            image,
            texture: None,
            stats,
            took: started.elapsed(),
            scopes,
        },
        stamp,
    ))
}

/// Run `job`, turning a panic into `None` and a log line naming `what`.
fn contained<T>(what: &str, job: impl FnOnce() -> T) -> Option<T> {
    match catch_unwind(AssertUnwindSafe(job)) {
        Ok(value) => Some(value),
        Err(panic) => {
            let message = panic
                .downcast_ref::<&str>()
                .map(|text| (*text).to_owned())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "a non-text panic".to_owned());
            tracing::error!("{what} panicked on the preview worker: {message}");
            None
        }
    }
}

#[cfg(test)]
#[path = "preview_worker_tests.rs"]
mod tests;
