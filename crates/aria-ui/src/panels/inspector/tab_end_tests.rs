//! An Effect Controls tab scrolls no further than its content's end.
//!
//! The class closed here: a tab body whose scroll range is longer than what it
//! draws, so the wheel carries the last row up the frame and leaves empty
//! ground under it, or the whole body blank. The Text tab of a title did this
//! under its font list and cloud presets while Transform stopped on its last
//! row. Every tab the tab host shows for a media clip, a title and a shape is
//! drawn, wheeled to its end over the top, the middle and the foot of the body,
//! so a nested list under the pointer is crossed too, and the lowest line the
//! body draws must sit within [`FOOT`] of the body's foot. The tabs are read
//! from the tab host at run time, so a tab added later is held to the rule on
//! the run that adds it.
//!
//! Not caught: a tab whose last drawn element is not text, which reads as a
//! gap of that element's height; a tab shorter than its body, which does not
//! scroll and is skipped.

use gpui::{
    Bounds, Modifiers, Pixels, ScrollDelta, ScrollWheelEvent, TestAppContext, TouchPhase, point, px,
};

use super::tab_host::host_of;
use crate::dock::PANEL_INSPECTOR;
use crate::panels::panel_casing_tests::dress;
use crate::test_support::drawn_shell;
use crate::theme::Theme;

/// The most ground a body scrolled to its end may draw under its last line:
/// the body's padding, a row's gap and the line's own leading.
const FOOT: f32 = Theme::SPACE_MD * 4.0;

/// Wheel the body at `at` far past its end and draw the result.
fn wheel_down(visual: &mut gpui::VisualTestContext, at: gpui::Point<Pixels>) {
    visual.simulate_mouse_move(at, None, Modifiers::none());
    visual.simulate_event(ScrollWheelEvent {
        position: at,
        delta: ScrollDelta::Pixels(point(px(0.0), px(-10_000.0))),
        modifiers: Modifiers::none(),
        touch_phase: TouchPhase::Moved,
    });
    visual.run_until_parked();
}

/// The bottom of the lowest line drawn inside `body`.
fn lowest_line(visual: &mut gpui::VisualTestContext, body: Bounds<Pixels>) -> Option<Pixels> {
    visual.update(|window, _| {
        window
            .rendered_text_runs()
            .iter()
            .filter(|run| !run.text.trim().is_empty() && body.contains(&run.bounds.center()))
            .map(|run| run.bounds.bottom())
            .max_by(|a, b| a.partial_cmp(b).expect("a drawn run has a position"))
    })
}

/// The body frame's offset, scroll range, bounds and children, for a failure.
fn extent(state: &gpui::Entity<crate::app::AppState>, visual: &gpui::VisualTestContext) -> String {
    visual.read(|app| {
        let frame = &state.read(app).inspector_reveal.frame;
        let children: Vec<_> = (0..frame.children_count())
            .filter_map(|ix| frame.bounds_for_item(ix))
            .map(|bounds| (bounds.origin.y, bounds.size.height))
            .collect();
        format!(
            "offset {:?} of max {:?}, frame {:?}, children (top, height) {children:?}",
            frame.offset().y,
            frame.max_offset().y,
            frame.bounds()
        )
    })
}

#[gpui::test]
fn every_tab_scrolls_no_further_than_its_last_line(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, "tab-end");
    visual.update(|_, app| app.set_reduce_motion(true));
    let clips = visual.update(|_, app| {
        state.update(app, |state, cx| {
            let clips = dress(state);
            state.dock_tree.open(PANEL_INSPECTOR);
            cx.notify();
            clips
        })
    });
    visual.run_until_parked();

    let mut tall = 0;
    let mut gaps = Vec::new();
    for (clip, kind) in clips {
        let ui = visual.read(|app| state.read(app).ui_clip(clip).expect("a projected clip"));
        let tabs: Vec<String> = host_of()
            .visible(&ui)
            .map(|tab| tab.id.to_string())
            .collect();
        for tab in tabs {
            visual.update(|window, app| {
                state.update(app, |state, _| {
                    state.select_clip(clip);
                    state.inspector_tab = tab.clone();
                });
                window.refresh();
            });
            visual.run_until_parked();
            let room = visual.read(|app| state.read(app).inspector_reveal.frame.max_offset().y);
            if room <= px(0.0) {
                continue;
            }
            tall += 1;
            let body = visual
                .debug_bounds("inspector-body")
                .expect("the Inspector draws its body");
            let mut wheeled = Vec::new();
            for at in [0.1, 0.5, 0.9] {
                let y = body.origin.y + body.size.height * at;
                wheel_down(visual, point(body.center().x, y));
                wheeled.push(format!("at {at}: {}", extent(&state, visual)));
            }
            let lowest = lowest_line(visual, body).unwrap_or_else(|| {
                panic!("the {tab} tab of a {kind:?} clip scrolled blank: {wheeled:#?}")
            });
            let gap = f32::from(body.bottom() - lowest);
            if gap > FOOT {
                gaps.push(format!("{tab} of a {kind:?} clip: {gap:.0} px"));
            }
        }
    }
    assert!(tall > 0, "no tab of a dressed clip is taller than its body");
    assert!(
        gaps.is_empty(),
        "scrolled to its end, a tab draws empty ground under its last line: {gaps:?}"
    );
}
