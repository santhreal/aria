//! Preview audio: cpal output driven by [`ExportAudioMixer`].
//!
//! [`AudioSystem`] owns the cpal stream and is `!Send` (stays on the UI
//! thread). [`AudioHandle`] is `Send + Clone` and lives on [`crate::app::AppState`].
//! Tests that never attach a system keep the wall-clock playhead.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, Ordering, fence};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use aria_models::TrackId;
use aria_render::{EXPORT_AUDIO_CHANNELS, EXPORT_AUDIO_RATE, ExportAudioMixer};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::{Receiver, Sender, bounded};
use parking_lot::Mutex;

const MIX_FRAMES: usize = 1024;
const MIX_BLOCK_POOL: usize = 9;
const INACTIVE: i64 = i64::MIN;
/// Duration of one audio scrub grain in milliseconds.
pub const SCRUB_GRAIN_MS: u32 = 40;
/// Envelope fade length at each edge of a scrub grain, in stereo frames.
const SCRUB_FADE_FRAMES: usize = 32;

/// Stereo frames in one 40 ms scrub grain at `sample_rate`.
pub const fn scrub_grain_frames(sample_rate: u32) -> usize {
    ((sample_rate as u64 * SCRUB_GRAIN_MS as u64) / 1000) as usize
}

/// Snapshot the mixer needs to start (or restart) preview audio.
#[derive(Clone)]
pub struct PlaybackSnapshot {
    /// A mixer plan containing only audible spans and the resolved bus graph.
    ///
    /// Building this copies the audio subset while the project is locked. The
    /// snapshot itself is then a cheap handle, and the audio system consumes
    /// the prepared mixer when it rebuilds the stream.
    pub mixer: Arc<Mutex<Option<Result<Option<ExportAudioMixer>, String>>>>,
    pub start_tick: i64,
    pub fps_num: i32,
    pub fps_den: i32,
    pub speed_num: i32,
    pub speed_den: i32,
}

/// One pending 40 ms scrub grain request.
///
/// The queue holds a single slot: a new pointer move overwrites any grain that
/// has not started playing yet, so a fast drag never queues more than one grain
/// behind the pointer.
#[derive(Clone)]
pub struct ScrubGrain {
    pub mixer: Option<Arc<Mutex<Option<Result<Option<ExportAudioMixer>, String>>>>>,
    pub tick: i64,
    pub fps_num: i32,
    pub fps_den: i32,
}

impl std::fmt::Debug for PlaybackSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlaybackSnapshot")
            .field("start_tick", &self.start_tick)
            .field("fps_num", &self.fps_num)
            .field("fps_den", &self.fps_den)
            .field("speed_num", &self.speed_num)
            .field("speed_den", &self.speed_den)
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for ScrubGrain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScrubGrain")
            .field("has_mixer", &self.mixer.is_some())
            .field("tick", &self.tick)
            .field("fps_num", &self.fps_num)
            .field("fps_den", &self.fps_den)
            .finish()
    }
}

/// Post-fader peak per mixer lane, keyed by the lane it belongs to.
///
/// Armed once when the stream starts, from the mixer's own lane list, and
/// written from the mix thread: a block neither allocates nor takes a lock, and
/// a reading cannot be attributed to a lane the mix does not carry. A lane with
/// no slot reads silence, which is what a strip for a lane outside the mix
/// draws.
pub struct LaneMeters {
    lanes: Vec<(TrackId, [AtomicU32; 2])>,
}

impl LaneMeters {
    /// A table with a slot per lane, in the mixer's fold order.
    pub fn for_lanes(lanes: impl Iterator<Item = TrackId>) -> Self {
        Self {
            lanes: lanes
                .map(|track| (track, [AtomicU32::new(0), AtomicU32::new(0)]))
                .collect(),
        }
    }

    /// Record what each lane just sent on. Called once per mixed block from the
    /// mix thread, so it stores into slots it already has and takes no lock.
    ///
    /// A lane with no slot is dropped rather than grown into: the table's shape
    /// is the mix's, and a stream whose graph changed is restarted.
    pub fn publish(&self, peaks: impl Iterator<Item = (TrackId, [f32; 2])>) {
        for (track, peak) in peaks {
            let Some(slot) = self.slot(track) else {
                continue;
            };
            slot[0].store(peak[0].to_bits(), Ordering::Relaxed);
            slot[1].store(peak[1].to_bits(), Ordering::Relaxed);
        }
    }

    /// The lane's last peak, per channel. Silence for a lane the mix is not
    /// carrying, so a strip outside the mix draws no level rather than someone
    /// else's.
    pub fn peak(&self, track: TrackId) -> [f32; 2] {
        match self.slot(track) {
            Some(slot) => [
                f32::from_bits(slot[0].load(Ordering::Relaxed)),
                f32::from_bits(slot[1].load(Ordering::Relaxed)),
            ],
            None => [0.0, 0.0],
        }
    }

    /// Every metered lane and its level, for a pass that advances all of them.
    pub fn iter(&self) -> impl Iterator<Item = (TrackId, [f32; 2])> + '_ {
        self.lanes
            .iter()
            .map(|(track, _)| (*track, self.peak(*track)))
    }

    /// How many lanes are metered. Zero while nothing is playing.
    pub fn len(&self) -> usize {
        self.lanes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lanes.is_empty()
    }

    fn slot(&self, track: TrackId) -> Option<&[AtomicU32; 2]> {
        self.lanes
            .iter()
            .find(|(id, _)| *id == track)
            .map(|(_, slot)| slot)
    }
}

impl Default for LaneMeters {
    fn default() -> Self {
        Self::for_lanes(std::iter::empty())
    }
}

/// Where the device is in the stream that is playing: written by the audio
/// callback once per block, read by the frame clock at any time.
///
/// A sequence count around the three values keeps a read from mixing two
/// blocks' numbers. The callback is the only writer, and neither side takes a
/// lock.
#[derive(Default)]
struct DeviceClock {
    seq: AtomicU64,
    heard: AtomicI64,
    handed: AtomicI64,
    at: AtomicU64,
}

/// One block's reading of the device clock, in output frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DeviceAnchor {
    /// Frames heard at `at`: the frames handed before this block, less the
    /// device's output latency. Negative until the first sample is heard.
    heard: i64,
    /// Frames handed to the device, which the clock never passes.
    handed: i64,
    /// When the block was handed over, in nanoseconds since the handle's epoch.
    at: u64,
}

impl DeviceClock {
    fn write(&self, anchor: DeviceAnchor) {
        self.seq.fetch_add(1, Ordering::Relaxed);
        fence(Ordering::Release);
        self.heard.store(anchor.heard, Ordering::Relaxed);
        self.handed.store(anchor.handed, Ordering::Relaxed);
        self.at.store(anchor.at, Ordering::Relaxed);
        self.seq.fetch_add(1, Ordering::Release);
    }

    /// The latest anchor, or `None` while no block has been handed. Gives up
    /// after a few torn reads rather than spinning against the callback.
    fn read(&self) -> Option<DeviceAnchor> {
        for _ in 0..4 {
            let before = self.seq.load(Ordering::Acquire);
            if before == 0 {
                return None;
            }
            if before % 2 == 1 {
                std::hint::spin_loop();
                continue;
            }
            let anchor = DeviceAnchor {
                heard: self.heard.load(Ordering::Relaxed),
                handed: self.handed.load(Ordering::Relaxed),
                at: self.at.load(Ordering::Relaxed),
            };
            fence(Ordering::Acquire);
            if self.seq.load(Ordering::Relaxed) == before {
                return Some(anchor);
            }
        }
        None
    }
}

/// What converts device frames to sequence ticks for one stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StreamTiming {
    start_tick: i64,
    rate: u32,
    fps_num: i32,
    fps_den: i32,
    speed_num: i32,
    speed_den: i32,
}

impl StreamTiming {
    /// The tick being heard `now` nanoseconds after the epoch: the anchor's
    /// heard position run forward at the device rate, held at the stream's
    /// start before the first sample is heard and at the last frame handed
    /// when the device stalls.
    fn tick_at(self, anchor: DeviceAnchor, now: u64) -> i64 {
        let since = u128::from(now.saturating_sub(anchor.at));
        let ran = i64::try_from(since * u128::from(self.rate) / 1_000_000_000).unwrap_or(i64::MAX);
        let frames = anchor
            .heard
            .saturating_add(ran)
            .clamp(0, anchor.handed.max(0));
        tick_from_samples(
            self.start_tick,
            frames,
            self.rate,
            self.fps_num,
            self.fps_den,
            self.speed_num,
            self.speed_den,
        )
    }

    /// Frames the device takes to play `latency`.
    fn frames(self, latency: Duration) -> i64 {
        i64::try_from(latency.as_nanos() * u128::from(self.rate) / 1_000_000_000)
            .unwrap_or(i64::MAX)
    }
}

struct Shared {
    playing: AtomicBool,
    /// The tick heard at the last block, or [`INACTIVE`]. Read when the device
    /// clock has no anchor yet.
    tick: AtomicI64,
    epoch: Instant,
    device: DeviceClock,
    timing: Mutex<Option<StreamTiming>>,
    snapshot: Mutex<Option<PlaybackSnapshot>>,
    generation: std::sync::atomic::AtomicU64,
    scrub: Mutex<Option<ScrubGrain>>,
    scrub_generation: std::sync::atomic::AtomicU64,
    /// `f32` bits. Written from the audio callback, so it must stay lock-free.
    peak_l: AtomicU32,
    peak_r: AtomicU32,
    rms_l: AtomicU32,
    rms_r: AtomicU32,
    /// Set once a sample reached full scale and stays set until it is read and
    /// cleared: a clip that lasted one block is exactly the one worth catching.
    clipped: AtomicBool,
    /// The lanes of the mix now playing and what each last sent on. Replaced
    /// wholesale when a stream starts or stops, and read by the repaint clock:
    /// the mix thread holds its own reference, so publishing a block never
    /// touches this lock.
    lanes: Mutex<Arc<LaneMeters>>,
}

impl Shared {
    fn nanos(&self) -> u64 {
        u64::try_from(self.epoch.elapsed().as_nanos()).unwrap_or(u64::MAX)
    }

    /// Record a block handed to the device: `before` frames were handed
    /// before it, `after` with it, and the device plays its first frame
    /// `latency` from now. Called from the audio callback: no lock, no
    /// allocation.
    fn handed(&self, timing: StreamTiming, before: i64, after: i64, latency: Duration) {
        let heard = before.saturating_sub(timing.frames(latency));
        self.device.write(DeviceAnchor {
            heard,
            handed: after,
            at: self.nanos(),
        });
        self.tick.store(
            tick_from_samples(
                timing.start_tick,
                heard.max(0),
                timing.rate,
                timing.fps_num,
                timing.fps_den,
                timing.speed_num,
                timing.speed_den,
            ),
            Ordering::Relaxed,
        );
    }

    /// Record what this block will sound like: the loudest sample and the root
    /// mean square per channel, and whether anything reached full scale.
    fn publish(&self, data: &[f32], channels: usize) {
        let levels = block_levels(data, channels);
        self.peak_l
            .store(levels.peak[0].to_bits(), Ordering::Relaxed);
        self.peak_r
            .store(levels.peak[1].to_bits(), Ordering::Relaxed);
        self.rms_l.store(levels.rms[0].to_bits(), Ordering::Relaxed);
        self.rms_r.store(levels.rms[1].to_bits(), Ordering::Relaxed);
        if levels.clipped {
            self.clipped.store(true, Ordering::Relaxed);
        }
    }
}

/// The levels of one interleaved block, per meter channel, in full scale.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct BlockLevels {
    /// The loudest magnitude.
    pub peak: [f32; 2],
    /// The root mean square.
    pub rms: [f32; 2],
    /// Whether any sample reached full scale.
    pub clipped: bool,
}

/// The peak and root mean square of an interleaved block, per meter channel,
/// and whether any sample reached full scale.
///
/// Two bars whatever the device offers: a mono device doubles its one channel and
/// a surround device folds everything past the second into the nearer bar, so the
/// meter reads the same everywhere rather than going blank on unusual hardware.
pub fn block_levels(data: &[f32], channels: usize) -> BlockLevels {
    if channels == 0 || data.is_empty() {
        return BlockLevels::default();
    }
    let mut peak = [0.0f32; 2];
    let mut squares = [0.0f32; 2];
    let mut counts = [0u32; 2];
    let mut clipped = false;
    for (index, sample) in data.iter().enumerate() {
        let magnitude = sample.abs();
        // NaN from a broken decoder must not poison the meter or read as a clip.
        if !magnitude.is_finite() {
            continue;
        }
        let side = usize::from(index % channels >= (channels + 1) / 2 && channels > 1);
        peak[side] = peak[side].max(magnitude);
        squares[side] += magnitude * magnitude;
        counts[side] += 1;
        clipped |= magnitude >= 1.0;
    }
    let rms = [0, 1].map(|side| {
        if counts[side] == 0 {
            0.0
        } else {
            (squares[side] / counts[side] as f32).sqrt()
        }
    });
    if channels == 1 {
        peak[1] = peak[0];
        return BlockLevels {
            peak,
            rms: [rms[0], rms[0]],
            clipped,
        };
    }
    BlockLevels { peak, rms, clipped }
}

/// Cross-thread clock and play/stop commands. Inactive until a system attaches.
#[derive(Clone)]
pub struct AudioHandle {
    inner: Arc<Shared>,
}

impl AudioHandle {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Shared {
                playing: AtomicBool::new(false),
                tick: AtomicI64::new(INACTIVE),
                epoch: Instant::now(),
                device: DeviceClock::default(),
                timing: Mutex::new(None),
                snapshot: Mutex::new(None),
                generation: std::sync::atomic::AtomicU64::new(0),
                scrub: Mutex::new(None),
                scrub_generation: std::sync::atomic::AtomicU64::new(0),
                peak_l: AtomicU32::new(0),
                peak_r: AtomicU32::new(0),
                rms_l: AtomicU32::new(0),
                rms_r: AtomicU32::new(0),
                clipped: AtomicBool::new(false),
                lanes: Mutex::new(Arc::new(LaneMeters::default())),
            }),
        }
    }

    /// The peak and root mean square of the last block handed to the device,
    /// per channel, and whether anything has reached full scale since the flag
    /// was last cleared.
    ///
    /// Read rather than pushed: the meter is drawn on a repaint clock, and an
    /// audio callback that woke the UI thread would be a glitch source.
    pub fn levels(&self) -> BlockLevels {
        let load = |bits: &AtomicU32| f32::from_bits(bits.load(Ordering::Relaxed));
        BlockLevels {
            peak: [load(&self.inner.peak_l), load(&self.inner.peak_r)],
            rms: [load(&self.inner.rms_l), load(&self.inner.rms_r)],
            clipped: self.inner.clipped.load(Ordering::Relaxed),
        }
    }

    /// Forget the clip indication. The operator has seen it; a meter that could
    /// never be reset would report one old clip for the rest of the session.
    pub fn clear_clip(&self) {
        self.inner.clipped.store(false, Ordering::Relaxed);
    }

    /// Record a block as if the device had just been handed it.
    ///
    /// The stream's own callback is the only production caller path (through the
    /// shared state); this exists so the meter's behaviour is reachable without an
    /// audio device.
    pub fn publish_block(&self, data: &[f32], channels: usize) {
        self.inner.publish(data, channels);
    }

    /// Give the mix now starting a table with a slot per lane, and hand it back
    /// so the mix thread can write into it without ever taking the lock.
    ///
    /// Replaces whatever the last stream armed: the shape of the table is the
    /// shape of the mix, and a project whose routing changed restarts the
    /// stream.
    pub fn arm_lane_meters(&self, lanes: impl Iterator<Item = TrackId>) -> Arc<LaneMeters> {
        let armed = Arc::new(LaneMeters::for_lanes(lanes));
        *self.inner.lanes.lock() = Arc::clone(&armed);
        armed
    }

    /// The lane levels the mix is publishing. One refcount bump per repaint,
    /// taken on the UI thread; the mix thread never waits on it.
    pub fn lane_meters(&self) -> Arc<LaneMeters> {
        Arc::clone(&self.inner.lanes.lock())
    }

    /// The tick being heard now, while audio is driving the clock.
    ///
    /// The device's output latency is subtracted, and between blocks the
    /// position runs forward at the device rate, so two reads one vsync apart
    /// differ by one vsync rather than by a block or nothing.
    pub fn current_tick(&self) -> Option<i64> {
        let tick = self.inner.tick.load(Ordering::Relaxed);
        if tick == INACTIVE {
            return None;
        }
        let timing = *self.inner.timing.lock();
        match (timing, self.inner.device.read()) {
            (Some(timing), Some(anchor)) => Some(timing.tick_at(anchor, self.inner.nanos())),
            _ => Some(tick),
        }
    }

    pub fn publish_play(&self, snapshot: PlaybackSnapshot) {
        *self.inner.scrub.lock() = None;
        *self.inner.snapshot.lock() = Some(snapshot);
        self.inner.playing.store(true, Ordering::Release);
        self.inner.generation.fetch_add(1, Ordering::AcqRel);
    }

    pub fn publish_stop(&self) {
        self.inner.playing.store(false, Ordering::Release);
        *self.inner.scrub.lock() = None;
        self.inner.tick.store(INACTIVE, Ordering::Relaxed);
        *self.inner.timing.lock() = None;
        // Silence reads as silence: a stopped stream leaves no bars standing.
        // The clip flag survives, because it is a receipt for what was heard.
        self.inner.peak_l.store(0, Ordering::Relaxed);
        self.inner.peak_r.store(0, Ordering::Relaxed);
        self.inner.rms_l.store(0, Ordering::Relaxed);
        self.inner.rms_r.store(0, Ordering::Relaxed);
        // The lanes go with the stream: the next play arms the mix it is
        // actually mixing rather than inheriting the last project's lanes.
        *self.inner.lanes.lock() = Arc::new(LaneMeters::default());
        self.inner.generation.fetch_add(1, Ordering::AcqRel);
    }

    /// Queue a 40 ms scrub grain at a new pointer position.
    ///
    /// Replaces any pending grain that has not yet been taken by the output
    /// stream, so a burst of pointer moves never queues more than one grain
    /// behind.
    pub fn queue_scrub(&self, grain: ScrubGrain) {
        let mut slot = self.inner.scrub.lock();
        let keep_mixer = grain
            .mixer
            .or_else(|| slot.as_ref().and_then(|prev| prev.mixer.clone()));
        *slot = Some(ScrubGrain {
            mixer: keep_mixer,
            ..grain
        });
        drop(slot);
        self.inner.scrub_generation.fetch_add(1, Ordering::AcqRel);
    }

    /// How many scrub grains are waiting to be played (always `0` or `1`).
    pub fn pending_scrub_grains(&self) -> usize {
        usize::from(self.inner.scrub.lock().is_some())
    }

    /// Take the single pending scrub grain, if any.
    pub fn take_pending_scrub(&self) -> Option<ScrubGrain> {
        self.inner.scrub.lock().take()
    }

    fn take_if_playing(&self) -> Option<PlaybackSnapshot> {
        if self.inner.playing.load(Ordering::Acquire) {
            self.inner.snapshot.lock().clone()
        } else {
            None
        }
    }
}

impl Default for AudioHandle {
    fn default() -> Self {
        Self::new()
    }
}

/// UI-thread owner of the cpal stream. Pump from the shell observe path.
pub struct AudioSystem {
    handle: AudioHandle,
    stream: Option<cpal::Stream>,
    stop: Option<Arc<AtomicBool>>,
    feeder: Option<JoinHandle<()>>,
    scrub_slot: Option<Arc<Mutex<Option<(i64, i32, i32)>>>>,
    last_generation: u64,
    last_scrub_generation: u64,
}

impl AudioSystem {
    pub fn new(handle: AudioHandle) -> Self {
        Self {
            handle,
            stream: None,
            stop: None,
            feeder: None,
            scrub_slot: None,
            last_generation: 0,
            last_scrub_generation: 0,
        }
    }

    pub fn handle(&self) -> AudioHandle {
        self.handle.clone()
    }

    /// Start or stop the stream to match the latest handle command, or feed a
    /// pending scrub grain to the output stream while paused.
    pub fn pump(&mut self) {
        let generation = self.handle.inner.generation.load(Ordering::Acquire);
        if generation != self.last_generation {
            self.last_generation = generation;
            self.tear_down();
            if let Some(snapshot) = self.handle.take_if_playing()
                && let Err(err) = self.start(snapshot, None)
            {
                tracing::warn!("preview audio not started: {err}");
                self.handle.inner.tick.store(INACTIVE, Ordering::Relaxed);
            }
            return;
        }
        if self.handle.inner.playing.load(Ordering::Acquire) {
            return;
        }
        let scrub_gen = self.handle.inner.scrub_generation.load(Ordering::Acquire);
        if scrub_gen == self.last_scrub_generation {
            return;
        }
        self.last_scrub_generation = scrub_gen;
        let Some(grain) = self.handle.take_pending_scrub() else {
            return;
        };
        if grain.mixer.is_none()
            && let Some(slot) = &self.scrub_slot
        {
            *slot.lock() = Some((grain.tick, grain.fps_num, grain.fps_den));
            return;
        }
        let Some(mixer) = grain.mixer else {
            return;
        };
        self.tear_down();
        let slot = Arc::new(Mutex::new(Some((grain.tick, grain.fps_num, grain.fps_den))));
        let snapshot = PlaybackSnapshot {
            mixer,
            start_tick: grain.tick,
            fps_num: grain.fps_num,
            fps_den: grain.fps_den,
            speed_num: 0,
            speed_den: 100,
        };
        if let Err(err) = self.start(snapshot, Some(Arc::clone(&slot))) {
            tracing::warn!("scrub audio not started: {err}");
        } else {
            self.scrub_slot = Some(slot);
        }
    }

    fn start(
        &mut self,
        snapshot: PlaybackSnapshot,
        scrub_slot: Option<Arc<Mutex<Option<(i64, i32, i32)>>>>,
    ) -> Result<(), String> {
        let prepared = snapshot
            .mixer
            .lock()
            .take()
            .ok_or_else(|| "the prepared audio mix was already consumed".to_string())?;
        let mixer = match prepared? {
            Some(mixer) => mixer,
            // Nothing audible: playing silence needs no device.
            None => return Ok(()),
        };
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| "no audio output device. Plug in speakers or a headset.".to_string())?;
        let config = device
            .default_output_config()
            .map_err(|e| format!("couldn't query the audio device: {e}. Check sound settings."))?;
        let sample_format = config.sample_format();
        let mut stream_config = config.config();
        stream_config.channels = stream_config.channels.max(1);

        let (tx, rx) = bounded::<Vec<f32>>(8);
        let (recycle_tx, recycle_rx) = bounded::<Vec<f32>>(MIX_BLOCK_POOL);
        let stop = Arc::new(AtomicBool::new(false));
        let start_tick = snapshot.start_tick;
        let fps_num = snapshot.fps_num.max(1);
        let fps_den = snapshot.fps_den.max(1);
        let speed_num = snapshot.speed_num;
        let speed_den = snapshot.speed_den.max(1);
        let is_scrub = scrub_slot.is_some();
        let clock = self.handle.inner.clone();
        if !is_scrub {
            clock.tick.store(start_tick, Ordering::Relaxed);
        }

        // The mixer is built, so the feeder needs one number rather than the
        // snapshot: holding it would keep the whole project alive for as long as
        // playback ran, and the snapshot is dropped here instead.
        let start_sample = ticks_to_samples(start_tick, fps_num, fps_den, EXPORT_AUDIO_RATE);
        let feeder_stop = stop.clone();
        // Armed from the mixer's own lane list, so the table the mix thread
        // writes into and the lanes it folds are the same list.
        let meters = self
            .handle
            .arm_lane_meters(mixer.lane_peaks().map(|(track, _)| track));
        self.feeder = Some(
            thread::Builder::new()
                .name("aria-audio-mix".into())
                .spawn(move || {
                    feed_mixer(
                        mixer,
                        start_sample,
                        speed_num,
                        speed_den,
                        scrub_slot,
                        tx,
                        recycle_rx,
                        feeder_stop,
                        meters,
                    )
                })
                .map_err(|e| format!("couldn't start the audio mixer thread: {e}."))?,
        );

        let peaks = self.handle.inner.clone();
        let channels = stream_config.channels as usize;
        let timing = StreamTiming {
            start_tick,
            rate: stream_config.sample_rate.0,
            fps_num,
            fps_den,
            speed_num,
            speed_den,
        };
        if !is_scrub {
            // The clock holds at the start until the device takes its first block.
            *clock.timing.lock() = Some(timing);
            clock.device.write(DeviceAnchor {
                heard: 0,
                handed: 0,
                at: clock.nanos(),
            });
        }
        let mut pending = Vec::new();
        let mut pending_at = 0usize;
        let mut mixed_frames: i64 = 0;
        let recycle_f32 = recycle_tx.clone();
        let recycle_i16 = recycle_tx;

        let mut i16_scratch = Vec::new();
        let err_fn = |err| tracing::warn!("audio output: {err}");
        let stream = match sample_format {
            cpal::SampleFormat::F32 => device.build_output_stream(
                &stream_config,
                move |data: &mut [f32], info: &cpal::OutputCallbackInfo| {
                    fill_output(
                        data,
                        channels,
                        &rx,
                        &mut pending,
                        &mut pending_at,
                        &recycle_f32,
                    );
                    peaks.publish(data, channels);
                    if !is_scrub {
                        let before = mixed_frames;
                        mixed_frames += (data.len() / channels.max(1)) as i64;
                        clock.handed(timing, before, mixed_frames, output_latency(info));
                    }
                },
                err_fn,
                None,
            ),
            cpal::SampleFormat::I16 => device.build_output_stream(
                &stream_config,
                move |data: &mut [i16], info: &cpal::OutputCallbackInfo| {
                    if i16_scratch.len() != data.len() {
                        i16_scratch.resize(data.len(), 0.0);
                    } else {
                        i16_scratch.fill(0.0);
                    }
                    fill_output(
                        &mut i16_scratch,
                        channels,
                        &rx,
                        &mut pending,
                        &mut pending_at,
                        &recycle_i16,
                    );
                    // Measured before the i16 clamp, so a mix over full scale is
                    // reported as the clip it is rather than as an exact 1.0.
                    peaks.publish(&i16_scratch, channels);
                    for (dst, src) in data.iter_mut().zip(&i16_scratch) {
                        *dst = (src.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
                    }
                    if !is_scrub {
                        let before = mixed_frames;
                        mixed_frames += (data.len() / channels.max(1)) as i64;
                        clock.handed(timing, before, mixed_frames, output_latency(info));
                    }
                },
                err_fn,
                None,
            ),
            other => {
                return Err(format!(
                    "audio device sample format {other} is not supported. Switch the output device."
                ));
            }
        }
        .map_err(|e| format!("couldn't open the audio device: {e}. Check sound settings."))?;
        stream
            .play()
            .map_err(|e| format!("couldn't start audio output: {e}. Check sound settings."))?;
        self.stream = Some(stream);
        self.stop = Some(stop);
        Ok(())
    }

    fn tear_down(&mut self) {
        self.scrub_slot = None;
        if let Some(stop) = self.stop.take() {
            stop.store(true, Ordering::Release);
        }
        self.stream = None;
        if let Some(feeder) = self.feeder.take() {
            let _ = feeder.join();
        }
        self.handle.inner.tick.store(INACTIVE, Ordering::Relaxed);
        *self.handle.inner.timing.lock() = None;
    }
}

impl Drop for AudioSystem {
    fn drop(&mut self) {
        self.tear_down();
    }
}

/// Render one 40 ms stereo scrub grain starting at `start_sample`, with a short
/// cosine-free linear edge fade so adjacent grains do not click.
pub fn render_scrub_grain(
    mixer: &mut ExportAudioMixer,
    start_sample: i64,
    sample_rate: u32,
    out: &mut Vec<f32>,
) -> Result<(), String> {
    let frames = scrub_grain_frames(sample_rate);
    let ch = usize::from(EXPORT_AUDIO_CHANNELS);
    let samples = frames * ch;
    out.clear();
    out.resize(samples, 0.0);
    if frames == 0 {
        return Ok(());
    }
    mixer
        .mix_into(start_sample, out)
        .map_err(|err| err.to_string())?;
    let fade = SCRUB_FADE_FRAMES.min(frames / 2);
    for i in 0..fade {
        let gain = (i as f32 + 0.5) / fade as f32;
        let head = i * ch;
        let tail = (frames - 1 - i) * ch;
        for c in 0..ch {
            out[head + c] *= gain;
            out[tail + c] *= gain;
        }
    }
    Ok(())
}

/// Mix blocks from `start_sample` until the stream is torn down, or render
/// 40 ms scrub grains when `scrub_slot` is present.
#[allow(clippy::too_many_arguments)]
fn feed_mixer(
    mut mixer: ExportAudioMixer,
    start_sample: i64,
    speed_num: i32,
    speed_den: i32,
    scrub_slot: Option<Arc<Mutex<Option<(i64, i32, i32)>>>>,
    tx: Sender<Vec<f32>>,
    recycle: Receiver<Vec<f32>>,
    stop: Arc<AtomicBool>,
    meters: Arc<LaneMeters>,
) {
    // Playback: an insert that overruns the real time of its blocks three
    // times in a row is bypassed rather than starving the device.
    mixer.set_realtime(true);
    if let Some(slot) = scrub_slot {
        let mut grain = Vec::new();
        while !stop.load(Ordering::Acquire) {
            let _ = recycle.try_recv();
            let Some((tick, fps_num, fps_den)) = slot.lock().take() else {
                thread::sleep(Duration::from_millis(4));
                continue;
            };
            let sample = ticks_to_samples(tick, fps_num.max(1), fps_den.max(1), EXPORT_AUDIO_RATE);
            if render_scrub_grain(&mut mixer, sample, EXPORT_AUDIO_RATE, &mut grain).is_err() {
                break;
            }
            meters.publish(mixer.lane_peaks());
            if tx.send(grain.clone()).is_err() {
                break;
            }
        }
        return;
    }
    let mut pool: Vec<Vec<f32>> = (0..MIX_BLOCK_POOL)
        .map(|_| vec![0.0; MIX_FRAMES * usize::from(EXPORT_AUDIO_CHANNELS)])
        .collect();
    let mut stream_frame: i64 = 0;
    while !stop.load(Ordering::Acquire) {
        let Some(mut block) = pool.pop().or_else(|| recycle.recv().ok()) else {
            break;
        };
        block.fill(0.0);
        if mixer
            .mix_shuttle_into(start_sample, stream_frame, speed_num, speed_den, &mut block)
            .is_err()
        {
            break;
        }
        // Published here rather than from the device callback: these are the
        // levels the mix produced, one store per lane into a table this thread
        // already holds, so the callback stays free of the meter entirely.
        meters.publish(mixer.lane_peaks());
        stream_frame += MIX_FRAMES as i64;
        if tx.send(block).is_err() {
            break;
        }
    }
}

/// How long after this callback the device plays the block it is handed.
/// Zero when the host reports no timestamps.
fn output_latency(info: &cpal::OutputCallbackInfo) -> Duration {
    let stamp = info.timestamp();
    stamp
        .playback
        .duration_since(&stamp.callback)
        .unwrap_or_default()
}

fn fill_output(
    data: &mut [f32],
    out_channels: usize,
    rx: &Receiver<Vec<f32>>,
    pending: &mut Vec<f32>,
    pending_at: &mut usize,
    recycle: &Sender<Vec<f32>>,
) {
    let src_ch = usize::from(EXPORT_AUDIO_CHANNELS);
    let mut written = 0;
    while written < data.len() {
        if *pending_at >= pending.len() {
            if !pending.is_empty() {
                let returned = std::mem::take(pending);
                let _ = recycle.try_send(returned);
            }
            match rx.try_recv() {
                Ok(next) => {
                    *pending = next;
                    *pending_at = 0;
                }
                Err(_) => {
                    data[written..].fill(0.0);
                    return;
                }
            }
        }
        while *pending_at + src_ch <= pending.len() && written + out_channels <= data.len() {
            let l = pending[*pending_at];
            let r = pending[*pending_at + 1.min(src_ch.saturating_sub(1))];
            data[written] = l;
            if out_channels > 1 {
                data[written + 1] = r;
                for extra in data[written + 2..written + out_channels].iter_mut() {
                    *extra = 0.0;
                }
            }
            *pending_at += src_ch;
            written += out_channels;
        }
        if *pending_at + src_ch > pending.len() {
            *pending_at = pending.len();
        }
    }
}

pub(crate) fn ticks_to_samples(ticks: i64, fps_num: i32, fps_den: i32, rate: u32) -> i64 {
    if fps_num <= 0 || fps_den <= 0 {
        return 0;
    }
    ticks
        .saturating_mul(i64::from(rate))
        .saturating_mul(i64::from(fps_den))
        / i64::from(fps_num)
}

pub(crate) fn tick_from_samples(
    start_tick: i64,
    samples: i64,
    sample_rate: u32,
    fps_num: i32,
    fps_den: i32,
    speed_num: i32,
    speed_den: i32,
) -> i64 {
    if sample_rate == 0 || fps_num <= 0 || fps_den <= 0 || speed_den <= 0 || speed_num == 0 {
        return start_tick;
    }
    let num = i128::from(samples) * i128::from(fps_num) * i128::from(speed_num);
    let den = i128::from(sample_rate) * i128::from(fps_den) * i128::from(speed_den);
    let delta = i64::try_from(num / den).unwrap_or(if num < 0 { i64::MIN } else { i64::MAX });
    start_tick.saturating_add(delta)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inactive_handle_has_no_tick() {
        let handle = AudioHandle::new();
        assert_eq!(handle.current_tick(), None);
    }

    #[test]
    fn stop_clears_the_clock() {
        let handle = AudioHandle::new();
        handle.inner.tick.store(24, Ordering::Relaxed);
        *handle.inner.timing.lock() = Some(TIMING);
        handle.publish_stop();
        assert_eq!(handle.current_tick(), None);
        assert_eq!(*handle.inner.timing.lock(), None);
        assert!(!handle.inner.playing.load(Ordering::Relaxed));
    }

    const TIMING: StreamTiming = StreamTiming {
        start_tick: 100,
        rate: 48_000,
        fps_num: 24,
        fps_den: 1,
        speed_num: 100,
        speed_den: 100,
    };

    /// The clock reports what is heard, not what was handed: a block handed
    /// with half a second of output latency is heard half a second later.
    #[test]
    fn a_block_is_heard_one_latency_after_it_is_handed() {
        let handle = AudioHandle::new();
        assert_eq!(handle.inner.device.read(), None, "no block handed yet");
        *handle.inner.timing.lock() = Some(TIMING);
        handle
            .inner
            .handed(TIMING, 48_000, 49_024, Duration::from_millis(500));
        let anchor = handle.inner.device.read().expect("a block was handed");
        assert_eq!(anchor.heard, 24_000);
        assert_eq!(anchor.handed, 49_024);
        assert_eq!(handle.inner.tick.load(Ordering::Relaxed), 112);
        let tick = handle.current_tick().expect("audio drives the clock");
        assert!(
            (112..=124).contains(&tick),
            "the clock read {tick}, outside what was heard and what was handed"
        );
    }

    /// Between blocks the clock runs at the device rate, so two vsyncs inside
    /// one block read two positions. It never passes what the device was
    /// handed, and it holds at the start until the first sample is heard.
    #[test]
    fn the_clock_runs_between_blocks_and_holds_at_its_bounds() {
        let anchor = DeviceAnchor {
            heard: 24_000,
            handed: 48_000,
            at: 1_000_000_000,
        };
        assert_eq!(TIMING.tick_at(anchor, 1_000_000_000), 112);
        assert_eq!(TIMING.tick_at(anchor, 1_250_000_000), 118);
        assert_eq!(
            TIMING.tick_at(anchor, 9_000_000_000),
            124,
            "a stalled device"
        );
        assert_eq!(
            TIMING.tick_at(anchor, 0),
            112,
            "a read stamped before the block"
        );
        let early = DeviceAnchor {
            heard: -9_600,
            handed: 1_024,
            at: 0,
        };
        assert_eq!(TIMING.tick_at(early, 100_000_000), 100);
    }

    #[test]
    fn one_second_of_48k_is_24_ticks_at_24fps() {
        assert_eq!(tick_from_samples(0, 48_000, 48_000, 24, 1, 100, 100), 24);
        assert_eq!(ticks_to_samples(24, 24, 1, 48_000), 48_000);
    }

    #[test]
    fn audio_clock_maps_device_frames_to_ticks_scaled_by_speed_and_direction() {
        // After N device frames at speed s = speed_num / speed_den:
        // tick = start + s * N * fps / sample_rate.
        let start = 200;
        let rate = 48_000;
        let n = 48_000; // 1.0 s of device frames
        // +1x -> +24 ticks
        assert_eq!(tick_from_samples(start, n, rate, 24, 1, 100, 100), 224);
        // +2x -> +48 ticks
        assert_eq!(tick_from_samples(start, n, rate, 24, 1, 200, 100), 248);
        // +0.25x (1/4x) -> +6 ticks
        assert_eq!(tick_from_samples(start, n, rate, 24, 1, 25, 100), 206);
        // +1.1x -> +26 ticks (floor(26.4))
        assert_eq!(tick_from_samples(start, n, rate, 24, 1, 110, 100), 226);
        // -1x -> -24 ticks
        assert_eq!(tick_from_samples(start, n, rate, 24, 1, -100, 100), 176);
        // -4x -> -96 ticks
        assert_eq!(tick_from_samples(start, n, rate, 24, 1, -400, 100), 104);
    }

    #[test]
    fn scrub_burst_of_50_pointer_moves_queues_at_most_one_pending_grain_of_40ms() {
        let handle = AudioHandle::new();
        for tick in 1..=50 {
            handle.queue_scrub(ScrubGrain {
                mixer: None,
                tick,
                fps_num: 24,
                fps_den: 1,
            });
            assert!(
                handle.pending_scrub_grains() <= 1,
                "move {tick} queued more than one pending grain"
            );
        }
        assert_eq!(handle.pending_scrub_grains(), 1);
        let pending = handle.take_pending_scrub().expect("latest grain queued");
        assert_eq!(pending.tick, 50);
        assert_eq!(handle.pending_scrub_grains(), 0);
        assert_eq!(scrub_grain_frames(48_000), 1_920);
        assert_eq!(scrub_grain_frames(44_100), 1_764);
    }

    #[test]
    fn invalid_rates_keep_the_anchor() {
        assert_eq!(tick_from_samples(7, 48_000, 0, 24, 1, 100, 100), 7);
        assert_eq!(ticks_to_samples(24, 0, 1, 48_000), 0);
    }

    #[test]
    fn consumed_mix_blocks_return_to_the_feeder_pool() {
        let (tx, rx) = bounded(1);
        let (recycle_tx, recycle_rx) = bounded(1);
        let block = vec![0.25, -0.25, 0.5, -0.5];
        let allocation = block.as_ptr();
        tx.send(block).expect("queue block");
        let mut pending = Vec::new();
        let mut pending_at = 0;
        let mut output = [0.0; 4];

        fill_output(
            &mut output,
            2,
            &rx,
            &mut pending,
            &mut pending_at,
            &recycle_tx,
        );
        assert_eq!(output, [0.25, -0.25, 0.5, -0.5]);

        fill_output(
            &mut output,
            2,
            &rx,
            &mut pending,
            &mut pending_at,
            &recycle_tx,
        );
        let returned = recycle_rx.try_recv().expect("block returned");
        assert_eq!(
            returned.as_ptr(),
            allocation,
            "the callback allocated a replacement block"
        );
    }

    #[test]
    fn fill_output_empty_receiver_underflow_zero_fills() {
        let (_tx, rx) = bounded(1);
        let (recycle_tx, _recycle_rx) = bounded(1);
        let mut pending = Vec::new();
        let mut pending_at = 0;
        let mut output = [1.0f32; 8];

        fill_output(
            &mut output,
            2,
            &rx,
            &mut pending,
            &mut pending_at,
            &recycle_tx,
        );
        assert_eq!(output, [0.0; 8]);
    }

    #[test]
    fn fill_output_mono_and_multichannel_alignment() {
        let (tx, rx) = bounded(1);
        let (recycle_tx, _recycle_rx) = bounded(1);
        let block = vec![0.3, -0.4, 0.7, -0.8]; // 2 stereo frames: (0.3, -0.4) and (0.7, -0.8)
        tx.send(block).expect("send block");
        let mut pending = Vec::new();
        let mut pending_at = 0;

        // Mono output (1 channel): reads left channel of each frame
        let mut mono_out = [0.0f32; 2];
        fill_output(
            &mut mono_out,
            1,
            &rx,
            &mut pending,
            &mut pending_at,
            &recycle_tx,
        );
        assert_eq!(mono_out, [0.3, 0.7]);

        // 6-channel output (5.1): maps L/R to first two channels, zeros remaining
        let (tx6, rx6) = bounded(1);
        tx6.send(vec![0.5, -0.5]).expect("send stereo frame");
        let mut pending6 = Vec::new();
        let mut pending_at6 = 0;
        let mut out6 = [9.0f32; 6];
        fill_output(
            &mut out6,
            6,
            &rx6,
            &mut pending6,
            &mut pending_at6,
            &recycle_tx,
        );
        assert_eq!(out6, [0.5, -0.5, 0.0, 0.0, 0.0, 0.0]);
    }
}

/// The mixer surface: strips, routing gestures and fader drags. Declared here
/// because the mixer is what this module's mixdown is a view of.
#[cfg(test)]
#[path = "audio_submix_tests.rs"]
mod submix;
