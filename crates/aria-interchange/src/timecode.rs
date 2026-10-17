//! SMPTE timecode: `HH:MM:SS:FF`, and `HH:MM:SS;FF` for drop-frame.
//!
//! Drop-frame applies to 30000/1001 and 60000/1001: the first two (or four)
//! frame numbers of every minute are skipped, except every tenth minute, so
//! the count stays within a frame of wall-clock time.

use aria_models::Rational;

/// Whether timecode at `rate` is counted drop-frame.
pub fn is_drop_rate(rate: Rational) -> bool {
    let reduced = rate.reduced();
    reduced == Rational::FPS_29_97 || reduced == Rational::FPS_59_94
}

/// Frame numbers skipped each minute in drop-frame at `nominal` fps.
fn dropped(nominal: i64) -> i64 {
    nominal / 15
}

/// `frames` as timecode at `rate`. Negative counts format as zero.
pub fn format(frames: i64, rate: Rational, drop: bool) -> String {
    let fps = rate.nominal_fps();
    let mut count = frames.max(0);
    let drop = drop && is_drop_rate(rate);
    if drop {
        let skip = dropped(fps);
        let per_minute = fps * 60 - skip;
        let per_ten = fps * 600 - skip * 9;
        let tens = count / per_ten;
        let rest = count % per_ten;
        count += skip * 9 * tens;
        if rest > skip {
            count += skip * ((rest - skip) / per_minute);
        }
    }
    let frame = count % fps;
    let seconds = count / fps;
    let separator = if drop { ';' } else { ':' };
    format!(
        "{:02}:{:02}:{:02}{separator}{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60,
        frame
    )
}

/// The frame count `text` names at `rate`. Drop-frame is read from a `;` or
/// `.` before the frames, or forced by `drop`.
pub fn parse(text: &str, rate: Rational, drop: bool) -> Result<i64, String> {
    let bad = || format!("{text:?} is not a timecode");
    let bytes = text.as_bytes();
    if bytes.len() < 11 || !bytes.is_ascii() {
        return Err(bad());
    }
    let separator = bytes[bytes.len() - 3];
    let marked = matches!(separator, b';' | b'.' | b',');
    if !marked && separator != b':' {
        return Err(bad());
    }
    let mut parts = [0_i64; 4];
    let fields = text[..text.len() - 3]
        .split(':')
        .chain(std::iter::once(&text[text.len() - 2..]));
    let mut count = 0;
    for (slot, field) in parts.iter_mut().zip(fields) {
        if field.is_empty() || field.len() > 4 || !field.bytes().all(|b| b.is_ascii_digit()) {
            return Err(bad());
        }
        *slot = field.parse().map_err(|_| bad())?;
        count += 1;
    }
    if count != 4 || text[..text.len() - 3].split(':').count() != 3 {
        return Err(bad());
    }
    let [hours, minutes, seconds, frames] = parts;
    let fps = rate.nominal_fps();
    if minutes >= 60 || seconds >= 60 || frames >= fps {
        return Err(format!("{text:?} is out of range at {fps} frames a second"));
    }
    let total = ((hours * 60 + minutes) * 60 + seconds) * fps + frames;
    if !((drop || marked) && is_drop_rate(rate)) {
        return Ok(total);
    }
    let skip = dropped(fps);
    if seconds == 0 && minutes % 10 != 0 && frames < skip {
        return Err(format!("{text:?} names a frame drop-frame timecode skips"));
    }
    let all_minutes = hours * 60 + minutes;
    Ok(total - skip * (all_minutes - all_minutes / 10))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DF: Rational = Rational::FPS_29_97;

    #[test]
    fn non_drop_counts_frames_and_round_trips() {
        let rate = Rational::FPS_24;
        assert_eq!(parse("01:00:00:00", rate, false), Ok(86_400));
        assert_eq!(parse("00:00:01:23", rate, false), Ok(47));
        assert_eq!(format(47, rate, false), "00:00:01:23");
        for frames in [0, 1, 23, 24, 1439, 1440, 86_399, 86_400, 2_073_599] {
            assert_eq!(parse(&format(frames, rate, false), rate, false), Ok(frames));
        }
    }

    #[test]
    fn drop_frame_skips_two_numbers_a_minute_but_every_tenth() {
        assert_eq!(format(1799, DF, true), "00:00:59;29");
        assert_eq!(
            format(1800, DF, true),
            "00:01:00;02",
            "minute one drops 00 and 01"
        );
        assert_eq!(
            format(17_982, DF, true),
            "00:10:00;00",
            "minute ten drops nothing"
        );
        assert_eq!(
            format(107_892, DF, true),
            "01:00:00;00",
            "an hour is 107892 frames"
        );
        assert_eq!(parse("00:01:00;02", DF, false), Ok(1800));
        assert_eq!(parse("01:00:00;00", DF, false), Ok(107_892));
        assert!(parse("00:01:00;01", DF, false).is_err(), "a skipped number");
        assert_eq!(
            parse("00:01:00:00", DF, false),
            Ok(1800),
            "a colon reads non-drop"
        );
        for frames in (0..40_000)
            .step_by(7)
            .chain([1798, 1799, 1800, 1801, 17_981, 17_982])
        {
            assert_eq!(
                parse(&format(frames, DF, true), DF, false),
                Ok(frames),
                "{frames}"
            );
        }
    }

    #[test]
    fn sixty_drop_skips_four() {
        let rate = Rational::FPS_59_94;
        assert_eq!(format(3600, rate, true), "00:01:00;04");
        assert_eq!(parse("00:01:00;04", rate, false), Ok(3600));
    }

    #[test]
    fn malformed_timecode_is_refused() {
        let rate = Rational::FPS_24;
        for text in [
            "",
            "1:00:00:00",
            "01:00:00",
            "01:00:00:24",
            "01:60:00:00",
            "01:00:60:00",
            "0a:00:00:00",
            "01-00-00-00",
            "01:00:00:0é",
            "+1:00:00:00",
            "01::0:00:00",
        ] {
            assert!(parse(text, rate, false).is_err(), "{text:?}");
        }
    }
}
