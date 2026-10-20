//! The mix thread and the device callback: blocks mixed ahead into a
//! recycled pool or 40 ms scrub grains, and the copy of each block into the
//! device's channel layout.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use aria_render::{EXPORT_AUDIO_CHANNELS, EXPORT_AUDIO_RATE, ExportAudioMixer, sample_boundary};
use crossbeam_channel::{Receiver, Sender};
use parking_lot::Mutex;

use super::meters::LaneMeters;
use super::scrub_grain_frames;

const MIX_FRAMES: usize = 1024;
/// Block buffers the mix thread allocates once and the device callback recycles.
pub(super) const MIX_BLOCK_POOL: usize = 9;
/// Envelope fade length at each edge of a scrub grain, in stereo frames.
const SCRUB_FADE_FRAMES: usize = 32;

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
pub(super) fn feed_mixer(
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
            let sample = sample_boundary(tick, fps_num.max(1), fps_den.max(1));
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
pub(super) fn output_latency(info: &cpal::OutputCallbackInfo) -> Duration {
    let stamp = info.timestamp();
    stamp
        .playback
        .duration_since(&stamp.callback)
        .unwrap_or_default()
}

pub(super) fn fill_output(
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

#[cfg(test)]
mod tests {
    use crossbeam_channel::bounded;

    use super::*;

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
