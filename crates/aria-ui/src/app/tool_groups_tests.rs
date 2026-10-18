//! The tool bar's grouped buttons.
//!
//! The class closed here: a tool bar that drew a button per tool, thirteen in a
//! row, and a variants popover that listed one hand-written set of siblings
//! while every other group had none. These pin, against the catalog rather
//! than a written list, that each group is one run of rows in bar order, that
//! the bar shows the armed tool and every other group's first row, that a
//! button's popover lists its whole group (or the Pen's modes), that every tool
//! is reachable by pointer from the bar at rest, and that the drawn bar has one
//! button per group and swaps the shown member when another one is armed. The
//! edit-mode chip beside the tools states a value ("Ripple") next to a group
//! whose tool has the same name, so its tip is pinned in both modes. The bar
//! draws no toggle the Timeline header draws.
//!
//! Not covered: the popover's drawn rows (the state calls they run are pinned
//! in `tool_hold_tests`), and the 300 ms hold timer, which runs on the
//! executor.

use gpui::{TestAppContext, VisualTestContext};

use crate::app::clip_text::tool_hold::ToolVariant;
use crate::test_support::drawn_shell;
use crate::tool_catalog::{ToolGroup, tool_specs};
use crate::tooltip::{CONTROL_TIPS, SHOW_DELAY};
use crate::types::{EditMode, EditorTool, PenVariant};

/// The first row of `group` in the table: the tool its button shows at rest.
fn first_row(group: ToolGroup) -> EditorTool {
    tool_specs()
        .iter()
        .find(|spec| spec.group == group)
        .map(|spec| spec.tool)
        .unwrap_or_else(|| panic!("{group:?} has no row"))
}

#[test]
fn every_group_is_one_run_of_rows_in_bar_order() {
    let mut runs: Vec<ToolGroup> = tool_specs().iter().map(|spec| spec.group).collect();
    runs.dedup();
    assert_eq!(
        runs,
        ToolGroup::ALL,
        "the table's groups are split, out of bar order, or missing a row"
    );
}

#[test]
fn the_bar_shows_the_armed_tool_and_every_other_groups_first_row() {
    for armed in EditorTool::all() {
        let shown: Vec<EditorTool> = ToolGroup::ALL.map(|group| group.shown(armed)).to_vec();
        assert_eq!(
            shown.iter().filter(|&&tool| tool == armed).count(),
            1,
            "{armed:?} is armed and drawn {shown:?}"
        );
        for (group, tool) in ToolGroup::ALL.into_iter().zip(shown) {
            assert_eq!(tool.group(), group, "{group:?} shows {tool:?}");
            if group != armed.group() {
                assert_eq!(
                    tool,
                    first_row(group),
                    "{group:?} shows {tool:?} while {armed:?} is armed"
                );
            }
        }
    }
}

#[test]
fn a_buttons_variants_are_its_whole_group_or_the_pens_modes() {
    for tool in EditorTool::all() {
        let variants = tool.variants();
        assert_eq!(
            tool.has_variants(),
            !variants.is_empty(),
            "{tool:?} draws a triangle that disagrees with its list"
        );
        let expected: Vec<ToolVariant> = if tool.pens() {
            PenVariant::ALL.map(ToolVariant::Pen).to_vec()
        } else {
            let group: Vec<EditorTool> = tool.group().tools().collect();
            if group.len() > 1 {
                group.into_iter().map(ToolVariant::Tool).collect()
            } else {
                Vec::new()
            }
        };
        assert_eq!(variants, expected, "{tool:?} offers the wrong list");
    }
}

#[test]
fn every_tool_is_reachable_from_the_bar_at_rest() {
    let mut reachable = Vec::new();
    for group in ToolGroup::ALL {
        let shown = group.shown(EditorTool::Pointer);
        reachable.push(shown);
        reachable.extend(shown.variants().into_iter().map(|variant| match variant {
            ToolVariant::Tool(tool) => tool,
            ToolVariant::Pen(_) => EditorTool::Pen,
        }));
    }
    for tool in EditorTool::all() {
        assert!(
            reachable.contains(&tool),
            "{tool:?} has no button and no popover row"
        );
    }
}

fn drawn_tools(visual: &mut VisualTestContext) -> Vec<EditorTool> {
    visual.run_until_parked();
    EditorTool::all()
        .filter(|tool| visual.debug_bounds(tool.control_id()).is_some())
        .collect()
}

#[gpui::test]
fn the_drawn_bar_has_a_button_per_group_and_shows_the_armed_member(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, "tool-groups");
    let at_rest: Vec<EditorTool> = ToolGroup::ALL.map(first_row).to_vec();
    assert_eq!(drawn_tools(visual), at_rest, "the bar at rest");

    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.tool = EditorTool::Slip;
            cx.notify();
        })
    });
    let drawn = drawn_tools(visual);
    assert_eq!(drawn.len(), ToolGroup::ALL.len(), "{drawn:?}");
    assert!(
        drawn.contains(&EditorTool::Slip),
        "the armed Slip is not drawn"
    );
    assert!(
        !drawn.contains(&first_row(ToolGroup::Trim)),
        "the trim button still shows its first row while Slip is armed"
    );
}

/// WHY: the chip read as a second Ripple tool, with no tip to say it sets what
/// a drop does. Hovers the drawn chip in each mode and needs the tip after the
/// rest delay. Does not check the tip's wording.
#[gpui::test]
fn the_edit_mode_chip_has_a_tip_in_both_modes(cx: &mut TestAppContext) {
    let executor = cx.executor();
    let (state, visual) = drawn_shell(cx, "tool-edit-mode-tip");
    let none = gpui::Modifiers::none();
    let away = gpui::point(gpui::px(1.), gpui::px(1.));
    for mode in [EditMode::Ripple, EditMode::Overwrite] {
        visual.update(|_, app| {
            state.update(app, |state, cx| {
                state.edit_mode = mode;
                cx.notify();
            })
        });
        visual.run_until_parked();
        let chip = visual
            .debug_bounds("edit-mode")
            .unwrap_or_else(|| panic!("the tool bar draws no edit-mode chip in {mode:?}"))
            .center();
        visual.simulate_mouse_move(away, None, none);
        visual.simulate_mouse_move(chip, None, none);
        executor.advance_clock(SHOW_DELAY * 2);
        visual.run_until_parked();
        assert!(
            visual.debug_bounds("tooltip").is_some(),
            "no tip on the {mode:?} chip"
        );
    }
}

/// WHY: the tool bar repeated the Timeline header's snapping, linked-selection
/// and display-settings toggles, so the window drew three actions twice. Every
/// control in the tip table that draws inside the tool bar must be a tool or the
/// edit-mode chip, so a toggle added to the bar goes red until it is listed
/// here; the header still draws the three. Does not see a control drawn without
/// a debug selector (the workspace pills and Export).
#[gpui::test]
fn the_tool_bar_draws_only_the_tools_and_the_edit_mode_chip(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (_state, visual) = drawn_shell(cx, "tool-bar-contents");
    visual.run_until_parked();
    let bar = visual
        .debug_bounds("tool-bar")
        .expect("the window draws the tool bar");
    let tools: Vec<&str> = EditorTool::all().map(EditorTool::control_id).collect();
    let mut drawn = 0;
    let mut extra = Vec::new();
    for tip in CONTROL_TIPS {
        let Some(bounds) = visual.debug_bounds(tip.control) else {
            continue;
        };
        if !bar.contains(&bounds.center()) {
            continue;
        }
        drawn += 1;
        if !tools.contains(&tip.control) && tip.control != "edit-mode" {
            extra.push(tip.control);
        }
    }
    assert_eq!(
        extra,
        Vec::<&str>::new(),
        "the tool bar draws these besides its tools"
    );
    assert_eq!(
        drawn,
        ToolGroup::ALL.len() + 1,
        "the tool bar draws a button per group and the edit-mode chip"
    );
    for header in [
        "timeline-snap",
        "timeline-linked",
        "timeline-display-settings",
    ] {
        assert!(
            visual.debug_bounds(header).is_some(),
            "the Timeline header does not draw {header}"
        );
    }
}
