//! The docked Effects & Presets panel stands off its frame's edges and keeps
//! its search row lean.
//!
//! The classes closed here: a tab drawn as a panel of its own without the inset
//! its host gave it; and chrome in the Effects & Presets panel that repeats the
//! tab title in the search placeholder, keeps a Clear word button beside an
//! empty search box, or spends lines under the search row on gesture prose that
//! belongs in the entry tooltip.
//!
//! The panel is measured from the frame the dock reports, inside its border and
//! under its tab row, so a panel drawn with no inset of its own is measured
//! too. Every line of text centred in the panel is checked against the inset,
//! so a row added to the panel joins the check. Not covered: a wrapped
//! paragraph, recorded at its unwrapped width and so centred past the panel;
//! the other Project tabs, which are drawn only inside the Project panel.

use gpui::{Bounds, Modifiers, TestAppContext, point, px, size};

use crate::components::drawn::{self, Drawn};
use crate::dock::PANEL_EFFECTS;
use crate::test_support::drawn_shell;
use crate::theme::Theme;
const EPS: f32 = 0.51;

#[gpui::test]
fn the_docked_effects_panel_keeps_its_inset(cx: &mut TestAppContext) {
    let (state, visual) = drawn_shell(cx, "effects-inset");
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.open_effects_panel();
            state.focus_effects_search(false);
            cx.notify();
        });
    });
    visual.run_until_parked();
    // The frame holding the panel, from the dock rather than from the panel's
    // own elements, so a panel drawn without its inset is measured too.
    let [x, y, w, h] = visual
        .read(|app| {
            let state = state.read(app);
            let frame = state.dock_tree.find(PANEL_EFFECTS)?;
            state.dock_frame_rects.lock().get(&frame).copied()
        })
        .expect("the Effects & Presets panel is docked and drawn");
    let tab = visual
        .debug_bounds(crate::interned::intern(&format!(
            "dock-tab-{PANEL_EFFECTS}"
        )))
        .expect("the panel's tab is drawn");
    // Inside the frame's 1 px border and under its tab row.
    let panel = Bounds::new(
        point(px(x + 1.0), tab.bottom()),
        size(px(w - 2.0), px(y + h) - tab.bottom()),
    );
    let left = panel.left() + px(Theme::SPACE_MD);
    let right = panel.right() - px(Theme::SPACE_MD);
    let runs = visual.update(|window, _| window.rendered_text_runs().to_vec());
    let inside: Vec<_> = runs
        .iter()
        .filter(|run| panel.contains(&run.bounds.center()))
        .collect();
    // The filter's placeholder stands inside the field's border and padding,
    // which stand on the panel's inset.
    let placeholder = inside
        .iter()
        .find(|run| run.text.as_ref() == "Search")
        .expect("the panel draws its filter");
    let field_text = left + px(1.0 + Theme::SPACE_SM);
    assert!(
        (placeholder.bounds.left() - field_text).abs() < px(EPS),
        "the filter's text starts at {:?}, not {field_text:?} inside the panel's inset",
        placeholder.bounds.left()
    );
    let folder = inside
        .iter()
        .find(|run| run.text.as_ref() == "Video Effects")
        .expect("the panel draws its folders");
    assert!(
        folder.bounds.left() > left + px(EPS),
        "a folder's label stands right of its chevron, not on the inset: {:?}",
        folder.bounds.left()
    );
    for run in &inside {
        assert!(
            run.bounds.left() >= left - px(EPS),
            "{:?} starts at {:?}, left of the panel's inset {left:?}",
            run.text,
            run.bounds.left()
        );
        assert!(
            run.bounds.right() <= right + px(EPS),
            "{:?} ends at {:?}, right of the panel's inset {right:?}",
            run.text,
            run.bounds.right()
        );
    }
}

#[gpui::test]
fn at_rest_the_effects_panel_draws_a_lean_search_row_and_clears_with_an_icon(
    cx: &mut TestAppContext,
) {
    let (state, visual) = drawn_shell(cx, "effects-search-row");
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.open_effects_panel();
            state.focus_effects_search(false);
            cx.notify();
        });
    });
    visual.run_until_parked();

    let dock = visual
        .debug_bounds("effects-dock")
        .expect("the Effects & Presets panel is drawn");
    let runs = visual.update(|window, _| window.rendered_text_runs().to_vec());
    let inside: Vec<&str> = runs
        .iter()
        .filter(|run| !drawn::is_marker(run) && dock.contains(&run.bounds.center()))
        .map(|run| run.text.as_ref())
        .collect();
    assert!(
        inside.contains(&"Search"),
        "an empty search box draws the short placeholder: {inside:?}"
    );
    assert!(
        !inside.iter().any(|text| {
            *text == "Clear"
                || text.contains("Effects") && text.contains("Search")
                || text.contains("Double-click")
                || text.contains("Drag onto")
        }),
        "the panel body draws neither a Clear word button nor gesture prose: {inside:?}"
    );
    assert!(
        visual.debug_bounds("effects-query-clear").is_none(),
        "an empty search box draws no clear button"
    );
    let search = runs
        .iter()
        .find(|run| dock.contains(&run.bounds.center()) && run.text.as_ref() == "Search")
        .expect("the Search placeholder is drawn");
    let first_folder = runs
        .iter()
        .find(|run| dock.contains(&run.bounds.center()) && run.text.as_ref() == "Video Effects")
        .expect("the first folder is drawn");
    assert!(
        first_folder.bounds.top() - search.bounds.bottom() <= px(Theme::ICON_HIT),
        "the folder tree sits directly under the search row: search bottom {:?}, folder top {:?}",
        search.bounds.bottom(),
        first_folder.bounds.top()
    );

    drawn::start();
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.effects_browser.query = "Blur".into();
            cx.notify();
        });
    });
    visual.run_until_parked();
    let boxes = drawn::take();

    let clear = visual
        .debug_bounds("effects-query-clear")
        .expect("a non-empty query draws the clear button");
    let runs = visual.update(|window, _| window.rendered_text_runs().to_vec());
    let clear_runs: Vec<&str> = runs
        .iter()
        .filter(|run| !drawn::is_marker(run) && clear.contains(&run.bounds.center()))
        .map(|run| run.text.as_ref())
        .collect();
    assert!(
        clear_runs.is_empty(),
        "the clear control is an icon button, not a word: {clear_runs:?}"
    );
    assert!(
        boxes
            .iter()
            .any(|(kind, at)| *kind == Drawn::Image && clear.contains(&at.center())),
        "the clear button draws its close icon"
    );
    visual.simulate_click(clear.center(), Modifiers::none());
    visual.run_until_parked();
    assert!(
        visual.read(|app| state.read(app).effects_browser.query.is_empty()),
        "clicking the clear button empties the query"
    );
    assert!(
        visual.debug_bounds("effects-query-clear").is_none(),
        "clearing the query hides the clear button again"
    );
}
