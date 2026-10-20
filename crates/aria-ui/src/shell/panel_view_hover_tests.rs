//! A hover redraws what it changed and nothing else.
//!
//! The class closed here is a hover that rebuilds panel bodies it did not
//! change. A hover inside one panel body notifies that cached body alone. A
//! hover over the window chrome (a dock tab, a frame header control, a frame
//! menu row, the timeline splitter, a palette row) fades through the motion
//! store and redraws the chrome alone: a gpui hover style on chrome the Shell
//! builds notifies the Shell, and a Shell notification rebuilds the bodies.
//!
//! Checked at every point the frame draws a quad or a text run in the chrome,
//! at launch, with a frame menu open, and with the palette open, and at the
//! timeline splitter of a window under the minimum width. Not caught: chrome
//! drawn only in a state these cases do not open, such as a floating window's
//! header or a maximized frame's Restore control.

use gpui::{Bounds, Entity, Pixels, Point, TestAppContext, VisualTestContext};

use crate::app::AppState;
use crate::dock::PanelId;
use crate::theme::Theme;

use super::builds;
use super::tests::{first_clip, opened};

/// Every docked frame: its active panel, its id, and the bounds the last draw
/// recorded for it.
fn frame_rects(
    state: &Entity<AppState>,
    visual: &mut VisualTestContext,
) -> Vec<(PanelId, u32, Bounds<Pixels>)> {
    visual.read(|app| {
        let state = state.read(app);
        let rects = state.dock_frame_rects.lock();
        state
            .dock_tree
            .frames()
            .into_iter()
            .map(|frame| {
                let [x, y, w, h] = *rects
                    .get(&frame.id)
                    .expect("every docked frame records its bounds");
                (
                    frame.active_panel(),
                    frame.id.0,
                    Bounds::new(
                        gpui::point(gpui::px(x), gpui::px(y)),
                        gpui::size(gpui::px(w), gpui::px(h)),
                    ),
                )
            })
            .collect()
    })
}

/// The centre of every text run, then of every quad, the last frame drew, in
/// logical pixels.
fn drawn_points(visual: &mut VisualTestContext) -> Vec<Point<Pixels>> {
    visual.update(|window, _| {
        let scale = window.scale_factor();
        let mut points: Vec<_> = window
            .rendered_text_runs()
            .iter()
            .map(|run| run.bounds.center())
            .collect();
        points.extend(window.painted_quads().iter().map(|quad| {
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
            .center()
        }));
        points
    })
}

/// Whether `pt` lies in the chrome the Shell draws: outside every frame, or in
/// a frame's tab row.
fn in_chrome(pt: &Point<Pixels>, frames: &[(PanelId, u32, Bounds<Pixels>)]) -> bool {
    frames.iter().all(|(_, _, rect)| {
        !rect.contains(pt) || pt.y < rect.origin.y + gpui::px(1.0 + Theme::TAB_ROW_HEIGHT)
    })
}

/// Moves the pointer to the window corner, lets every hover fade there settle,
/// and drops the builds that took, so the next move is measured from rest.
fn rest_pointer(visual: &mut VisualTestContext) {
    visual.simulate_mouse_move(
        gpui::point(gpui::px(0.0), gpui::px(0.0)),
        None::<gpui::MouseButton>,
        gpui::Modifiers::none(),
    );
    visual.run_until_parked();
    visual
        .executor()
        .advance_clock(std::time::Duration::from_secs(1));
    visual.update(|window, cx| {
        window.simulate_next_frame(cx);
        window.draw(cx).clear(cx);
    });
    visual.run_until_parked();
    builds::take();
}

/// Moves the pointer to each of `points` from the window corner and returns a
/// line for each one whose hover built a body other than that of the frame
/// under it. A point in the chrome lies in no frame's body, so any body it
/// builds is a failure.
fn sweep(
    visual: &mut VisualTestContext,
    frames: &[(PanelId, u32, Bounds<Pixels>)],
    points: Vec<Point<Pixels>>,
    case: &str,
) -> Vec<String> {
    visual.simulate_mouse_move(
        gpui::point(gpui::px(0.0), gpui::px(0.0)),
        None::<gpui::MouseButton>,
        gpui::Modifiers::none(),
    );
    visual.run_until_parked();
    builds::take();
    let mut seen = std::collections::HashSet::new();
    let mut failures = Vec::new();
    for pt in points {
        // Points closer than a pixel reach the same control.
        if !seen.insert((f32::from(pt.x) as i32, f32::from(pt.y) as i32)) {
            continue;
        }
        visual.simulate_mouse_move(pt, None::<gpui::MouseButton>, gpui::Modifiers::none());
        visual.run_until_parked();
        let mut built = builds::take();
        built.sort_unstable();
        built.dedup();
        let under: Vec<PanelId> = frames
            .iter()
            .filter(|(_, _, rect)| !in_chrome(&pt, frames) && rect.contains(&pt))
            .map(|&(panel, _, _)| panel)
            .collect();
        if built.iter().any(|panel| !under.contains(panel)) {
            failures.push(format!("{case}: hovering {pt:?} built {built:?}"));
        }
    }
    failures
}

/// The centre of every dock tab's close control the last frame drew. A close
/// paints a glyph on no wash at rest, which no quad or text run reaches.
fn tab_closes(state: &Entity<AppState>, visual: &mut VisualTestContext) -> Vec<Point<Pixels>> {
    let panels: Vec<PanelId> = visual.read(|app| {
        state
            .read(app)
            .dock_tree
            .frames()
            .into_iter()
            .flat_map(|frame| frame.tabs.iter().copied())
            .collect()
    });
    panels
        .into_iter()
        .filter_map(|panel| {
            visual.debug_bounds(crate::interned::intern(&format!("dock-tab-close-{panel}")))
        })
        .map(|bounds| bounds.center())
        .collect()
}

/// Hovering the chrome builds no panel body, and hovering an open frame menu
/// or the palette builds at most the body under it.
#[gpui::test]
fn a_hover_over_the_chrome_builds_no_panel_body(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = opened(cx, "panel-view-chrome-hover");
    visual.update(|_, app| app.set_reduce_motion(false));
    visual.run_until_parked();
    let frames = frame_rects(&state, visual);
    assert!(frames.len() >= 2, "the launch layout docks multiple panels");

    let closes = tab_closes(&state, visual);
    assert!(!closes.is_empty(), "no dock tab draws its close");
    let mut chrome: Vec<_> = drawn_points(visual)
        .into_iter()
        .filter(|pt| in_chrome(pt, &frames))
        .collect();
    chrome.extend(closes);
    let tab_rows = chrome
        .iter()
        .filter(|pt| frames.iter().any(|(_, _, rect)| rect.contains(pt)))
        .count();
    assert!(
        tab_rows >= frames.len(),
        "the chrome points miss the tab rows: {tab_rows} for {} frames",
        frames.len()
    );
    let mut failures = sweep(visual, &frames, chrome, "launch");

    // A frame menu, opened from its frame's menu control.
    let (_, frame, _) = frames[0];
    let menu = visual
        .debug_bounds(crate::interned::intern(&format!("dock-frame-menu-{frame}")))
        .expect("a frame menu control in the tab row");
    visual.simulate_click(menu.center(), gpui::Modifiers::none());
    visual.run_until_parked();
    let card = visual
        .debug_bounds("dock-frame-menu-card")
        .expect("the frame menu opens");
    let rows: Vec<_> = drawn_points(visual)
        .into_iter()
        .filter(|pt| card.contains(pt))
        .collect();
    assert!(rows.len() >= 4, "the frame menu draws its rows");
    failures.extend(sweep(visual, &frames, rows, "frame menu"));
    visual.simulate_keystrokes("escape");
    visual.run_until_parked();

    // Under the minimum width the Viewer sits over the Timeline, split by a row
    // the Shell draws. It lies in no frame's body.
    let full = visual.update(|window, _| window.viewport_size());
    visual.simulate_resize(gpui::size(
        gpui::px(crate::settings::MIN_WINDOW_WIDTH - 1.0),
        full.height,
    ));
    visual.run_until_parked();
    let split = visual
        .debug_bounds("split-timeline")
        .expect("a window under the minimum width draws the timeline splitter");
    failures.extend(sweep(visual, &[], vec![split.center()], "small window"));
    visual.simulate_resize(full);
    visual.run_until_parked();

    visual.simulate_keystrokes("ctrl-shift-p");
    visual.run_until_parked();
    let list = visual
        .debug_bounds("dialog-content")
        .expect("Ctrl+Shift+P opens the palette");
    let rows: Vec<_> = drawn_points(visual)
        .into_iter()
        .filter(|pt| list.contains(pt))
        .collect();
    assert!(rows.len() >= 4, "the palette draws its rows");
    failures.extend(sweep(visual, &frames, rows, "palette"));

    failures.truncate(12);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[gpui::test]
fn a_hover_in_one_panel_builds_only_that_panels_body(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = opened(cx, "panel-view-hover");
    visual.update(|_, app| {
        app.set_reduce_motion(false);
        state.update(app, |state, cx| {
            let clip = first_clip(state);
            state.select_clip(clip);
            cx.notify();
        });
    });
    visual.run_until_parked();
    let frames = frame_rects(&state, visual);
    assert!(
        frames.len() >= 2,
        "the launch layout docks multiple panels at once"
    );

    for (panel, _, rect) in frames {
        let mut candidates: Vec<_> = drawn_points(visual)
            .into_iter()
            .filter(|pt| rect.contains(pt))
            .collect();
        candidates.dedup();

        rest_pointer(visual);

        let mut hovered = false;
        for pt in candidates {
            visual.simulate_mouse_move(pt, None::<gpui::MouseButton>, gpui::Modifiers::none());
            visual.run_until_parked();
            let built = builds::take();
            if built.is_empty() {
                continue;
            }
            assert_eq!(
                built,
                vec![panel],
                "hovering {pt:?} in {panel} built other panel bodies"
            );
            visual
                .executor()
                .advance_clock(std::time::Duration::from_millis(16));
            visual.update(|window, cx| {
                let callbacks = window.simulate_next_frame(cx);
                assert!(callbacks > 0, "hovering in {panel} scheduled no fade frame");
                window.draw(cx).clear(cx);
            });
            assert_eq!(
                builds::take(),
                vec![panel],
                "the hover fade frame in {panel} built other panel bodies"
            );
            hovered = true;
            break;
        }
        assert!(hovered, "no point in {panel} triggered a hover redraw");
    }

    let menu_pt = visual
        .debug_bounds("menu-title-File")
        .expect("the File menu title in the window chrome")
        .center();
    // From the window corner with every fade settled: moving straight from
    // the last panel's control fades that control out, which is its body's.
    rest_pointer(visual);
    visual.simulate_mouse_move(menu_pt, None::<gpui::MouseButton>, gpui::Modifiers::none());
    visual.run_until_parked();
    assert!(
        builds::take().is_empty(),
        "hovering a window-chrome button rebuilt panel bodies"
    );
    visual
        .executor()
        .advance_clock(std::time::Duration::from_millis(16));
    visual.update(|window, cx| {
        assert!(
            window.simulate_next_frame(cx) > 0,
            "hovering the File menu title scheduled no fade frame"
        );
        window.draw(cx).clear(cx);
    });
    assert!(
        builds::take().is_empty(),
        "the window-chrome hover fade frame rebuilt panel bodies"
    );
}
