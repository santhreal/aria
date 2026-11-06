//! The Assistant panel lists every skill the agent is offered, and each is a
//! row with a switch: its name, its description, and the switch that states
//! whether this conversation offers it.
//!
//! The classes closed here:
//!
//! * A list narrower than the prompt. The panel read the skills on disk while
//!   the prompt merged the bundled set in, so a fresh profile read "No skills
//!   installed" and no bundled skill could be held back. A fresh profile must
//!   draw two or more rows.
//! * An on/off state typed as a glyph. The list printed `✓` or `○` before each
//!   name, inside the name's own text, and washed the row by hand. Each name
//!   must be a run of its own with a switch right of it on the same line; a
//!   press on the name holds the skill back and a second press offers it
//!   again.
//!
//! Not covered: the switch's paint, which is not a text run, an installed
//! skill replacing a bundled one (`merge_skills`' own tests), and the cloud
//! catalog below the list.

use gpui::{Modifiers, TestAppContext, px};

use crate::dock::{DockTree, FrameId, PANEL_ASSISTANT, Zone};
use crate::test_support::drawn_shell;

const EPS: f32 = 1.0;

#[gpui::test]
fn every_installed_skill_draws_a_switch_beside_its_name(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, "skill-switches");
    visual.update(|_, app| app.set_reduce_motion(true));
    let skills = visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.dock_tree = DockTree::editing_for_width(1920.0);
            state
                .dock_tree
                .dock(PANEL_ASSISTANT, FrameId::RIGHT, Zone::Center);
            cx.notify();
            state.assistant_skills()
        })
    });
    assert!(
        skills.len() >= 2,
        "the bundled skills draw a list of one row or none: {skills:?}"
    );
    visual.run_until_parked();
    visual.update(|window, _| window.refresh());
    visual.run_until_parked();
    let runs = visual.update(|window, _| window.rendered_text_runs().to_vec());
    for skill in &skills {
        let selector: &'static str =
            Box::leak(format!("ai-skill-switch-{}", skill.id).into_boxed_str());
        let switch = visual
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("{:?} draws no switch", skill.name));
        let name = runs
            .iter()
            .filter(|run| run.text.as_ref() == skill.name)
            .find(|run| (run.bounds.center().y - switch.center().y).abs() < px(EPS))
            .unwrap_or_else(|| {
                panic!(
                    "{:?} is not drawn as a run of its own on its switch's line",
                    skill.name
                )
            });
        assert!(
            switch.left() > name.bounds.right(),
            "{:?}'s switch at {switch:?} stands over its name at {:?}",
            skill.name,
            name.bounds
        );
    }
    let first = &skills[0];
    let at = runs
        .iter()
        .find(|run| run.text.as_ref() == first.name)
        .map(|run| run.bounds.center())
        .expect("the first skill's name is drawn");
    let offered = |visual: &mut gpui::VisualTestContext| {
        visual.read(|app| state.read(app).assistant_skill_enabled(&first.id))
    };
    assert!(offered(visual), "an installed skill starts offered");
    visual.simulate_click(at, Modifiers::none());
    visual.run_until_parked();
    assert!(!offered(visual), "a press on the name holds the skill back");
    visual.simulate_click(at, Modifiers::none());
    visual.run_until_parked();
    assert!(offered(visual), "a second press offers the skill again");
}
