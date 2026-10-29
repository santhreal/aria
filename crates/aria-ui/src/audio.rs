//! Preview audio: cpal output driven by [`ExportAudioMixer`].
//!
//! [`AudioSystem`] owns the cpal stream and is `!Send` (stays on the UI
//! thread). [`AudioHandle`] is `Send + Clone` and lives on [`crate::app::AppState`].
//! Tests that never attach a system keep the wall-clock playhead.

mod clock;
mod feed;
mod meters;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use aria_models::TrackId;
use aria_render::{ExportAudioMixer, sample_boundary};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::bounded;
use parking_lot::Mutex;

use clock::{DeviceAnchor, DeviceClock, StreamTiming, tick_from_samples};
pub use feed::render_scrub_grain;
use feed::{MIX_BLOCK_POOL, feed_mixer, fill_output, output_latency};
pub use meters::{BlockLevels, LaneMeters, block_levels};

const INACTIVE: i64 = i64::MIN;
/// Duration of one audio scrub grain in milliseconds.
pub const SCRUB_GRAIN_MS: u32 = 40;

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

/// Cross-thread clock and play/stop commands. Inactive until a system attaches.
#[derive(Clone)]
pub struct AudioHandle {
    inner: Arc<Shared>,
}

impl std::fmt::Debug for AudioHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AudioHandle")
            .field("playing", &self.inner.playing.load(Ordering::Relaxed))
            .field("tick", &self.inner.tick.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
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

    /// Forget the clip indication once it has been seen; a meter that could
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

impl std::fmt::Debug for AudioSystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AudioSystem")
            .field("handle", &self.handle)
            .field("active", &self.stream.is_some())
            .field("last_generation", &self.last_generation)
            .finish_non_exhaustive()
    }
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
    ///
    /// Returns why a stream this call tried to start did not start, such as a
    /// machine with no output device, so the caller can post it: the picture
    /// plays on regardless, and without the line a missing sound reads as a
    /// silent clip.
    #[must_use]
    pub fn pump(&mut self) -> Option<String> {
        let generation = self.handle.inner.generation.load(Ordering::Acquire);
        if generation != self.last_generation {
            self.last_generation = generation;
            self.tear_down();
            let snapshot = self.handle.take_if_playing()?;
            let err = self.start(snapshot, None).err()?;
            tracing::warn!("preview audio not started: {err}");
            self.handle.inner.tick.store(INACTIVE, Ordering::Relaxed);
            return Some(err);
        }
        if self.handle.inner.playing.load(Ordering::Acquire) {
            return None;
        }
        let scrub_gen = self.handle.inner.scrub_generation.load(Ordering::Acquire);
        if scrub_gen == self.last_scrub_generation {
            return None;
        }
        self.last_scrub_generation = scrub_gen;
        let grain = self.handle.take_pending_scrub()?;
        if grain.mixer.is_none()
            && let Some(slot) = &self.scrub_slot
        {
            *slot.lock() = Some((grain.tick, grain.fps_num, grain.fps_den));
            return None;
        }
        let mixer = grain.mixer?;
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
            return Some(err);
        }
        self.scrub_slot = Some(slot);
        None
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
        // Nothing audible: playing silence needs no device.
        let Some(mixer) = prepared? else {
            return Ok(());
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
        let start_sample = sample_boundary(start_tick, fps_num, fps_den);
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
        // A system that never started a stream drives no clock, so dropping
        // it leaves the clock another system on the handle may be driving.
        if self.stream.is_some() || self.feeder.is_some() {
            self.tear_down();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::clock::TIMING;
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

    /// A system that never started a stream leaves the clock alone when it
    /// is dropped, so a window that closes while another one plays does not
    /// stop the clock the playing one drives.
    #[test]
    fn dropping_a_system_that_drives_nothing_leaves_the_clock() {
        let handle = AudioHandle::new();
        handle.inner.tick.store(24, Ordering::Relaxed);
        *handle.inner.timing.lock() = Some(TIMING);
        drop(AudioSystem::new(handle.clone()));
        assert_eq!(handle.inner.tick.load(Ordering::Relaxed), 24);
        assert_eq!(*handle.inner.timing.lock(), Some(TIMING));
    }

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
}

/// The mixer surface: strips, routing gestures and fader drags. Declared here
/// because the mixer is what this module's mixdown is a view of.
#[cfg(test)]
#[path = "audio_submix_tests.rs"]
mod submix;

#[cfg(test)]
#[path = "audio_start_tests.rs"]
mod start;
