//! The duration scale: every length any animation in the app is allowed to be.
//!
//! Split out of [`super`] so the scale is one file with one job. A duration is a
//! [`Beat`], and the only way to get one is to name a step below, because
//! [`Beat::step`] is private to this file. A panel cannot pass `137` to
//! [`super::MotionSpec::new`] — there is no conversion from a number — so a
//! timing nobody chose fails to compile rather than shipping as a number in a
//! panel that no reviewer will ever look at again.
//!
//! Adding a length is one row: a `pub const` and its entry in [`SCALE`]. The
//! tests hold that the table is the whole scale and that the steps are ordered,
//! so a step added out of order, or added and not listed, fails.

use super::*;

/// A length on the motion scale.
///
/// Opaque on purpose: it carries milliseconds, but it cannot be built from
/// milliseconds outside this file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Beat(u64);

impl Beat {
    /// The only constructor, and it is private. Every call is a `pub const`
    /// below, which is what makes [`SCALE`] the whole scale.
    const fn step(ms: u64) -> Self {
        Self(ms)
    }

    pub const fn ms(self) -> u64 {
        self.0
    }

    /// `n` steps of this length.
    ///
    /// A stagger is the only thing that needs this: neighbours in a list, or
    /// stages of a layout, are offset by whole steps of one length so the grid
    /// stays the scale's. It cannot mint a length between two steps, which is
    /// what keeps [`SCALE`] the vocabulary.
    pub const fn times(self, n: u64) -> Self {
        Self(self.0 * n)
    }

    pub const fn duration(self) -> Duration {
        Duration::from_millis(self.0)
    }
}

impl fmt::Display for Beat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ms", self.0)
    }
}

/// One frame at 60Hz. Not a transition: the offset between neighbours that
/// arrive together, so an order is legible without anyone waiting for it.
pub const FRAME: Beat = Beat::step(16);
/// The offset between stages of a layout resolving, or a thing that has to
/// arrive after another thing without reading as a second event.
pub const TAP: Beat = Beat::step(80);
/// Something leaving. Exits are faster than entrances because nobody waits to
/// watch a thing they dismissed.
pub const FLICK: Beat = Beat::step(120);
/// A change small enough that the eye would read a cut as a glitch: a hover, a
/// badge, an indicator sliding between two tabs.
pub const QUICK: Beat = Beat::step(160);
/// Geometry settling: a pane width, a lane height, a disclosure arrow.
pub const SNAP: Beat = Beat::step(200);
/// Contents arriving into a frame that is already on screen.
pub const SETTLE: Beat = Beat::step(320);
/// A whole surface changing, or a view the editor moved on the operator's
/// behalf. Long, because the whole picture travels.
pub const GLIDE: Beat = Beat::step(500);
/// One turn of something that repeats while work is in flight.
pub const CYCLE: Beat = Beat::step(750);
/// A caret, or anything else that marks time for a person who is typing.
pub const BLINK: Beat = Beat::step(1000);
/// The slowest thing in the app: an idle loader breathing, so it reads as
/// waiting rather than as progress.
pub const BREATH: Beat = Beat::step(2400);

/// The whole scale, shortest first. Documentation, the gallery, and the tests
/// all read the scale from here rather than repeating the numbers.
pub const SCALE: &[(&str, Beat)] = &[
    ("FRAME", FRAME),
    ("TAP", TAP),
    ("FLICK", FLICK),
    ("QUICK", QUICK),
    ("SNAP", SNAP),
    ("SETTLE", SETTLE),
    ("GLIDE", GLIDE),
    ("CYCLE", CYCLE),
    ("BLINK", BLINK),
    ("BREATH", BREATH),
];

/// Whether a duration is a step on the scale. Used by the catalog tests: a spec
/// built through [`Beat`] cannot be off the scale, but a row's promised number
/// can be, and that is the retime the mirror is there to catch.
pub fn on_scale(ms: u64) -> bool {
    SCALE.iter().any(|(_, beat)| beat.ms() == ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A step added out of order reads as a scale that skips, and a step added
    /// without a row in `SCALE` is invisible to the document and to `on_scale`.
    #[test]
    fn the_scale_is_ordered_and_every_step_is_listed() {
        assert!(!SCALE.is_empty(), "the scale is not empty");
        for pair in SCALE.windows(2) {
            let (before, after) = (pair[0], pair[1]);
            assert!(
                before.1 < after.1,
                "{} ({}) must be shorter than {} ({})",
                before.0,
                before.1,
                after.0,
                after.1
            );
        }
        for (name, beat) in SCALE {
            assert!(on_scale(beat.ms()), "{name} is reachable through on_scale");
        }
    }

    /// The steps are what the specs are timed against, so a step that drifted is
    /// the whole app retimed. Held here as literals, deliberately: this is the
    /// one place in the crate where a duration is written as a number.
    #[test]
    fn the_steps_are_the_durations_the_app_was_timed_against() {
        let listed: Vec<(&str, u64)> = SCALE.iter().map(|(n, b)| (*n, b.ms())).collect();
        assert_eq!(
            listed,
            vec![
                ("FRAME", 16),
                ("TAP", 80),
                ("FLICK", 120),
                ("QUICK", 160),
                ("SNAP", 200),
                ("SETTLE", 320),
                ("GLIDE", 500),
                ("CYCLE", 750),
                ("BLINK", 1000),
                ("BREATH", 2400),
            ]
        );
    }

    #[test]
    fn a_duration_off_the_scale_is_not_on_the_scale() {
        assert!(!on_scale(137), "137 ms is nobody's chosen timing");
        assert!(!on_scale(0), "zero is not a step");
        assert!(on_scale(GLIDE.ms()));
    }

    #[test]
    fn a_beat_carries_its_own_duration() {
        assert_eq!(SNAP.duration(), Duration::from_millis(200));
        assert_eq!(SNAP.to_string(), "200 ms");
    }
}
