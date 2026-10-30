//! A cached chrome part draws what a full redraw draws, and a change builds
//! the chrome parts that draw it and no others.
//!
//! The editor bar and each dock frame's tab row are [`ChromePart`]s: cached
//! views keyed by what they are drawn from besides the app state. The class
//! closed here is a part that replays a stale drawing, because a change it
//! draws did not notify it or because its key leaves out an input, and a
//! change that rebuilds parts it did not reach. Each change is made the way
//! the app makes it, through a click, a pointer move, a resize or a state
//! notification, and the frame then drawn must equal the frame drawn after a
//! refresh, which builds every part anew. Every change must also alter the
//! frame, so a case cannot pass by drawing nothing new.
//!
//! The changes a panel body draws are compared over the whole window, chrome
//! included, by the tests of [`super`]. Not caught: a part stale only in an
//! image or a glyph, and a floating window's tab row.
//!
//! [`ChromePart`]: super::ChromePart

use std::time::Duration;

use gpui::{Entity, Pixels, Point, Size, TestAppContext, VisualTestContext};

use crate::app::AppState;
use crate::dock::Frame;
use crate::shell::Moved;

use super::builds::{self, Part};
use super::tests::{Change, Drawing, drawing, make, moving, opened, shell_of};

/// The frames the layout docks, as the state holds them.
fn docked(state: &Entity<AppState>, visual: &mut VisualTestContext) -> Vec<Frame> {
    visual.read(|app| {
        state
            .read(app)
            .dock_tree
            .frames()
            .into_iter()
            .cloned()
            .collect()
    })
}

/// The centre of what `selector` painted in the last frame.
fn centre(visual: &mut VisualTestContext, selector: &str) -> Point<Pixels> {
    visual
        .debug_bounds(crate::interned::intern(selector))
        .unwrap_or_else(|| panic!("the last frame painted no {selector}"))
        .center()
}

fn click(visual: &mut VisualTestContext, selector: &str) {
    let at = centre(visual, selector);
    visual.simulate_click(at, gpui::Modifiers::none());
    visual.run_until_parked();
}

fn hover(visual: &mut VisualTestContext, at: Point<Pixels>) {
    visual.simulate_mouse_move(at, None::<gpui::MouseButton>, gpui::Modifiers::none());
    visual.run_until_parked();
}

/// Delivers the frames the last draw requested until none is requested, as
/// the platform's frame loop does, with the clock moving on between them so an
/// animation lands. Fails when the window still requests frames after 16.
fn settle(visual: &mut VisualTestContext) {
    for _ in 0..16 {
        visual.executor().advance_clock(Duration::from_millis(100));
        let ran = visual.update(|window, cx| window.simulate_next_frame(cx));
        visual.run_until_parked();
        if ran == 0 {
            return;
        }
    }
    panic!("the window still requests a frame after 16 frames");
}

/// The parts built since the last call, each once, in order.
fn parts_built() -> Vec<Part> {
    let mut built = builds::take_parts();
    built.sort_unstable();
    built.dedup();
    built
}

/// The left end of the active tab of `frame`, clear of its close control.
fn active_tab_start(visual: &mut VisualTestContext, frame: &Frame) -> Point<Pixels> {
    let selector = format!("dock-tab-{}", frame.active_panel());
    let tab = visual
        .debug_bounds(crate::interned::intern(&selector))
        .unwrap_or_else(|| panic!("the last frame painted no {selector}"));
    gpui::point(tab.origin.x + gpui::px(4.0), tab.center().y)
}

/// A change to the chrome.
#[derive(Clone, Copy, Debug)]
enum ChromeChange {
    /// A click on a tool the strip does not have selected.
    Tool,
    /// A click on a dock tab that is not its frame's active one.
    Tab,
    /// A click on the File title, which opens its drop.
    MenuOpen,
    /// A second click on the File title, which closes it.
    MenuClose,
    /// The pointer onto the close control of a frame's active tab.
    CloseHover,
    /// The pointer off the close control, onto its tab.
    CloseRest,
    /// A frame maximized.
    Maximize,
    /// The maximized frame restored.
    Restore,
    /// The window made narrower, so each frame draws its row at a new width.
    Narrower,
    /// The window back at the width it opened at.
    Wider,
}

const CHROME_CHANGES: [ChromeChange; 10] = [
    ChromeChange::Tool,
    ChromeChange::Tab,
    ChromeChange::MenuOpen,
    ChromeChange::MenuClose,
    ChromeChange::CloseHover,
    ChromeChange::CloseRest,
    ChromeChange::Maximize,
    ChromeChange::Restore,
    ChromeChange::Narrower,
    ChromeChange::Wider,
];

/// Makes `change` in a window that opened at `opened_at`.
fn make_chrome(
    state: &Entity<AppState>,
    visual: &mut VisualTestContext,
    change: ChromeChange,
    opened_at: Size<Pixels>,
) {
    let first = docked(state, visual)
        .into_iter()
        .next()
        .expect("the layout docks a frame");
    let toggle_maximize = |visual: &mut VisualTestContext| {
        visual.update(|_, app| {
            state.update(app, |state, cx| {
                state.dock_tree.toggle_maximize(first.id);
                state.record_dock_edit(cx);
                cx.notify();
            })
        });
        visual.run_until_parked();
    };
    match change {
        ChromeChange::Tool => {
            let selected = visual.read(|app| state.read(app).tool);
            let id = crate::tool_catalog::tool_specs()
                .iter()
                .filter(|spec| spec.tool != selected)
                .map(|spec| spec.id)
                .find(|id| visual.debug_bounds(id).is_some())
                .expect("the strip draws a tool that is not selected");
            click(visual, id);
            assert!(
                visual.read(|app| state.read(app).tool) != selected,
                "a click on {id} selected no other tool"
            );
        }
        ChromeChange::Tab => {
            let (frame, panel) = docked(state, visual)
                .into_iter()
                .find_map(|frame| {
                    let other = frame
                        .tabs
                        .iter()
                        .copied()
                        .filter(|&panel| panel != frame.active_panel())
                        .find(|panel| {
                            let selector = format!("dock-tab-{panel}");
                            visual
                                .debug_bounds(crate::interned::intern(&selector))
                                .is_some()
                        })?;
                    Some((frame.id, other))
                })
                .expect("the layout draws a frame with a tab that is not active");
            click(visual, &format!("dock-tab-{panel}"));
            let active = docked(state, visual)
                .into_iter()
                .find(|drawn| drawn.id == frame)
                .map(|drawn| drawn.active_panel());
            assert_eq!(active, Some(panel), "a click on the tab activated it");
        }
        ChromeChange::MenuOpen | ChromeChange::MenuClose => {
            click(visual, "menu-title-File");
            let open = visual.read(|app| state.read(app).open_menu.is_some());
            assert_eq!(
                open,
                matches!(change, ChromeChange::MenuOpen),
                "a click on the File title toggled its drop"
            );
        }
        ChromeChange::CloseHover => {
            let close = centre(visual, &format!("dock-tab-close-{}", first.active_panel()));
            hover(visual, close);
        }
        ChromeChange::CloseRest => {
            let tab = active_tab_start(visual, &first);
            hover(visual, tab);
        }
        ChromeChange::Maximize | ChromeChange::Restore => toggle_maximize(visual),
        ChromeChange::Narrower => {
            visual.simulate_resize(gpui::size(opened_at.width * 0.7, opened_at.height));
            visual.run_until_parked();
        }
        ChromeChange::Wider => {
            visual.simulate_resize(opened_at);
            visual.run_until_parked();
        }
    }
}

#[gpui::test]
fn every_cached_chrome_part_draws_what_a_full_redraw_draws(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = opened(cx, "chrome-part-view");
    let opened_at = visual.update(|window, _| window.viewport_size());
    settle(visual);
    let mut before = drawing(visual);
    let mut stale = Vec::new();
    let mut unchanged = Vec::new();
    for change in CHROME_CHANGES {
        make_chrome(&state, visual, change, opened_at);
        settle(visual);
        let cached = drawing(visual);
        visual.update(|window, _| window.refresh());
        visual.run_until_parked();
        let full = drawing(visual);
        if cached != full {
            stale.push(format!(
                "after {change:?}: the cached drawing differs from a full redraw\n{}",
                cached.differences(&full)
            ));
        }
        if before == full {
            unchanged.push(change);
        }
        before = full;
    }
    assert!(stale.is_empty(), "stale chrome:\n{}", stale.join("\n"));
    assert!(
        unchanged.is_empty(),
        "these changes drew nothing new, so they test nothing: {unchanged:?}"
    );
}

/// A state notification rebuilds the bar and every frame's row. A change that
/// moved only the view, the playhead, a pool tile, a filmstrip cell or the
/// Viewer's frame rebuilds none of them: nothing a part draws reads what those
/// move.
#[gpui::test]
fn a_state_change_builds_every_part_and_a_narrow_change_none(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = opened(cx, "chrome-part-scope");
    let shell = shell_of(visual);
    settle(visual);
    let mut every = vec![Part::EditorBar];
    every.extend(visual.read(|app| {
        let state = state.read(app);
        state
            .dock_tree
            .frames()
            .into_iter()
            .filter(|frame| {
                frame
                    .tabs
                    .iter()
                    .any(|&tab| state.is_dock_panel_registered(tab))
            })
            .map(|frame| Part::TabRow(frame.id))
            .collect::<Vec<_>>()
    }));
    every.sort_unstable();
    assert!(every.len() >= 3, "the layout docks frames: {every:?}");

    builds::take_parts();
    make(visual, &shell, Change::Setting, 1);
    assert_eq!(parts_built(), every, "the parts a state change built");

    for moved in Moved::ALL {
        let change = moving(visual, &shell, moved, 2);
        builds::take_parts();
        make(visual, &shell, change, 2);
        assert_eq!(
            parts_built(),
            Vec::<Part>::new(),
            "the parts a {moved:?} change built"
        );
    }
}

/// The pointer onto a tab row's control rebuilds that row and nothing else.
#[gpui::test]
fn a_hover_on_a_tab_row_builds_that_row_alone(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = opened(cx, "chrome-part-hover");
    settle(visual);
    let frame = docked(&state, visual)
        .into_iter()
        .next()
        .expect("the layout docks a frame");
    let tab = active_tab_start(visual, &frame);
    let close = centre(visual, &format!("dock-tab-close-{}", frame.active_panel()));
    hover(visual, tab);
    settle(visual);
    builds::take();
    builds::take_parts();

    hover(visual, close);
    settle(visual);
    assert_eq!(
        parts_built(),
        [Part::TabRow(frame.id)],
        "the parts a hover on a close control built"
    );
    assert!(
        builds::take().is_empty(),
        "a hover on a close control built a panel body"
    );
}

/// The editor arriving draws, on every frame of the arrival, what a full
/// redraw at the same instant draws, and the editor bar moves in with the
/// route. The bar and the rows are built in place while the editor arrives,
/// and every panel body is cached, so a body that replayed the opacity of an
/// earlier frame shows here, and so does a bar drawn as its cached part.
#[gpui::test]
fn the_window_drawn_while_the_editor_arrives_is_what_a_full_redraw_draws(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = opened(cx, "chrome-part-arrival");
    let shell = shell_of(visual);
    visual.update(|_, app| app.set_reduce_motion(false));
    let route = |visual: &mut VisualTestContext, home: bool| {
        visual.update(|_, app| {
            state.update(app, |state, cx| {
                if home {
                    state.go_home();
                } else {
                    state.enter_editor();
                }
                cx.notify();
            })
        });
        visual.run_until_parked();
    };
    route(visual, true);
    settle(visual);
    route(visual, false);

    let mut stale = Vec::new();
    let mut drawn: Vec<Drawing> = Vec::new();
    let mut bar_at = Vec::new();
    for step in 1..=24 {
        visual.executor().advance_clock(Duration::from_millis(25));
        visual.update(|window, cx| window.simulate_next_frame(cx));
        visual.run_until_parked();
        let cached = drawing(visual);
        visual.update(|window, _| window.refresh());
        visual.run_until_parked();
        let full = drawing(visual);
        if cached != full {
            stale.push(format!(
                "{} ms into the arrival: the cached drawing differs from a full redraw\n{}",
                step * 25,
                cached.differences(&full)
            ));
        }
        if drawn.last() != Some(&full) {
            drawn.push(full);
        }
        let at = visual.debug_bounds("menu-bar").map(|bounds| bounds.origin);
        if bar_at.last() != Some(&at) {
            bar_at.push(at);
        }
    }
    assert!(stale.is_empty(), "stale arrival:\n{}", stale.join("\n"));
    assert!(
        drawn.len() >= 3,
        "the arrival was drawn in {} distinct frames, so it was not observed",
        drawn.len()
    );
    assert!(
        bar_at.len() >= 2,
        "the editor bar stood still while the editor arrived, at {bar_at:?}"
    );

    // At rest the bar is a cached part again.
    settle(visual);
    builds::take_parts();
    make(visual, &shell, Change::Setting, 1);
    assert!(
        parts_built().contains(&Part::EditorBar),
        "the editor bar at rest is not a cached part"
    );
}
