//! A held drag's card draws each frame of its take where the release puts
//! that frame, and a lane's shifts move each clip as the engine moves it.
//!
//! # Why
//!
//! A filmstrip cell is a frame drawn at an offset into its card. A card drawn
//! trimmed, rippled or stretched while it kept the offsets of the clip before
//! the drag drew frames the trim dropped and drew them at the wrong ticks.
//! Each case maps one frame through an edit that moves the take against the
//! card and checks the tick it lands on, forward and reversed. The shifts are
//! folded in the order the release sends them, so a shift that moves a clip
//! past the start of a later shift moves it again.

use super::{LaneShift, TakeSpan};

/// A clip on ticks 100 to 200 playing seconds 4 to 8 of its take.
const WAS: TakeSpan = TakeSpan {
    ticks: (100, 200),
    source: (4.0, 8.0),
    reversed: false,
};

/// Two pixels a tick.
const PX: f32 = 2.0;

/// The tick, in `now`, of the frame `WAS` (turned as `reversed`) draws at
/// tick 115.
fn lands(reversed: bool, now: TakeSpan) -> Option<f32> {
    let was = TakeSpan { reversed, ..WAS };
    was.carry(now, 15.0 * PX, PX)
        .map(|x| now.ticks.0 as f32 + x / PX)
}

#[track_caller]
fn assert_lands(reversed: bool, now: TakeSpan, tick: f32) {
    let landed = lands(reversed, now).expect("both spans play a take");
    assert!(
        (landed - tick).abs() < 1e-3,
        "the frame at tick 115 landed at {landed}, not {tick}"
    );
}

#[test]
fn a_trimmed_head_leaves_each_frame_where_it_was() {
    let now = TakeSpan {
        ticks: (110, 200),
        source: (4.4, 8.0),
        reversed: false,
    };
    assert_lands(false, now, 115.0);
}

#[test]
fn a_ripple_trimmed_head_pulls_each_frame_back_by_the_trim() {
    let now = TakeSpan {
        ticks: (100, 190),
        source: (4.4, 8.0),
        reversed: false,
    };
    assert_lands(false, now, 105.0);
}

#[test]
fn a_stretch_spreads_each_frame_from_the_head() {
    let now = TakeSpan {
        ticks: (100, 300),
        source: (4.0, 8.0),
        reversed: false,
    };
    assert_lands(false, now, 130.0);
}

#[test]
fn a_reversed_trimmed_head_drops_the_end_of_the_take() {
    // Reversed, the head plays the take's end: a head trim drops seconds
    // from the top of the window.
    let now = TakeSpan {
        ticks: (110, 200),
        source: (4.0, 7.6),
        reversed: true,
    };
    assert_lands(true, now, 115.0);
}

#[test]
fn a_clip_with_no_take_carries_no_frame() {
    let none = TakeSpan {
        source: (0.0, 0.0),
        ..WAS
    };
    assert_eq!(none.carry(WAS, 30.0, PX), None);
    assert_eq!(WAS.carry(none, 30.0, PX), None);
}

#[test]
fn shifts_fold_in_the_order_the_release_sends_them() {
    let shifts = [
        LaneShift {
            lane: 1,
            from: 50,
            by: -10,
        },
        LaneShift {
            lane: 1,
            from: 40,
            by: 5,
        },
    ];
    // Pulled back to 50 by the first, then pushed on by the second.
    assert_eq!(LaneShift::total(&shifts, 1, 60), -5);
    // Short of the first, past the second.
    assert_eq!(LaneShift::total(&shifts, 1, 45), 5);
    // Short of both.
    assert_eq!(LaneShift::total(&shifts, 1, 30), 0);
    // Another lane.
    assert_eq!(LaneShift::total(&shifts, 2, 60), 0);
}
