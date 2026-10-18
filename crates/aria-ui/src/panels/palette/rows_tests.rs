//! The palette's rows stand at a menu's pitch.
//!
//! The class closed here: a list of commands drawn at its own height. The
//! palette offers the menus' rows filtered, and drew them eight pixels taller
//! with a gap between, so the same command read at two pitches.
//!
//! The highlight is walked down the list and read after every step, so every
//! row it passes is measured. Not covered: rows past the first screen, which
//! the scrolling test reaches.

use gpui::{TestAppContext, px};

use crate::test_support::drawn_shell;
use crate::theme::Theme;

const EPS: f32 = 0.51;
const WALK: usize = 6;

#[gpui::test]
fn palette_rows_stand_at_the_menu_pitch(cx: &mut TestAppContext) {
    let (_state, visual) = drawn_shell(cx, "palette-rows");
    visual.update(|_, app| app.set_reduce_motion(true));
    visual.simulate_keystrokes("ctrl-shift-p");
    visual.run_until_parked();
    let mut rows = Vec::with_capacity(WALK);
    for _ in 0..WALK {
        let row = visual
            .debug_bounds("palette-highlight")
            .expect("the palette draws its highlighted row");
        rows.push(row);
        visual.simulate_keystrokes("down");
        visual.run_until_parked();
    }
    for (at, row) in rows.iter().enumerate() {
        assert!(
            (f32::from(row.size.height) - Theme::ROW_HEIGHT).abs() < EPS,
            "row {at} is {:?} tall, not one row",
            row.size.height
        );
    }
    for (at, pair) in rows.windows(2).enumerate() {
        let step = pair[1].top() - pair[0].top();
        assert!(
            (step - px(Theme::ROW_HEIGHT)).abs() < px(EPS),
            "row {} follows row {at} by {step:?}",
            at + 1
        );
    }
}
