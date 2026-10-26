//! The Speed tab's ramp drawn in a window, pressed on a point and dragged
//! through the window's own events.

use aria_models::{MAX_SPEED, SPEED_CURVE_SCALE};
use gpui::{Modifiers, MouseButton, Pixels, Point, TestAppContext, VisualTestContext, point, px};

use super::graph_tests::scrolled_to;
use super::strip_tests::drawn_with;
use crate::app::AppState;
use crate::app::clip_speed::{ramp_tick_to_frac, ramp_value_to_frac, speed_presets};
use crate::surfaces::clip_pick::{self, Pick};

/// Where the drag in flight holds its point, `(tick, multiplier)`, or `None`
/// with no ramp drag in flight.
fn held(state: &gpui::Entity<AppState>, visual: &VisualTestContext) -> Option<(i64, f32)> {
    visual.read(|app| {
        state
            .read(app)
            .ramp_dragging()
            .map(|drag| (drag.tick, drag.value))
    })
}

/// The ramp's points as the graph draws them, `(tick, multiplier)`.
fn points(state: &gpui::Entity<AppState>, visual: &VisualTestContext) -> Vec<(i64, f32)> {
    visual.read(|app| state.read(app).selected_ramp_curve())
}

/// Put a point at `tick` at unit speed and return the window point it is drawn
/// at, read through the rectangle the graph recorded as it drew.
fn seeded(
    state: &gpui::Entity<AppState>,
    visual: &mut VisualTestContext,
    tick: i64,
) -> Point<Pixels> {
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state
                .set_selected_ramp_point(tick, 1.0)
                .expect("put a point on the ramp");
            cx.notify();
        });
    });
    visual.run_until_parked();
    let [left, top, width, height] = visual
        .read(|app| state.read(app).ramp_frame)
        .expect("the graph records the rectangle it is drawn in");
    point(
        px(left + ramp_tick_to_frac(tick) * width),
        px(top + (1.0 - ramp_value_to_frac(1.0)) * height),
    )
}

/// A ramp point dragged off the graph keeps following the pointer, held to
/// the ramp's span and to the speed bounds, and a release outside the graph,
/// or outside the window, writes it and ends the drag.
///
/// Closes the class "a drag held by the element it started on" for the ramp:
/// the graph took the pointer's moves and its release only while the pointer
/// was over it, so a drag that left it stopped where it left, and a release
/// outside it left the point grabbed with the button up. Past the right edge
/// the point stops one tick short of the ramp's end, which is pinned. A
/// release outside the window reaches the window as a move with no button
/// held.
#[gpui::test]
async fn a_ramp_point_dragged_off_the_graph_follows_the_pointer_and_lands_on_its_release(
    cx: &mut TestAppContext,
) {
    let preset = speed_presets()
        .first()
        .expect("the model ships a speed preset");
    let (state, visual) = drawn_with(cx, "ramp-drag-out", "texture.png", "speed", |state| {
        clip_pick::apply(state, Pick::SpeedPreset(preset.id.to_string())).expect("ramp the clip");
    });
    let graph = scrolled_to(visual, "speed-graph");

    let middle = SPEED_CURVE_SCALE / 2;
    let at = seeded(&state, visual, middle);
    visual.simulate_mouse_move(at, None, Modifiers::none());
    visual.simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
    assert_eq!(
        held(&state, visual).map(|(tick, _)| tick),
        Some(middle),
        "a press on the point drawn at {at:?} grabs no point"
    );
    let off = point(graph.right() + px(40.0), graph.top() - px(30.0));
    visual.simulate_mouse_move(off, MouseButton::Left, Modifiers::none());
    let last = SPEED_CURVE_SCALE - 1;
    assert!(
        held(&state, visual)
            .is_some_and(|(tick, value)| tick == last && (value - MAX_SPEED).abs() < 1e-3),
        "the pointer past the graph's top right corner holds the point at {:?}, not one tick \
         short of the end at the fastest speed",
        held(&state, visual)
    );
    visual.simulate_mouse_up(off, MouseButton::Left, Modifiers::none());
    visual.run_until_parked();
    assert_eq!(
        held(&state, visual),
        None,
        "a release outside the graph left the point grabbed"
    );
    let written = points(&state, visual);
    assert!(
        written
            .iter()
            .any(|&(tick, value)| tick == last && (value - MAX_SPEED).abs() < 1e-3)
            && !written.iter().any(|&(tick, _)| tick == middle),
        "a release outside the graph wrote {written:?}"
    );

    let quarter = SPEED_CURVE_SCALE / 4;
    let at = seeded(&state, visual, quarter);
    visual.simulate_mouse_move(at, None, Modifiers::none());
    visual.simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
    let down = at + point(px(0.0), px(40.0));
    visual.simulate_mouse_move(down, MouseButton::Left, Modifiers::none());
    let (tick, value) = held(&state, visual).expect("the second drag outlived its move");
    assert!(
        value < 1.0,
        "40 px down the graph holds the point at {value}x"
    );
    visual.simulate_mouse_move(down, None, Modifiers::none());
    visual.run_until_parked();
    assert_eq!(
        held(&state, visual),
        None,
        "a move with the button up left the point grabbed"
    );
    let written = points(&state, visual);
    assert!(
        written
            .iter()
            .any(|&(at, speed)| at == tick && (speed - value).abs() < 1e-4),
        "a release outside the window wrote {written:?} for the ({tick}, {value}) the drag held"
    );
}
