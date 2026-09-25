//! SMPTE timecode formatting.
//!
//! Single source of truth for converting a frame count `f` and a rate
//! (as a rational `num/den`) into a timecode string:
//!
//!   * "01:23:45:18"  — non-drop (NDF). Separator is `:` between every field.
//!   * "01:23:45;18"  — drop-frame (DF). Separator is `;` between SS and FF.

use std::fmt;

#[derive(Clone, Copy)]
struct NonDropTimecode {
    frame: i64,
    fps_num: i64,
    fps_den: i64,
}

impl fmt::Display for NonDropTimecode {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        let nfps = nominal_fps(self.fps_num, self.fps_den);
        let total_seconds = self.frame.div_euclid(nfps);
        let frame_part = self.frame.rem_euclid(nfps);
        let hours = total_seconds / 3600;
        let minutes = (total_seconds / 60) % 60;
        let seconds = total_seconds % 60;
        write!(
            output,
            "{hours:02}:{minutes:02}:{seconds:02}:{frame_part:02}"
        )
    }
}

/// A non-allocating SMPTE display value for one project tick.
pub(crate) fn display_ticks(frame: i64, num: i32, den: i32) -> impl fmt::Display {
    NonDropTimecode {
        frame,
        fps_num: i64::from(num.max(1)),
        fps_den: i64::from(den.max(1)),
    }
}

/// Format a frame index as a SMPTE timecode string.
pub fn format_timecode(frame: i64, fps_num: i64, fps_den: i64, drop_frame: bool) -> String {
    if drop_frame {
        format_drop_frame(frame, fps_num, fps_den)
    } else {
        format_non_drop(frame, fps_num, fps_den)
    }
}

/// Convenience for the project rate stored as [`aria_models::Rational`].
pub fn format_ticks(frame: i64, num: i32, den: i32) -> String {
    display_ticks(frame, num, den).to_string()
}

/// Parse an operator-typed timecode into a frame index.
///
/// Accepts the shapes an editor actually types: a bare frame count (`120`), and
/// one to four colon-separated fields filled from the right, so `12` is twelve
/// frames, `5:12` is five seconds and twelve frames, and `1:00:05:12` is the
/// full clock. `;` is accepted wherever `:` is, because that is what a
/// drop-frame display shows. Returns `None` for anything else, including a
/// negative value or an out-of-range frame field: a refusal is what restores the
/// previous value.
pub fn parse_ticks(text: &str, num: i32, den: i32) -> Option<i64> {
    let nfps = nominal_fps(i64::from(num.max(1)), i64::from(den.max(1)));
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let fields: Vec<&str> = text.split([':', ';']).collect();
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

fn nominal_fps(fps_num: i64, fps_den: i64) -> i64 {
    (fps_num + fps_den - 1) / fps_den
}

fn format_non_drop(frame: i64, fps_num: i64, fps_den: i64) -> String {
    NonDropTimecode {
        frame,
        fps_num,
        fps_den,
    }
    .to_string()
}

fn format_drop_frame(frame: i64, fps_num: i64, fps_den: i64) -> String {
    let nfps = nominal_fps(fps_num, fps_den);
    let rate = fps_num as f64 / fps_den as f64;
    let drop = (rate * 0.066666_f64).round() as i64;
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
        assert_eq!(format_timecode(0, 24, 1, false), "00:00:00:00");
    }

    #[test]
    fn ndf_24fps_one_second() {
        assert_eq!(format_timecode(24, 24, 1, false), "00:00:01:00");
    }

    #[test]
    fn ndf_24fps_one_hour_minus_one_frame() {
        assert_eq!(format_timecode(86399, 24, 1, false), "00:59:59:23");
    }

    #[test]
    fn ndf_25fps_one_minute() {
        assert_eq!(format_timecode(1500, 25, 1, false), "00:01:00:00");
    }

    #[test]
    fn df_2997_top_of_minute_one_drops_two() {
        assert_eq!(format_timecode(1800, 30000, 1001, true), "00:01:00;02");
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
    fn nonsense_is_refused() {
        for text in [
            "",
            "  ",
            "abc",
            "-4",
            "1:2:3:4:5",
            "1::2",
            "12:",
            "1.5",
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
}
