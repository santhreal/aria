//! Applying a plan that contains a render.
//!
//! The class this closes: a queued render folded into the plan's own history
//! entry, or started against a half-applied sequence. A plan is one undo, and a
//! render is not part of it — the file is written after the plan is in force and
//! is adopted as its own entry, so undoing the plan does not also silently undo
//! an import, and a render started mid-replay would encode a
//! sequence that never existed.
//!
//! The ordering is observable without a GPU because the file's name is a digest
//! of what the render would show: the render standing on disk is the one the
//! clip asks for *after* the plan's edit, so a render started before that edit
//! would ask for a name that is not there and spawn an encode instead of
//! adopting.
//!
//! What it does not catch: the encode itself. That is `aria-render`'s coverage.

use aria_commands::{Command, EditCommand};

use aria_ai::WireCommand;
use aria_ai::wire::{AddEffect, RenderAndReplace};

use crate::app::assistant::PlanStep;
use crate::app::fixtures::{bake_destination, bakeable, rendered_file};
use crate::scratch_pool::scratch;

/// A plan whose last step is a render: the edits land as one entry, the render
/// is adopted as a second, and the file it adopts is the one the finished plan
/// asks for.
#[test]
fn a_planned_render_runs_after_the_plan_and_is_its_own_history_entry() {
    let (mut state, clip) = bakeable();
    state.project_path = Some(scratch("plan-render").join("planned.aria"));
    let raw = clip.raw();

    // Stand the render the *finished* plan asks for on disk: apply the plan's
    // edit, take the name, then take the edit back.
    state
        .apply_command(Command::Edit(EditCommand::AddEffect {
            clip,
            effect_id: "vignette".into(),
        }))
        .expect("the look the plan will add");
    let dest = bake_destination(&state, clip);
    assert!(state.engine.lock().undo(), "put the look back");
    state.sync_from_engine();
    std::fs::create_dir_all(dest.parent().expect("a parent")).expect("the renders directory");
    std::fs::copy(rendered_file(), &dest).expect("stand a render in");

    state.assistant.plan = vec![
        PlanStep {
            description: "added vignette".into(),
            command: WireCommand::AddEffect(AddEffect {
                clip: raw,
                effect: "vignette".into(),
            }),
            outcome: None,
        },
        PlanStep {
            description: "render it".into(),
            command: WireCommand::RenderAndReplace(RenderAndReplace { clip: raw }),
            outcome: None,
        },
    ]
    .into();

    let applied = state.apply_agent_plan().expect("apply the plan");
    assert_eq!(applied, 1, "the render was counted as an applied edit");
    assert!(
        state
            .transcript
            .iter()
            .any(|entry| entry.content.contains("decodes through the render on disk")),
        "the transcript does not report the render as adopted, so it ran before \
         the plan's edit and asked for a file that was not there"
    );

    {
        let engine = state.engine.lock();
        let baked = engine.project().clip(clip).expect("the clip");
        assert!(baked.is_baked(), "the planned render was not adopted");
        assert!(
            baked.effects.is_empty(),
            "the render did not fold in the chain the plan left"
        );
    }

    // Two entries, in that order: undoing the adoption leaves the plan standing.
    // A render started inside the plan's group would take both back at once.
    assert!(state.engine.lock().undo(), "the adoption left no entry");
    state.sync_from_engine();
    {
        let engine = state.engine.lock();
        let back = engine.project().clip(clip).expect("the clip");
        assert!(!back.is_baked(), "one undo did not put the clip back");
        assert_eq!(
            back.effects.len(),
            2,
            "undoing the render also undid what the plan added"
        );
    }
    assert!(state.engine.lock().undo(), "the plan left no entry");
    state.sync_from_engine();
    let engine = state.engine.lock();
    assert_eq!(
        engine.project().clip(clip).expect("the clip").effects.len(),
        1,
        "undoing the plan did not take back what it added"
    );
}
