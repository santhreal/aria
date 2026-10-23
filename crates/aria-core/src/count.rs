//! A count written with the noun that agrees with it: `1 frame`, `3 frames`.
//!
//! Every message that states how many of something it touched, from a status
//! line to a job detail to command-line output, takes its noun from here, so a
//! count of one never reads `1 frames` and no message falls back to
//! `frame(s)`. Both forms are written out because English plurals are not a
//! suffix: `boundary` and `boundaries`, `proxy job` and `proxy jobs`.

use std::fmt;

/// `count` followed by `one` when it is exactly one, else by `many`.
///
/// Formats without allocating, so it goes straight into `format!`:
///
/// ```
/// use aria_core::count::counted;
///
/// assert_eq!(format!("Removed {}", counted(1, "effect", "effects")), "Removed 1 effect");
/// assert_eq!(format!("Removed {}", counted(0_u64, "effect", "effects")), "Removed 0 effects");
/// ```
pub fn counted<'a, C>(count: C, one: &'a str, many: &'a str) -> Counted<'a, C>
where
    C: PartialEq + From<u8>,
{
    let noun = if is_one(&count) { one } else { many };
    Counted { count, noun }
}

/// `one` when `count` is exactly one, else `many`: the noun alone, for a count
/// written apart from it, or a verb that agrees with the count.
///
/// ```
/// use aria_core::count::noun;
///
/// assert_eq!(noun(1_usize, "matches", "match"), "matches");
/// assert_eq!(noun(2_usize, "matches", "match"), "match");
/// ```
pub fn noun<'a, C>(count: C, one: &'a str, many: &'a str) -> &'a str
where
    C: PartialEq + From<u8>,
{
    if is_one(&count) { one } else { many }
}

/// The one count that takes the singular. Zero takes the plural: `0 frames`.
fn is_one<C: PartialEq + From<u8>>(count: &C) -> bool {
    *count == C::from(1)
}

/// A count and its noun; see [`counted`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Counted<'a, C> {
    count: C,
    noun: &'a str,
}

impl<C: fmt::Display> fmt::Display for Counted<'_, C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.count, self.noun)
    }
}

#[cfg(test)]
mod tests {
    use super::{counted, noun};

    /// One takes the singular; zero, two, and every other count take the
    /// plural, for each integer width a count arrives in.
    #[test]
    fn only_a_count_of_one_is_singular() {
        for (count, written) in [
            (0, "0 frames"),
            (1, "1 frame"),
            (2, "2 frames"),
            (11, "11 frames"),
            (21, "21 frames"),
        ] {
            assert_eq!(
                counted(count as usize, "frame", "frames").to_string(),
                written
            );
            assert_eq!(
                counted(count as u32, "frame", "frames").to_string(),
                written
            );
            assert_eq!(
                counted(count as u64, "frame", "frames").to_string(),
                written
            );
            assert_eq!(
                counted(count as i64, "frame", "frames").to_string(),
                written
            );
        }
        assert_eq!(counted(-1_i64, "frame", "frames").to_string(), "-1 frames");
        assert_eq!(
            counted(usize::MAX, "frame", "frames").to_string(),
            format!("{} frames", usize::MAX)
        );
    }

    /// The plural is the one written, not the singular with a suffix.
    #[test]
    fn an_irregular_plural_is_written_as_given() {
        assert_eq!(
            counted(3, "shot boundary", "shot boundaries").to_string(),
            "3 shot boundaries"
        );
        assert_eq!(
            counted(1, "shot boundary", "shot boundaries").to_string(),
            "1 shot boundary"
        );
    }

    #[test]
    fn the_noun_alone_agrees_with_the_count() {
        assert_eq!(noun(1_u64, "is", "are"), "is");
        assert_eq!(noun(0_u64, "is", "are"), "are");
        assert_eq!(noun(7_u64, "is", "are"), "are");
    }
}
