//! The Project panel's header and its tabs' bodies stand their labels on one
//! edge and their controls on one height.
//!
//! WHY: the Stock tab stacked a route caption, the query box, the kind chips,
//! a hint line that was blank at rest and a Search button drawn as bare text,
//! so the button stood four rows under the box it submits; the box was 20 px
//! tall over 26 px chips; the tab row's labels stood 4 px left of the group
//! row's; and "Not loaded" sat over an empty panel. Every catalog row carried
//! its own two-line caption, so forty transitions repeated one sentence forty
//! times; the rows stood 8 px inside the column; and a list taller than the
//! panel ran off its bottom. The classes closed here:
//!
//! - over every group that lists tabs, at both group paddings: a tab label
//!   that does not start on the group labels' edge;
//! - in Stock: a line drawn between the header and the query box, an action
//!   that does not stand beside the box at its height, and kind chips that do
//!   not start on the box's edge one row under it;
//! - in Stock and SFX at rest: anything drawn in the body but the form and one
//!   centered empty state that fills the rest of the panel;
//! - over every tab that lists entries: a usage line drawn in a row, a line
//!   off the column's edge, and a list that does not end on the panel's bottom
//!   inset. Every tab is placed in [`LISTS`] or [`OTHER_LAYOUTS`], so a new tab
//!   fails until it is placed in one;
//! - over every tab in [`FORMS`]: a label off the column's edge or off the
//!   first line of its controls, controls that do not start on the controls'
//!   edge and run to the column's right inset, and, in Captions, a threads or
//!   output value in force that no chip offers, as the machine default was
//!   before the Auto chip.
//!
//! Not covered: colors, glyphs and hover, which the test renderer does not
//! read; a feed with results or a failure, which needs the network;
//! Generate, whose layout depends on a provider key; the Captions actions with
//! a model installed, which needs a model file; a plugin clip type's swatch
//! and icon, which stand before its name.

use gpui::{Bounds, Pixels, TestAppContext, VisualTestContext, px};

use super::STOCK_KINDS;
use super::layout_tests::{bounds, project_width};
use crate::app::AppState;
use crate::app::captions::CaptionCommit;
use crate::test_support::drawn_shell;
use crate::theme::Theme;
use crate::types::{SidebarGroup, SidebarTab};

/// Layout is in floats; an edge two boxes share lands within this.
const EPS: f32 = 0.51;

/// The tabs that list entries to place or apply, with the selector of each
/// list.
const LISTS: [(SidebarTab, &str); 7] = [
    (SidebarTab::Titles, "sidebar-titles"),
    (SidebarTab::Shapes, "sidebar-shapes"),
    (SidebarTab::Stickers, "sidebar-stickers"),
    (SidebarTab::Generators, "sidebar-generators"),
    (SidebarTab::Animations, "sidebar-animations"),
    (SidebarTab::Masks, "sidebar-masks"),
    (SidebarTab::Transitions, "sidebar-transitions"),
];

/// The tabs laid out some other way: the pool, a browser, a form or a feed.
const OTHER_LAYOUTS: [SidebarTab; 8] = [
    SidebarTab::Media,
    SidebarTab::Audio,
    SidebarTab::Effects,
    SidebarTab::Stock,
    SidebarTab::CloudSfx,
    SidebarTab::Generate,
    SidebarTab::Captions,
    SidebarTab::Analysis,
];

/// The tabs drawn as a form, with the form's rows top to bottom by label.
const FORMS: [(SidebarTab, &[&str]); 2] = [
    (
        SidebarTab::Captions,
        &["Language", "Output", "Split", "Threads"],
    ),
    (
        SidebarTab::Analysis,
        &["Detect", "Loudness", "Show", "Confidence"],
    ),
];

/// Opens `tab` and moves the clock past the rows' arrival, so every row is
/// read where it rests.
fn open(state: &gpui::Entity<AppState>, visual: &mut VisualTestContext, tab: SidebarTab) {
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.sidebar_tab = tab;
            cx.notify();
        })
    });
    visual.run_until_parked();
    visual
        .executor()
        .advance_clock(std::time::Duration::from_secs(2));
    visual.update(|window, _| window.refresh());
    visual.run_until_parked();
}

/// The bounds of the run drawing `label` inside `within`.
fn label(visual: &mut VisualTestContext, label: &str, within: Bounds<Pixels>) -> Bounds<Pixels> {
    let runs = visual.update(|window, _| window.rendered_text_runs().to_vec());
    if let Some(run) = runs
        .iter()
        .find(|run| run.text.as_ref() == label && within.contains(&run.bounds.center()))
    {
        return run.bounds;
    }
    let elsewhere: Vec<_> = runs
        .iter()
        .filter(|run| run.text.as_ref() == label)
        .map(|run| run.bounds)
        .collect();
    let inside: Vec<_> = runs
        .iter()
        .filter(|run| within.contains(&run.bounds.center()))
        .map(|run| run.text.to_string())
        .collect();
    panic!(
        "no run draws `{label}` inside {within:?}; it is drawn at {elsewhere:?}, and {inside:?} inside"
    )
}

/// Every text run drawn in the panel under the header and outside `placed`.
fn strays(
    visual: &mut VisualTestContext,
    header: Bounds<Pixels>,
    placed: &[Bounds<Pixels>],
) -> Vec<String> {
    let panel = bounds(visual, "sidebar");
    let runs = visual.update(|window, _| window.rendered_text_runs().to_vec());
    runs.iter()
        .filter(|run| {
            let mid = run.bounds.center();
            panel.contains(&mid)
                && mid.y > header.bottom()
                && !placed.iter().any(|at| at.contains(&mid))
        })
        .map(|run| format!("`{}` at {:?}", run.text, run.bounds))
        .collect()
}

fn near(a: Pixels, b: Pixels) -> bool {
    (a - b).abs() <= px(EPS)
}

#[gpui::test]
fn every_tab_label_starts_on_the_group_labels_edge(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, "sidebar-tab-edge");
    let lead = SidebarGroup::ALL[0];
    let listed: Vec<SidebarGroup> = SidebarGroup::ALL
        .into_iter()
        .filter(|group| {
            let tabs = group.tabs();
            tabs.len() > 1 && !tabs.iter().all(|tab| tab.shows_pool())
        })
        .collect();
    assert!(!listed.is_empty(), "no group lists its tabs");
    // At the launch layout's 280 px the groups take the compact padding; at
    // 420 px they hold the regular one. In the narrow panel an inactive chip
    // may draw its label cut short, which starts where the whole label does.
    for width in [280.0, 420.0] {
        project_width(&state, visual, width);
        for group in &listed {
            let first = group.tabs()[0];
            open(&state, visual, first);
            let chip = bounds(visual, &format!("sidebar-group-{}", lead.label()));
            let runs = visual.update(|window, _| window.rendered_text_runs().to_vec());
            let group_edge = runs
                .iter()
                .find(|run| chip.contains(&run.bounds.center()))
                .unwrap_or_else(|| panic!("the {lead:?} chip at {chip:?} draws no label"))
                .bounds
                .left();
            let tab = bounds(visual, &format!("sidebar-tab-{}", first.label()));
            let tab_edge = label(visual, first.label(), tab).left();
            assert!(
                near(tab_edge, group_edge),
                "in a {width} px panel the {first:?} tab's label starts at {tab_edge:?}, \
                 the {lead:?} group's at {group_edge:?}"
            );
        }
    }
}

#[gpui::test]
fn the_stock_tab_stands_search_beside_its_box_and_fills_the_rest(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, "sidebar-stock");
    open(&state, visual, SidebarTab::Stock);
    let panel = bounds(visual, "sidebar");
    let header = bounds(visual, "sidebar-tabs");
    let field = bounds(visual, "stock-query");
    let search = bounds(visual, "stock-search");
    let control = px(Theme::CONTROL_HEIGHT);

    // The body starts one border and one inset under the tab row, so a line
    // over the box moves the box down.
    let body = header.bottom() + px(1.0 + Theme::SPACE_MD);
    assert!(
        near(field.top(), body),
        "the box starts at {:?}, the body at {body:?}",
        field.top()
    );
    assert!(
        near(field.top(), search.top())
            && near(field.size.height, control)
            && near(search.size.height, control),
        "the box {field:?} and Search {search:?} are not one control tall on one row"
    );
    assert!(
        near(search.left(), field.right() + px(Theme::SPACE_XS))
            && near(search.right(), panel.right() - px(Theme::SPACE_MD)),
        "Search {search:?} does not end the box's row {field:?} on the panel's inset"
    );

    let chips: Vec<Bounds<Pixels>> = STOCK_KINDS
        .iter()
        .map(|row| bounds(visual, row.id))
        .collect();
    let under = field.bottom() + px(Theme::SPACE_SM);
    assert!(
        near(chips[0].left(), field.left()) && chips.iter().all(|chip| near(chip.top(), under)),
        "the kind chips {chips:?} do not start on the box's edge one row under it {field:?}"
    );

    let empty = bounds(visual, "empty-state-Stock");
    let line = bounds(visual, "empty-state-Stock-line");
    let chips_bottom =
        chips
            .iter()
            .map(|chip| chip.bottom())
            .fold(
                field.bottom(),
                |low, bottom| if bottom > low { bottom } else { low },
            );
    assert!(
        near(empty.top(), chips_bottom + px(Theme::SPACE_MD))
            && near(empty.bottom(), panel.bottom() - px(Theme::SPACE_MD)),
        "the empty state {empty:?} does not fill the panel {panel:?} under the chips"
    );
    assert!(
        near(line.center().x, empty.center().x),
        "the empty state's line {line:?} is not centered in {empty:?}"
    );

    let mut placed = chips;
    placed.extend([field, search, empty]);
    let stray = strays(visual, header, &placed);
    assert!(
        stray.is_empty(),
        "the Stock tab draws outside its form and its empty state:\n{}",
        stray.join("\n")
    );
}

#[gpui::test]
fn the_sfx_tab_at_rest_is_one_empty_state_with_its_load_button(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, "sidebar-sfx");
    open(&state, visual, SidebarTab::CloudSfx);
    let panel = bounds(visual, "sidebar");
    let header = bounds(visual, "sidebar-tabs");
    let empty = bounds(visual, "empty-state-SFX");
    let line = bounds(visual, "empty-state-SFX-line");
    let load = bounds(visual, "cloud-feed-load");

    assert!(
        near(empty.top(), header.bottom() + px(1.0 + Theme::SPACE_MD))
            && near(empty.bottom(), panel.bottom() - px(Theme::SPACE_MD)),
        "the empty state {empty:?} does not fill the panel {panel:?} under the header"
    );
    assert!(
        near(line.center().x, empty.center().x)
            && near(load.center().x, empty.center().x)
            && load.top() > line.bottom()
            && near(load.size.height, px(Theme::CONTROL_HEIGHT)),
        "Load catalog {load:?} is not one control centered under the line {line:?}"
    );
    let stray = strays(visual, header, &[empty]);
    assert!(
        stray.is_empty(),
        "the SFX tab draws outside its empty state:\n{}",
        stray.join("\n")
    );
}

#[gpui::test]
fn every_entry_list_states_its_click_once_and_starts_on_the_column_edge(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    for &tab in SidebarTab::ALL {
        let listed = LISTS.iter().any(|(list, _)| *list == tab);
        assert!(
            listed != OTHER_LAYOUTS.contains(&tab),
            "{tab:?} is in both or neither of LISTS and OTHER_LAYOUTS; place it in one"
        );
    }
    let (state, visual) = drawn_shell(cx, "sidebar-entry-lists");
    for (tab, id) in LISTS {
        open(&state, visual, tab);
        let panel = bounds(visual, "sidebar");
        let edge = panel.left() + px(Theme::SPACE_MD);
        let usage = bounds(visual, &format!("{id}-usage"));
        let list = bounds(visual, id);
        let runs = visual.update(|window, _| window.rendered_text_runs().to_vec());

        let usage_runs: Vec<_> = runs
            .iter()
            .filter(|run| usage.contains(&run.bounds.center()))
            .collect();
        let [line] = usage_runs.as_slice() else {
            panic!("{tab:?}'s usage line is drawn as {usage_runs:?}, not one run");
        };
        assert!(
            near(line.bounds.left(), edge),
            "{tab:?}'s usage line starts at {:?}, not on the column's edge {edge:?}",
            line.bounds.left()
        );

        let rows: Vec<_> = runs
            .iter()
            .filter(|run| list.contains(&run.bounds.center()))
            .collect();
        assert!(!rows.is_empty(), "{tab:?} lists nothing");
        assert!(
            rows.iter().all(|run| run.text != line.text),
            "{tab:?} repeats its usage line `{}` in a row",
            line.text
        );
        let left = rows
            .iter()
            .map(|run| run.bounds.left())
            .fold(list.right(), |low, at| if at < low { at } else { low });
        assert!(
            near(left, edge),
            "{tab:?}'s rows start at {left:?}, not on the column's edge {edge:?}"
        );
        assert!(
            near(list.bottom(), panel.bottom() - px(Theme::SPACE_MD)),
            "{tab:?}'s list ends at {:?}, not on the panel's bottom inset {:?}",
            list.bottom(),
            panel.bottom() - px(Theme::SPACE_MD)
        );
    }
}

#[gpui::test]
fn every_panel_form_starts_its_labels_and_its_controls_on_two_edges(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, "panel-forms");
    for (tab, rows) in FORMS {
        open(&state, visual, tab);
        let panel = bounds(visual, "sidebar");
        let edge = panel.left() + px(Theme::SPACE_MD);
        let inner_right = panel.right() - px(Theme::SPACE_MD);
        let controls: Vec<Bounds<Pixels>> = rows
            .iter()
            .map(|row| bounds(visual, &format!("form-{row}-controls")))
            .collect();
        let column = controls[0].left();
        let mut above = panel.top();
        for (row, control) in rows.iter().zip(&controls) {
            let name = label(visual, row, panel);
            assert!(
                near(name.left(), edge),
                "{tab:?}: the `{row}` label starts at {:?}, not on the column's edge {edge:?}",
                name.left()
            );
            assert!(
                control.left() > name.right(),
                "{tab:?}: the `{row}` controls start at {:?}, over their label ending at {:?}",
                control.left(),
                name.right()
            );
            assert!(
                near(control.left(), column) && near(control.right(), inner_right),
                "{tab:?}: the `{row}` controls span {:?}..{:?}, not the column \
                 {column:?}..{inner_right:?}",
                control.left(),
                control.right()
            );
            assert!(
                name.center().y > control.top()
                    && name.center().y < control.top() + px(Theme::CONTROL_HEIGHT),
                "{tab:?}: the `{row}` label stands at {:?}, off the first line of its \
                 controls at {:?}",
                name.center().y,
                control.top()
            );
            assert!(
                control.top() >= above,
                "{tab:?}: the `{row}` row starts at {:?}, over the row above ending at {above:?}",
                control.top()
            );
            above = control.bottom();
        }
    }

    // A chip row with none of its chips lit leaves the value in force unstated.
    let (threads, commit) = visual.update(|_, app| {
        let captions = &state.read(app).captions;
        (captions.threads, captions.commit)
    });
    assert!(
        crate::panels::captions::thread_offers().any(|offer| offer == threads),
        "the form holds {threads} threads, which no Threads chip offers"
    );
    assert!(
        CaptionCommit::ALL
            .iter()
            .any(|(offer, _, _)| *offer == commit),
        "the form holds {commit:?}, which no Output chip offers"
    );
}
