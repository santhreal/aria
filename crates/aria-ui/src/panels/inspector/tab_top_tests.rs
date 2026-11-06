//! A tab of Effect Controls opens at its top.
//!
//! Closes the class "a tab opens scrolled to where the last one was": the
//! body's scroll handle outlives the tab, so a switch away from a tab scrolled
//! to its end drew the next tab at that depth, and Look opened past its Filter
//! and Color Wheels. Every tab the texture offers that is taller than its body
//! is scrolled to its end and left for every other tab, the tabs read from the
//! tab host at run time, so a tab added later is held to the rule. A tall tab
//! that does not scroll fails too, so a body put back at its top on every frame
//! does not pass. Not covered: a switch of clip on one tab, which keeps the
//! body's place, and a tab no clip kind but the texture's offers.

use gpui::{Entity, Pixels, Point, TestAppContext, VisualTestContext, px};

use super::graph_tests::scroll_to_end;
use super::strip_tests::drawn_with;
use super::tab_host::host_of;
use crate::app::{AppState, DEFAULT_INSPECTOR_TAB};

/// The body's scroll offset as the last frame drew it.
fn offset(state: &Entity<AppState>, visual: &VisualTestContext) -> Point<Pixels> {
    visual.read(|app| state.read(app).inspector_reveal.frame.offset())
}

/// How far the tab the last frame drew scrolls.
fn room(state: &Entity<AppState>, visual: &VisualTestContext) -> Pixels {
    visual.read(|app| state.read(app).inspector_reveal.frame.max_offset().y)
}

/// Put `tab` in front and draw it.
fn open(state: &Entity<AppState>, visual: &mut VisualTestContext, tab: &str) {
    visual.update(|window, app| {
        state.update(app, |state, _| state.inspector_tab = tab.to_owned());
        window.refresh();
    });
    visual.run_until_parked();
}

#[gpui::test]
fn every_tab_opens_at_its_top_whatever_the_last_one_was_scrolled_to(cx: &mut TestAppContext) {
    let (state, visual) = drawn_with(cx, "tab-top", "texture.png", DEFAULT_INSPECTOR_TAB, |_| {});
    let kind = visual.read(|app| {
        state
            .read(app)
            .selected_ui_clip()
            .expect("the texture is selected")
            .kind
    });
    let tabs: Vec<String> = host_of()
        .visible(kind)
        .map(|tab| tab.id.to_string())
        .collect();
    let mut tall = Vec::new();
    for from in &tabs {
        open(&state, visual, from);
        if room(&state, visual) <= px(0.0) {
            continue;
        }
        tall.push(from.as_str());
        for to in tabs.iter().filter(|to| *to != from) {
            open(&state, visual, from);
            scroll_to_end(visual);
            let deep = offset(&state, visual);
            assert!(
                deep.y < px(0.0),
                "the {from} tab, taller than its body, stays at {deep:?} under the wheel"
            );
            open(&state, visual, to);
            assert_eq!(
                offset(&state, visual),
                Point::default(),
                "Effect Controls left on {from} scrolled to {deep:?} opens {to} at"
            );
        }
    }
    assert!(
        tall.contains(&DEFAULT_INSPECTOR_TAB),
        "of {tabs:?}, only {tall:?} is taller than its body, so the switch away from a \
         scrolled {DEFAULT_INSPECTOR_TAB} is not tested"
    );
}
