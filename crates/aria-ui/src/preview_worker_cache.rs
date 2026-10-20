//! Playback timing and the render-ahead frame cache: which tick each playback
//! slot shows, and the frames rendered for the slots after the playhead.

use std::collections::BTreeMap;

use aria_core::Rational;
use aria_render::{DropCause, DroppedFrames};
use gpui::GpuTexture;

use super::{CACHE_BYTES, PreviewFrame, Stamp};

const fn gcd_u128(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    if a == 0 { 1 } else { a }
}

/// Which ticks playback shows: `anchor + floor(slot * speed)` for slot 0, 1, 2,
/// and so on, where slot 0 is the frame on screen when playback started. One
/// slot comes due per frame period of `rate`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cadence {
    /// The tick playback started from.
    pub anchor: i64,
    /// Numerator of the shuttle speed (`speed = speed_num / speed_den`), negative in reverse.
    pub speed_num: i64,
    /// Denominator of the shuttle speed (`> 0`).
    pub speed_den: i64,
    /// The sequence's length in ticks. No frame at or past it is rendered.
    pub end: i64,
    /// The sequence's frame rate, which ticks are counted in.
    pub rate: Rational,
}

impl Cadence {
    /// The tick shown at `slot`: `anchor + floor(slot * speed_num / speed_den)`.
    pub fn tick(self, slot: i64) -> i64 {
        if self.speed_num == 0 || self.speed_den <= 0 {
            return self.anchor;
        }
        let scaled = i128::from(slot) * i128::from(self.speed_num);
        let delta = scaled.div_euclid(i128::from(self.speed_den));
        let tick = i128::from(self.anchor) + delta;
        tick.clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64
    }

    /// The newest slot whose tick the clock has reached at `tick`.
    pub fn slot_at(self, tick: i64) -> i64 {
        if self.speed_num == 0 || self.speed_den <= 0 {
            return 0;
        }
        let g = gcd_u128(
            self.speed_num.unsigned_abs() as u128,
            self.speed_den as u128,
        ) as i128;
        let den = i128::from(self.speed_den) / g;
        if self.speed_num > 0 {
            let num = i128::from(self.speed_num) / g;
            let offset = i128::from(tick) - i128::from(self.anchor);
            let pad = (den - 1).min(num - 1);
            let slot = (offset * den + pad).div_euclid(num);
            slot.clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64
        } else {
            let abs_num = (-i128::from(self.speed_num)) / g;
            let back_offset = i128::from(self.anchor) - i128::from(tick);
            let slot = (back_offset * den).div_euclid(abs_num);
            slot.clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64
        }
    }

    /// Largest slot `s` whose tick has not passed `tick`.
    fn last_slot_of(self, tick: i64) -> i64 {
        if self.speed_num == 0 || self.speed_den <= 0 {
            return 0;
        }
        let den = i128::from(self.speed_den);
        if self.speed_num > 0 {
            let num = i128::from(self.speed_num);
            let offset = i128::from(tick) - i128::from(self.anchor);
            let slot = ((offset + 1) * den - 1).div_euclid(num);
            slot.clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64
        } else {
            let abs_num = -i128::from(self.speed_num);
            let back_offset = i128::from(self.anchor) - i128::from(tick);
            let slot = (back_offset * den).div_euclid(abs_num);
            slot.clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64
        }
    }

    /// Whether some future slot `slot > now` shows `tick`.
    pub fn has_slot_after(self, tick: i64, now: i64) -> bool {
        if self.speed_num == 0 || self.speed_den <= 0 {
            return false;
        }
        let last = self.last_slot_of(tick);
        last > now && self.tick(last) == tick
    }

    /// Whether the sequence has a frame at `tick`.
    pub(super) fn renders(self, tick: i64) -> bool {
        (0..self.end).contains(&tick)
    }
}

/// Frames rendered ahead of the playhead, keyed by tick.
#[derive(Debug, Default)]
pub struct FrameCache {
    frames: BTreeMap<i64, PreviewFrame>,
    bytes: usize,
    stamp: Option<Stamp>,
}

/// The outcome of one display tick: the frame to show and the frames dropped.
#[derive(Debug, Default)]
pub struct Presented {
    /// The frame to put on screen, or `None` to keep the one there.
    pub frame: Option<PreviewFrame>,
    /// Slots that came due during this tick and were not shown, by cause.
    pub dropped: DroppedFrames,
}

impl FrameCache {
    /// Frames held.
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    /// No frame held.
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// Bytes the held frames occupy.
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// Whether a frame for `tick` is held.
    pub fn contains(&self, tick: i64) -> bool {
        self.frames.contains_key(&tick)
    }

    /// Drop every frame.
    pub fn clear(&mut self) {
        self.frames.clear();
        self.bytes = 0;
    }

    pub(super) fn holds_texture(&self, texture: &GpuTexture) -> bool {
        self.frames
            .values()
            .any(|frame| frame.texture.as_ref() == Some(texture))
    }

    pub(super) fn drop_texture_only(&mut self) {
        let mut freed = 0;
        self.frames.retain(|_, frame| {
            let keep = !frame.rgba.pixels.is_empty();
            if !keep {
                freed += frame.bytes();
            }
            keep
        });
        self.bytes = self.bytes.saturating_sub(freed);
    }

    /// Hold `frame`, rendered from `stamp`. A different stamp empties the cache
    /// first, because every frame held shows a session state that is gone.
    /// While the cache is over [`CACHE_BYTES`], the frames farthest from `near`
    /// leave first; a frame larger than the whole budget is not held.
    pub(super) fn insert(&mut self, stamp: Stamp, frame: PreviewFrame, near: i64) {
        self.restamp(stamp);
        if frame.bytes() > CACHE_BYTES {
            return;
        }
        self.bytes += frame.bytes();
        if let Some(replaced) = self.frames.insert(frame.tick, frame) {
            self.bytes -= replaced.bytes();
        }
        while self.bytes > CACHE_BYTES {
            let Some(farthest) = self
                .frames
                .keys()
                .copied()
                .max_by_key(|tick| tick.abs_diff(near))
            else {
                break;
            };
            self.remove(farthest);
        }
    }

    /// Learn the session state the next frame is rendered from, emptying the
    /// cache when it moved.
    fn restamp(&mut self, stamp: Stamp) {
        if self.stamp != Some(stamp) {
            self.clear();
            self.stamp = Some(stamp);
        }
    }

    fn remove(&mut self, tick: i64) -> Option<PreviewFrame> {
        let frame = self.frames.remove(&tick)?;
        self.bytes -= frame.bytes();
        Some(frame)
    }

    /// Take what is due at slot `now`, after `shown` was the newest slot that
    /// came due on an earlier tick.
    ///
    /// Every slot in `shown + 1 ..= now` is due. The newest due slot with a
    /// frame is shown; every other due slot is dropped: as
    /// [`DropCause::PresentLate`] when its frame was held, because the display
    /// reached it after its time, and as `not_ready` when its frame was not
    /// rendered. Every frame at or before `now`, and every frame off the
    /// cadence, leaves the cache.
    pub fn present(
        &mut self,
        cadence: Cadence,
        shown: i64,
        now: i64,
        not_ready: DropCause,
    ) -> Presented {
        let mut out = Presented::default();
        if now <= shown {
            return out;
        }
        let mut held = 0u32;
        let mut newest: Option<i64> = None;
        for slot in (shown + 1)..=now {
            let tick = cadence.tick(slot);
            if self.frames.contains_key(&tick) {
                held += 1;
                newest = Some(tick);
            }
        }
        if let Some(tick) = newest {
            out.frame = if cadence.has_slot_after(tick, now) {
                self.frames.get(&tick).cloned()
            } else {
                self.remove(tick)
            };
        }
        let mut freed = 0;
        self.frames.retain(|tick, frame| {
            let keep = cadence.has_slot_after(*tick, now);
            if !keep {
                freed += frame.bytes();
            }
            keep
        });
        self.bytes -= freed;
        let due = u32::try_from(now - shown).unwrap_or(u32::MAX);
        out.dropped.record(not_ready, due.saturating_sub(held));
        out.dropped
            .record(DropCause::PresentLate, held.saturating_sub(1));
        out
    }
}
