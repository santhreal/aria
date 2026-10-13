//! Preview audio: cpal output driven by [`ExportAudioMixer`].
//!
//! [`AudioSystem`] owns the cpal stream and is `!Send` (stays on the UI
//! thread). [`AudioHandle`] is `Send + Clone` and lives on [`crate::app::AppState`].
//! Tests that never attach a system keep the wall-clock playhead.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, Ordering};
use std::thread::{self, JoinHandle};

use aria_models::TrackId;
use aria_render::{EXPORT_AUDIO_CHANNELS, EXPORT_AUDIO_RATE, ExportAudioMixer};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::{Receiver, Sender, bounded};
use parking_lot::Mutex;

const MIX_FRAMES: usize = 1024;
const MIX_BLOCK_POOL: usize = 9;
const INACTIVE: i64 = i64::MIN;

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

struct Shared {
    playing: AtomicBool,
    tick: AtomicI64,
    snapshot: Mutex<Option<PlaybackSnapshot>>,
    generation: std::sync::atomic::AtomicU64,
    /// Peak magnitude of the last block the device was handed, per channel, as
    /// `f32` bits. Written from the audio callback, so it must stay lock-free.
    peak_l: AtomicU32,
    peak_r: AtomicU32,
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
    /// Record what this block will sound like: the loudest sample per channel,
    /// and whether anything reached full scale.
    fn publish(&self, data: &[f32], channels: usize) {
        let ([l, r], clipped) = block_peaks(data, channels);
        self.peak_l.store(l.to_bits(), Ordering::Relaxed);
        self.peak_r.store(r.to_bits(), Ordering::Relaxed);
        if clipped {
            self.clipped.store(true, Ordering::Relaxed);
        }
    }
}

/// The loudest magnitude in an interleaved block, per meter channel, and whether
/// any sample reached full scale.
///
/// Two bars whatever the device offers: a mono device doubles its one channel and
/// a surround device folds everything past the second into the nearer bar, so the
/// meter reads the same everywhere rather than going blank on unusual hardware.
pub fn block_peaks(data: &[f32], channels: usize) -> ([f32; 2], bool) {
    if channels == 0 || data.is_empty() {
        return ([0.0, 0.0], false);
    }
    let mut peak = [0.0f32; 2];
    let mut clipped = false;
    for (index, sample) in data.iter().enumerate() {
        let magnitude = sample.abs();
        // NaN from a broken decoder must not poison the meter or read as a clip.
        if !magnitude.is_finite() {
            continue;
        }
        let side = usize::from(index % channels >= (channels + 1) / 2 && channels > 1);
        if magnitude > peak[side] {
            peak[side] = magnitude;
        }
        clipped |= magnitude >= 1.0;
    }
    if channels == 1 {
        peak[1] = peak[0];
    }
    (peak, clipped)
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
                snapshot: Mutex::new(None),
                generation: std::sync::atomic::AtomicU64::new(0),
                peak_l: AtomicU32::new(0),
                peak_r: AtomicU32::new(0),
                clipped: AtomicBool::new(false),
                lanes: Mutex::new(Arc::new(LaneMeters::default())),
            }),
        }
    }

    /// The peak of the last block handed to the device, per channel, and whether
    /// anything has reached full scale since the flag was last cleared.
    ///
    /// Read rather than pushed: the meter is drawn on a repaint clock, and an
    /// audio callback that woke the UI thread would be a glitch source.
    pub fn peaks(&self) -> ([f32; 2], bool) {
        (
            [
                f32::from_bits(self.inner.peak_l.load(Ordering::Relaxed)),
                f32::from_bits(self.inner.peak_r.load(Ordering::Relaxed)),
            ],
            self.inner.clipped.load(Ordering::Relaxed),
        )
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

    /// Playhead in timeline ticks while audio is driving the clock.
    pub fn current_tick(&self) -> Option<i64> {
        let tick = self.inner.tick.load(Ordering::Relaxed);
        if tick == INACTIVE { None } else { Some(tick) }
    }

    pub fn publish_play(&self, snapshot: PlaybackSnapshot) {
        *self.inner.snapshot.lock() = Some(snapshot);
        self.inner.playing.store(true, Ordering::Release);
        self.inner.generation.fetch_add(1, Ordering::AcqRel);
    }

    pub fn publish_stop(&self) {
        self.inner.playing.store(false, Ordering::Release);
        self.inner.tick.store(INACTIVE, Ordering::Relaxed);
        // Silence reads as silence: a stopped stream leaves no bars standing.
        // The clip flag survives, because it is a receipt for what was heard.
        self.inner.peak_l.store(0, Ordering::Relaxed);
        self.inner.peak_r.store(0, Ordering::Relaxed);
        // The lanes go with the stream: the next play arms the mix it is
        // actually mixing rather than inheriting the last project's lanes.
        *self.inner.lanes.lock() = Arc::new(LaneMeters::default());
        self.inner.generation.fetch_add(1, Ordering::AcqRel);
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
    last_generation: u64,
}

impl AudioSystem {
    pub fn new(handle: AudioHandle) -> Self {
        Self {
            handle,
            stream: None,
            stop: None,
            feeder: None,
            last_generation: 0,
        }
    }

    pub fn handle(&self) -> AudioHandle {
        self.handle.clone()
    }

    /// Start or stop the stream to match the latest handle command.
    pub fn pump(&mut self) {
        let generation = self.handle.inner.generation.load(Ordering::Acquire);
        if generation == self.last_generation {
            return;
        }
        self.last_generation = generation;
        self.tear_down();
        let Some(snapshot) = self.handle.take_if_playing() else {
            return;
        };
        if let Err(err) = self.start(snapshot) {
            tracing::warn!("preview audio not started: {err}");
            self.handle.inner.tick.store(INACTIVE, Ordering::Relaxed);
        }
    }

    fn start(&mut self, snapshot: PlaybackSnapshot) -> Result<(), String> {
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
        let clock = self.handle.inner.clone();
        clock.tick.store(start_tick, Ordering::Relaxed);

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
                .spawn(move || feed_mixer(mixer, start_sample, tx, recycle_rx, feeder_stop, meters))
                .map_err(|e| format!("couldn't start the audio mixer thread: {e}."))?,
        );

        let peaks = self.handle.inner.clone();
        let channels = stream_config.channels as usize;
        let out_rate = stream_config.sample_rate.0;
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
                move |data: &mut [f32], _| {
                    fill_output(
                        data,
                        channels,
                        &rx,
                        &mut pending,
                        &mut pending_at,
                        &recycle_f32,
                    );
                    peaks.publish(data, channels);
                    mixed_frames += (data.len() / channels.max(1)) as i64;
                    clock.tick.store(
                        tick_from_samples(start_tick, mixed_frames, out_rate, fps_num, fps_den),
                        Ordering::Relaxed,
                    );
                },
                err_fn,
                None,
            ),
            cpal::SampleFormat::I16 => device.build_output_stream(
                &stream_config,
                move |data: &mut [i16], _| {
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
                    mixed_frames += (data.len() / channels.max(1)) as i64;
                    clock.tick.store(
                        tick_from_samples(start_tick, mixed_frames, out_rate, fps_num, fps_den),
                        Ordering::Relaxed,
                    );
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
        if let Some(stop) = self.stop.take() {
            stop.store(true, Ordering::Release);
        }
        self.stream = None;
        if let Some(feeder) = self.feeder.take() {
            let _ = feeder.join();
        }
        self.handle.inner.tick.store(INACTIVE, Ordering::Relaxed);
    }
}

impl Drop for AudioSystem {
    fn drop(&mut self) {
        self.tear_down();
    }
}

/// Mix blocks from `start_sample` until the stream is torn down.
///
/// Takes the position rather than the snapshot it came from: the mixer already
/// holds everything about the project that mixing needs, and a snapshot here
/// would keep a whole project alive for the length of the playback.
fn feed_mixer(
    mut mixer: ExportAudioMixer,
    start_sample: i64,
    tx: Sender<Vec<f32>>,
    recycle: Receiver<Vec<f32>>,
    stop: Arc<AtomicBool>,
    meters: Arc<LaneMeters>,
) {
    // Playback: an insert that overruns the real time of its blocks three
    // times in a row is bypassed rather than starving the device.
    mixer.set_realtime(true);
    let mut pool: Vec<Vec<f32>> = (0..MIX_BLOCK_POOL)
        .map(|_| vec![0.0; MIX_FRAMES * usize::from(EXPORT_AUDIO_CHANNELS)])
        .collect();
    let mut pos = start_sample;
    while !stop.load(Ordering::Acquire) {
        let Some(mut block) = pool.pop().or_else(|| recycle.recv().ok()) else {
            break;
        };
        block.fill(0.0);
        if mixer.mix_into(pos, &mut block).is_err() {
            break;
        }
        // Published here rather than from the device callback: these are the
        // levels the mix produced, one store per lane into a table this thread
        // already holds, so the callback stays free of the meter entirely.
        meters.publish(mixer.lane_peaks());
        pos += MIX_FRAMES as i64;
        if tx.send(block).is_err() {
            break;
        }
    }
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
) -> i64 {
    if sample_rate == 0 || fps_num <= 0 || fps_den <= 0 {
        return start_tick;
    }
    start_tick
        + samples.saturating_mul(i64::from(fps_num))
            / (i64::from(sample_rate).saturating_mul(i64::from(fps_den)))
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
        handle.publish_stop();
        assert_eq!(handle.current_tick(), None);
        assert!(!handle.inner.playing.load(Ordering::Relaxed));
    }

    #[test]
    fn one_second_of_48k_is_24_ticks_at_24fps() {
        assert_eq!(tick_from_samples(0, 48_000, 48_000, 24, 1), 24);
        assert_eq!(ticks_to_samples(24, 24, 1, 48_000), 48_000);
    }

    #[test]
    fn invalid_rates_keep_the_anchor() {
        assert_eq!(tick_from_samples(7, 48_000, 0, 24, 1), 7);
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
