//! Assistant turn execution and precheck tests.
//!
//! Only `#[test]` functions are defined here. The tests reach the fixtures and
//! helpers in `assistant_tests.rs` through `use super::*`.

use aria_core::count::counted;

use super::*;

/// The refusal order is a promise: an empty box is answered before anything
/// costs a lock, a thread, or a request. Reordering these would send a blank
/// prompt to a provider, or open Settings for a box that is still empty.
#[test]
fn an_empty_prompt_is_the_first_refusal() {
    let mut state = state();
    // Whitespace is empty: a space bar is not a prompt.
    assert_eq!(
        state.assistant_precheck("   ").err().as_deref(),
        Some("Type a prompt first")
    );
    // Running and unconfigured both hold, and the empty box still wins.
    state.assistant.running = true;
    assert_eq!(
        state.assistant_precheck("").err().as_deref(),
        Some("Type a prompt first")
    );
}
/// A second prompt during a turn is refused, not queued: two turns sharing one
/// engine would interleave their edit groups and neither could be undone whole.
#[test]
fn a_turn_in_flight_refuses_the_next_prompt() {
    let mut state = state();
    state.assistant.running = true;
    assert_eq!(
        state.assistant_precheck("cut it").err().as_deref(),
        Some("The assistant is still working on the last prompt")
    );
}
/// With a real prompt and no turn running, the provider is the remaining
/// question — and a scratch profile has none configured.
#[test]
fn an_unconfigured_provider_is_the_last_refusal() {
    let state = state();
    let refusal = state.assistant_precheck("cut it").err().expect("refusal");
    assert_eq!(
        refusal, "Configure an AI provider in Settings to send this prompt.",
        "the one refusal that points at another dialog must be the provider"
    );
}
/// Deltas join one bubble. A bubble per delta would print an answer one word to
/// a line.
#[test]
fn text_deltas_join_one_bubble() {
    let mut state = state();
    state.fold_beat(Beat::Text("Trim".into()));
    state.fold_beat(Beat::Text("ming".into()));
    assert_eq!(kinds(&state), vec![AgentEntryKind::Assistant]);
    assert_eq!(state.transcript[0].content, "Trimming");
}
/// Reasoning is its own bubble, never appended to the answer: a summary of how
/// the model thought is not part of what it said.
#[test]
fn reasoning_never_joins_the_answer() {
    let mut state = state();
    state.fold_beat(Beat::Reasoning("checking the lanes".into()));
    state.fold_beat(Beat::Text("Done".into()));
    assert_eq!(
        kinds(&state),
        vec![AgentEntryKind::Reasoning, AgentEntryKind::Assistant]
    );
    assert_eq!(state.transcript[0].content, "checking the lanes");
    assert_eq!(state.transcript[1].content, "Done");
}
/// An edit breaks the bubble. The class this closes: an answer written around an
/// applied edit used to read as one sentence with the edit note after it, so
/// "I'll split it" and "that worked" were the same paragraph.
#[test]
fn an_edit_breaks_the_bubble() {
    let mut state = state();
    state.fold_beat(Beat::Text("Splitting".into()));
    state.fold_beat(Beat::Action("split clip 1 at 2.00s".into()));
    state.fold_beat(Beat::Text("Done".into()));
    assert_eq!(
        kinds(&state),
        vec![
            AgentEntryKind::Assistant,
            AgentEntryKind::Action,
            AgentEntryKind::Assistant
        ]
    );
    assert_eq!(state.transcript[2].content, "Done");
}
/// A provider that streams the answer and returns it whole must not print it
/// twice.
#[test]
fn a_streamed_answer_is_not_repeated_at_the_end() {
    let mut state = state();
    state.fold_beat(Beat::Text("Trimmed the head".into()));
    state.fold_beat(Beat::Done(Box::new(finished(
        "Trimmed the head",
        PromptStatus::Completed,
        1,
    ))));
    assert_eq!(kinds(&state), vec![AgentEntryKind::Assistant]);
    assert_eq!(state.transcript[0].content, "Trimmed the head");
}
/// A turn that answered nothing but streamed nothing either still gets its
/// answer, or the panel would show only the edits.
#[test]
fn an_unstreamed_answer_is_appended_once() {
    let mut state = state();
    state.finish_agent_turn(Some(finished("All set", PromptStatus::Completed, 0)));
    assert_eq!(kinds(&state), vec![AgentEntryKind::Assistant]);
    assert_eq!(state.transcript[0].content, "All set");
}
/// A completed turn is remembered, so the next prompt can say "undo that".
#[test]
fn a_completed_turn_keeps_the_conversation() {
    let mut state = state();
    state.assistant.running = true;
    state.finish_agent_turn(Some(finished("ok", PromptStatus::Completed, 2)));
    assert_eq!(state.assistant.history.len(), 1);
    assert!(!state.assistant.running);
    assert_eq!(
        state.status_message.as_deref(),
        Some("The assistant made 2 edits")
    );
}
/// The class this closes: an interrupted turn whose partial conversation was
/// kept made the next prompt answer a question the model never finished asking.
/// `aria-ai` returns no messages for an abort, and none are invented here. The
/// edits are gone with it — the loop rolls the group back — so a turn that had
/// applied any says how many were undone, in the transcript and in the status
/// line, instead of leaving action rows describing a timeline that no longer
/// holds them.
#[test]
fn an_aborted_turn_is_not_remembered() {
    let mut state = state();
    state.assistant.running = true;
    state.finish_agent_turn(Some(finished(
        "",
        PromptStatus::Aborted("cancelled".into()),
        1,
    )));
    assert!(
        state.assistant.history.is_empty(),
        "an abort must contribute no conversational memory"
    );
    assert_eq!(
        kinds(&state),
        vec![AgentEntryKind::Error, AgentEntryKind::Action]
    );
    assert_eq!(state.transcript[1].content, "1 edit undone");
    assert_eq!(
        state.status_message.as_deref(),
        Some("The assistant stopped: cancelled, 1 edit undone")
    );
    assert!(!state.assistant.running);
}
/// A turn stopped before it edited anything has nothing to report undoing: a
/// "0 edit(s) undone" row would point at a change that was never made.
#[test]
fn an_aborted_turn_that_edited_nothing_says_only_that_it_stopped() {
    let mut state = state();
    state.assistant.running = true;
    state.finish_agent_turn(Some(finished(
        "",
        PromptStatus::Aborted("cancelled".into()),
        0,
    )));
    assert_eq!(kinds(&state), vec![AgentEntryKind::Error]);
    assert_eq!(
        state.status_message.as_deref(),
        Some("The assistant stopped: cancelled")
    );
}
/// A dry run says nothing was applied, and says it in the number of steps Apply
/// would replay. Reporting a plan as an edit would point at a change that is
/// not there; reporting a count the plan does not hold would
/// have them approving three edits and getting two.
#[test]
fn a_dry_run_says_nothing_was_applied() {
    let mut state = state();
    let plan = marker_plan();
    let steps = plan.len();
    state.finish_agent_turn(Some(Finished {
        plan,
        ..finished("here is the plan", PromptStatus::DryRun, steps)
    }));
    assert_eq!(
        kinds(&state),
        vec![AgentEntryKind::Assistant, AgentEntryKind::Action]
    );
    assert_eq!(
        state.transcript[1].content,
        format!(
            "{}, nothing applied",
            counted(steps, "planned edit", "planned edits")
        )
    );
    assert_eq!(state.assistant.plan.len(), steps, "Apply replays this plan");
}
/// A job that died without reporting must not leave the panel saying "working"
/// forever, and must not leave Stop as the only button.
#[test]
fn a_lost_turn_stops_running() {
    let mut state = state();
    state.assistant.running = true;
    state.finish_agent_turn(None);
    assert!(!state.assistant.running);
    assert!(state.assistant.cancel.is_none());
    assert_eq!(kinds(&state), vec![AgentEntryKind::Error]);
}
/// Up walks back through sent prompts; Down walks forward and past the newest
/// clears the box, so recall always has a way out.
#[test]
fn recall_walks_back_and_clears_past_the_newest() {
    let mut state = state();
    state.assistant.remember("first");
    state.assistant.remember("second");

    state.recall_agent_prompt(-1);
    assert_eq!(state.assistant.prompt, "second");
    state.recall_agent_prompt(-1);
    assert_eq!(state.assistant.prompt, "first");
    // The oldest is the floor, not a wrap: walking off the end would silently
    // hand back the newest prompt as if it were older.
    state.recall_agent_prompt(-1);
    assert_eq!(state.assistant.prompt, "first");

    state.recall_agent_prompt(1);
    assert_eq!(state.assistant.prompt, "second");
    state.recall_agent_prompt(1);
    assert_eq!(
        state.assistant.prompt, "",
        "past the newest is an empty box"
    );
}
/// Arrows in an empty history do nothing rather than clearing what is typed.
#[test]
fn recall_with_no_history_leaves_the_box_alone() {
    let mut state = state();
    state.assistant.focused = true;
    state.assistant.prompt = "half a thought".into();
    state.recall_agent_prompt(-1);
    state.recall_agent_prompt(1);
    assert_eq!(state.assistant.prompt, "half a thought");
}
/// Typing leaves recall: the next Up must offer the newest sent prompt, not the
/// one before wherever recall happened to be sitting.
#[test]
fn typing_leaves_recall() {
    let mut state = state();
    state.assistant.focused = true;
    state.assistant.remember("older");
    state.assistant.remember("newer");
    state.recall_agent_prompt(-1);
    state.recall_agent_prompt(-1);
    assert_eq!(state.assistant.prompt, "older");

    state.assistant_prompt_push('!');
    assert!(state.assistant.recall.is_none());
    state.recall_agent_prompt(-1);
    assert_eq!(state.assistant.prompt, "newer");
}
/// An unfocused box swallows nothing: a keystroke that reached it without focus
/// would type into a panel nobody clicked.
#[test]
fn an_unfocused_composer_takes_no_keys() {
    let mut state = state();
    state.assistant_prompt_push('x');
    assert_eq!(state.assistant.prompt, "");
    state.assistant.focused = true;
    state.assistant_prompt_push('x');
    state.assistant_prompt_pop();
    assert_eq!(state.assistant.prompt, "");
}
/// Sending the same prompt twice in a row remembers it once, so Up is not two
/// presses away from the prompt before it.
#[test]
fn a_repeated_prompt_is_remembered_once() {
    let mut state = state();
    state.assistant.remember("again");
    state.assistant.remember("again");
    assert_eq!(state.assistant.recent, vec!["again".to_string()]);
}
/// Clearing forgets the conversation on both sides — the bubbles and the
/// provider's history. Clearing one would leave a panel that looks empty while
/// the next prompt still carries the old turn.
#[test]
fn clearing_forgets_both_halves_of_the_conversation() {
    let mut state = state();
    state.fold_beat(Beat::Text("something".into()));
    state.finish_agent_turn(Some(finished("something", PromptStatus::Completed, 0)));
    assert!(!state.assistant.history.is_empty());

    state.clear_agent_transcript();
    assert!(state.transcript.is_empty());
    assert!(state.assistant.history.is_empty());
}
/// The context handed to the model is in seconds, and the marked range travels
/// with it: an agent asked to "trim this" needs the in/out points on screen.
#[test]
fn the_context_reports_the_marks_in_seconds() {
    let mut state = state();
    let rate = state.playhead.rate;
    state.mark_in = Some(i64::from(rate.num) / i64::from(rate.den));
    state.mark_out = Some(2 * i64::from(rate.num) / i64::from(rate.den));

    let context = state.assistant_context();
    assert_eq!(context.in_point_seconds, Some(1.0));
    assert_eq!(context.out_point_seconds, Some(2.0));
    assert_eq!(context.playhead_seconds, 0.0);
    assert!(context.selected_clips.is_empty());

    // An in point with no out is not a range: half a range would send the model
    // a duration it cannot honour.
    state.mark_out = None;
    assert_eq!(state.assistant_context().in_point_seconds, None);
}
/// The composer names where a prompt would go, and says so honestly when no
/// model is chosen.
#[test]
fn the_composer_names_its_target() {
    let state = state();
    assert_eq!(state.assistant_target(), "No model chosen");
}
/// Rules cut at the prompt cap must be said out loud. The class this closes: a
/// rule that was written, looks in force, and never reached the model because
/// it fell off the end of a 4 KB block.
#[test]
fn truncated_rules_are_reported_before_the_turn() {
    let state = state();
    let rules = state.agent_dir().join("rules");
    std::fs::create_dir_all(&rules).expect("rules dir");
    std::fs::write(
        rules.join("house.md"),
        "x".repeat(aria_ai::MAX_RULES_BYTES * 2),
    )
    .expect("write rules");

    let (extensions, notes) = state.assistant_extensions();
    assert!(
        extensions
            .rules
            .ends_with("[…rules truncated at the size cap]")
    );
    assert_eq!(
        notes.len(),
        1,
        "the truncation must be reported exactly once"
    );
    assert!(notes[0].contains("cut short"), "{}", notes[0]);
}
/// A skill the directory could not parse is named, not dropped in silence.
#[test]
fn an_unreadable_skill_is_reported() {
    let state = state();
    let skill = state.agent_dir().join("skills").join("broken");
    std::fs::create_dir_all(&skill).expect("skill dir");
    std::fs::write(skill.join("SKILL.md"), "no front matter here").expect("write skill");

    let (_, notes) = state.assistant_extensions();
    assert!(
        notes.iter().any(|note| note.contains("broken")),
        "the skipped skill must be named: {notes:?}"
    );
}
/// The bundled skills reach the prompt without anything installed, so a fresh
/// profile is not a model with no procedures.
#[test]
fn the_bundled_skills_reach_the_prompt() {
    let state = state();
    let (extensions, notes) = state.assistant_extensions();
    assert!(
        notes.is_empty(),
        "a clean agent directory has nothing to report"
    );
    assert_eq!(
        extensions.skills.len(),
        aria_ai::bundled_skills().len(),
        "every bundled skill is offered"
    );
    assert!(extensions.rules.is_empty());
}
/// A dry run holds the plan back and the project is untouched until Apply.
#[test]
fn a_dry_run_plan_is_applied_only_on_request() {
    let mut state = state();
    let mut turn = finished("here is the plan", PromptStatus::DryRun, 0);
    turn.plan = marker_plan();
    state.finish_agent_turn(Some(turn));

    assert_eq!(state.assistant.plan.len(), 2);
    assert!(
        state.store.sequence.markers.is_empty(),
        "a plan must not have touched the project"
    );

    assert_eq!(state.apply_agent_plan(), Ok(2));
    assert_eq!(state.store.sequence.markers.len(), 2);
    assert!(
        state.assistant.plan.is_empty(),
        "an applied plan is no longer pending"
    );
}
/// One undo entry for the whole plan. The class this closes: a plan applied as
/// N commands took N presses of Ctrl+Z to reverse, so a half-undone plan was a
/// state the model never proposed.
#[test]
fn an_applied_plan_undoes_in_one_step() {
    let mut state = state();
    let mut turn = finished("", PromptStatus::DryRun, 0);
    turn.plan = marker_plan();
    state.finish_agent_turn(Some(turn));
    state.apply_agent_plan().expect("applied");

    assert!(state.undo());
    assert!(
        state.store.sequence.markers.is_empty(),
        "one undo reverses the whole plan"
    );
}
/// A plan that no longer fits names the step and the reason, and leaves the
/// project exactly as it was — never half applied.
#[test]
fn a_plan_that_no_longer_validates_is_rolled_back() {
    let mut state = state();
    let mut turn = finished("", PromptStatus::DryRun, 0);
    let mut plan = marker_plan();
    plan.push(PlanStep {
        description: "remove marker 999999".into(),
        command: WireCommand::RemoveMarker(aria_ai::wire::RemoveMarker { marker: 999_999 }),
        // An id no step created: nothing remaps it, so the replay refuses it.
        outcome: None,
    });
    turn.plan = plan;
    state.finish_agent_turn(Some(turn));

    let err = state.apply_agent_plan().expect_err("the plan cannot apply");
    assert!(err.starts_with("remove marker 999999 · "), "{err}");
    assert!(
        state.store.sequence.markers.is_empty(),
        "the earlier steps must be rolled back with the failing one"
    );
    assert_eq!(kinds(&state).last(), Some(&AgentEntryKind::Error));
}
/// Discard drops a plan without touching the project, and says so.
#[test]
fn discarding_a_plan_changes_nothing() {
    let mut state = state();
    let mut turn = finished("", PromptStatus::DryRun, 0);
    turn.plan = marker_plan();
    state.finish_agent_turn(Some(turn));

    state.discard_agent_plan();
    assert!(state.assistant.plan.is_empty());
    assert!(state.store.sequence.markers.is_empty());
    assert_eq!(
        state.apply_agent_plan().err().as_deref(),
        Some("There is no plan to apply")
    );
}
