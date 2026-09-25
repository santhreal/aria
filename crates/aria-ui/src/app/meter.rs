//! Output level as the meter draws it: the programme mix, and every mixer lane.
//!
//! The audio callback publishes the peak of the block it just handed to the
//! device and nothing else, and the mix thread publishes each lane's post-fader
//! peak into a table it already holds: a callback that woke the UI thread, or
//! that carried its own decay clock, would be a glitch source. The ballistics
//! live here, on the repaint clock, so the fall rate is the same whatever the
//! device's block size is and can be tested without an audio device.

use std::time::Instant;

use aria_models::TrackId;

use crate::app::AppState;

/// How fast a bar falls, in full scale per second.
///
/// Fast enough to follow a mix, slow enough that a peak is still visible after
/// the repaint that first drew it. A decay per block instead of per second would
/// change speed with the device's buffer size.
const FALL_PER_SECOND: f32 = 2.0;

/// Reading the meter draws: `([left, right], clipped)`, both in full scale.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MeterReading {
    pub peaks: [f32; 2],
    pub clipped: bool,
}

impl MeterReading {
    /// Whether there is anything to draw. A meter that has fallen to silence
    /// stops asking for repaints.
    pub fn active(&self) -> bool {
        self.peaks[0] > 0.0 || self.peaks[1] > 0.0
    }
}

/// The held level after `elapsed` seconds of fall towards `target`.
///
/// A rise is immediate and a fall is rate-limited: a meter that eased upward
/// would under-report a transient, which is the one thing a peak meter is for.
pub fn decay(held: f32, target: f32, elapsed: f32) -> f32 {
    if target >= held {
        return target;
    }
    (held - FALL_PER_SECOND * elapsed.max(0.0))
        .max(target)
        .max(0.0)
}

impl AppState {
    /// Advance the meter to `now` and report what to draw.
    ///
    /// Returns whether the reading changed, so an idle app does not repaint for a
    /// meter that is already at rest.
    pub fn meter_tick(&mut self, now: Instant) -> bool {
        let (peaks, clipped) = self.audio.peaks();
        let elapsed = match self.meter_at {
            Some(then) => now.saturating_duration_since(then).as_secs_f32(),
            None => 0.0,
        };
        self.meter_at = Some(now);
        let before = self.meter;
        self.meter = MeterReading {
            peaks: [
                decay(before.peaks[0], peaks[0], elapsed),
                decay(before.peaks[1], peaks[1], elapsed),
            ],
            clipped,
        };
        // Both, never short-circuited: the strips advance on this clock whether
        // or not the programme meter happened to move.
        let strips = self.strip_meters_tick(elapsed);
        self.meter != before || strips
    }

    /// Advance every lane's bar by the same clock, and adopt a lane the mix
    /// started carrying.
    ///
    /// A held lane the mix has dropped falls to silence rather than being
    /// forgotten at its last level: a strip whose stream stopped must empty,
    /// which is the same rule the programme meter follows.
    fn strip_meters_tick(&mut self, elapsed: f32) -> bool {
        let lanes = self.audio.lane_meters();
        let mut moved = false;
        for (track, held) in self.strip_meters.iter_mut() {
            let target = lanes.peak(*track);
            let next = [
                decay(held[0], target[0], elapsed),
                decay(held[1], target[1], elapsed),
            ];
            moved |= next != *held;
            *held = next;
        }
        for (track, target) in lanes.iter() {
            if target == [0.0, 0.0] || self.strip_meters.iter().any(|(id, _)| *id == track) {
                continue;
            }
            // A rise is immediate, so a lane joining the mix arrives at its own
            // level rather than climbing to it from silence.
            self.strip_meters.push((track, target));
            moved = true;
        }
        moved
    }

    /// What one strip's meter should draw right now. Silence for a lane the mix
    /// is not carrying, which is what a strip outside the mix reads.
    pub fn strip_meter(&self, track: TrackId) -> [f32; 2] {
        self.strip_meters
            .iter()
            .find(|(id, _)| *id == track)
            .map_or([0.0, 0.0], |(_, held)| *held)
    }

    /// What the meter should draw right now, without advancing it: a render reads
    /// state, and only the ticker moves it.
    pub fn meter(&self) -> MeterReading {
        self.meter
    }

    /// Acknowledge the clip indication.
    pub fn clear_clip(&mut self) {
        self.audio.clear_clip();
        self.meter.clipped = false;
    }
}
