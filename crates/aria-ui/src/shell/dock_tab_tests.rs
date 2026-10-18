//! A dock tab gives up its close before its title, and a narrow frame still
//! reads the start of its title.
//!
//! The class closed here: a narrow tab drawn as a lone close button, its title
//! shrunk to nothing beside a close that kept its width, and a frame as narrow
//! as the Audio Meters frame drawing its tab as a bare ellipsis. Every tab of
//! every drawn frame is checked: in the launch layout, where the titles fit
//! and each close keeps its box and the narrowest frame reads its title's
//! first letters, and with every panel crowded into one frame, where the
//! titles truncate and each close is gone first.
//!
//! Not covered: the close's opacity on inactive tabs, which the test frame
//! does not report.

use gpui::{Pixels, TestAppContext, VisualTestContext};

use crate::app::AppState;
use crate::dock::{FrameId, Zone};
use crate::test_support::drawn_shell;

use super::{DOCK_TAB_CLOSE, DOCK_TAB_COMPACT_W};

/// Layout is in floats; an edge two boxes share lands within this.
const EPS: f32 = 0.51;

/// Each drawn tab: its title, whether the title is drawn whole, and its
/// close's width.
fn drawn_tabs(
    state: &gpui::Entity<AppState>,
    visual: &mut VisualTestContext,
) -> Vec<(&'static str, bool, Pixels)> {
    let panels: Vec<(FrameId, Vec<&'static str>)> = visual.read(|app| {
        let state = state.read(app);
        let rects = state.dock_frame_rects.lock();
        rects
            .keys()
            .filter_map(|id| {
                state
                    .dock_tree
                    .frame(*id)
                    .map(|frame| (*id, frame.tabs.clone()))
            })
            .collect()
    });
    let runs = visual.update(|window, _| window.rendered_text_runs().to_vec());
    let mut out = Vec::new();
    for (_, tabs) in panels {
        for panel in tabs {
            let title = visual.read(|app| state.read(app).dock_panel_title(panel));
            let selector = crate::interned::intern(&format!("dock-tab-{panel}"));
            let Some(tab) = visual.debug_bounds(selector) else {
                continue;
            };
            let close = visual
                .debug_bounds(crate::interned::intern(&format!("dock-tab-close-{panel}")))
                .unwrap_or_else(|| panic!("the {panel} tab draws no close"));
            let whole = runs
                .iter()
                .any(|run| tab.contains(&run.bounds.center()) && run.text.as_ref() == title);
            out.push((title, whole, close.size.width));
        }
    }
    out
}

/// A truncated title stands beside no close.
fn assert_close_gives_way(tabs: &[(&'static str, bool, Pixels)], when: &str) {
    for (title, whole, close) in tabs {
        if !whole {
            assert!(
                f32::from(*close) <= EPS,
                "{when}: `{title}` is truncated beside a close {close:?} wide"
            );
        }
    }
}

#[gpui::test]
fn a_narrow_tab_keeps_its_title_before_its_close(cx: &mut TestAppContext) {
    let (state, visual) = drawn_shell(cx, "dock-tab-close");
    let launch = drawn_tabs(&state, visual);
    assert!(
        launch.iter().any(|(_, whole, _)| *whole),
        "no launch tab draws its whole title: {launch:?}"
    );
    // At launch every tab bar has room to spare, so a whole title keeps a
    // whole close.
    for (title, whole, close) in &launch {
        if *whole {
            assert!(
                (f32::from(*close) - DOCK_TAB_CLOSE).abs() <= EPS,
                "`{title}` fits, and its close is {close:?} wide"
            );
        }
    }
    assert_close_gives_way(&launch, "launch");

    let crowded = visual.read(|app| {
        let state = state.read(app);
        let rects = state.dock_frame_rects.lock();
        let mut ids: Vec<FrameId> = rects
            .keys()
            .copied()
            .filter(|id| {
                state
                    .dock_tree
                    .frame(*id)
                    .is_some_and(|f| !f.tabs.is_empty())
            })
            .collect();
        ids.sort();
        ids[0]
    });
    // Every panel in the first frame, then the last split off to its right, in
    // a window narrower than the launch one: the frame holds more tabs than
    // its width fits.
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            let panels = state.registered_panel_ids();
            for &panel in &panels {
                state.dock_tree.dock(panel, crowded, Zone::Center);
            }
            let last = panels
                .last()
                .copied()
                .expect("the ui plugin registers panels");
            state.dock_tree.dock(last, crowded, Zone::Right);
            cx.notify();
        });
    });
    visual.simulate_resize(gpui::size(gpui::px(1280.), gpui::px(800.)));
    visual.run_until_parked();
    let tabs = drawn_tabs(&state, visual);
    assert!(
        tabs.iter().any(|(_, whole, _)| !whole),
        "the crowded frame truncates no title: {tabs:?}"
    );
    assert_close_gives_way(&tabs, "crowded");
}

/// Letters of its title a tab in the narrowest frame draws before the
/// ellipsis.
const LEAD: usize = 3;

#[gpui::test]
fn a_narrow_frame_reads_the_start_of_its_title(cx: &mut TestAppContext) {
    let (state, visual) = drawn_shell(cx, "dock-tab-compact");
    // A second draw: the first records each frame's width, which the tab row
    // reads.
    visual.update(|_, app| state.update(app, |_, cx| cx.notify()));
    visual.run_until_parked();
    let frames: Vec<(f32, Vec<&'static str>)> = visual.read(|app| {
        let state = state.read(app);
        let rects = state.dock_frame_rects.lock();
        rects
            .iter()
            .filter_map(|(id, rect)| {
                let frame = state.dock_tree.frame(*id)?;
                Some((rect[2], frame.tabs.clone()))
            })
            .collect()
    });
    let runs = visual.update(|window, _| window.rendered_text_runs().to_vec());
    let mut narrow = 0;
    for (width, tabs) in frames {
        if width >= DOCK_TAB_COMPACT_W || tabs.len() != 1 {
            continue;
        }
        narrow += 1;
        let panel = tabs[0];
        let title = visual.read(|app| state.read(app).dock_panel_title(panel));
        let tab = visual
            .debug_bounds(crate::interned::intern(&format!("dock-tab-{panel}")))
            .unwrap_or_else(|| panic!("the {panel} frame draws no tab"));
        let drawn = runs
            .iter()
            .find(|run| tab.contains(&run.bounds.center()))
            .unwrap_or_else(|| panic!("the {panel} tab in a {width} px frame draws no title"));
        let lead: String = title.chars().take(LEAD).collect();
        assert!(
            drawn.text.starts_with(&lead),
            "the {panel} tab in a {width} px frame draws `{}`, not `{lead}…`",
            drawn.text
        );
    }
    assert!(
        narrow > 0,
        "the launch layout holds no one-tab frame under {DOCK_TAB_COMPACT_W} px"
    );
}
