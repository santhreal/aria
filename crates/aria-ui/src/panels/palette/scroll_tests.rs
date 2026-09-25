//! The highlighted palette row is on screen after every key that moves it.
//!
//! The class closed here: a highlight that moves while the list stays where it
//! is. Down past the last visible row walked the highlight off the bottom of the
//! card, and Up from the first row highlighted the last row, out of view.
//!
//! Each case drives the shell's own key handler in a window that shows a few
//! rows of a longer list, and after every key reads the frame just drawn: the
//! highlighted row lies inside the card's scrolling region. The keys are every
//! one that moves the highlight: Down, Up, a typed character, Backspace, and
//! closing and reopening the palette.
//!
//! Not covered: the pointer wheel, which scrolls the list and leaves the
//! highlight alone; a paste, which types through the same entry point as a
//! typed character; and pixels.

use gpui::{Bounds, Entity, Pixels, Size, TestAppContext, VisualTestContext, px, size};

use crate::app::{AppState, DialogKind};
use crate::test_support::{bounds_inside, drawn_shell};

/// A window that shows a few palette rows and not the whole list.
const SHORT: Size<Pixels> = size(px(480.), px(320.));

/// The shell in a short window with the palette open on its first hit, drawn
/// with motion off so every frame shows the settled layout.
fn open_palette<'a>(
    cx: &'a mut TestAppContext,
    tag: &str,
) -> (Entity<AppState>, &'a mut VisualTestContext) {
    let (state, visual) = drawn_shell(cx, tag);
    visual.update(|_, app| app.set_reduce_motion(true));
    visual.simulate_resize(SHORT);
    press(visual, "ctrl-shift-p");
    assert_eq!(
        visual.read(|app| state.read(app).dialog),
        DialogKind::Palette,
        "Ctrl+Shift+P did not open the palette"
    );
    expect_shown(&state, visual, 0, "on open");
    (state, visual)
}

fn press(visual: &mut VisualTestContext, keys: &str) {
    visual.simulate_keystrokes(keys);
    visual.run_until_parked();
}

fn hit_count(state: &Entity<AppState>, visual: &mut VisualTestContext) -> usize {
    visual.read(|app| state.read(app).palette_hits().len())
}

/// Fails unless hit `want` is highlighted and its row lies inside the scrolling
/// region on the frame just drawn. Also fails when the whole list fits the
/// region, since every row is then in view whether or not anything scrolls.
fn expect_shown(
    state: &Entity<AppState>,
    visual: &mut VisualTestContext,
    want: usize,
    after: &str,
) {
    let count = hit_count(state, visual);
    let highlight = visual.read(|app| state.read(app).palette_highlight());
    assert_eq!(
        highlight,
        Some(want),
        "{after}: the highlight is not on hit {want} of {count}"
    );
    let region: Bounds<Pixels> = visual
        .debug_bounds("dialog-content")
        .unwrap_or_else(|| panic!("{after}: the palette drew no scrolling region"));
    let row = visual
        .debug_bounds("palette-highlight")
        .unwrap_or_else(|| panic!("{after}: no row is drawn highlighted"));
    // Rows are one height, and the gaps between them only add to the total.
    assert!(
        row.size.height * count as f32 > region.size.height,
        "{after}: {count} rows of {:?} fit the region {region:?}; the window shows the whole list",
        row.size.height
    );
    assert!(
        bounds_inside(row, region),
        "{after}: hit {want} of {count} is drawn at {row:?}, outside the region {region:?}"
    );
}

#[gpui::test]
fn every_row_the_arrow_keys_reach_is_shown(cx: &mut TestAppContext) {
    let (state, visual) = open_palette(cx, "palette-walk");
    let count = hit_count(&state, visual);
    // Down to the last row, then once more wraps to the first.
    for step in 1..=count {
        press(visual, "down");
        expect_shown(&state, visual, step % count, &format!("Down #{step}"));
    }
    // Up from the first row wraps to the last, then walks back to the first.
    for step in 1..=count {
        press(visual, "up");
        expect_shown(&state, visual, count - step, &format!("Up #{step}"));
    }
}

#[gpui::test]
fn typing_or_deleting_returns_the_list_to_the_first_hit(cx: &mut TestAppContext) {
    let (state, visual) = open_palette(cx, "palette-type");
    press(visual, "up");
    let last = hit_count(&state, visual) - 1;
    expect_shown(&state, visual, last, "Up from the first hit");
    press(visual, "e");
    expect_shown(&state, visual, 0, "typing 'e' at the last hit");
    press(visual, "up");
    let last = hit_count(&state, visual) - 1;
    expect_shown(&state, visual, last, "Up from the first hit for 'e'");
    press(visual, "backspace");
    expect_shown(&state, visual, 0, "Backspace at the last hit");
}

#[gpui::test]
fn reopening_the_palette_starts_at_the_first_hit(cx: &mut TestAppContext) {
    let (state, visual) = open_palette(cx, "palette-reopen");
    press(visual, "up");
    let last = hit_count(&state, visual) - 1;
    expect_shown(&state, visual, last, "Up from the first hit");
    press(visual, "escape");
    assert_eq!(
        visual.read(|app| state.read(app).dialog),
        DialogKind::None,
        "Escape did not close the palette"
    );
    press(visual, "ctrl-shift-p");
    expect_shown(&state, visual, 0, "reopening after Up");
}
