//! Every context menu opens whole inside the window, against the pointer.
//!
//! The class closed here: a menu placed with its top left corner at the
//! pointer and held off the window's foot and right edge by a guess of its
//! size ran past the window whenever it was larger than the guess, as the
//! lane menu opened on a header near the foot did, its last rows cut.
//!
//! The menu of every target a dressed timeline offers is opened at each
//! corner of the smallest window and at its middle. The card lies inside the
//! window. On each axis its near edge stands at the pointer where the card
//! fits after the pointer, and its far edge where it fits only before it. The
//! card is as tall as the same menu opened at the window's top left, so no row
//! scrolls out of a window that has room for every row.
//!
//! Not covered: a menu taller than the window, whose rows scroll, and the
//! frost's pixels, which the test platform does not raster on Linux.

use gpui::{Bounds, TestAppContext, point, px, size};

use super::grid_tests::dress;
use crate::settings::{MIN_WINDOW_HEIGHT, MIN_WINDOW_WIDTH};
use crate::test_support::drawn_shell_at;

/// The placement rounds to whole pixels.
const EPS: f32 = 1.0;

/// How far inside the window's edges the pointer goes down.
const INSET: f32 = 4.0;

/// Whether a card `length` long stands right against the pointer at `at` on
/// an axis `extent` long, its near edge at `near` and its far edge at `far`.
fn against_pointer(at: f32, length: f32, extent: f32, near: f32, far: f32) -> bool {
    if at + length <= extent {
        (near - at).abs() <= EPS
    } else if at - length >= 0.0 {
        (far - at).abs() <= EPS
    } else {
        true
    }
}

#[gpui::test]
fn every_context_menu_opens_whole_inside_the_window(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (w, h) = (MIN_WINDOW_WIDTH, MIN_WINDOW_HEIGHT);
    let at = Bounds::new(point(px(0.), px(0.)), size(px(w), px(h)));
    let (state, mut visual) = drawn_shell_at(cx, "ctx-place", at);
    let visual = &mut visual;
    visual.update(|_, app| app.set_reduce_motion(true));
    let targets = visual.update(|_, app| {
        state.update(app, |state, cx| {
            let targets = dress(state);
            cx.notify();
            targets
        })
    });
    let pointers = [
        (INSET, INSET),
        (w - INSET, INSET),
        (INSET, h - INSET),
        (w - INSET, h - INSET),
        (w / 2.0, h / 2.0),
    ];
    let mut faults = Vec::new();
    for target in targets {
        let mut top_left_height = None;
        for (x, y) in pointers {
            visual.update(|_, app| {
                state.update(app, |state, cx| {
                    state.close_context();
                    state.open_context(target.clone(), x, y);
                    cx.notify();
                })
            });
            visual.run_until_parked();
            let card = visual
                .debug_bounds("context-menu-card")
                .unwrap_or_else(|| panic!("the menu on {target:?} drew no card"));
            let [left, top, right, bottom] =
                [card.left(), card.top(), card.right(), card.bottom()].map(f32::from);
            let (width, height) = (right - left, bottom - top);
            let opened = format!("the menu on {target:?} opened at ({x}, {y})");
            if left < -EPS || top < -EPS || right > w + EPS || bottom > h + EPS {
                faults.push(format!(
                    "{opened}: its card {card:?} runs past the {w} x {h} window"
                ));
                continue;
            }
            let first = *top_left_height.get_or_insert(height);
            if (height - first).abs() > EPS {
                faults.push(format!(
                    "{opened}: its card is {height} tall, and {first} opened at the top left"
                ));
            }
            if !against_pointer(x, width, w, left, right) {
                faults.push(format!(
                    "{opened}: its card spans x {left}..{right}, not against the pointer"
                ));
            }
            if !against_pointer(y, height, h, top, bottom) {
                faults.push(format!(
                    "{opened}: its card spans y {top}..{bottom}, not against the pointer"
                ));
            }
        }
    }
    assert!(faults.is_empty(), "{}", faults.join("\n"));
}
