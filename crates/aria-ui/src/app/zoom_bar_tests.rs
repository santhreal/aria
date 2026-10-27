//! The zoom scroll bar along the bottom of the timeline.
//!
//! The class closed here: a pointer gesture on the bar that moves the view by
//! something other than the pointer's place on the strip. That covers a move
//! read as a step from the last move rather than as a place, a thumb drawn on
//! one mapping and dragged on another, a strip that rescales under a held
//! press, a strip padded past the sequence and the view, an end that does
//! not stay under the pointer, a press that takes the wrong grip, a view that
//! leaves the strip, a press that keeps moving after Escape, and a drag that
//! stops at the bar's 24 px edge. Every grip is driven through the same press,
//! drag and release, and the drawn tests drive the bar the shell draws: the
//! thumb is drawn over the view and each knob over the grip a press on it
//! takes. A wheel turned over the bar zooms about the playhead, the way it
//! turns.
//!
//! Not covered: the bar's colors and the clip band drawn on it, which a
//! snapshot and the overview's own tests own.

use gpui::{
    Modifiers, MouseButton, ScrollDelta, ScrollWheelEvent, TouchPhase, VisualTestContext, point, px,
};

use super::{HANDLE_W, MIN_THUMB_W, Strip, ZoomBarGrip};
use crate::app::AppState;
use crate::app::premiere_tests::with_sequence;
use crate::app::viewer_row_tests::{change, shell_with_take};
use crate::interned::intern;
use crate::transport::{TIMELINE_BASE_PPS, WHEEL_LINE_PX, ZOOM_MAX, ZOOM_MIN, ZOOM_WHEEL_STEP};

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

#[test]
fn each_end_zooms_with_the_other_end_held_and_moves_with_the_pointer() {
    for share in [0.25, 0.5] {
        // (grip, the pressed end's travel)
        for (end, dx) in [
            (ZoomBarGrip::End, -80.0),
            (ZoomBarGrip::End, 60.0),
            (ZoomBarGrip::Start, 80.0),
            (ZoomBarGrip::Start, -60.0),
        ] {
            let mut state = showing(60, share);
            state.scroll_timeline_to(600.0, false);
            let (start, stop) = state.view_ticks();
            let (x0, x1) = thumb(&state);
            // A press a pixel inside the end moves the end, not the pixel.
            let (edge, from, held) = match end {
                ZoomBarGrip::End => (x1, x1 - 1.0, start),
                _ => (x0, x0 + 1.0, stop),
            };
            state.press_zoom_bar(from);
            assert_eq!(grip(&state), Some(end), "a press at {from}");
            let at = from + dx;
            // Twice, so a move read as a step from the last one shows.
            for _ in 0..2 {
                state.drag_zoom_bar(at);
                let (first, last) = state.view_ticks();
                let what = format!("{end:?} by {dx} at {share}");
                let (kept, pressed, drawn) = match end {
                    ZoomBarGrip::End => (first, last, thumb(&state).1),
                    _ => (last, first, thumb(&state).0),
                };
                near(kept, held, &format!("{what}: the other end"));
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

    // An end pulled onto the other stops at the closest zoom, the other end
    // still held.
    let mut state = showing(60, 0.25);
    state.scroll_timeline_to(600.0, false);
    let (start, _) = state.view_ticks();
    let (x0, x1) = thumb(&state);
    state.press_zoom_bar(x1 - 1.0);
    state.drag_zoom_bar(x0 + 1.0);
    assert_eq!(state.zoom, ZOOM_MAX, "the closest zoom");
    near(state.view_ticks().0, start, "the held end");
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
    for end in [ZoomBarGrip::Thumb, ZoomBarGrip::Start, ZoomBarGrip::End] {
        let mut state = showing(60, 0.25);
        state.scroll_timeline_to(600.0, false);
        let view = (state.zoom, state.scroll_x);
        let (x0, x1) = thumb(&state);
        let at = match end {
            ZoomBarGrip::Thumb => (x0 + x1) / 2.0,
            ZoomBarGrip::Start => x0 + 1.0,
            ZoomBarGrip::End => x1 - 1.0,
        };
        state.press_zoom_bar(at);
        assert!(state.drag_zoom_bar(at + 50.0), "{end:?} moved the view");
        assert!(state.cancel_zoom_bar_drag(), "{end:?} cancelled");
        assert_eq!((state.zoom, state.scroll_x), view, "{end:?} put back");
        assert!(!state.zoom_bar_held());
        assert!(
            !state.drag_zoom_bar(at + 90.0),
            "{end:?} still moved after Escape"
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

/// The view's first and last tick in the drawn window.
fn view(state: &gpui::Entity<AppState>, visual: &mut VisualTestContext) -> (f64, f64) {
    visual.read(|app| {
        let state = state.read(app);
        let fps = f64::from(state.store.sequence.ticks_per_second());
        let per_px = fps / f64::from(TIMELINE_BASE_PPS * state.zoom);
        let start = f64::from(state.scroll_x) * per_px;
        (
            start,
            start + f64::from(state.timeline_viewport_px()) * per_px,
        )
    })
}

/// Five takes end to end in the drawn shell and a view a third of them wide,
/// scrolled to `scroll_x`: a view the closest zoom can draw in any test window.
fn a_third_of_five_takes(
    state: &gpui::Entity<AppState>,
    visual: &mut VisualTestContext,
    take: aria_models::MediaId,
    scroll_x: impl FnOnce(&AppState) -> f32,
) {
    change(visual, state, |state| {
        for _ in 1..5 {
            let end = state.store.sequence.duration_ticks;
            state
                .drop_media_on_timeline(take, None, end)
                .expect("place the take again");
        }
        let sequence = &state.store.sequence;
        let seconds = sequence.duration_ticks as f32 / sequence.ticks_per_second() / 3.0;
        let zoom = state.timeline_viewport_px() / (seconds * TIMELINE_BASE_PPS);
        state.set_zoom(zoom, false);
        let scroll = scroll_x(state);
        state.scroll_timeline_to(scroll, false);
    });
}

#[gpui::test]
fn the_drawn_bar_scrolls_and_zooms_past_its_edges_and_escape_puts_the_view_back(
    cx: &mut gpui::TestAppContext,
) {
    let (state, visual, take) = shell_with_take(cx, "zoom-scroll-bar");
    a_third_of_five_takes(&state, visual, take, |_| 0.0);
    let bar = visual
        .debug_bounds(intern("sequence-overview"))
        .expect("the timeline draws its zoom scroll bar");
    let y = f32::from(bar.center().y);
    let (left, width, duration) = visual.read(|app| {
        let state = state.read(app);
        (
            f32::from(bar.left()) + state.outline_drawn_width(),
            state.timeline_viewport_px(),
            state.store.sequence.duration_ticks as f64,
        )
    });
    let tick = |x: f32| f64::from((x - left) / width) * duration;
    let x_of = |tick: f64| left + (tick / duration) as f32 * width;
    let at = |x: f32, y: f32| point(px(x), px(y));
    let (start, end) = view(&state, visual);
    assert!(end < duration * 0.5, "the view shows part of the take");

    // The thumb scrolls the view, and keeps scrolling once the pointer has
    // left the bar.
    let mid = (x_of(start) + x_of(end)) / 2.0;
    visual.simulate_mouse_down(at(mid, y), MouseButton::Left, Modifiers::none());
    visual.simulate_mouse_move(at(mid + 30.0, y), MouseButton::Left, Modifiers::none());
    visual.run_until_parked();
    near(
        view(&state, visual).0,
        tick(mid + 30.0) - tick(mid),
        "the thumb scrolled under the pointer",
    );
    let above = at(mid + 50.0, y - 80.0);
    visual.simulate_mouse_move(above, MouseButton::Left, Modifiers::none());
    visual.run_until_parked();
    near(
        view(&state, visual).0,
        tick(mid + 50.0) - tick(mid),
        "the drag stopped at the bar's edge",
    );
    visual.simulate_mouse_up(above, MouseButton::Left, Modifiers::none());
    visual.run_until_parked();
    let scrolled = view(&state, visual);
    visual.simulate_mouse_move(at(mid + 90.0, y), None, Modifiers::none());
    visual.run_until_parked();
    assert_eq!(
        view(&state, visual),
        scrolled,
        "the drag ended at the release"
    );

    // The right end zooms with the left end held, past the bar too.
    let (start, end) = scrolled;
    let grab = x_of(end) - 1.0;
    visual.simulate_mouse_down(at(grab, y), MouseButton::Left, Modifiers::none());
    let above = at(grab - 40.0, y - 80.0);
    visual.simulate_mouse_move(above, MouseButton::Left, Modifiers::none());
    visual.run_until_parked();
    let zoomed = view(&state, visual);
    near(zoomed.0, start, "the left end held");
    near(
        zoomed.1,
        tick(grab + 1.0 - 40.0),
        "the right end moved with the pointer",
    );
    visual.simulate_mouse_up(above, MouseButton::Left, Modifiers::none());
    visual.run_until_parked();

    // Escape with the button down puts the view back and ends the drag.
    let mid = (x_of(zoomed.0) + x_of(zoomed.1)) / 2.0;
    visual.simulate_mouse_down(at(mid, y), MouseButton::Left, Modifiers::none());
    visual.simulate_mouse_move(at(mid + 20.0, y), MouseButton::Left, Modifiers::none());
    visual.run_until_parked();
    assert_ne!(view(&state, visual), zoomed, "the thumb moved");
    visual.simulate_keystrokes("escape");
    visual.run_until_parked();
    assert_eq!(view(&state, visual), zoomed, "Escape put the view back");
    visual.simulate_mouse_move(at(mid + 40.0, y), MouseButton::Left, Modifiers::none());
    visual.simulate_mouse_up(at(mid + 40.0, y), MouseButton::Left, Modifiers::none());
    visual.run_until_parked();
    assert_eq!(view(&state, visual), zoomed, "the drag ended at Escape");
}

/// The drawn thumb spans the view's stretch of the strip, and each drawn knob
/// is a circle `HANDLE_W` across, centered on the bar's row, at its end of the
/// thumb and inside the bar wherever the view is: a press anywhere on a knob
/// takes that end's grip, and a press on the thumb past the knob scrolls.
///
/// The class: a bar whose drawing and whose grips disagree, a knob drawn wider
/// or narrower than the press it takes, off its end of the thumb, or out of
/// the bar at the strip's ends. Not covered: the colors.
#[gpui::test]
fn each_drawn_knob_takes_the_grip_of_its_end(cx: &mut gpui::TestAppContext) {
    let (state, visual, take) = shell_with_take(cx, "zoom-bar-knobs");
    let ends: [fn(&AppState) -> f32; 3] = [
        |_| 0.0,
        |state| state.timeline_viewport_px(),
        |state| 2.0 * state.timeline_viewport_px(),
    ];
    for (n, scroll_x) in ends.into_iter().enumerate() {
        if n == 0 {
            a_third_of_five_takes(&state, visual, take, scroll_x);
        } else {
            change(visual, &state, |state| {
                let scroll = scroll_x(state);
                state.scroll_timeline_to(scroll, false);
            });
        }
        let drawn = |visual: &mut VisualTestContext, name: &str| {
            visual
                .debug_bounds(intern(name))
                .unwrap_or_else(|| panic!("the bar draws {name}"))
        };
        let body = drawn(visual, "sequence-overview-body");
        let window = drawn(visual, "sequence-overview-window");
        let (x0, x1) = visual.read(|app| thumb(state.read(app)));
        let row = f64::from(body.center().y);
        let before = view(&state, visual);
        let at = |x: f32| point(px(x), body.center().y);
        near(
            f64::from(window.left()),
            f64::from(x0),
            "the thumb's left end",
        );
        near(
            f64::from(window.right()),
            f64::from(x1),
            "the thumb's right end",
        );
        near(
            f64::from(window.size.height),
            f64::from(HANDLE_W),
            "the thumb's height",
        );
        near(f64::from(window.center().y), row, "the thumb's middle");

        for (name, end) in [
            ("sequence-overview-handle-left", ZoomBarGrip::Start),
            ("sequence-overview-handle-right", ZoomBarGrip::End),
        ] {
            let knob = drawn(visual, name);
            near(
                f64::from(knob.size.width),
                f64::from(HANDLE_W),
                "a knob's width",
            );
            near(
                f64::from(knob.size.height),
                f64::from(HANDLE_W),
                "a knob's height",
            );
            near(f64::from(knob.center().y), row, "a knob's middle");
            let (outer, inner, past) = match end {
                ZoomBarGrip::Start => (window.left(), knob.left(), f32::from(knob.right()) + 2.0),
                _ => (window.right(), knob.right(), f32::from(knob.left()) - 2.0),
            };
            near(
                f64::from(inner),
                f64::from(outer),
                "a knob at its end of the thumb",
            );
            assert!(
                body.left() <= knob.left() && knob.right() <= body.right(),
                "{name} is inside the bar at scroll {n}: {knob:?} in {body:?}"
            );
            let (left, right) = (f32::from(knob.left()), f32::from(knob.right()));
            for (x, expected) in [
                (left + 0.5, end),
                ((left + right) / 2.0, end),
                (right - 0.5, end),
                (past, ZoomBarGrip::Thumb),
            ] {
                visual.simulate_mouse_down(at(x), MouseButton::Left, Modifiers::none());
                visual.run_until_parked();
                let pressed = visual.read(|app| grip(state.read(app)));
                visual.simulate_mouse_up(at(x), MouseButton::Left, Modifiers::none());
                visual.run_until_parked();
                assert_eq!(
                    pressed,
                    Some(expected),
                    "a press at {x} on {name}, scroll {n}"
                );
                assert_eq!(
                    view(&state, visual),
                    before,
                    "a press and release moved nothing"
                );
            }
        }
    }
}

/// The zoom, the scroll, and the playhead's pixel in the view.
fn zoom_scroll_playhead(
    state: &gpui::Entity<AppState>,
    visual: &mut VisualTestContext,
) -> (f32, f32, f32) {
    visual.read(|app| {
        let state = state.read(app);
        let seconds = state.playhead.value as f32 / state.store.sequence.ticks_per_second();
        let pixel = seconds * TIMELINE_BASE_PPS * state.zoom - state.scroll_x;
        (state.zoom, state.scroll_x, pixel)
    })
}

/// Turn the wheel once at `at` by `delta` with `modifiers` held.
fn wheel(
    visual: &mut VisualTestContext,
    at: gpui::Point<gpui::Pixels>,
    delta: ScrollDelta,
    modifiers: Modifiers,
) {
    visual.simulate_mouse_move(at, None, modifiers);
    visual.simulate_event(ScrollWheelEvent {
        position: at,
        delta,
        modifiers,
        touch_phase: TouchPhase::Moved,
    });
    visual.run_until_parked();
}

/// A wheel turned over the drawn bar zooms about the playhead at once: a notch
/// up zooms in by one wheel step, a notch down out by one, a trackpad's pixels
/// by the share of a notch they travel, Ctrl and a notch by one step and not
/// twice, and the playhead keeps its pixel through each. A turn with a press
/// held on the bar moves nothing. Shift and a notch down over the bar, and a
/// trackpad swiped left over the lanes, scroll later by the distance turned
/// and keep the zoom. Ctrl over the lanes zooms the same way round, about the
/// pointer.
///
/// The class: a wheel over the bar that reaches nothing, a wheel that zooms
/// out when turned up, a trackpad whose every small event zooms a whole
/// notch, a turn the bar and the timeline under it both take, and a sideways
/// turn that scrolls the wrong way. Not covered: pinch gestures, which gpui
/// does not report as wheel turns.
#[gpui::test]
fn a_wheel_over_the_bar_zooms_about_the_playhead_the_way_it_turns(cx: &mut gpui::TestAppContext) {
    let (state, visual, take) = shell_with_take(cx, "zoom-bar-wheel");
    // Twenty-four takes end to end at twice the base zoom, far longer than
    // the view, with the playhead twenty seconds in at the middle of the view.
    change(visual, &state, |state| {
        for _ in 1..24 {
            let end = state.store.sequence.duration_ticks;
            state
                .drop_media_on_timeline(take, None, end)
                .expect("place the take again");
        }
        state.set_zoom(2.0, false);
        let per_second = state.store.sequence.ticks_per_second();
        state.set_playhead((20.0 * per_second) as i64);
        let middle = 20.0 * TIMELINE_BASE_PPS * 2.0 - state.timeline_viewport_px() / 2.0;
        state.scroll_timeline_to(middle, false);
    });
    let bar = visual
        .debug_bounds(intern("sequence-overview-body"))
        .expect("the timeline draws its zoom scroll bar")
        .center();
    let (zoom, scroll, pixel) = zoom_scroll_playhead(&state, visual);
    let (width, end) = visual.read(|app| {
        let state = state.read(app);
        let seconds =
            state.store.sequence.duration_ticks as f32 / state.store.sequence.ticks_per_second();
        (
            state.timeline_viewport_px(),
            seconds * TIMELINE_BASE_PPS * zoom,
        )
    });
    assert!(scroll > 0.0, "the view starts past the sequence's start");
    assert!(
        scroll + 2.0 * width < end,
        "the view ends well before the sequence: {scroll} + 2 × {width} against {end}"
    );
    assert!((0.0..width).contains(&pixel), "the playhead is in the view");

    let lines = |notches: f32| ScrollDelta::Lines(point(0.0, notches));
    let mut expected = zoom;
    for (delta, modifiers, factor, turn) in [
        (lines(1.0), Modifiers::none(), ZOOM_WHEEL_STEP, "a notch up"),
        (
            lines(-1.0),
            Modifiers::none(),
            1.0 / ZOOM_WHEEL_STEP,
            "a notch down",
        ),
        (
            ScrollDelta::Pixels(point(px(0.0), px(6.0))),
            Modifiers::none(),
            ZOOM_WHEEL_STEP.powf(0.25),
            "a trackpad's six pixels up",
        ),
        // Once, not again about the pointer by the timeline under the bar.
        (
            lines(-1.0),
            Modifiers::control(),
            1.0 / ZOOM_WHEEL_STEP,
            "Ctrl and a notch down",
        ),
    ] {
        wheel(visual, bar, delta, modifiers);
        expected *= factor;
        let (zoom, _, moved) = zoom_scroll_playhead(&state, visual);
        assert!(
            (zoom - expected).abs() < expected * 1e-5,
            "{turn} over the bar: zoom {zoom}, expected {expected}"
        );
        assert!(
            (moved - pixel).abs() < 0.5,
            "{turn} over the bar moved the playhead from {pixel} px to {moved} px"
        );
        assert!(
            visual.read(|app| !state.read(app).zoom_eased),
            "{turn} over the bar lands at once"
        );
    }

    // A press held on the bar, and a turn under it.
    visual.simulate_mouse_down(bar, MouseButton::Left, Modifiers::none());
    visual.run_until_parked();
    let before = zoom_scroll_playhead(&state, visual);
    visual.simulate_event(ScrollWheelEvent {
        position: bar,
        delta: lines(1.0),
        modifiers: Modifiers::none(),
        touch_phase: TouchPhase::Moved,
    });
    visual.run_until_parked();
    assert_eq!(
        zoom_scroll_playhead(&state, visual),
        before,
        "a turn with a press held on the bar"
    );
    visual.simulate_mouse_up(bar, MouseButton::Left, Modifiers::none());
    visual.run_until_parked();

    // Shift and a notch down scroll later by one line and keep the zoom, and so
    // does a trackpad swiped left with no key held, over the lanes.
    let (zoom, scroll, _) = zoom_scroll_playhead(&state, visual);
    wheel(visual, bar, lines(-1.0), Modifiers::shift());
    let (shifted, scrolled, _) = zoom_scroll_playhead(&state, visual);
    assert_eq!(shifted, zoom, "a Shift turn over the bar keeps the zoom");
    assert_eq!(
        scrolled,
        scroll + WHEEL_LINE_PX,
        "Shift and a notch down over the bar scroll one line later"
    );
    let lanes = visual
        .debug_bounds(intern("timeline-video-canvas"))
        .expect("the timeline draws its video lanes")
        .center();
    wheel(
        visual,
        lanes,
        ScrollDelta::Pixels(point(px(-30.0), px(0.0))),
        Modifiers::none(),
    );
    let (swiped, swiped_to, _) = zoom_scroll_playhead(&state, visual);
    assert_eq!(swiped, zoom, "a sideways swipe keeps the zoom");
    assert_eq!(
        swiped_to,
        scrolled + 30.0,
        "a trackpad swiped left over the lanes scrolls thirty pixels later"
    );

    wheel(visual, lanes, lines(1.0), Modifiers::control());
    let (zoomed, _, _) = zoom_scroll_playhead(&state, visual);
    assert!(
        (zoomed - zoom * ZOOM_WHEEL_STEP).abs() < zoom * 1e-5,
        "Ctrl and a notch up over the lanes: zoom {zoomed}, expected {}",
        zoom * ZOOM_WHEEL_STEP
    );
}
