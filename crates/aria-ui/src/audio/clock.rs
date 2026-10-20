//! The preview audio clock: where the device is in the stream that is
//! playing, and the conversion from device frames to sequence ticks.

use std::sync::atomic::{AtomicI64, AtomicU64, Ordering, fence};
use std::time::Duration;

/// Where the device is in the stream that is playing: written by the audio
/// callback once per block, read by the frame clock at any time.
///
/// A sequence count around the three values keeps a read from mixing two
/// blocks' numbers. The callback is the only writer, and neither side takes a
/// lock.
#[derive(Default)]
pub(super) struct DeviceClock {
    seq: AtomicU64,
    heard: AtomicI64,
    handed: AtomicI64,
    at: AtomicU64,
}

/// One block's reading of the device clock, in output frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct DeviceAnchor {
    /// Frames heard at `at`: the frames handed before this block, less the
    /// device's output latency. Negative until the first sample is heard.
    pub(super) heard: i64,
    /// Frames handed to the device, which the clock never passes.
    pub(super) handed: i64,
    /// When the block was handed over, in nanoseconds since the handle's epoch.
    pub(super) at: u64,
}

impl DeviceClock {
    pub(super) fn write(&self, anchor: DeviceAnchor) {
        self.seq.fetch_add(1, Ordering::Relaxed);
        fence(Ordering::Release);
        self.heard.store(anchor.heard, Ordering::Relaxed);
        self.handed.store(anchor.handed, Ordering::Relaxed);
        self.at.store(anchor.at, Ordering::Relaxed);
        self.seq.fetch_add(1, Ordering::Release);
    }

    /// The latest anchor, or `None` while no block has been handed. Gives up
    /// after a few torn reads rather than spinning against the callback.
    pub(super) fn read(&self) -> Option<DeviceAnchor> {
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
pub(super) struct StreamTiming {
    pub(super) start_tick: i64,
    pub(super) rate: u32,
    pub(super) fps_num: i32,
    pub(super) fps_den: i32,
    pub(super) speed_num: i32,
    pub(super) speed_den: i32,
}

impl StreamTiming {
    /// The tick being heard `now` nanoseconds after the epoch: the anchor's
    /// heard position run forward at the device rate, held at the stream's
    /// start before the first sample is heard and at the last frame handed
    /// when the device stalls.
    pub(super) fn tick_at(self, anchor: DeviceAnchor, now: u64) -> i64 {
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
    pub(super) fn frames(self, latency: Duration) -> i64 {
        i64::try_from(latency.as_nanos() * u128::from(self.rate) / 1_000_000_000)
            .unwrap_or(i64::MAX)
    }
}

pub(super) fn tick_from_samples(
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
pub(super) const TIMING: StreamTiming = StreamTiming {
    start_tick: 100,
    rate: 48_000,
    fps_num: 24,
    fps_den: 1,
    speed_num: 100,
    speed_den: 100,
};

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(aria_render::sample_boundary(24, 24, 1), 48_000);
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
    fn invalid_rates_keep_the_anchor() {
        assert_eq!(tick_from_samples(7, 48_000, 0, 24, 1, 100, 100), 7);
        assert_eq!(aria_render::sample_boundary(24, 0, 1), 0);
    }
}
