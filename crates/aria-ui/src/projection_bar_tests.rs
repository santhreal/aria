//! What a clip bar's name strip prints for the clip's speed.
//!
//! Closes the class "the strip prints a speed the clip does not play at": a
//! percentage at 100 % forwards, a reversed clip read as forwards, a fraction
//! printed from the wrong side of the ratio, or one number for a ramp whose
//! speed changes along the clip. Not covered: where the strip draws it, which
//! the canvas tests in `panels/timeline/clip/bar_tests.rs` own.

use aria_models::{Clip, MediaId, Rational, TimeRange};

use super::speed_label;

fn clip(num: i32, den: i32, reversed: bool) -> Clip {
    let span = TimeRange::at_rate(0, 48, Rational::FPS_24);
    let mut clip = Clip::from_media(MediaId::from_raw(1), span, span);
    clip.speed = Rational::new(num, den);
    clip.reversed = reversed;
    clip
}

#[test]
fn the_strip_prints_the_speed_only_when_it_is_not_100_percent_forwards() {
    let cases: [((i32, i32, bool), Option<&str>); 7] = [
        ((1, 1, false), None),
        ((2, 2, false), None),
        ((6, 5, false), Some("120%")),
        ((1, 2, false), Some("50%")),
        ((1, 1, true), Some("-100%")),
        ((3, 1, true), Some("-300%")),
        ((1, 3, false), Some("33%")),
    ];
    for ((num, den, reversed), expected) in cases {
        let label = speed_label(&clip(num, den, reversed));
        assert_eq!(
            label.as_ref().map(|label| label.as_ref()),
            expected,
            "speed {num}/{den}, reversed {reversed}"
        );
    }
}

#[test]
fn a_speed_ramp_prints_no_single_speed() {
    // A ramp scales the clip's base speed along the clip, so the base speed is
    // not what plays: a ramped clip prints nothing whatever its base speed and
    // direction.
    for (num, den, reversed) in [(1, 1, false), (3, 2, false), (1, 2, true)] {
        let mut ramped = clip(num, den, reversed);
        ramped
            .speed_curve
            .set_keyframe(0, 1.0, aria_models::Easing::Linear);
        ramped
            .speed_curve
            .set_keyframe(24, 2.0, aria_models::Easing::Linear);
        assert!(ramped.has_speed_curve());
        assert_eq!(
            speed_label(&ramped),
            None,
            "ramp on speed {num}/{den}, reversed {reversed}"
        );
    }
}
