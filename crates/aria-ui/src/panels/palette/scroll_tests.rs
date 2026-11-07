//! The highlighted palette row is on screen after every key that moves it, and
//! the query field stands still while the list changes under it.
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
//! A second case types a query with a few hits, then one with none, and reads
//! the header holding the query field after each: it stands where it stood
//! over the full list.
//!
//! A third case scrolls the list by the wheel, then types: the first hit is in
//! view under the new query.
//!
//! Not covered: a wheel scroll by itself, which moves the list and leaves the
//! highlight alone; a paste, which types through the same entry point as a
//! typed character; and pixels.

use gpui::{
    Bounds, Entity, Modifiers, Pixels, ScrollDelta, ScrollWheelEvent, Size, TestAppContext,
    TouchPhase, VisualTestContext, point, px, size,
};

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

/// Down to the last row, then once more wraps to the first.
#[gpui::test]
fn every_row_the_down_key_reaches_is_shown(cx: &mut TestAppContext) {
    let (state, visual) = open_palette(cx, "palette-walk-down");
    let count = hit_count(&state, visual);
    for step in 1..=count {
        press(visual, "down");
        expect_shown(&state, visual, step % count, &format!("Down #{step}"));
    }
}

/// Up from the first row wraps to the last, then walks back to the first.
#[gpui::test]
fn every_row_the_up_key_reaches_is_shown(cx: &mut TestAppContext) {
    let (state, visual) = open_palette(cx, "palette-walk-up");
    let count = hit_count(&state, visual);
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

/// WHY: a list kept its place across a query that changed its hits, so a query
/// typed after the wheel scrolled the list highlighted the first hit while the
/// list stood rows past it. Each query draws its hits in a list of its own,
/// which stands at its top.
#[gpui::test]
fn typing_after_the_wheel_scrolled_the_list_shows_the_first_hit(cx: &mut TestAppContext) {
    let (state, visual) = open_palette(cx, "palette-wheel");
    let region = visual
        .debug_bounds("dialog-content")
        .expect("the palette drew no scrolling region");
    let position = region.center();
    visual.simulate_mouse_move(position, None, Modifiers::none());
    visual.simulate_event(ScrollWheelEvent {
        position,
        delta: ScrollDelta::Pixels(point(px(0.0), px(-2.0 * f32::from(region.size.height)))),
        modifiers: Modifiers::none(),
        touch_phase: TouchPhase::Moved,
    });
    visual.run_until_parked();
    assert!(
        visual.debug_bounds("palette-highlight").is_none(),
        "the wheel left the first hit in view, so typing after it reads nothing"
    );
    press(visual, "e");
    expect_shown(
        &state,
        visual,
        0,
        "typing 'e' after the wheel scrolled the list",
    );
}

/// WHY: the card stood centered in the window, so each key that changed how
/// many rows matched moved the query field being typed in up or down. The
/// header holding the field stands where it stood over the full list, through
/// a query with a few hits and one with none.
#[gpui::test]
fn the_query_field_stands_still_as_the_list_changes(cx: &mut TestAppContext) {
    let (state, visual) = open_palette(cx, "palette-anchor");
    let header = |visual: &mut VisualTestContext| {
        visual
            .debug_bounds("dialog-header")
            .expect("the palette drew no header")
    };
    let full = (hit_count(&state, visual), header(visual));
    let mut counts = vec![full.0];
    for keys in ["h i s t o r y", "z z z z"] {
        press(visual, keys);
        let count = hit_count(&state, visual);
        counts.push(count);
        assert_eq!(
            header(visual),
            full.1,
            "the query field moved when {count} rows matched instead of {}",
            full.0
        );
    }
    assert!(
        counts[0] > counts[1] && counts[1] > 0 && counts[2] == 0,
        "the queries did not narrow the list to a few rows, then none: {counts:?}"
    );
}
