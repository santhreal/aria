//! Loading a project the way the editor loads one.
//!
//! One owner for the load, so `frame`, `export`, and `inspect` all report a bad
//! path, an unreadable file, and a newer schema with the same words the editor
//! would use, and all three agree on how a wall-clock second becomes a tick.

use std::path::Path;

use aria_core::{Rational, RationalTime};
use aria_models::Project;

/// Read a `.aria` file.
pub(crate) fn load(path: &Path) -> Result<Project, String> {
    Project::load_from_file(path).map_err(|err| format!("{}: {err}", path.display()))
}

/// Print every driver that failed while `project` rendered. A failed driver's
/// property takes its keyframed value, so without this a frame drawn with the
/// keyframed value reads as the driven one.
pub(crate) fn warn_driver_faults(project: &Project) {
    for fault in aria_render::driver_faults() {
        eprintln!(
            "aria: warning: the driver on {} failed and its keyframed value was used: {}",
            aria_render::driver_target(project, fault.clip, fault.param),
            fault.message
        );
    }
}

/// The timeline tick nearest `seconds` at `rate`.
///
/// Rounds rather than truncates: `--at 1.0` on a 24 fps timeline must land on
/// frame 24, and floating-point seconds arriving as `0.999999` would truncate
/// to 23 — one frame before the instant the caller asked for.
pub(crate) fn tick_for_seconds(seconds: f64, rate: Rational) -> i64 {
    (seconds * f64::from(rate.num) / f64::from(rate.den)).round() as i64
}

/// `seconds` as a [`RationalTime`] on `rate`, clamped at zero.
pub(crate) fn time_for_seconds(seconds: f64, rate: Rational) -> RationalTime {
    RationalTime::new(tick_for_seconds(seconds.max(0.0), rate), rate)
}

/// `seconds` formatted as `H:MM:SS.mmm`.
pub(crate) fn clock(seconds: f64) -> String {
    let seconds = seconds.max(0.0);
    let whole = seconds as u64;
    let millis = ((seconds - whole as f64) * 1000.0).round() as u64;
    format!(
        "{}:{:02}:{:02}.{:03}",
        whole / 3600,
        (whole / 60) % 60,
        whole % 60,
        millis.min(999)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The class closed here is "an instant lands one frame early". A tick is
    /// rounded from seconds, so a rate whose ticks are not whole seconds still
    /// hits the frame the caller named.
    #[test]
    fn an_instant_rounds_to_the_frame_it_names() {
        let r24 = Rational::new(24, 1);
        assert_eq!(tick_for_seconds(0.0, r24), 0);
        assert_eq!(tick_for_seconds(1.0, r24), 24);
        assert_eq!(tick_for_seconds(0.999_999, r24), 24);
        assert_eq!(tick_for_seconds(1.02, r24), 24);
        assert_eq!(tick_for_seconds(1.5, r24), 36);

        let ntsc = Rational::new(30000, 1001);
        assert_eq!(tick_for_seconds(1.0, ntsc), 30);

        // A negative instant is a zero instant, never a negative tick.
        assert_eq!(time_for_seconds(-5.0, r24).value, 0);
    }

    /// A duration prints as a clock, so `inspect` reads like a timeline rather
    /// than like a float.
    #[test]
    fn a_duration_prints_as_a_clock() {
        assert_eq!(clock(0.0), "0:00:00.000");
        assert_eq!(clock(1.5), "0:00:01.500");
        assert_eq!(clock(61.25), "0:01:01.250");
        assert_eq!(clock(3661.0), "1:01:01.000");
        assert_eq!(clock(-1.0), "0:00:00.000");
    }
}
