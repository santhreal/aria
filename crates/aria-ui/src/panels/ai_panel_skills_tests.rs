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
//! * A draw reads the disk. `skills_block` called `load_agent_dir` and
//!   `merge_skills` on every frame; the merged list is held on `AppState` and
//!   refreshed when the panel comes forward, when a skill is installed from
//!   the catalog or a dropped `SKILL.md`, and when an assistant turn finishes.
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
    for skill in skills.iter() {
        let selector: &'static str =
            Box::leak(format!("ai-skill-switch-{}", skill.id).into_boxed_str());
        let switch = visual
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("{:?} draws no switch", skill.name));
        let name = runs
            .iter()
            .filter(|run| run.text == skill.name)
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
        .find(|run| run.text == first.name)
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

/// WHY: Closes "a draw reads the disk" for the Assistant panel's skills list.
/// `skills_block` reads [`AppState::assistant_skills`] on every frame; walking
/// `<data_dir>/agent/skills` per call hit the filesystem at frame rate. The
/// list is held on `AppState` and refreshed when the Assistant panel comes
/// forward, after `install_skill` or `install_skill_file`, and after
/// `finish_agent_turn`. Does not catch a caller outside `skills_block` that
/// calls `load_agent_dir` during a draw.
#[gpui::test]
fn ai_panel_draws_the_skills_the_state_holds(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, "ai-skills-held");
    visual.update(|_, app| app.set_reduce_motion(true));
    let (data_dir, agent_dir) = visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.dock_tree = DockTree::editing_for_width(1920.0);
            state.focus_pane(crate::types::ActivePanel::Ai);
            cx.notify();
            (state.data_dir.clone(), state.agent_dir())
        })
    });
    visual.run_until_parked();
    visual.update(|window, _| window.refresh());
    visual.run_until_parked();

    let drawn_has = |visual: &mut gpui::VisualTestContext, label: &str| {
        visual.update(|window, _| {
            window
                .rendered_text_runs()
                .iter()
                .any(|run| run.text.as_ref() == label)
        })
    };
    assert!(
        !drawn_has(visual, "Hand cut"),
        "uninstalled skill must not be drawn initially"
    );

    let hand_dir = agent_dir.join("skills/hand-cut");
    std::fs::create_dir_all(&hand_dir).expect("hand-cut dir");
    std::fs::write(
        hand_dir.join("SKILL.md"),
        "---\nname: Hand cut\ndescription: Trim by hand\n---\nBody.\n",
    )
    .expect("write hand-cut skill");

    visual.update(|window, _| window.refresh());
    visual.run_until_parked();
    assert!(
        !drawn_has(visual, "Hand cut"),
        "AI panel read the skills directory during a draw instead of drawing the held list"
    );

    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.focus_pane(crate::types::ActivePanel::Ai);
            cx.notify();
        });
    });
    visual.run_until_parked();
    visual.update(|window, _| window.refresh());
    visual.run_until_parked();
    assert!(
        drawn_has(visual, "Hand cut"),
        "bringing the Assistant panel forward did not refresh assistant_skills"
    );

    let catalog_file = data_dir.join("catalog-cut.md");
    std::fs::write(
        &catalog_file,
        "---\nname: Catalog cut\ndescription: From catalog\n---\nBody.\n",
    )
    .expect("write catalog skill file");
    let card = crate::cloud::CloudCard {
        id: "catalog-cut".into(),
        name: "Catalog cut".into(),
        detail: String::new().into(),
        file_url: "https://cdn.example/catalog-cut.md".into(),
        extension: "md".into(),
        attribution: String::new(),
    };
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state
                .install_skill(&catalog_file, &card)
                .expect("install_skill");
            cx.notify();
        });
    });
    visual.run_until_parked();
    visual.update(|window, _| window.refresh());
    visual.run_until_parked();
    assert!(
        drawn_has(visual, "Catalog cut"),
        "install_skill did not refresh assistant_skills"
    );

    let dropped_dir = data_dir.join("dropped-cut");
    std::fs::create_dir_all(&dropped_dir).expect("dropped-cut dir");
    let dropped_file = dropped_dir.join("SKILL.md");
    std::fs::write(
        &dropped_file,
        "---\nname: Dropped cut\ndescription: From drop\n---\nBody.\n",
    )
    .expect("write dropped skill file");
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state
                .install_skill_file(&dropped_file)
                .expect("install_skill_file");
            cx.notify();
        });
    });
    visual.run_until_parked();
    visual.update(|window, _| window.refresh());
    visual.run_until_parked();
    assert!(
        drawn_has(visual, "Dropped cut"),
        "install_skill_file did not refresh assistant_skills"
    );

    let turn_dir = agent_dir.join("skills/turn-cut");
    std::fs::create_dir_all(&turn_dir).expect("turn-cut dir");
    std::fs::write(
        turn_dir.join("SKILL.md"),
        "---\nname: Turn cut\ndescription: From turn\n---\nBody.\n",
    )
    .expect("write turn-cut skill");
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.finish_agent_turn(None);
            cx.notify();
        });
    });
    visual.run_until_parked();
    visual.update(|window, _| window.refresh());
    visual.run_until_parked();
    assert!(
        drawn_has(visual, "Turn cut"),
        "finish_agent_turn did not refresh assistant_skills"
    );
}
