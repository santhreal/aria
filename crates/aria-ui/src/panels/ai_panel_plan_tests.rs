//! A pending plan's steps stand in two columns: the bullet, and the step's
//! line with the tool it arrives under beneath it.
//!
//! The class closed here: an indent typed as spaces. A step's tool name stood
//! under its line by three leading spaces, which in the panel's face start
//! neither where the line starts nor at one place for every step. Every step
//! is drawn, and each line must start at one left edge right of its bullet,
//! with its tool name starting where it does.
//!
//! Not covered: a line that wraps, and the card's header and buttons.

use aria_ai::WireCommand;
use aria_ai::wire::{AddEffect, RenderAndReplace};
use gpui::{Bounds, TestAppContext, point, px, size};

use crate::app::assistant::PlanStep;
use crate::dock::{DockTree, FrameId, PANEL_ASSISTANT, Zone};
use crate::test_support::drawn_shell;

const EPS: f32 = 0.51;

#[gpui::test]
fn a_plan_step_stands_its_tool_name_under_its_line(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, "plan-card-columns");
    visual.update(|_, app| app.set_reduce_motion(true));
    let steps: Vec<(String, &'static str)> = visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.dock_tree = DockTree::editing_for_width(1920.0);
            state
                .dock_tree
                .dock(PANEL_ASSISTANT, FrameId::RIGHT, Zone::Center);
            state.assistant.plan = vec![
                PlanStep {
                    description: "Added Vignette to clip 1".into(),
                    command: WireCommand::AddEffect(AddEffect {
                        clip: 1,
                        effect: "vignette".into(),
                    }),
                    outcome: None,
                },
                PlanStep {
                    description: "Rendered clip 1 and replaced it".into(),
                    command: WireCommand::RenderAndReplace(RenderAndReplace { clip: 1 }),
                    outcome: None,
                },
            ]
            .into();
            cx.notify();
            state
                .assistant
                .plan
                .iter()
                .map(|step| (step.description.clone(), step.command.tool_name()))
                .collect()
        })
    });
    visual.run_until_parked();
    visual.update(|_, app| state.update(app, |_, cx| cx.notify()));
    visual.run_until_parked();
    let [x, y, w, h] = visual
        .read(|app| {
            state
                .read(app)
                .dock_frame_rects
                .lock()
                .get(&FrameId::RIGHT)
                .copied()
        })
        .expect("the right frame is drawn");
    let frame = Bounds::new(point(px(x), px(y)), size(px(w), px(h)));
    let runs = visual.update(|window, _| window.rendered_text_runs().to_vec());
    let drawn = |text: &str| {
        runs.iter()
            .filter(|run| run.text.as_ref() == text && frame.contains(&run.bounds.center()))
            .map(|run| run.bounds)
            .collect::<Vec<_>>()
    };
    let bullets = drawn("·");
    assert_eq!(
        bullets.len(),
        steps.len(),
        "the card draws one bullet per step: {bullets:?}"
    );
    let mut edge = None;
    for (line, tool) in &steps {
        let [line_at] = drawn(line)[..] else {
            panic!("the card draws the step {line:?} once");
        };
        let [tool_at] = drawn(tool)[..] else {
            panic!("the card draws the tool {tool:?} once");
        };
        let edge = *edge.get_or_insert(line_at.left());
        assert!(
            (line_at.left() - edge).abs() < px(EPS),
            "{line:?} starts at {:?}, the first step's line at {edge:?}",
            line_at.left()
        );
        assert!(
            (tool_at.left() - line_at.left()).abs() < px(EPS),
            "{tool:?} starts at {:?}, its line {line:?} at {:?}",
            tool_at.left(),
            line_at.left()
        );
        assert!(
            tool_at.top() >= line_at.bottom() - px(EPS),
            "{tool:?} is not drawn under {line:?}"
        );
        assert!(
            bullets.iter().any(|bullet| bullet.right() <= line_at.left()
                && (bullet.top() - line_at.top()).abs() < px(EPS)),
            "no bullet stands left of {line:?} on its row"
        );
    }
}
