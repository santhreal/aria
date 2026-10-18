//! A window at rest draws no frames.
//!
//! The class closed here is a draw loop: a frame that leaves the window dirty,
//! so gpui draws the next one for no change, without end. The loop found was a
//! control that moves under its own hover: a media tile rises 2 px as it
//! lights, and with the pointer within those 2 px of its lower edge every frame
//! flipped its hover and refreshed the window.
//!
//! Every panel the registry holds is docked in turn, and the pointer rests at
//! points read from what the panel draws: the middle of each text run, then
//! one pixel inside and one pixel outside each edge of every quad the hover
//! lit. After each rest the window may draw a bounded number of frames (the
//! move, the hover's redraw, a deferred notification), and once it settles,
//! further ticks of the clock draw none.
//!
//! Not caught: a loop driven only by a next-frame callback (the test executor
//! runs none), and a hover control with no text run inside it that draws
//! nothing on hover.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::Duration;

use gpui::{Bounds, Modifiers, MouseButton, Pixels, Point, TestAppContext, VisualTestContext};

use crate::motion::frames;

use super::tests::{dock_right, opened};

/// Frames opening a window and docking one panel may draw before it counts as
/// a loop.
const SETUP_FRAMES: usize = 400;

/// Frames one rest may draw before it counts as a loop.
const REST_FRAMES: usize = 8;

/// Clock ticks after a rest that must draw nothing.
const IDLE_TICKS: usize = 3;

/// Points tried per panel and phase, spread over what the panel draws.
const POINTS: usize = 48;

/// The quads drawn in the last frame, in logical pixels.
fn quads(visual: &mut VisualTestContext) -> Vec<Bounds<Pixels>> {
    visual.update(|window, _| {
        let scale = window.scale_factor();
        window
            .painted_quads()
            .iter()
            .map(|quad| {
                let b = quad.bounds;
                Bounds::new(
                    gpui::point(
                        gpui::px(b.origin.x.0 / scale),
                        gpui::px(b.origin.y.0 / scale),
                    ),
                    gpui::size(
                        gpui::px(b.size.width.0 / scale),
                        gpui::px(b.size.height.0 / scale),
                    ),
                )
            })
            .collect()
    })
}

/// The middle of each text run drawn inside `rect`.
fn run_middles(visual: &mut VisualTestContext, rect: Bounds<Pixels>) -> Vec<Point<Pixels>> {
    let mut middles: Vec<Point<Pixels>> = visual.update(|window, _| {
        window
            .rendered_text_runs()
            .iter()
            .map(|run| run.bounds.center())
            .filter(|middle| rect.contains(middle))
            .collect()
    });
    middles.sort_by(|a, b| (a.y, a.x).partial_cmp(&(b.y, b.x)).expect("finite"));
    middles.dedup();
    middles
}

/// Up to [`POINTS`] of `all`, evenly spread.
fn spread<T: Copy>(all: &[T]) -> Vec<T> {
    let step = all.len().div_ceil(POINTS).max(1);
    all.iter().step_by(step).copied().collect()
}

/// One pixel inside and one outside the middle of each edge of `quad`.
fn edges(quad: Bounds<Pixels>) -> [Point<Pixels>; 8] {
    let one = gpui::px(1.0);
    let (left, right) = (quad.left(), quad.right());
    let (top, bottom) = (quad.top(), quad.bottom());
    let (x, y) = (quad.center().x, quad.center().y);
    [
        gpui::point(x, top + one),
        gpui::point(x, top - one),
        gpui::point(x, bottom - one),
        gpui::point(x, bottom + one),
        gpui::point(left + one, y),
        gpui::point(left - one, y),
        gpui::point(right - one, y),
        gpui::point(right + one, y),
    ]
}

/// Runs `f` with at most `limit` frames drawn; a further frame is reported as
/// a draw loop instead of hanging the run. A loop inside one effect flush
/// never returns to the test, so no phase runs without a ceiling.
fn bounded<R>(limit: usize, f: impl FnOnce() -> R) -> Result<R, String> {
    frames::allow(Some(limit));
    let ran = catch_unwind(AssertUnwindSafe(f));
    frames::allow(None);
    ran.map_err(|panic| {
        panic
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| panic.downcast_ref::<&str>().map(|s| (*s).to_owned()))
            .unwrap_or_default()
    })
}

/// Rests the pointer at `at`: the frames the rest draws are bounded, and once
/// the window settles, [`IDLE_TICKS`] ticks draw none.
fn rest(visual: &mut VisualTestContext, at: Point<Pixels>) -> Result<(), String> {
    bounded(REST_FRAMES, || {
        visual.simulate_mouse_move(at, None::<MouseButton>, Modifiers::none());
        visual.run_until_parked();
        visual.executor().advance_clock(Duration::from_secs(1));
        visual.run_until_parked();
    })?;
    bounded(0, || {
        for _ in 0..IDLE_TICKS {
            visual.executor().advance_clock(Duration::from_millis(16));
            visual.run_until_parked();
        }
    })
    .map_err(|why| format!("drawn at rest: {why}"))
}

#[gpui::test]
fn a_resting_pointer_leaves_every_panel_idle(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let panels = bounded(SETUP_FRAMES, || {
        let (state, visual) = opened(cx, "panel-idle");
        let panels = visual.read(|app| state.read(app).registered_panel_ids());
        visual.update(|window, _| window.remove_window());
        panels
    })
    .unwrap_or_else(|why| panic!("opening the census window: {why}"));
    assert!(!panels.is_empty(), "the ui plugin registers panels");

    let mut looped = Vec::new();
    let mut lit_any = false;
    for (index, &panel) in panels.iter().enumerate() {
        let docked = bounded(SETUP_FRAMES, || {
            let (state, visual) = opened(cx, &format!("panel-idle-{index}"));
            let rect = dock_right(&state, visual, panel);
            (visual, rect)
        });
        let (visual, rect) = match docked {
            Ok((visual, Some(rect))) => (visual, rect),
            Ok((_, None)) => {
                looped.push(format!("{panel}: the right frame was not drawn"));
                continue;
            }
            Err(why) => {
                looped.push(format!("{panel} while docking: {why}"));
                continue;
            }
        };
        let unlit = quads(visual);
        let mut lit: Vec<Bounds<Pixels>> = Vec::new();
        let mut failed = None;
        for at in spread(&run_middles(visual, rect)) {
            if let Err(why) = rest(visual, at) {
                failed = Some((at, why));
                break;
            }
            for quad in quads(visual) {
                let control = quad.size.height <= gpui::px(200.0) && rect.contains(&quad.origin);
                if control && !unlit.contains(&quad) && !lit.contains(&quad) {
                    lit.push(quad);
                }
            }
        }
        lit_any |= !lit.is_empty();
        if failed.is_none() {
            let points: Vec<Point<Pixels>> = spread(&lit).into_iter().flat_map(edges).collect();
            for at in points {
                if let Err(why) = rest(visual, at) {
                    failed = Some((at, why));
                    break;
                }
            }
        }
        if let Some((at, why)) = failed {
            // A panic inside a draw leaves this window unusable; the next
            // panel opens its own.
            looped.push(format!("{panel} with the pointer at {at:?}: {why}"));
            continue;
        }
        visual.update(|window, _| window.remove_window());
    }
    assert!(
        lit_any,
        "no rest lit a hover anywhere, so the edges of a lit control were never tried"
    );
    assert!(
        looped.is_empty(),
        "windows that kept drawing:\n{}",
        looped.join("\n")
    );
}
