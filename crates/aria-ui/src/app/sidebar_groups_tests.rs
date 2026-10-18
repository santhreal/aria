//! Contracts for the sidebar's group and tab chip rows.
//!
//! The class closed here: a tab no chip reaches, and a group chip that moves
//! the panel to a tab of another group. What is asserted, for every group:
//!
//! - A click on the group chip opens the group's first tab.
//! - The second row lists exactly the group's tabs when it has more than one
//!   and they are not the pool's, and is not drawn otherwise.
//! - A click on each listed tab opens it, and a click on the open group's
//!   chip keeps that tab.
//! - A group of pool tabs reaches each of them through the Media tab's Audio
//!   only switch, so a third pool tab, which the switch cannot reach, fails.
//!
//! Not covered: the chip look, which `components::chip` draws.

use crate::interned::intern;
use crate::types::{SidebarGroup, SidebarTab};

/// Click the centre of the element drawn under `selector`.
fn click(visual: &mut gpui::VisualTestContext, selector: &str) {
    let bounds = visual
        .debug_bounds(intern(selector))
        .unwrap_or_else(|| panic!("the sidebar draws no {selector}"));
    visual.simulate_click(bounds.center(), gpui::Modifiers::none());
    visual.run_until_parked();
}

#[gpui::test]
fn each_group_opens_its_first_tab_and_lists_only_its_tabs(cx: &mut gpui::TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = crate::test_support::drawn_shell(cx, "sidebar-groups");
    visual.update(|_, app| state.update(app, |_, cx| cx.notify()));
    visual.run_until_parked();
    let open =
        |visual: &mut gpui::VisualTestContext| visual.read(|app| state.read(app).sidebar_tab);

    for group in SidebarGroup::ALL {
        let tabs = group.tabs();
        click(visual, &format!("sidebar-group-{}", group.label()));
        assert_eq!(open(visual), tabs[0], "{group:?} opened another tab");

        let pool = tabs.iter().all(|tab| tab.shows_pool());
        for &tab in SidebarTab::ALL {
            let drawn = visual
                .debug_bounds(intern(&format!("sidebar-tab-{}", tab.label())))
                .is_some();
            let listed = tabs.len() > 1 && !pool && tab.group() == group;
            assert_eq!(
                drawn, listed,
                "with {group:?} open, the {tab:?} chip drawn is {drawn}"
            );
        }

        if pool {
            let mut reached = vec![open(visual)];
            for _ in 1..tabs.len() {
                click(visual, "pool-audio-only");
                reached.push(open(visual));
            }
            reached.sort_by_key(|tab| tab.label());
            let mut listed = tabs.to_vec();
            listed.sort_by_key(|tab| tab.label());
            assert_eq!(reached, listed, "the Audio only switch reached other tabs");
        } else if tabs.len() > 1 {
            for &tab in tabs {
                click(visual, &format!("sidebar-tab-{}", tab.label()));
                assert_eq!(open(visual), tab, "a click on {tab:?} opened another tab");
            }
        }
        let last = open(visual);
        click(visual, &format!("sidebar-group-{}", group.label()));
        assert_eq!(
            open(visual),
            last,
            "a click on the open {group:?} chip moved off {last:?}"
        );
    }
}
