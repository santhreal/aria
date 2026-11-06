//! A tool name in the assistant's capability list is drawn no larger than
//! the title of the group that holds it.
//!
//! The class closed here: a row set in a larger size than its heading. Each
//! group's tool ids were drawn at `TEXT_TITLE` 12 under a card title at
//! `TEXT_UI` 11, so the rows outranked the heading they belong to.
//!
//! Opens the Assistant in the drawn shell and brings Effect Controls forward
//! in its frame (the Editing layout opens Project in front of it), so the list
//! stacks in the inspector column, and reads every group and tool back from
//! [`AppState::ai_tool_groups`], so a group or tool added to the registry is
//! held to the rule on the day it is added. Groups scrolled out of the column
//! draw no text and are not checked.
//!
//! Not covered: the tool rows' color and weight.

use gpui::TestAppContext;

use crate::app::AppState;
use crate::dock::DockTree;
use crate::test_support::drawn_shell;

#[gpui::test]
fn every_tool_is_drawn_no_larger_than_its_group_title(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, "ai-tools-sizes");
    visual.update(|_, app| app.set_reduce_motion(true));
    let groups = visual.update(|_, app| {
        state.update(app, |state: &mut AppState, cx| {
            state.dock_tree = DockTree::editing_for_width(1920.0);
            state.focus_pane(crate::types::ActivePanel::Ai);
            state.dock_tree.open(crate::dock::PANEL_INSPECTOR);
            cx.notify();
            state.ai_tool_groups()
        })
    });
    visual.run_until_parked();
    visual.update(|window, _| window.refresh());
    visual.run_until_parked();
    let runs = visual.update(|window, _| window.rendered_text_runs().to_vec());

    let mut checked = 0;
    for (name, tools) in &groups {
        let title = format!("{name} · {}", tools.len());
        let Some(title_run) = runs.iter().find(|run| run.text.as_ref() == title) else {
            continue;
        };
        for tool in tools {
            for run in runs.iter().filter(|run| run.text == *tool) {
                assert!(
                    run.font_size <= title_run.font_size,
                    "`{tool}` is drawn at {:?}, over its group title `{title}` at {:?}",
                    run.font_size,
                    title_run.font_size
                );
                checked += 1;
            }
        }
    }
    let header = runs
        .iter()
        .any(|run| run.text.as_ref() == "What the assistant can do");
    let titled: Vec<&str> = runs
        .iter()
        .map(|run| run.text.as_ref())
        .filter(|text| text.contains(" · "))
        .collect();
    assert!(
        checked > 0,
        "the Assistant's inspector column draws no tool under a group title; header drawn \
         {header}, titles drawn {titled:?}, groups {:?}",
        groups
            .iter()
            .map(|(name, tools)| (name, tools.len()))
            .collect::<Vec<_>>()
    );
}
