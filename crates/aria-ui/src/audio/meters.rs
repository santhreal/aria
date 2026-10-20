//! Meter readings: the post-fader peak of each mixer lane, published by the
//! mix thread, and the peak and root mean square of one interleaved block.

use std::sync::atomic::{AtomicU32, Ordering};

use aria_models::TrackId;

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
        let side = usize::from(index % channels >= channels.div_ceil(2) && channels > 1);
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
