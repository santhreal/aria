//! The timeline playhead as drawn on the frame each move lands, and the view
//! it lands in.
//!
//! The class closed here: a move of the playhead whose line trails the move,
//! and a seek that leaves the playhead off screen. The line eased toward a
//! frame step or a jump, so for a beat after each one it stood short of its
//! tick, over a frame the Viewer no longer showed. Every move is read on the
//! frame it lands, with the test clock where the move left it, against the
//! pixel of its tick: a ruler press, a drag, a drag past the end, a step back,
//! a jump to the start and a step forward. The run is made twice, once with
//! the shell's layer painting the playhead and once with the timeline body
//! painting it. The head on the ruler is read; the lines across the lane
//! groups are painted at the same tick by the same paint path. A drag in the
//! ruler past the view leaves the view where it is, and a step from there
//! pages the view to the playhead.
//!
//! Not covered: the timeline's scale and offset, which glide on purpose; the
//! page is read from the committed offset and drawn once the glide is over.

use std::time::Duration;

use gpui::{Modifiers, MouseButton, Point, VisualTestContext, point, px};

use crate::app::viewer_row_tests::{bounds_of, change, shell_with_take};
use crate::interned::intern;
use crate::timeline_playhead;
use crate::transport;

/// Where the timeline draws its playhead line, in window pixels.
fn drawn() -> f32 {
    let (_, line) = timeline_playhead::painted_mark().expect("the timeline draws its playhead");
    f32::from(line.left())
}

/// Moves the clock past every transition and draws again.
fn settle(visual: &mut VisualTestContext) {
    visual.executor().advance_clock(Duration::from_secs(2));
    visual.update(|window, _| window.refresh());
    visual.run_until_parked();
}

/// One move of the playhead.
enum Move {
    /// Press the ruler at a point.
    Press(Point<gpui::Pixels>),
    /// Drag the held press to a point.
    Drag(Point<gpui::Pixels>),
    /// Click a Viewer transport button.
    Click(&'static str),
}

#[gpui::test]
fn every_move_draws_the_playhead_at_its_tick_on_the_frame_it_lands(cx: &mut gpui::TestAppContext) {
    let (state, visual, _) = shell_with_take(cx, "playhead-line");
    // The boot cover takes the presses until it is gone.
    crate::test_support::wait_out_splash(visual);
    settle(visual);
    let ruler = visual
        .debug_bounds(intern("timeline-ruler"))
        .expect("the timeline draws its ruler");
    let (tick_px, duration, scroll) = visual.read(|app| {
        let state = state.read(app);
        (
            transport::TIMELINE_BASE_PPS * state.zoom / state.store.sequence.ticks_per_second(),
            state.store.sequence.duration_ticks,
            state.scroll_x,
        )
    });
    // The take is placed at zero, so the sequence ends `end` px right of the
    // ruler's left edge.
    let end = tick_px * duration as f32 - scroll;
    assert!(
        end > 40.0 && end < f32::from(ruler.size.width),
        "the sequence is {end} px long in a {} px ruler: the moves would leave the view",
        f32::from(ruler.size.width)
    );
    let line_of = |tick: i64| f32::from(ruler.left()) + tick as f32 * tick_px - scroll;
    // Off the tick grid, so a press rounding to the nearest tick instead of
    // the one before reads as a line ahead of the pointer.
    let inside = |share: f32| point(ruler.left() + px(end * share + 0.4), ruler.center().y);
    let past = point(ruler.left() + px(end + 40.0), ruler.center().y);
    let moves = [
        ("a ruler press", Move::Press(inside(0.3))),
        ("a ruler drag", Move::Drag(inside(0.7))),
        ("a drag past the end", Move::Drag(past)),
        ("a step back", Move::Click("prev-frame")),
        ("a jump to the start", Move::Click("go-start")),
        ("a step forward", Move::Click("next-frame")),
    ];
    for (path, inline) in [("the layer", false), ("the timeline body", true)] {
        timeline_playhead::FORCE_INLINE.set(inline);
        change(visual, &state, |state| state.set_playhead(duration / 2));
        settle(visual);
        let mut last = visual.read(|app| state.read(app).playhead.value);
        // Where the ruler press is held, so the first click releases it there.
        let mut held = None;
        for (what, step) in &moves {
            match *step {
                Move::Press(at) => {
                    visual.simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
                    held = Some(at);
                }
                Move::Drag(at) => {
                    visual.simulate_mouse_move(at, MouseButton::Left, Modifiers::none());
                    held = Some(at);
                }
                Move::Click(button) => {
                    if let Some(at) = held.take() {
                        visual.simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
                        visual.run_until_parked();
                    }
                    let at = bounds_of(visual, button).center();
                    visual.simulate_click(at, Modifiers::none());
                }
            }
            visual.run_until_parked();
            let tick = visual.read(|app| state.read(app).playhead.value);
            assert_ne!(tick, last, "{what} left the playhead where it was");
            last = tick;
            let landed = drawn();
            assert!(
                (landed - line_of(tick)).abs() < 0.5,
                "with {path} painting, {what} to tick {tick} drew the line at {landed} px on \
                 the frame it landed, not at the tick's {} px",
                line_of(tick)
            );
        }
        assert_eq!(
            visual.read(|app| state.read(app).scroll_x),
            scroll,
            "with {path} painting, a move inside the view moved the view"
        );
    }
    timeline_playhead::FORCE_INLINE.set(false);
}

#[gpui::test]
fn a_ruler_drag_past_the_view_leaves_it_and_a_step_from_there_pages_it(
    cx: &mut gpui::TestAppContext,
) {
    let (state, visual, _) = shell_with_take(cx, "playhead-reveal");
    crate::test_support::wait_out_splash(visual);
    // Zoomed all the way in, the take outruns the ruler.
    change(visual, &state, |state| {
        state.set_zoom(transport::ZOOM_MAX, false);
        state.set_playhead(0);
        state.scroll_timeline_to(0.0, false);
    });
    settle(visual);
    let ruler = visual
        .debug_bounds(intern("timeline-ruler"))
        .expect("the timeline draws its ruler");
    let width = f32::from(ruler.size.width);
    let (tick_px, duration) = visual.read(|app| {
        let state = state.read(app);
        (
            transport::TIMELINE_BASE_PPS * state.zoom / state.store.sequence.ticks_per_second(),
            state.store.sequence.duration_ticks,
        )
    });
    // Past the edge by more than a frame, so the frame under the pointer is
    // off screen too.
    let beyond = 2.0 * tick_px + 20.0;
    assert!(
        tick_px * duration as f32 > width + beyond + tick_px,
        "the take is {} px long in a {width} px ruler: a drag cannot leave the view",
        tick_px * duration as f32
    );
    let y = ruler.center().y;
    let held = point(ruler.right() + px(beyond), y);
    visual.simulate_mouse_down(
        point(ruler.right() - px(20.0), y),
        MouseButton::Left,
        Modifiers::none(),
    );
    visual.simulate_mouse_move(held, MouseButton::Left, Modifiers::none());
    visual.run_until_parked();
    let (tick, scroll) = visual.read(|app| {
        let state = state.read(app);
        (state.playhead.value, state.scroll_x)
    });
    assert!(
        tick as f32 * tick_px > width,
        "the drag {beyond} px past the ruler left the playhead at tick {tick}, on screen"
    );
    assert_eq!(
        scroll, 0.0,
        "a drag in the ruler past the view moved the view"
    );
    visual.simulate_mouse_up(held, MouseButton::Left, Modifiers::none());
    visual.run_until_parked();

    let at = bounds_of(visual, "next-frame").center();
    visual.simulate_click(at, Modifiers::none());
    visual.run_until_parked();
    let (stepped, paged) = visual.read(|app| {
        let state = state.read(app);
        (state.playhead.value, state.scroll_x)
    });
    assert_eq!(stepped, tick + 1, "the step moved the playhead one frame");
    let into = stepped as f32 * tick_px - paged;
    assert!(
        paged > 0.0 && (0.0..width * 0.5).contains(&into),
        "a step to tick {stepped} off screen left the view at {paged} px, the playhead {into} \
         px into it"
    );
    settle(visual);
    let line = f32::from(ruler.left()) + into;
    assert!(
        (drawn() - line).abs() < 0.5,
        "the paged view drew the line at {} px, not at the tick's {line} px",
        drawn()
    );
}
