//! Assistant conversation tests.
//!
//! Only `#[test]` functions are defined here. The tests reach the fixtures and
//! helpers in `assistant_tests.rs` through `use super::*`.

use super::*;

/// Clearing the conversation drops a pending plan: a plan is an answer, and an
/// answer nobody can see must not still be able to edit the project.
#[test]
fn clearing_the_conversation_drops_a_pending_plan() {
    let mut state = state();
    let mut turn = finished("", PromptStatus::DryRun, 0);
    turn.plan = marker_plan();
    state.finish_agent_turn(Some(turn));

    state.clear_agent_transcript();
    assert!(state.assistant.plan.is_empty());
}
/// A skill held back is dropped from the prompt without being uninstalled, and
/// clearing the conversation offers it again.
#[test]
fn a_skill_can_be_held_back_for_one_conversation() {
    let mut state = state();
    let first = aria_ai::bundled_skills()
        .first()
        .expect("a bundled skill")
        .id
        .clone();

    assert!(state.assistant_skill_enabled(&first));
    state.toggle_assistant_skill(&first);
    assert!(!state.assistant_skill_enabled(&first));

    let (extensions, _) = state.assistant_extensions();
    assert!(
        !extensions.skills.iter().any(|skill| skill.id == first),
        "a held-back skill must not reach the prompt"
    );
    assert_eq!(
        extensions.skills.len(),
        aria_ai::bundled_skills().len() - 1,
        "only the one skill is held back"
    );

    state.clear_agent_transcript();
    assert!(state.assistant_skill_enabled(&first));
}
/// Plan only reaches the turn it configures. Without this the switch is a
/// painted control.
#[test]
fn plan_only_reaches_the_precheck() {
    let mut state = state();
    state.set_assistant_plan_only(true);
    assert!(state.assistant.plan_only);
    // The precheck still refuses for want of a provider, which is what proves
    // the flag is read after the refusals rather than instead of them.
    assert!(state.assistant_precheck("go").is_err());
}
/// The clipboard text names each speaker: a pasted wall of alternating
/// paragraphs cannot be read.
#[test]
fn the_conversation_copies_with_its_speakers() {
    let mut state = state();
    state.push_agent_message(AgentEntryKind::You, "cut it");
    state.push_agent_message(AgentEntryKind::Assistant, "done");
    assert_eq!(state.transcript_text(), "You: cut it\nAssistant: done\n");
}
/// The scope line says what the next prompt is pointed at, in timecode.
#[test]
fn the_scope_line_names_the_selection_and_the_playhead() {
    let (mut state, _clip) = crate::app::premiere_tests::with_clip();
    assert!(
        state
            .assistant_scope()
            .starts_with("1 clip selected · playhead "),
        "{}",
        state.assistant_scope()
    );

    state.clear_selection();
    let scope = state.assistant_scope();
    assert!(scope.starts_with("Whole sequence · playhead "), "{scope}");
    assert!(!scope.contains("marked"), "{scope}");

    state.mark_in = Some(0);
    state.mark_out = Some(24);
    assert!(state.assistant_scope().contains("marked "));
}
/// **A frame holding the conversation does not block the next token.**
///
/// The panel draws from a clone of the transcript handle, so while a frame is
/// alive the store's list is shared. An append has to keep working anyway, and
/// what the frame holds has to keep being the answer as it was drawn: a mutation
/// that reached through the shared handle would rewrite a frame already on
/// screen, and one that demanded sole ownership would panic mid-stream.
#[test]
fn a_frame_holding_the_conversation_still_receives_the_next_token() {
    let mut state = state();
    state.push_agent_message(AgentEntryKind::You, "cut it");
    let drawn = state.transcript.clone();

    // A stream opens its own row, so the first beat is the second row.
    state.fold_beat(Beat::Text("done".into()));
    assert_eq!(drawn.len(), 1, "the drawn frame gained a row it never drew");
    assert_eq!(state.transcript.len(), 2);

    let index = state.transcript.len() - 1;
    let held = state.transcript.clone();
    state.fold_beat(Beat::Text(" and saved".into()));
    assert_eq!(
        held[index].content, "done",
        "a held frame's row changed under it"
    );
    assert_eq!(state.transcript[index].content, "done and saved");
}
