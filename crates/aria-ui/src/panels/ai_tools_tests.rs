//! A tool name in the assistant's capability list is drawn no larger than
//! the title of the group that holds it.
//!
//! The class closed here: a row set in a larger size than its heading. Each
//! group's tool ids were drawn at `TEXT_TITLE` 12 under a card title at
//! `TEXT_UI` 11, so the rows outranked the heading they belong to.
//!
//! Opens the Assistant in the drawn shell so the capability list renders in
//! the Assistant panel's empty transcript, and reads every group and tool back
//! from [`AppState::ai_tool_groups`], so a group or tool added to the registry
//! is held to the rule on the day it is added. Groups scrolled out of the
//! column draw no text and are not checked.
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
        "the Assistant panel draws no tool under a group title; header drawn \
         {header}, titles drawn {titled:?}, groups {:?}",
        groups
            .iter()
            .map(|(name, tools)| (name, tools.len()))
            .collect::<Vec<_>>()
    );
}

/// WHY: Closes the defect class where the assistant's tool list (`ai_tools`)
/// is registered at a hidden frame tab (`PanelLocation::Inspector`) or opened
/// by a palette command that fails to bring `PANEL_ASSISTANT` forward in the
/// dock tree, leaving "What the assistant can do" undrawn when the Assistant
/// opens. Enumerates `WORKSPACES` and exercises every shipped open route
/// (`Shift+0` key binding, `Window > Assistant` menu row, and the
/// `pal-ai-tools` command palette row), asserting the header is drawn inside
/// `panel-body-aria.panel.assistant`. Does not catch styling of individual
/// tool rows (covered above).
#[gpui::test]
fn every_open_route_in_every_shipped_workspace_draws_the_assistant_tool_list(
    cx: &mut TestAppContext,
) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, "ai-tools-routes-workspaces");
    visual.update(|_, app| app.set_reduce_motion(true));

    for ws in crate::layouts::WORKSPACES {
        for route in ["key-assistant", "win-assistant", "pal-ai-tools"] {
            visual.update(|_, app| {
                state.update(app, |state: &mut AppState, cx| {
                    state.apply_workspace(ws.id, cx);
                    match route {
                        "key-assistant" => {
                            let host =
                                crate::keys::host_of(&state.ctx).expect("KeyHost registered");
                            let binding = host
                                .all()
                                .into_iter()
                                .find(|b| b.id == "key-assistant")
                                .expect("key-assistant binding registered");
                            let crate::keys::KeyBody::Plain(run) = binding.run else {
                                panic!("key-assistant is not a Plain binding");
                            };
                            run(state);
                        }
                        "win-assistant" => {
                            let items = crate::chrome::menu_items::items_for(
                                crate::types::AppMenu::Window,
                                state,
                            );
                            let item = items
                                .iter()
                                .find(|item| item.id == "win-assistant")
                                .expect("win-assistant menu item registered");
                            (item.run)(item.id, state, cx);
                        }
                        "pal-ai-tools" => {
                            let host = crate::commands::host::host_of(&state.ctx)
                                .expect("CommandHost registered");
                            let cmd = host
                                .all()
                                .into_iter()
                                .find(|cmd| cmd.id == "pal-ai-tools")
                                .expect("pal-ai-tools command registered");
                            let crate::commands::host::CommandBody::Plain(run) = cmd.body else {
                                panic!("pal-ai-tools is not a Plain command");
                            };
                            run(state);
                        }
                        _ => unreachable!(),
                    }
                    cx.notify();
                });
            });
            visual.run_until_parked();
            visual.update(|window, _| window.refresh());
            visual.run_until_parked();

            let panel_body = crate::test_support::panel_bounds(
                visual,
                crate::interned::intern("panel-body-aria.panel.assistant"),
            )
            .unwrap_or_else(|| {
                panic!(
                    "workspace `{}` route `{route}` did not draw `panel-body-aria.panel.assistant`",
                    ws.name
                )
            });
            let runs = visual.update(|window, _| window.rendered_text_runs().to_vec());
            let header_in_assistant = runs.iter().any(|run| {
                run.text.as_ref() == "What the assistant can do"
                    && panel_body.contains(&run.bounds.center())
            });
            assert!(
                header_in_assistant,
                "workspace `{}` route `{route}` did not draw `What the assistant can do` inside the Assistant panel",
                ws.name
            );
        }
    }
}
