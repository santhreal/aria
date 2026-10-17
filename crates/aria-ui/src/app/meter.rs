//! Output level as the meter draws it: the programme mix, and every mixer lane.
//!
//! The audio callback publishes the peak and root mean square of the block it
//! just handed to the device and nothing else, and the mix thread publishes each
//! lane's post-fader peak into a table it already holds: a callback that woke
//! the UI thread, or that carried its own decay clock, would be a glitch
//! source. The ballistics live here, on the repaint clock, so the fall rate is
//! the same whatever the device's block size is and can be tested without an
//! audio device.

use std::time::Instant;

use aria_models::TrackId;

use crate::app::AppState;

/// How fast a bar falls, in full scale per second.
///
/// Fast enough to follow a mix, slow enough that a peak is still visible after
/// the repaint that first drew it. A decay per block instead of per second would
/// change speed with the device's buffer size.
const FALL_PER_SECOND: f32 = 2.0;

/// How long the peak-hold line stays at a peak before it follows the bar down.
pub const PEAK_HOLD_SECONDS: f32 = 1.5;

/// The bottom of the Audio Meters scale, in dBFS. The top is 0 dBFS.
pub const METER_FLOOR_DBFS: f32 = -60.0;

/// Reading the meter draws, per channel `[left, right]`, in full scale.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MeterReading {
    pub peaks: [f32; 2],
    /// Root mean square, drawn behind the peak.
    pub rms: [f32; 2],
    /// The highest bar of the last [`PEAK_HOLD_SECONDS`], or the bar itself
    /// once the hold has run out.
    pub hold: [f32; 2],
    /// Seconds each hold has stood, capped at [`PEAK_HOLD_SECONDS`].
    pub hold_age: [f32; 2],
    pub clipped: bool,
}

impl MeterReading {
    /// Whether there is anything to draw. A meter that has fallen to silence
    /// stops asking for repaints.
    pub fn active(&self) -> bool {
        self.peaks
            .iter()
            .chain(&self.rms)
            .chain(&self.hold)
            .any(|level| *level > 0.0)
    }

    /// Everything the meter draws: the hold's age changes no pixel.
    fn drawn(&self) -> ([f32; 2], [f32; 2], [f32; 2], bool) {
        (self.peaks, self.rms, self.hold, self.clipped)
    }
}

/// Where `level`, in full scale, sits on the Audio Meters scale: `0.0` at
/// [`METER_FLOOR_DBFS`] and below, `1.0` at 0 dBFS and above.
pub fn meter_fraction(level: f32) -> f32 {
    aria_analysis::db_from_linear(level).map_or(0.0, |db| {
        ((db - METER_FLOOR_DBFS) / -METER_FLOOR_DBFS).clamp(0.0, 1.0)
    })
}

/// The hold line and its age after `elapsed` seconds with the bar at `bar`.
///
/// A bar above the line moves the line up and restarts its clock. A line
/// older than [`PEAK_HOLD_SECONDS`] follows the bar down until the next rise.
pub fn hold_peak(held: f32, age: f32, bar: f32, elapsed: f32) -> (f32, f32) {
    if bar > held {
        return (bar, 0.0);
    }
    let age = (age + elapsed.max(0.0)).min(PEAK_HOLD_SECONDS);
    if age >= PEAK_HOLD_SECONDS {
        (bar, age)
    } else {
        (held, age)
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
        let levels = self.audio.levels();
        let elapsed = match self.meter_at {
            Some(then) => now.saturating_duration_since(then).as_secs_f32(),
            None => 0.0,
        };
        self.meter_at = Some(now);
        let before = self.meter;
        let peaks = [0, 1].map(|side| decay(before.peaks[side], levels.peak[side], elapsed));
        let held = [0, 1].map(|side| {
            hold_peak(
                before.hold[side],
                before.hold_age[side],
                peaks[side],
                elapsed,
            )
        });
        self.meter = MeterReading {
            peaks,
            rms: [0, 1].map(|side| decay(before.rms[side], levels.rms[side], elapsed)),
            hold: held.map(|(level, _)| level),
            hold_age: held.map(|(_, age)| age),
            clipped: levels.clipped,
        };
        // Both, never short-circuited: the strips advance on this clock whether
        // or not the programme meter happened to move.
        let strips = self.strip_meters_tick(elapsed);
        self.meter.drawn() != before.drawn() || strips
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
