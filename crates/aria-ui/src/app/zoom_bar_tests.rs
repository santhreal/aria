//! The zoom scroll bar along the bottom of the timeline.
//!
//! The class closed here: a pointer gesture on the bar that moves the view by
//! something other than the pointer's place on the strip. That covers a move
//! read as a step from the last move rather than as a place, a thumb drawn on
//! one mapping and dragged on another, a strip that rescales under a held
//! press, a strip padded past the sequence and the view, an end that does
//! not stay under the pointer, a knob that holds still what its setting does
//! not set, a press that takes the wrong grip, a view that leaves the strip,
//! and a press that keeps moving after Escape. Every grip is driven through
//! the same press, drag and release, at each knob setting; [`drawn`] drives
//! the bar the shell draws.
//!
//! Not covered: the knobs' colors, and the spans of the clip band, which the
//! overview's own tests own.

use super::{HANDLE_W, MIN_THUMB_W, Strip, ZoomBarGrip};
use crate::app::AppState;
use crate::app::premiere_tests::with_sequence;
use crate::timeline_display::ZoomBarKnobs;
use crate::transport::{TIMELINE_BASE_PPS, ZOOM_MAX, ZOOM_MIN};

#[path = "zoom_bar_drawn_tests.rs"]
mod drawn;

/// A sequence of `clips` two-second clips on a Timeline frame 1200 px wide,
/// zoomed so the view shows `share` of the sequence, at its start.
fn showing(clips: usize, share: f64) -> AppState {
    let mut state = with_sequence(1, clips);
    state.record_timeline_frame([100.0, 300.0, 1200.0, 240.0]);
    let zoom = zoom_showing(&state, share);
    state.set_zoom(zoom, false);
    assert_eq!(
        state.zoom, zoom,
        "a view of {share} of the sequence is a zoom the timeline draws"
    );
    state
}

/// The zoom at which the view shows `share` of the sequence.
fn zoom_showing(state: &AppState, share: f64) -> f32 {
    let sequence = &state.store.sequence;
    let seconds = sequence.duration_ticks as f64 * share / f64::from(sequence.ticks_per_second());
    (f64::from(state.timeline_viewport_px()) / (seconds * f64::from(TIMELINE_BASE_PPS))) as f32
}

/// The strip as drawn now.
fn strip(state: &AppState) -> Strip {
    state
        .zoom_strip(state.view_ticks().1)
        .expect("the timeline is drawn")
}

/// The tick under window `x` on the strip as drawn now, without the clamp.
fn tick(state: &AppState, x: f32) -> f64 {
    let strip = strip(state);
    f64::from(x - strip.left) / f64::from(strip.width) * strip.span
}

/// The window x of `tick` on the strip as drawn now.
fn x_of(state: &AppState, tick: f64) -> f32 {
    let strip = strip(state);
    strip.left + (tick / strip.span * f64::from(strip.width)) as f32
}

/// The window x of the thumb's two ends as drawn now.
fn thumb(state: &AppState) -> (f32, f32) {
    let ((x, w), _) = state.zoom_bar_thumb(state.scroll_x, state.zoom);
    let left = strip(state).left;
    (left + x, left + x + w)
}

fn grip(state: &AppState) -> Option<ZoomBarGrip> {
    state.zoom_bar_grip()
}

/// The playhead's pixel in the view.
fn playhead_pixel(state: &AppState) -> f64 {
    state.playhead.value as f64 / state.ticks_per_px() - f64::from(state.scroll_x)
}

#[track_caller]
fn near(actual: f64, expected: f64, what: &str) {
    assert!(
        (actual - expected).abs() < 0.01,
        "{what}: {actual}, expected {expected}"
    );
}

#[test]
fn the_thumb_is_drawn_over_the_view_on_a_strip_of_the_sequence() {
    let mut state = showing(60, 0.25);
    let duration = state.store.sequence.duration_ticks as f64;
    assert_eq!(
        strip(&state).span,
        duration,
        "a view inside the sequence draws the sequence"
    );
    // A sequence shorter than any floor a strip might keep still fills it.
    let short = showing(2, 0.5);
    assert_eq!(
        strip(&short).span,
        short.store.sequence.duration_ticks as f64,
        "a four-second sequence draws a four-second strip"
    );
    for scroll in [0.0, 400.0, 2000.0] {
        state.scroll_timeline_to(scroll, false);
        let (start, end) = state.view_ticks();
        let (x0, x1) = thumb(&state);
        near(tick(&state, x0), start, "the thumb's left end");
        near(tick(&state, x1), end, "the thumb's right end");
    }

    // A view narrower than the thumb's least width draws that width, centered
    // on the view, and held to the strip at either end.
    state.set_zoom(ZOOM_MAX, false);
    state.scroll_timeline_to((duration / 2.0 / state.ticks_per_px()) as f32, false);
    let (start, end) = state.view_ticks();
    let (x0, x1) = thumb(&state);
    assert!(
        x_of(&state, end) - x_of(&state, start) < MIN_THUMB_W,
        "the view is narrower than the least thumb"
    );
    near(
        f64::from(x1 - x0),
        f64::from(MIN_THUMB_W),
        "a narrow thumb's width",
    );
    near(
        f64::from((x0 + x1) / 2.0),
        f64::from(x_of(&state, (start + end) / 2.0)),
        "a narrow thumb's center",
    );
    let past = (duration / state.ticks_per_px()) as f32;
    state.scroll_timeline_to(past, false);
    let edge = strip(&state);
    near(
        f64::from(thumb(&state).1),
        f64::from(edge.left + edge.width),
        "a narrow thumb at the end",
    );
    state.scroll_timeline_to(0.0, false);
    near(
        f64::from(thumb(&state).0),
        f64::from(edge.left),
        "a narrow thumb at the start",
    );

    // A view that reaches past the sequence's end draws a strip out to the
    // view's end, with the clips across the sequence's share of it.
    let mut state = showing(10, 1.0);
    state.set_zoom(ZOOM_MIN, false);
    let duration = state.store.sequence.duration_ticks as f64;
    let (_, end) = state.view_ticks();
    assert!(end > duration * 2.0, "the view reaches past the end");
    let edge = strip(&state);
    assert_eq!(edge.span, end, "the strip reaches the view's end");
    let ((x, w), share) = state.zoom_bar_thumb(state.scroll_x, state.zoom);
    near(f64::from(x), 0.0, "the thumb's left end");
    near(f64::from(w), f64::from(edge.width), "the thumb's width");
    near(
        f64::from(share),
        duration / end,
        "the clips' share of the strip",
    );
}

#[test]
fn a_press_takes_the_grip_it_lands_on() {
    let mut state = showing(60, 0.25);
    state.scroll_timeline_to(1000.0, false);
    let (x0, x1) = thumb(&state);
    let cases = [
        (x0 - 2.0, ZoomBarGrip::Start),
        (x0 + 1.0, ZoomBarGrip::Start),
        (x0 + HANDLE_W - 1.0, ZoomBarGrip::Start),
        (x0 + HANDLE_W + 2.0, ZoomBarGrip::Thumb),
        ((x0 + x1) / 2.0, ZoomBarGrip::Thumb),
        (x1 - HANDLE_W - 2.0, ZoomBarGrip::Thumb),
        (x1 - HANDLE_W + 1.0, ZoomBarGrip::End),
        (x1 + 2.0, ZoomBarGrip::End),
        // Beside the thumb: the view comes to the press, which scrolls.
        (x1 + 10.0, ZoomBarGrip::Thumb),
        (x0 - 10.0, ZoomBarGrip::Thumb),
    ];
    for (x, expected) in cases {
        let mut state = showing(60, 0.25);
        state.scroll_timeline_to(1000.0, false);
        state.press_zoom_bar(x);
        assert!(state.zoom_bar_held(), "a press at {x} is held");
        assert_eq!(grip(&state), Some(expected), "a press at {x}");
    }

    // A thumb at its least width keeps a middle that scrolls.
    let mut state = showing(60, 0.25);
    state.set_zoom(ZOOM_MAX, false);
    state.scroll_timeline_to(4000.0, false);
    let (x0, x1) = thumb(&state);
    state.press_zoom_bar((x0 + x1) / 2.0);
    assert_eq!(
        grip(&state),
        Some(ZoomBarGrip::Thumb),
        "a narrow thumb's middle"
    );
}

#[test]
fn a_held_thumb_keeps_the_pressed_tick_under_the_pointer_from_any_move() {
    let mut state = showing(60, 0.25);
    state.scroll_timeline_to(1000.0, false);
    let (x0, x1) = thumb(&state);
    let at = (x0 + x1) / 2.0;
    state.press_zoom_bar(at);
    let grabbed = tick(&state, at) - state.view_ticks().0;
    let zoom = state.zoom;
    let mut last = 0.0;
    for dx in [40.0, 40.0, -25.0, 120.0, 120.0, 40.0] {
        let scroll = state.scroll_x;
        let moved = state.drag_zoom_bar(at + dx);
        assert_eq!(
            moved,
            dx != last,
            "a move by {dx} after a move by {last} reports a change"
        );
        if dx == last {
            assert_eq!(state.scroll_x, scroll, "a move to the same place");
        }
        near(
            tick(&state, at + dx) - state.view_ticks().0,
            grabbed,
            "the pressed tick's place in the view",
        );
        near(
            f64::from(thumb(&state).0),
            f64::from(x0 + dx),
            "the drawn thumb follows the pointer",
        );
        assert_eq!(state.zoom, zoom, "a scroll keeps the zoom");
        last = dx;
    }
    assert!(state.release_zoom_bar(), "let go");
    assert!(!state.zoom_bar_held());
    assert!(!state.drag_zoom_bar(at), "a move after letting go");
}

#[test]
fn the_thumb_stops_at_the_strip_ends() {
    let mut state = showing(60, 0.25);
    let duration = state.store.sequence.duration_ticks as f64;
    state.scroll_timeline_to(1000.0, false);
    let (x0, x1) = thumb(&state);
    state.press_zoom_bar((x0 + x1) / 2.0);
    state.drag_zoom_bar(-5000.0);
    assert_eq!(
        state.scroll_x, 0.0,
        "the view stops at the sequence's start"
    );
    state.drag_zoom_bar(9000.0);
    near(
        state.view_ticks().1,
        duration,
        "the view stops at the sequence's end",
    );
}

/// A knob drag at each setting, with the playhead inside the view, before it
/// and past it: the dragged end follows the pointer, and what holds still is
/// read from the setting here rather than from the code under test. The
/// playhead's pixel holds under [`ZoomBarKnobs::Playhead`] while the
/// playhead is inside the view; the view's other end holds otherwise.
///
/// The class: a knob that holds what its setting does not set, a knob that
/// leaves the pointer to keep the playhead, and a playhead outside the view
/// that the knob still holds, which throws the view off the bar on the first
/// move. Not covered: a playhead on the view's last tick, which no tick
/// reaches at a float zoom; the first tick is pressed below.
#[test]
fn each_end_moves_with_the_pointer_and_holds_what_the_setting_sets() {
    for knobs in ZoomBarKnobs::ALL {
        // The playhead as a share of the view: inside, then before and past it.
        for at in [0.3, 0.7, -0.2, 1.2] {
            for share in [0.25, 0.5] {
                // (grip, the pressed end's travel)
                for (end, dx) in [
                    (ZoomBarGrip::End, -40.0),
                    (ZoomBarGrip::End, 60.0),
                    (ZoomBarGrip::Start, 40.0),
                    (ZoomBarGrip::Start, -60.0),
                ] {
                    let mut state = showing(60, share);
                    state.ui_settings.zoom_bar_knobs = knobs;
                    state.scroll_timeline_to(600.0, false);
                    let (start, stop) = state.view_ticks();
                    state.place_playhead((start + at * (stop - start)).round() as i64);
                    let holds_playhead = match knobs {
                        ZoomBarKnobs::Playhead => at > 0.0 && at < 1.0,
                        ZoomBarKnobs::OtherEnd => false,
                    };
                    let pixel = playhead_pixel(&state);
                    let (x0, x1) = thumb(&state);
                    // A press a pixel inside the end moves the end, not the pixel.
                    let (edge, from, other) = match end {
                        ZoomBarGrip::End => (x1, x1 - 1.0, start),
                        _ => (x0, x0 + 1.0, stop),
                    };
                    state.press_zoom_bar(from);
                    assert_eq!(grip(&state), Some(end), "a press at {from}");
                    let to = from + dx;
                    // Twice, so a move read as a step from the last one shows.
                    for _ in 0..2 {
                        state.drag_zoom_bar(to);
                        let (first, last) = state.view_ticks();
                        let what =
                            format!("{knobs:?}, playhead at {at}: {end:?} by {dx} at {share}");
                        let (kept, pressed, drawn) = match end {
                            ZoomBarGrip::End => (first, last, thumb(&state).1),
                            _ => (last, first, thumb(&state).0),
                        };
                        if holds_playhead {
                            near(
                                playhead_pixel(&state),
                                pixel,
                                &format!("{what}: the playhead"),
                            );
                        } else {
                            near(kept, other, &format!("{what}: the other end"));
                        }
                        let moved_to = edge + dx;
                        near(
                            pressed,
                            tick(&state, moved_to),
                            &format!("{what}: the pressed end"),
                        );
                        near(
                            f64::from(drawn),
                            f64::from(moved_to),
                            &format!("{what}: the drawn end"),
                        );
                    }
                }
            }
        }
    }
}

/// A playhead on the view's first tick is not inside the view, so the start
/// knob holds the view's last tick rather than a playhead at no share of the
/// view, which no width puts the start knob under the pointer for.
#[test]
fn a_playhead_on_the_views_first_tick_holds_the_other_end() {
    let mut state = showing(60, 0.25);
    state.place_playhead(0);
    let (start, stop) = state.view_ticks();
    assert_eq!(start, 0.0, "the view starts on the playhead");
    let (x0, _) = thumb(&state);
    state.press_zoom_bar(x0 + 1.0);
    assert_eq!(grip(&state), Some(ZoomBarGrip::Start));
    state.drag_zoom_bar(x0 + 41.0);
    let (first, last) = state.view_ticks();
    near(last, stop, "the other end");
    near(first, tick(&state, x0 + 40.0), "the pressed end");
}

/// Under [`ZoomBarKnobs::Playhead`], a drag that would take the far end off
/// the strip stops it at the strip's end and keeps the dragged end under the
/// pointer; a drag onto or past the playhead stops at the closest zoom with
/// the playhead's pixel held. Under [`ZoomBarKnobs::OtherEnd`], an end pulled
/// onto the other stops at the closest zoom, the other end still held.
///
/// The class: a held playhead that scrolls the view before the sequence's
/// start, which the scroll clamp then shifts out from under the pointer, a
/// view held past the strip's end, which the thumb cannot draw, and a drag
/// past the pivot that zooms out instead of in.
#[test]
fn a_knob_stops_the_far_end_at_the_strip_and_the_near_end_at_the_closest_zoom() {
    let duration = with_sequence(1, 60).store.sequence.duration_ticks as f64;
    // (grip, the playhead as a share of the view, the pressed end's travel,
    // where the far end stops)
    for (end, at, dx, far) in [
        (ZoomBarGrip::End, 0.9, 200.0, 0.0),
        (ZoomBarGrip::Start, 0.1, -100.0, duration),
    ] {
        let mut state = showing(60, 0.25);
        state.ui_settings.zoom_bar_knobs = ZoomBarKnobs::Playhead;
        state.scroll_timeline_to(600.0, false);
        let (start, stop) = state.view_ticks();
        state.place_playhead((start + at * (stop - start)).round() as i64);
        let (x0, x1) = thumb(&state);
        let (edge, from) = match end {
            ZoomBarGrip::End => (x1, x1 - 1.0),
            _ => (x0, x0 + 1.0),
        };
        state.press_zoom_bar(from);
        state.drag_zoom_bar(from + dx);
        let (first, last) = state.view_ticks();
        let (kept, pressed) = match end {
            ZoomBarGrip::End => (first, last),
            _ => (last, first),
        };
        let what = format!("{end:?} by {dx}");
        near(kept, far, &format!("{what}: the far end"));
        near(
            pressed,
            tick(&state, edge + dx),
            &format!("{what}: the pressed end"),
        );
    }

    for knobs in ZoomBarKnobs::ALL {
        let mut state = showing(60, 0.25);
        state.ui_settings.zoom_bar_knobs = knobs;
        state.scroll_timeline_to(600.0, false);
        let (start, stop) = state.view_ticks();
        state.place_playhead(((start + stop) / 2.0).round() as i64);
        let pixel = playhead_pixel(&state);
        let (x0, x1) = thumb(&state);
        state.press_zoom_bar(x1 - 1.0);
        state.drag_zoom_bar(x0 + 1.0);
        assert_eq!(state.zoom, ZOOM_MAX, "{knobs:?}: the closest zoom");
        match knobs {
            ZoomBarKnobs::Playhead => near(playhead_pixel(&state), pixel, "the playhead"),
            ZoomBarKnobs::OtherEnd => near(state.view_ticks().0, start, "the held end"),
        }
    }
}

#[test]
fn a_press_beside_the_thumb_centers_the_view_there_and_scrolls_from_it() {
    let mut state = showing(60, 0.25);
    let duration = state.store.sequence.duration_ticks as f64;
    let (_, x1) = thumb(&state);
    let at = x_of(&state, duration * 0.6);
    assert!(at > x1 + 10.0, "the press is beside the thumb");
    let target = tick(&state, at);
    state.press_zoom_bar(at);
    let (start, end) = state.view_ticks();
    near((start + end) / 2.0, target, "the view's center");
    state.drag_zoom_bar(at - 30.0);
    near(
        state.view_ticks().0,
        start - (target - tick(&state, at - 30.0)),
        "the drag scrolls from the centered view",
    );

    // Beside the thumb at the strip's end, the view stops at the end.
    let mut state = showing(60, 0.25);
    let edge = strip(&state);
    state.press_zoom_bar(edge.left + edge.width - 1.0);
    near(state.view_ticks().1, duration, "the view's end");
}

#[test]
fn escape_puts_the_zoom_and_scroll_back_and_ends_the_press() {
    for (knobs, end) in ZoomBarKnobs::ALL.into_iter().flat_map(|knobs| {
        [ZoomBarGrip::Thumb, ZoomBarGrip::Start, ZoomBarGrip::End].map(|end| (knobs, end))
    }) {
        let mut state = showing(60, 0.25);
        state.ui_settings.zoom_bar_knobs = knobs;
        state.scroll_timeline_to(600.0, false);
        let (start, stop) = state.view_ticks();
        state.place_playhead(((start + stop) / 2.0).round() as i64);
        let view = (state.zoom, state.scroll_x);
        let (x0, x1) = thumb(&state);
        let at = match end {
            ZoomBarGrip::Thumb => (x0 + x1) / 2.0,
            ZoomBarGrip::Start => x0 + 1.0,
            ZoomBarGrip::End => x1 - 1.0,
        };
        state.press_zoom_bar(at);
        let what = format!("{knobs:?} {end:?}");
        assert!(state.drag_zoom_bar(at + 50.0), "{what} moved the view");
        assert!(state.cancel_zoom_bar_drag(), "{what} cancelled");
        assert_eq!((state.zoom, state.scroll_x), view, "{what} put back");
        assert!(!state.zoom_bar_held());
        assert!(
            !state.drag_zoom_bar(at + 90.0),
            "{what} still moved after Escape"
        );
        assert!(
            !state.cancel_zoom_bar_drag(),
            "a second Escape has no press"
        );
        assert_eq!((state.zoom, state.scroll_x), view);
    }
}

#[test]
fn a_held_press_keeps_the_strip_it_found() {
    // A view out past the sequence's end: the strip reaches the view's end
    // until the press lets go, so the grabbed end stays under the pointer
    // while the view comes back inside the sequence.
    let mut state = showing(10, 1.0);
    state.set_zoom(ZOOM_MIN, false);
    let duration = state.store.sequence.duration_ticks as f64;
    let found = strip(&state);
    assert!(
        found.span > duration * 2.0,
        "the strip reaches past the end"
    );
    let (_, x1) = thumb(&state);
    state.press_zoom_bar(x1 - 1.0);
    assert_eq!(grip(&state), Some(ZoomBarGrip::End));
    // Halfway into the sequence, which is a small part of the strip.
    let at = found.left + found.width * (duration / found.span / 2.0) as f32;
    state.drag_zoom_bar(at);
    assert_eq!(strip(&state), found, "the held strip");
    // The press landed a pixel inside the end, and stays a pixel inside it.
    near(
        state.view_ticks().1,
        tick(&state, at + 1.0),
        "the pressed end",
    );
    near(
        f64::from(thumb(&state).1),
        f64::from(at + 1.0),
        "the drawn end",
    );

    // Let go, the strip is the sequence again.
    state.release_zoom_bar();
    assert_eq!(strip(&state).span, duration, "the strip after letting go");
}
