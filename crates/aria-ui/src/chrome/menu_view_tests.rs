//! The keyboard focus ring on the menu bar's drops.
//!
//! The class closed here: keyboard focus with nothing drawn to show where it
//! is. F10 and the arrows lit a menu row and drew it like a row under the
//! pointer, so the row Enter runs looked like any hovered row. The lit row
//! draws the 1 px accent ring, the ring follows the arrows into a flyout, and
//! a drop the pointer opened draws none.
//!
//! Each case drives the shell's own key handler in a drawn window and reads
//! the frame just drawn.
//!
//! Not covered: the ring's colour and width, and focus outside the menu bar.

use gpui::{Bounds, Pixels, TestAppContext, VisualTestContext};

use crate::chrome::menu_items::{DropEntry, drop_entries};
use crate::test_support::drawn_shell;
use crate::types::AppMenu;

fn press(visual: &mut VisualTestContext, keys: &str) {
    visual.simulate_keystrokes(keys);
    visual.run_until_parked();
}

fn ring(visual: &mut VisualTestContext) -> Option<Bounds<Pixels>> {
    visual.debug_bounds("focus-ring")
}

#[gpui::test]
fn the_keyboard_lit_row_draws_the_focus_ring(cx: &mut TestAppContext) {
    let (state, visual) = drawn_shell(cx, "menu-focus-ring");
    visual.update(|_, app| app.set_reduce_motion(true));
    press(visual, "f10");
    let first = ring(visual).expect("F10 lit the first row and drew no ring");
    press(visual, "down");
    let second = ring(visual).expect("Down lit the next row and drew no ring");
    assert!(
        second.origin.y > first.origin.y,
        "the ring did not follow Down: {first:?} then {second:?}"
    );
    press(visual, "escape");
    assert_eq!(visual.read(|app| state.read(app).open_menu), None);
    assert_eq!(ring(visual), None, "a closed drop drew a ring");

    // A title click opens its drop with nothing lit.
    let menu = AppMenu::ALL[0];
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.toggle_menu(menu);
            cx.notify();
        });
    });
    visual.run_until_parked();
    assert_eq!(visual.read(|app| state.read(app).open_menu), Some(menu));
    assert_eq!(ring(visual), None, "a drop the pointer opened drew a ring");
}

/// Right on a submenu hands the ring to the flyout's first row; Left gives it
/// back to the submenu entry.
#[gpui::test]
fn the_ring_follows_the_keyboard_into_a_flyout(cx: &mut TestAppContext) {
    let (state, visual) = drawn_shell(cx, "menu-focus-flyout");
    visual.update(|_, app| app.set_reduce_motion(true));
    let (menu, at) = visual.read(|app| {
        let state = state.read(app);
        AppMenu::ALL
            .into_iter()
            .find_map(|menu| {
                let rows = state.menu_rows(menu);
                drop_entries(&rows)
                    .iter()
                    .position(|entry| matches!(entry, DropEntry::Sub { .. }))
                    .map(|at| (menu, at))
            })
            .expect("some menu holds a submenu")
    });
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.toggle_menu(menu);
            state.menu_row = Some(at);
            cx.notify();
        });
    });
    visual.run_until_parked();
    let entry = ring(visual).expect("the lit submenu entry drew no ring");
    press(visual, "right");
    assert_eq!(
        visual.read(|app| state.read(app).menu_flyout),
        Some((at, Some(0))),
        "Right did not enter the flyout"
    );
    let child = ring(visual).expect("the flyout's lit row drew no ring");
    assert!(
        child.origin.x > entry.origin.x,
        "the ring stayed on the submenu entry: {entry:?} then {child:?}"
    );
    press(visual, "left");
    assert_eq!(
        ring(visual),
        Some(entry),
        "Left did not give the ring back to the submenu entry"
    );
}
