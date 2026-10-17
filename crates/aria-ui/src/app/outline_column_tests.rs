//! The timeline's outline column takes its width from the settings, and a drag
//! on its right border sets it.
//!
//! The class closed here: a part of the timeline that measures the column on
//! its own. The content origin every pointer conversion reads and the lanes the
//! content draws in both take `UiSettings::outline_width`, so a column drawn at
//! one width and converted at another puts a click on the content beside the
//! frame it was aimed at. A drag holds the width to its range, Escape puts back
//! the width the drag started from, and a release saves the width, including a
//! release outside the window. A move past the window's edge still drags, and
//! the first pixel right of the column is the content's, not the band's.
//!
//! Not covered: the graph editor's column, which draws only while a property
//! row's graph is open. It reads the same setting.

use std::time::Duration;

use gpui::{Modifiers, MouseButton, VisualTestContext, point, px};

use crate::settings::{OUTLINE_DEFAULT, OUTLINE_MAX, OUTLINE_MIN, SAVE_DEBOUNCE_MS, UiSettings};

#[gpui::test]
fn the_outline_border_resizes_the_column_within_its_range(cx: &mut gpui::TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = crate::test_support::drawn_shell(cx, "outline-column");
    let width = |visual: &mut VisualTestContext| {
        visual.read(|app| state.read(app).ui_settings.outline_width)
    };
    let [x, y, _, h] = visual
        .read(|app| state.read(app).timeline_frame)
        .expect("the timeline records where it was drawn");
    // Where the content starts by the conversion, and by the drawn lanes.
    let lefts = |visual: &mut VisualTestContext| {
        let converted = visual.read(|app| state.read(app).timeline_content_left());
        let drawn = visual
            .debug_bounds("timeline-video-canvas")
            .map(|canvas| f32::from(canvas.left()));
        (converted, drawn)
    };
    let check = |visual: &mut VisualTestContext, want: f32, what: &str| {
        assert_eq!(width(visual), want, "{what}: the setting");
        let (converted, drawn) = lefts(visual);
        assert_eq!(converted, Some(x + want), "{what}: the conversion's origin");
        let drawn = drawn.unwrap_or_else(|| panic!("{what}: the video lanes are not drawn"));
        assert!(
            (drawn - (x + want)).abs() <= 1.0,
            "{what}: the lanes start at {drawn}, the column ends at {}",
            x + want
        );
    };
    check(visual, OUTLINE_DEFAULT, "at launch");

    // The column's last pixel, where its border draws.
    let edge = point(px(x + OUTLINE_DEFAULT - 1.0), px(y + h / 2.0));
    let press = |visual: &mut VisualTestContext| {
        visual.simulate_mouse_down(edge, MouseButton::Left, Modifiers::none());
        visual.run_until_parked();
    };
    let drag = |visual: &mut VisualTestContext, by: f32| {
        visual.simulate_mouse_move(
            point(edge.x + px(by), edge.y),
            MouseButton::Left,
            Modifiers::none(),
        );
        visual.run_until_parked();
    };

    // A press on the content's first pixel is the content's: moving from it
    // leaves the column as it was.
    let content = point(px(x + OUTLINE_DEFAULT), edge.y);
    visual.simulate_mouse_down(content, MouseButton::Left, Modifiers::none());
    visual.simulate_mouse_move(
        point(content.x + px(60.0), content.y),
        MouseButton::Left,
        Modifiers::none(),
    );
    visual.simulate_mouse_up(content, MouseButton::Left, Modifiers::none());
    visual.run_until_parked();
    check(
        visual,
        OUTLINE_DEFAULT,
        "a drag from the content's first pixel",
    );

    press(visual);
    // Past the window's left edge, where the platform still reports a held
    // pointer and nothing in the window is under it.
    let past_left = -20.0 - f32::from(edge.x);
    for (by, want) in [
        (60.0, OUTLINE_DEFAULT + 60.0),
        (past_left, OUTLINE_MIN),
        (400.0, OUTLINE_MAX),
    ] {
        drag(visual, by);
        check(visual, want, &format!("a drag of {by}px"));
    }
    visual.simulate_keystrokes("escape");
    visual.run_until_parked();
    check(visual, OUTLINE_DEFAULT, "Escape");
    visual.simulate_mouse_up(edge, MouseButton::Left, Modifiers::none());
    visual.run_until_parked();
    drag(visual, 90.0);
    check(visual, OUTLINE_DEFAULT, "a move after Escape");

    // Let go left of the window: the drag ends there and the width is saved.
    press(visual);
    drag(visual, 60.0);
    visual.simulate_mouse_up(
        point(px(-10.), edge.y),
        MouseButton::Left,
        Modifiers::none(),
    );
    visual.run_until_parked();
    drag(visual, 120.0);
    check(visual, OUTLINE_DEFAULT + 60.0, "a move after the release");
    visual
        .executor()
        .advance_clock(Duration::from_millis(SAVE_DEBOUNCE_MS + 1));
    visual.run_until_parked();
    let dir = visual.read(|app| state.read(app).data_dir.clone());
    assert_eq!(
        UiSettings::load(&dir).outline_width,
        OUTLINE_DEFAULT + 60.0,
        "the next launch draws the column at the width it was let go at"
    );
}
