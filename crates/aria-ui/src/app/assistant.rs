//! The assistant: one prompt, one job, one turn of conversation.
//!
//! `aria-ai` owns the provider protocol, the tool vocabulary, and the agent loop;
//! nothing here reimplements a request, a schema, or a retry. What this module
//! owns:
//!
//! - **A turn is a job.** The provider call and every edit it makes run on the
//!   job thread, so a model that thinks for thirty seconds does not freeze a
//!   frame of the editor, and the Jobs board can cancel it like any other work.
//! - **The edits land in the session project, not a copy.** The bridge edits
//!   through the same [`ProjectHost`] as the timeline, so an edit is undoable
//!   with `Ctrl+Z` the moment the turn ends and every `CommandProposed` handler
//!   sees it. The turn holds the project for its whole reply: an edit from the
//!   window meanwhile is refused with the reason rather than mixed into the
//!   turn's undo step. An agent that edited a clone would produce a transcript
//!   describing a project nobody can see. A plan-only turn edits it too, inside
//!   a group the loop rolls back, because a step has to be validated against
//!   the plan so far and a second engine is not what that costs.
//! - **The stream is the surface.** Text, reasoning, and each applied edit
//!   arrive as separate beats and are appended as they come, because a model
//!   that answers in twenty seconds must not look like a hung panel for
//!   nineteen of them.
//! - **A refusal is decided before anything costs time**
//!   (`AppState::assistant_precheck`), so every reason a prompt cannot be sent
//!   is reachable without a window and without a network call.
//! - **The conversation is the provider's own history.** `PromptOutcome`'s
//!   `turn_messages` is appended verbatim, so the next prompt remembers this one
//!   exactly as the model saw it rather than as the panel re-rendered it.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use aria_ai::agent::{
    AgentConfig, AgentEvent, Applied, EngineBridge, PromptStatus, run_prompt_with_host,
};
use aria_ai::config::provider_from_ai;
use aria_ai::{
    AgentExtensions, EditorContext, Lowered, Message, ProjectSummary, QueuedWork, SummaryRequest,
    WireCommand, compose_rules, load_agent_dir, merge_skills, summarize_for, validate,
};
use aria_commands::{Command, EditOutcome};
use aria_engine::ApplyOutcome;
use aria_jobs::JobCompletion;
use aria_plugin::{EditGroup, Origin, ProjectHost};
use aria_settings::Autonomy;
use gpui::Context;

use super::AppState;
use super::assistant_host::{HostRequest, PendingAsk, ShellHost, on_ui_thread};
use crate::types::{AgentEntryKind, UiAgentEntry};

/// How many sent prompts the composer remembers for Up/Down recall.
const RECALL_DEPTH: usize = 32;

/// The composer's state, the conversation, and whatever turn is in flight.
#[derive(Debug, Default)]
pub struct AssistantForm {
    /// What is typed in the composer.
    pub prompt: String,
    /// Whether the composer owns the keyboard.
    pub focused: bool,
    /// Whether a turn is in flight.
    pub running: bool,
    /// The flag the running turn polls; set to stop it.
    pub cancel: Option<Arc<AtomicBool>>,
    /// The provider's own view of this conversation, appended turn by turn.
    pub history: Vec<Message>,
    /// Prompts already sent, newest last, for Up/Down recall.
    pub recent: Vec<String>,
    /// Where recall currently sits in [`Self::recent`].
    pub recall: Option<usize>,
    /// Plan the next turn instead of applying it.
    pub plan_only: bool,
    /// The last plan a dry run produced, waiting to be applied or dropped.
    ///
    /// Behind a handle because the panel draws the whole plan card every frame
    /// and cannot hold a borrow of the store while it builds the card's
    /// listeners. Applying takes the steps back out, which is free while no
    /// frame is holding them and a copy while one is.
    pub plan: Arc<Vec<PlanStep>>,
    /// Installed skills held back from this conversation, by id.
    pub skills_off: Vec<String>,
    /// A destructive host call waiting on Approve or Deny.
    pub pending_ask: Option<PendingAsk>,
    /// The transcript's scroll position, so a new row does not throw the view
    /// back to the top and a streaming answer stays in sight.
    pub scroll: gpui::ScrollHandle,
    /// When a beat last asked the transcript to show its newest row. The panel
    /// travels the view there for one glide and then stops writing the offset,
    /// so scrolling back through the conversation is never fought.
    pub follow_at: Option<std::time::Instant>,
    /// Index in the transcript that text deltas append to.
    streaming: Option<usize>,
    /// Index in the transcript that reasoning deltas append to.
    reasoning: Option<usize>,
}

impl AssistantForm {
    /// The transcript row text is currently appending to, if a turn is
    /// streaming. The panel draws a caret at its end: an answer arriving one
    /// token at a time and an answer that has stopped mid-sentence look
    /// identical without one.
    pub fn streaming_row(&self) -> Option<usize> {
        self.streaming
    }

    /// Remember a sent prompt and reset recall.
    pub(crate) fn remember(&mut self, prompt: &str) {
        if self.recent.last().is_some_and(|last| last == prompt) {
            self.recall = None;
            return;
        }
        self.recent.push(prompt.to_string());
        if self.recent.len() > RECALL_DEPTH {
            self.recent.remove(0);
        }
        self.recall = None;
    }
}

/// Everything a turn needs, gathered before the job starts.
pub(crate) struct Ready {
    provider: aria_ai::providers::OpenAiProvider,
    context: EditorContext,
    extensions: AgentExtensions,
    /// What the agent directory could not use, said once before the turn runs.
    notes: Vec<String>,
    history: Vec<Message>,
    prompt: String,
    label: String,
    /// Validate and report the plan without touching the project.
    dry_run: bool,
    /// How much the host may do without asking, read at send time so changing
    /// the setting takes effect on the next prompt rather than the next launch.
    autonomy: Autonomy,
}

/// One validated edit a dry run would have made.
#[derive(Debug, Clone, PartialEq)]
pub struct PlanStep {
    /// The line the transcript shows.
    pub description: String,
    /// The command itself, kept so Apply replays exactly what was validated
    /// rather than asking the model again and hoping for the same answer.
    pub command: WireCommand,
    /// The entity the rehearsal created, when the step created one. It is gone
    /// by the time Apply runs, so this is what a later step's reference to it is
    /// rewritten from.
    pub outcome: Option<EditOutcome>,
}

/// One thing that happened during a turn.
pub(crate) enum Beat {
    Text(String),
    Reasoning(String),
    Action(String),
    /// Work a host tool needs done on the UI thread. The job thread is blocked
    /// on `reply` until it is answered.
    Apply {
        request: HostRequest,
        reply: crossbeam_channel::Sender<Result<String, String>>,
    },
    /// A destructive host call that needs an Allow click.
    Ask(PendingAsk),
    Done(Box<Finished>),
}

/// How a turn ended.
pub(crate) struct Finished {
    pub(crate) text: String,
    pub(crate) status: PromptStatus,
    pub(crate) messages: Vec<Message>,
    pub(crate) edits: usize,
    /// Every command the turn validated. Applied already unless the turn was a
    /// dry run, in which case this is the plan Apply replays.
    pub(crate) plan: Vec<PlanStep>,
}

/// The session project behind `aria-ai`'s bridge.
///
/// One engine lock per call rather than one for the whole turn: a model that
/// thinks between edits would otherwise hold the engine for its whole reply and
/// freeze every preview frame in the meantime. The turn's group holds the
/// project instead, which stops other edits and nothing else.
struct SessionBridge {
    host: Arc<ProjectHost>,
    /// The turn's group while it is open.
    group: Option<EditGroup>,
    /// Why the group could not open; every edit of the turn fails with it.
    unavailable: Option<String>,
    /// Where work the engine cannot do goes: a render is a job on the UI
    /// thread, and this bridge runs on the turn's thread.
    beats: crate::handoff::Wake<Beat>,
    /// The turn's stop flag, so a render waiting on the UI thread is released
    /// when Stop is pressed.
    cancel: Arc<AtomicBool>,
}

impl SessionBridge {
    fn new(
        host: Arc<ProjectHost>,
        beats: crate::handoff::Wake<Beat>,
        cancel: Arc<AtomicBool>,
    ) -> Self {
        Self {
            host,
            group: None,
            unavailable: None,
            beats,
            cancel,
        }
    }

    /// Validate one command against the project as the turn has left it.
    fn lower(&self, command: &WireCommand) -> Result<Lowered, String> {
        if let Some(reason) = &self.unavailable {
            return Err(reason.clone());
        }
        // The snapshot drops before the edit that follows, so it costs no copy.
        validate(command, &self.host.snapshot()).map_err(|refusal| refusal.message)
    }

    /// Apply one validated edit into the turn's group.
    fn edit(&self, lowered: Command) -> Result<EditOutcome, String> {
        match self.host.apply(&Origin::Assistant, lowered) {
            Ok(ApplyOutcome::Edited(outcome)) => Ok(outcome),
            Ok(other) => Err(format!("unexpected engine outcome: {other:?}")),
            Err(err) => Err(err.to_string()),
        }
    }
}

impl EngineBridge for SessionBridge {
    fn summary(&mut self, request: SummaryRequest) -> ProjectSummary {
        summarize_for(&self.host.snapshot(), request)
    }

    fn apply(&mut self, command: &WireCommand) -> Result<Applied, String> {
        match self.lower(command)? {
            Lowered::Edit(lowered) => self.edit(lowered).map(Applied::Edited),
            // Started, not done: the answer is what the editor reports back
            // about the job it just spawned, so the transcript says a render
            // began rather than that a clip was replaced.
            Lowered::Queued(QueuedWork::RenderAndReplace { clip }) => {
                on_ui_thread(&self.beats, HostRequest::RenderClip { clip }, &self.cancel)
                    .map(Applied::Queued)
            }
        }
    }

    fn rehearse(&mut self, command: &WireCommand) -> Result<Option<EditOutcome>, String> {
        match self.lower(command)? {
            // Applied into the turn's group, which the loop rolls back before it
            // returns: the next step is validated against this one, and the
            // project ends the turn as it started.
            Lowered::Edit(lowered) => self.edit(lowered).map(Some),
            // A render is host work, and a rehearsal starts none: validated, and
            // what it would have produced is not projected. Nothing in the wire
            // vocabulary names a render's output, so no later step needs it.
            Lowered::Queued(QueuedWork::RenderAndReplace { .. }) => Ok(None),
        }
    }

    fn begin_group(&mut self) {
        // The turn runs on its own thread, so it can wait out a plugin's
        // transaction or a gesture in flight; only Stop ends the wait.
        loop {
            match self.host.group(&Origin::Assistant) {
                Ok(group) => {
                    self.group = Some(group);
                    return;
                }
                Err(err) if self.cancel.load(Ordering::Relaxed) => {
                    self.unavailable = Some(err.to_string());
                    return;
                }
                Err(_) => {}
            }
        }
    }

    fn end_group(&mut self) {
        if let Some(group) = self.group.take() {
            group.commit();
        }
    }

    fn rollback_group(&mut self) {
        if let Some(group) = self.group.take() {
            group.rollback();
        }
    }
}

impl AppState {
    fn transcript_ends_with(&self, text: &str) -> bool {
        self.transcript
            .last()
            .is_some_and(|entry| entry.kind == AgentEntryKind::Assistant && entry.content == text)
    }

    pub fn recall_agent_prompt(&mut self, step: i32) {
        if self.assistant.recent.is_empty() {
            return;
        }
        let last = self.assistant.recent.len() - 1;
        let next = match (self.assistant.recall, step) {
            (None, s) if s < 0 => Some(last),
            (None, _) => None,
            (Some(0), s) if s < 0 => Some(0),
            (Some(index), s) if s < 0 => Some(index - 1),
            (Some(index), _) if index >= last => None,
            (Some(index), _) => Some(index + 1),
        };
        self.assistant.recall = next;
        self.assistant.prompt = match next {
            Some(index) => self.assistant.recent[index].clone(),
            None => String::new(),
        };
    }

    pub fn focus_assistant_prompt(&mut self, focused: bool) {
        self.assistant.focused = focused;
    }

    pub fn assistant_prompt_push(&mut self, ch: char) {
        if self.assistant.focused {
            self.assistant.prompt.push(ch);
            self.assistant.recall = None;
        }
    }

    pub fn assistant_prompt_pop(&mut self) {
        if self.assistant.focused {
            self.assistant.prompt.pop();
            self.assistant.recall = None;
        }
    }

    pub fn clear_agent_transcript(&mut self) {
        std::sync::Arc::make_mut(&mut self.transcript).clear();
        self.assistant.history.clear();
        self.assistant.plan = Arc::default();
        self.assistant.skills_off.clear();
        self.assistant.streaming = None;
        self.assistant.reasoning = None;
        // Clearing is a decision, so it reaches the disk now rather than at the
        // next project save: a cleared conversation that came back after a
        // restart would be a deleted conversation.
        self.persist_conversation();
        self.status_message = Some("Cleared the conversation".into());
    }

    pub fn apply_agent_plan(&mut self) -> Result<usize, String> {
        if self.assistant.plan.is_empty() {
            return Err("There is no plan to apply".into());
        }
        // A busy project keeps the plan: it still fits, and Apply works once
        // the holder finishes.
        let host = Arc::clone(&self.host);
        let group = match host.group(&Origin::Assistant) {
            Ok(group) => group,
            Err(err) => {
                let reason = err.to_string();
                self.status_message = Some(reason.clone());
                return Err(reason);
            }
        };
        let mut plan = std::mem::take(Arc::make_mut(&mut self.assistant.plan));
        // The engine is locked per step and released before anything is
        // reported: `plan_failed` and `sync_from_engine` both want it.
        let outcome = {
            let mut applied = 0_usize;
            let mut renders = Vec::new();
            let mut failure = None;
            // The rehearsal's entities do not exist here: the turn rolled them
            // back when it handed the plan over. Each step is rewritten onto
            // what this replay created before it is validated, so a step that
            // named a planned clip, track or marker names the real one.
            let mut ids = aria_ai::plan::Ids::default();
            for step in &mut plan {
                ids.rewrite(&mut step.command);
                let lowered = {
                    let engine = host.engine().lock();
                    validate(&step.command, engine.project())
                };
                let lowered = match lowered {
                    Ok(lowered) => lowered,
                    Err(refusal) => {
                        failure = Some((step.description.clone(), refusal.message));
                        break;
                    }
                };
                match lowered {
                    Lowered::Edit(lowered) => {
                        match host.apply(&Origin::Assistant, lowered) {
                            Ok(ApplyOutcome::Edited(made)) => ids.note(step.outcome, made),
                            Ok(other) => {
                                failure = Some((
                                    step.description.clone(),
                                    format!("unexpected engine outcome: {other:?}"),
                                ));
                                break;
                            }
                            Err(err) => {
                                failure = Some((step.description.clone(), err.to_string()));
                                break;
                            }
                        }
                        applied += 1;
                    }
                    // A render is not part of the plan's history entry, and it
                    // renders what the plan produced rather than what the
                    // sequence looked like halfway through it. So it is kept in
                    // order and started once every edit is in force.
                    Lowered::Queued(QueuedWork::RenderAndReplace { clip }) => renders.push(clip),
                }
            }
            match failure {
                Some(failure) => {
                    group.rollback();
                    Err(failure)
                }
                None => {
                    group.commit();
                    Ok((applied, renders))
                }
            }
        };
        let (applied, renders) = match outcome {
            Ok(outcome) => outcome,
            Err((step, reason)) => {
                self.sync_from_engine();
                return self.plan_failed(&step, reason);
            }
        };
        self.sync_from_engine();
        self.push_agent_message(
            AgentEntryKind::Action,
            format!("Applied the plan: {applied} edit(s)"),
        );
        for clip in renders {
            let line = match self.render_clip(clip) {
                Ok(Some(_)) => format!("started rendering clip {}", clip.raw()),
                Ok(None) => format!("clip {} decodes through the render on disk", clip.raw()),
                Err(reason) => {
                    self.push_agent_message(
                        AgentEntryKind::Error,
                        format!("clip {} was not rendered · {reason}", clip.raw()),
                    );
                    continue;
                }
            };
            self.push_agent_message(AgentEntryKind::Action, line);
        }
        self.status_message = Some(format!("Applied {applied} planned edit(s)"));
        Ok(applied)
    }

    fn plan_failed(&mut self, step: &str, reason: String) -> Result<usize, String> {
        let message = format!("{step} · {reason}");
        self.push_agent_message(AgentEntryKind::Error, message.clone());
        self.status_message = Some("The plan no longer fits the project".into());
        Err(message)
    }

    pub fn discard_agent_plan(&mut self) {
        if self.assistant.plan.is_empty() {
            return;
        }
        let dropped = self.assistant.plan.len();
        self.assistant.plan = Arc::default();
        self.push_agent_message(
            AgentEntryKind::Action,
            format!("Discarded the plan: {dropped} edit(s) not applied"),
        );
        self.status_message = Some("Discarded the plan".into());
    }

    pub fn transcript_text(&self) -> String {
        let mut out = String::new();
        for entry in self.transcript.iter() {
            out.push_str(entry.kind.label());
            out.push_str(": ");
            out.push_str(&entry.content);
            out.push('\n');
        }
        out
    }

    pub fn assistant_scope(&self) -> String {
        let mut parts = Vec::new();
        match self.selection.len() {
            0 => parts.push("whole sequence".to_string()),
            1 => parts.push("1 clip selected".to_string()),
            n => parts.push(format!("{n} clips selected")),
        }
        let rate = self.store.sequence.frame_rate;
        let clock = |tick: i64| crate::timecode::format_ticks(tick, rate.num, rate.den);
        parts.push(format!("playhead {}", clock(self.playhead.value)));
        if let Some((start, end)) = self.marked_range() {
            parts.push(format!("marked {}–{}", clock(start), clock(end)));
        }
        parts.join(" · ")
    }

    pub fn assistant_target(&self) -> String {
        let ai = self.cloud_settings().ai;
        let model = ai.model.trim();
        if model.is_empty() {
            return "No model chosen".into();
        }
        format!("{} · {model}", ai.source.key())
    }
}

// Further methods live in sibling files; inherent methods keep their paths.
#[path = "assistant_agent.rs"]
mod agent;

#[cfg(test)]
#[path = "assistant_plan_render_tests.rs"]
mod plan_render;

#[cfg(test)]
#[path = "assistant_plan_validation_tests.rs"]
mod plan_validation;

#[cfg(test)]
#[path = "assistant_plugin_tools_tests.rs"]
mod plugin_tools;
