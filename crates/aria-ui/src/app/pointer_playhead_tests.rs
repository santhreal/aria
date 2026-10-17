//! The timeline playhead under a pointer.
//!
//! The class closed here: a pointer that moves the playhead through a path
//! that eases, so the drawn line trails the hand by the glide while a ruler
//! press or drag is held. A drag past the sequence holds the line at its end,
//! drawn there on the frame the move lands. A keyed step still eases, and that
//! is what tells the two paths apart: the line is read on the frame a move
//! lands and again once the clock has passed every glide.
//!
//! Not covered: the Viewer's strip and the property rows' strips, which draw
//! no eased playhead of their own.

use std::time::Duration;

use gpui::{Modifiers, MouseButton, VisualTestContext, point, px};

use crate::app::viewer_row_tests::{change, shell_with_take};
use crate::interned::intern;

/// Where the timeline draws its playhead line, in window pixels.
fn drawn(visual: &mut VisualTestContext) -> f32 {
    let head = visual
        .debug_bounds(intern("timeline-playhead"))
        .expect("the timeline draws its playhead");
    // The line stands 5 px into the 11 px head.
    f32::from(head.left()) + 5.0
}

/// Moves the clock past every glide and draws again.
fn settle(visual: &mut VisualTestContext) {
    visual.executor().advance_clock(Duration::from_secs(2));
    visual.update(|window, _| window.refresh());
    visual.run_until_parked();
}

#[gpui::test]
fn a_ruler_press_and_drag_draw_the_playhead_under_the_pointer_and_a_keyed_step_eases(
    cx: &mut gpui::TestAppContext,
) {
    let (state, visual, _) = shell_with_take(cx, "pointer-playhead");
    settle(visual);
    let ruler = visual
        .debug_bounds(intern("timeline-ruler"))
        .expect("the timeline draws its ruler");
    let at = |visual: &mut VisualTestContext| visual.read(|app| state.read(app).playhead.value);
    let start = at(visual);
    // A press lands on the tick at or before the pointer. The take is placed at
    // zero, so the sequence ends `end` px right of the ruler's left edge.
    let (tick_px, duration) = visual.read(|app| {
        let state = state.read(app);
        (
            crate::transport::TIMELINE_BASE_PPS * crate::transport::clamp_zoom(state.zoom)
                / state.store.sequence.ticks_per_second(),
            state.store.sequence.duration_ticks,
        )
    });
    let end = tick_px * duration as f32;
    assert!(
        end > 40.0,
        "the sequence is {end} px long, too short to drag in"
    );
    // Off the tick grid on purpose, so a press rounding to the nearest tick
    // instead of the one before reads as a line ahead of the pointer.
    let inside = |share: f32| point(ruler.left() + px(end * share + 0.4), ruler.center().y);
    let (press, drag) = (inside(0.3), inside(0.7));
    let mut last = start;
    for (gesture, pointer) in [("press", press), ("drag", drag)] {
        if gesture == "press" {
            visual.simulate_mouse_down(pointer, MouseButton::Left, Modifiers::none());
        } else {
            visual.simulate_mouse_move(pointer, MouseButton::Left, Modifiers::none());
        }
        visual.run_until_parked();
        let tick = at(visual);
        assert_ne!(tick, last, "the {gesture} moved the playhead");
        last = tick;
        let landed = drawn(visual);
        settle(visual);
        let target = drawn(visual);
        assert!(
            (landed - target).abs() < 0.5,
            "on the {gesture} the line drew at {landed} px and settled at {target} px: it \
             trailed the pointer"
        );
        let behind = f32::from(pointer.x) - target;
        assert!(
            (-0.5..tick_px + 0.5).contains(&behind),
            "the {gesture} at {} px put the line at {target} px, more than a tick of {tick_px} \
             px from the pointer",
            f32::from(pointer.x)
        );
    }

    // Past the sequence the playhead holds at its end, drawn there at once.
    let past = point(ruler.left() + px(end + 40.0), ruler.center().y);
    visual.simulate_mouse_move(past, MouseButton::Left, Modifiers::none());
    visual.run_until_parked();
    assert_eq!(at(visual), duration, "a drag past the end left the end");
    let landed = drawn(visual);
    settle(visual);
    let held = f32::from(ruler.left()) + end;
    assert!(
        (landed - held).abs() < 0.5 && (drawn(visual) - held).abs() < 0.5,
        "a drag past the end drew the line at {landed} px, not at the end's {held} px"
    );
    visual.simulate_mouse_up(past, MouseButton::Left, Modifiers::none());
    visual.run_until_parked();

    // A keyed step eases: the frame it lands on still draws where it was.
    let before = drawn(visual);
    change(visual, &state, |state| state.set_playhead(start));
    let landed = drawn(visual);
    settle(visual);
    let target = drawn(visual);
    assert!(
        (landed - target).abs() > 1.0,
        "the keyed step drew at its target on its first frame ({landed} px from {before} px): \
         the test cannot tell an eased move from one under the pointer"
    );
}
