//! The zoom scroll bar the shell draws, driven by the window's pointer and
//! wheel.
//!
//! The class closed here: a bar whose drawing and whose grips disagree, a
//! drag that stops at the bar's 24 px edge, a wheel over the bar that zooms
//! some other way than about the playhead the way it turns, and a thumb
//! painted no further from the track than the clip band. Every press, drag,
//! turn and release goes through the window, as a pointer's does.

use gpui::{
    Modifiers, MouseButton, ScrollDelta, ScrollWheelEvent, TouchPhase, VisualTestContext, point, px,
};

use super::{grip, near, thumb, tick, x_of};
use crate::app::AppState;
use crate::app::viewer_row_tests::{change, shell_with_take};
use crate::app::zoom_bar::{HANDLE_W, ZoomBarGrip};
use crate::interned::intern;
use crate::timeline_display::ZoomBarKnobs;
use crate::transport::{TIMELINE_BASE_PPS, WHEEL_LINE_PX, ZOOM_WHEEL_STEP};

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

    // Holding the other end, the right end zooms with the left end held,
    // past the bar too.
    change(visual, &state, |state| {
        state.ui_settings.zoom_bar_knobs = ZoomBarKnobs::OtherEnd;
    });
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

/// The thumb is the control on the bar and the clip band a hint under it, so
/// the painted thumb stands further from the track than the band does, and
/// further still while a press holds it. The class: a band painted brighter
/// than the thumb, which made the bar read as a lit pipe with the thumb lost
/// on it. Not covered: the thumb under the pointer, the knobs, and the light
/// appearance.
#[gpui::test]
fn the_painted_thumb_stands_further_from_the_track_than_the_clip_band(
    cx: &mut gpui::TestAppContext,
) {
    let (state, visual, take) = shell_with_take(cx, "zoom-bar-inks");
    a_third_of_five_takes(&state, visual, take, |_| 0.0);
    let body = visual
        .debug_bounds(intern("sequence-overview-body"))
        .expect("the bar draws its body");
    let window = visual
        .debug_bounds(intern("sequence-overview-window"))
        .expect("the bar draws its thumb");
    let surface = visual.read(|app| crate::theme::Theme::of(app).surface);
    // The bar's quads in paint order: the track, the clip band, the thumb.
    let inks = |visual: &mut VisualTestContext| {
        visual.update(|drawn, _| {
            let scale = drawn.scale_factor();
            drawn
                .painted_quads()
                .iter()
                .filter_map(|quad| {
                    let b = quad.bounds;
                    let (x, y) = (b.origin.x.0 / scale, b.origin.y.0 / scale);
                    let (w, h) = (b.size.width.0 / scale, b.size.height.0 / scale);
                    if !body.contains(&point(px(x + w / 2.0), px(y + h / 2.0))) {
                        return None;
                    }
                    Some((x, w, h, quad.background.as_solid()?))
                })
                .collect::<Vec<_>>()
        })
    };
    let lum = |color: gpui::Hsla, under: [f32; 3]| {
        let rgb = color.to_rgb();
        let over = |top: f32, base: f32| top * rgb.a + base * (1.0 - rgb.a);
        let out = [
            over(rgb.r, under[0]),
            over(rgb.g, under[1]),
            over(rgb.b, under[2]),
        ];
        (out, 0.2126 * out[0] + 0.7152 * out[1] + 0.0722 * out[2])
    };
    let departures = |visual: &mut VisualTestContext| {
        let quads = inks(visual);
        let tall = |h: f32| (h - HANDLE_W).abs() < 0.5;
        let track = quads
            .iter()
            .find(|(_, w, h, _)| tall(*h) && (w - f32::from(body.size.width)).abs() < 1.0)
            .expect("the bar paints its track across the body")
            .3;
        let band = quads
            .iter()
            .find(|(_, _, h, _)| *h < HANDLE_W / 2.0)
            .expect("the bar paints the take as a band")
            .3;
        let thumb = quads
            .iter()
            .rev()
            .find(|(x, w, h, _)| {
                tall(*h)
                    && (x - f32::from(window.left())).abs() < 0.5
                    && (w - f32::from(window.size.width)).abs() < 0.5
            })
            .expect("the bar paints the thumb over the view")
            .3;
        let base = surface.to_rgb();
        let (under, track_lum) = lum(track, [base.r, base.g, base.b]);
        let from_track = |ink| (lum(ink, under).1 - track_lum).abs();
        (from_track(band), from_track(thumb))
    };

    let (band, rest) = departures(visual);
    assert!(band > 0.0, "the band is painted apart from the track");
    assert!(
        rest > band,
        "the thumb at rest stands {rest} from the track, the band {band}"
    );

    let mid = point(window.center().x, body.center().y);
    visual.simulate_mouse_down(mid, MouseButton::Left, Modifiers::none());
    visual.run_until_parked();
    assert_eq!(
        visual.read(|app| grip(state.read(app))),
        Some(ZoomBarGrip::Thumb)
    );
    let (_, held) = departures(visual);
    visual.simulate_mouse_up(mid, MouseButton::Left, Modifiers::none());
    visual.run_until_parked();
    assert!(
        held > rest,
        "the held thumb stands {held} from the track, {rest} at rest"
    );
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
