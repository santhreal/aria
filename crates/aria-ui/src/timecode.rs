//! Sequence time formatting.
//!
//! Single source of truth for converting a frame count and a rate into the
//! text a sequence shows: SMPTE timecode, a bare frame count, or feet and
//! frames, as the sequence's [`TimeDisplay`] selects.
//!
//!   * "01:23:45:18"  — non-drop (NDF). Separator is `:` between every field.
//!   * "01:23:45;18"  — drop-frame (DF). Separator is `;` between SS and FF.
//!   * "2010"         — frames.
//!   * "0125+10"      — feet and frames of 35 mm 4-perf film, 16 to the foot.
//!
//! A tick is one frame at the sequence rate. The frame field counts the rate
//! rounded up ([`Rational::nominal_fps`]), so 23.976 counts 24 frames a
//! timecode second and 29.97 counts 30.

use std::fmt;

use aria_models::{Rational, TimeDisplay};

#[derive(Clone, Copy)]
struct NonDropTimecode {
    frame: i64,
    nominal_fps: i64,
}

impl fmt::Display for NonDropTimecode {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        let total_seconds = self.frame.div_euclid(self.nominal_fps);
        let frame_part = self.frame.rem_euclid(self.nominal_fps);
        let hours = total_seconds / 3600;
        let minutes = (total_seconds / 60) % 60;
        let seconds = total_seconds % 60;
        write!(
            output,
            "{hours:02}:{minutes:02}:{seconds:02}:{frame_part:02}"
        )
    }
}

/// One sequence tick in one [`TimeDisplay`], formatted without allocating.
#[derive(Clone, Copy)]
pub(crate) struct SequenceTime {
    frame: i64,
    rate: Rational,
    display: TimeDisplay,
}

impl fmt::Display for SequenceTime {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.display {
            TimeDisplay::Timecode => NonDropTimecode {
                frame: self.frame,
                nominal_fps: self.rate.nominal_fps(),
            }
            .fmt(output),
            TimeDisplay::Frames => write!(output, "{}", self.frame.max(0)),
            TimeDisplay::FeetFrames => {
                let frame = self.frame.max(0);
                let feet = frame / TimeDisplay::FRAMES_PER_FOOT;
                let rest = frame % TimeDisplay::FRAMES_PER_FOOT;
                write!(output, "{feet:04}+{rest:02}")
            }
        }
    }
}

/// `frame` as `display` reads it at `rate`, as a non-allocating value.
pub(crate) fn display_time(frame: i64, rate: Rational, display: TimeDisplay) -> SequenceTime {
    SequenceTime {
        frame,
        rate,
        display,
    }
}

/// `frame` as `display` reads it at `rate`.
pub fn format_time(frame: i64, rate: Rational, display: TimeDisplay) -> String {
    display_time(frame, rate, display).to_string()
}

/// A non-allocating SMPTE display value for one project tick.
pub(crate) fn display_ticks(frame: i64, num: i32, den: i32) -> impl fmt::Display {
    display_time(frame, Rational::new(num, den), TimeDisplay::Timecode)
}

/// Format a frame index as a SMPTE timecode string.
pub fn format_timecode(frame: i64, rate: Rational, drop_frame: bool) -> String {
    if drop_frame {
        format_drop_frame(frame, rate)
    } else {
        format_time(frame, rate, TimeDisplay::Timecode)
    }
}

/// Convenience for the project rate stored as [`aria_models::Rational`].
pub fn format_ticks(frame: i64, num: i32, den: i32) -> String {
    display_ticks(frame, num, den).to_string()
}

/// Parse a typed timecode into a frame index.
///
/// Accepts the shapes an editor actually types: a bare frame count (`120`), and
/// one to four colon-separated fields filled from the right, so `12` is twelve
/// frames, `5:12` is five seconds and twelve frames, and `1:00:05:12` is the
/// full clock. `;` is accepted wherever `:` is, because that is what a
/// drop-frame display shows, and so is `.`, the keypad's separator, so `1.00`
/// is one second. Feet and frames read as `feet+frames`, so `12+08` is twelve
/// feet and eight frames. Returns `None` for anything else, including a
/// negative value or an out-of-range frame field: a refusal is what restores the
/// previous value.
pub fn parse_ticks(text: &str, num: i32, den: i32) -> Option<i64> {
    let nfps = Rational::new(num, den).nominal_fps();
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    if let Some((feet, frames)) = text.split_once('+') {
        let digits = |field: &str| {
            let field = field.trim();
            (!field.is_empty() && field.bytes().all(|b| b.is_ascii_digit()))
                .then(|| field.parse::<i64>().ok())
                .flatten()
        };
        let (feet, frames) = (digits(feet)?, digits(frames)?);
        if frames >= TimeDisplay::FRAMES_PER_FOOT {
            return None;
        }
        return Some(feet * TimeDisplay::FRAMES_PER_FOOT + frames);
    }
    let fields: Vec<&str> = text.split([':', ';', '.']).collect();
    if fields.len() > 4 {
        return None;
    }
    let mut values = Vec::with_capacity(fields.len());
    for field in &fields {
        let field = field.trim();
        if field.is_empty() || !field.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        values.push(field.parse::<i64>().ok()?);
    }
    // Fields fill from the right: frames, then seconds, minutes, hours.
    let mut hours = 0;
    let mut minutes = 0;
    let mut seconds = 0;
    let frames = values.pop()?;
    if let Some(value) = values.pop() {
        seconds = value;
    }
    if let Some(value) = values.pop() {
        minutes = value;
    }
    if let Some(value) = values.pop() {
        hours = value;
    }
    // A single bare number is a frame count, not a frame field, so it may exceed
    // the rate. Once a clock is typed, each field has to be a real field.
    if fields.len() > 1 && (frames >= nfps || seconds > 59 || minutes > 59) {
        return None;
    }
    Some(((hours * 60 + minutes) * 60 + seconds) * nfps + frames)
}

fn format_drop_frame(frame: i64, rate: Rational) -> String {
    let nfps = rate.nominal_fps();
    let drop = (rate.as_f64() * 0.066666_f64).round() as i64;
    let fpm = nfps * 60 - drop;
    let fp10 = nfps * 60 * 10 - drop * 9;
    let d = frame / fp10;
    let m = frame % fp10;
    let adjusted = if m > drop {
        frame + drop * 9 * d + drop * ((m - drop) / fpm)
    } else {
        frame + drop * 9 * d
    };
    let total_seconds = adjusted / nfps;
    let frame_part = adjusted % nfps;
    let hours = total_seconds / 3600;
    let minutes = (total_seconds / 60) % 60;
    let seconds = total_seconds % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02};{frame_part:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ndf_24fps_zero() {
        assert_eq!(format_timecode(0, Rational::FPS_24, false), "00:00:00:00");
    }

    #[test]
    fn ndf_24fps_one_second() {
        assert_eq!(format_timecode(24, Rational::FPS_24, false), "00:00:01:00");
    }

    #[test]
    fn ndf_24fps_one_hour_minus_one_frame() {
        assert_eq!(
            format_timecode(86399, Rational::FPS_24, false),
            "00:59:59:23"
        );
    }

    #[test]
    fn ndf_25fps_one_minute() {
        assert_eq!(
            format_timecode(1500, Rational::FPS_25, false),
            "00:01:00:00"
        );
    }

    #[test]
    fn df_2997_top_of_minute_one_drops_two() {
        assert_eq!(
            format_timecode(1800, Rational::FPS_29_97, true),
            "00:01:00;02"
        );
    }

    #[test]
    fn format_ticks_uses_ndf() {
        assert_eq!(format_ticks(48, 24, 1), "00:00:02:00");
    }

    #[test]
    fn a_bare_number_is_a_frame_count() {
        assert_eq!(parse_ticks("0", 24, 1), Some(0));
        assert_eq!(parse_ticks("120", 24, 1), Some(120));
    }

    #[test]
    fn fields_fill_from_the_right() {
        assert_eq!(parse_ticks("12", 24, 1), Some(12));
        assert_eq!(parse_ticks("5:12", 24, 1), Some(5 * 24 + 12));
        assert_eq!(parse_ticks("2:05:12", 24, 1), Some((2 * 60 + 5) * 24 + 12));
        assert_eq!(
            parse_ticks("01:00:05:12", 24, 1),
            Some(((60 * 60) + 5) * 24 + 12)
        );
    }

    #[test]
    fn a_round_trip_returns_the_same_frame() {
        for frame in [0_i64, 1, 23, 24, 86399, 123_456] {
            let text = format_ticks(frame, 24, 1);
            assert_eq!(parse_ticks(&text, 24, 1), Some(frame), "{text}");
        }
    }

    #[test]
    fn a_drop_frame_separator_is_accepted() {
        assert_eq!(parse_ticks("00:00:01;02", 24, 1), Some(26));
    }

    #[test]
    fn the_keypad_separator_is_accepted() {
        assert_eq!(parse_ticks("1.00", 24, 1), Some(24));
        assert_eq!(parse_ticks("1.5", 24, 1), Some(29));
        assert_eq!(parse_ticks("1.00.00.00", 24, 1), Some(3600 * 24));
        assert_eq!(parse_ticks("1.24", 24, 1), None);
    }

    #[test]
    fn nonsense_is_refused() {
        for text in [
            "",
            "  ",
            "abc",
            "-4",
            "1:2:3:4:5",
            "1::2",
            "12:",
            "1..5",
            "1.",
            "00:00:00:24",
            "0:60:00:00",
            "0:00:60:00",
            "+4",
        ] {
            assert_eq!(parse_ticks(text, 24, 1), None, "{text:?} was accepted");
        }
    }

    #[test]
    fn the_frame_field_is_bounded_by_the_rate() {
        assert_eq!(parse_ticks("00:00:00:24", 25, 1), Some(24));
        assert_eq!(parse_ticks("00:00:00:25", 25, 1), None);
    }

    /// Every display reads the same tick the way the sequence counts it, at
    /// every rate the settings dialog offers.
    ///
    /// The class: a rate's numerator taken as the frames in a second, which
    /// put 36 frames in a 23.976 timecode second (`00:00:00:36`) and moved
    /// every tick-to-pixel conversion by a factor of a thousand. The rate list
    /// is the dialog's own, so a rate added there is checked here.
    #[test]
    fn every_display_reads_a_tick_at_every_offered_rate() {
        for rate in crate::surfaces::settings_pick::RATES {
            let fps = rate.nominal_fps();
            assert!(
                (rate.as_f64() - fps as f64).abs() < 1.0,
                "{rate:?} counts {fps} frames a timecode second"
            );
            let tick = fps * 61 + 5;
            assert_eq!(
                format_time(tick, rate, TimeDisplay::Timecode),
                "00:01:01:05",
                "{rate:?}"
            );
            assert_eq!(
                format_time(tick, rate, TimeDisplay::Frames),
                tick.to_string(),
                "{rate:?}"
            );
            let feet = format_time(tick, rate, TimeDisplay::FeetFrames);
            assert_eq!(
                feet,
                format!("{:04}+{:02}", tick / 16, tick % 16),
                "{rate:?}"
            );
            for display in TimeDisplay::ALL {
                let text = format_time(tick, rate, display);
                assert_eq!(
                    parse_ticks(&text, rate.num, rate.den),
                    Some(tick),
                    "{display:?} at {rate:?} read {text} back"
                );
            }
        }
    }

    #[test]
    fn the_23_976_clock_counts_twenty_four_frames_a_second() {
        let rate = Rational::FPS_23_976;
        assert_eq!(format_time(36, rate, TimeDisplay::Timecode), "00:00:01:12");
        assert_eq!(format_ticks(36, rate.num, rate.den), "00:00:01:12");
        assert_eq!(parse_ticks("1:12", rate.num, rate.den), Some(36));
    }

    #[test]
    fn feet_and_frames_read_sixteen_frames_to_the_foot() {
        assert_eq!(
            format_time(0, Rational::FPS_24, TimeDisplay::FeetFrames),
            "0000+00"
        );
        assert_eq!(
            format_time(16 * 125 + 10, Rational::FPS_24, TimeDisplay::FeetFrames),
            "0125+10"
        );
        assert_eq!(
            format_time(-3, Rational::FPS_24, TimeDisplay::FeetFrames),
            "0000+00"
        );
        assert_eq!(parse_ticks("12+08", 24, 1), Some(12 * 16 + 8));
        assert_eq!(parse_ticks("0+15", 24, 1), Some(15));
        for text in ["0+16", "+4", "4+", "1+2+3", "a+1", "1:00+02"] {
            assert_eq!(parse_ticks(text, 24, 1), None, "{text:?} was accepted");
        }
    }
}
