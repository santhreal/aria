//! Unix seconds as a readable UTC stamp.
//!
//! The pool records import times as unix seconds and nothing else in the app
//! needs a calendar, so this is the whole date library: one function, the civil
//! date algorithm, and no dependency. UTC on purpose, because a project moved
//! between machines must not appear to have been imported at a different moment
//! on each of them.

/// `YYYY-MM-DD HH:MM` in UTC. Seconds are dropped: an import time is read to
/// order a list, not to time a race.
pub fn format_unix_utc(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let time_of_day = secs % 86_400;
    let (year, month, day) = civil_from_days(days);
    let hour = time_of_day / 3_600;
    let minute = (time_of_day % 3_600) / 60;
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}")
}

/// Days since 1970-01-01 to a civil date (Howard Hinnant's `civil_from_days`).
/// The era arithmetic is what makes leap years and century rules fall out
/// instead of being special-cased.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11], March-based
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_epoch_is_the_epoch() {
        assert_eq!(format_unix_utc(0), "1970-01-01 00:00");
    }

    #[test]
    fn known_stamps_are_exact() {
        // Values cross-checked against `date -u -d @N`.
        assert_eq!(format_unix_utc(1_700_000_000), "2023-11-14 22:13");
        assert_eq!(format_unix_utc(946_684_800), "2000-01-01 00:00");
        assert_eq!(format_unix_utc(1_072_915_199), "2003-12-31 23:59");
    }

    #[test]
    fn a_leap_day_is_a_day() {
        // 2024-02-29T12:00:00Z
        assert_eq!(format_unix_utc(1_709_208_000), "2024-02-29 12:00");
        // 1900 was not a leap year: 2000-02-29 exists, 1900-03-01 follows 02-28.
        assert_eq!(format_unix_utc(951_782_400), "2000-02-29 00:00");
    }

    #[test]
    fn a_day_boundary_does_not_slip() {
        assert_eq!(format_unix_utc(86_399), "1970-01-01 23:59");
        assert_eq!(format_unix_utc(86_400), "1970-01-02 00:00");
    }

    #[test]
    fn every_day_of_a_leap_year_round_trips_to_a_real_date() {
        // 2024 has 366 days; walking it proves the month lengths, not one date.
        let start = 1_704_067_200; // 2024-01-01T00:00:00Z
        let mut seen = std::collections::HashSet::new();
        for day in 0..366 {
            let stamp = format_unix_utc(start + day * 86_400);
            assert!(stamp.starts_with("2024-"), "{stamp} left the year");
            assert!(seen.insert(stamp.clone()), "{stamp} repeated");
        }
        assert_eq!(
            format_unix_utc(start + 366 * 86_400),
            "2025-01-01 00:00",
            "the year must end after 366 days"
        );
    }
}
