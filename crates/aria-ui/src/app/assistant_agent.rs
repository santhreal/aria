//! `AppState` agent: 14 methods moved out of `assistant.rs`.
//!
//! A pure move to stay under the file ceiling. Inherent methods keep their
//! paths wherever they are written, because a method belongs to the type and
//! not to the file, so no caller changes.

use super::*;
use aria_core::count::counted;
use gpui::SharedString;

impl AppState {
    /// Append one transcript entry and return its index.
    pub fn push_agent_message(
        &mut self,
        kind: AgentEntryKind,
        content: impl Into<SharedString>,
    ) -> usize {
        let id = crate::app::assistant_store::compact_id(self.transcript.len() + 1);
        // `make_mut` copies only when a frame is still holding the previous
        // handle, so an append during a stream is the usual push.
        let rows = std::sync::Arc::make_mut(&mut self.transcript);
        rows.push(transcript_row(id, kind, content.into()));
        rows.len() - 1
    }

    pub(crate) fn assistant_context(&self) -> EditorContext {
        let rate = self.playhead.rate;
        let seconds = |tick: i64| tick as f64 * f64::from(rate.den) / f64::from(rate.num);
        let marked = self.marked_range();
        EditorContext {
            selected_clips: self.selection.iter().map(|clip| clip.raw()).collect(),
            playhead_seconds: self.playhead.seconds(),
            in_point_seconds: marked.map(|(start, _)| seconds(start)),
            out_point_seconds: marked.map(|(_, end)| seconds(end)),
        }
    }

    pub(crate) fn assistant_extensions(&self) -> (AgentExtensions, Vec<String>) {
        let dir = load_agent_dir(&self.agent_dir());
        let (rules, truncated) = compose_rules(&dir.rules);
        let mut notes = dir.warnings;
        if truncated {
            notes.push(format!(
                "The rules are longer than the {} byte prompt cap and were cut short.",
                aria_ai::MAX_RULES_BYTES
            ));
        }
        let mut skills = merge_skills(dir.skills);
        skills.retain(|skill| !self.assistant.skills_off.contains(&skill.id));
        (AgentExtensions { rules, skills }, notes)
    }

    pub fn toggle_assistant_skill(&mut self, id: &str) {
        if let Some(index) = self.assistant.skills_off.iter().position(|held| held == id) {
            self.assistant.skills_off.remove(index);
        } else {
            self.assistant.skills_off.push(id.to_string());
        }
    }

    pub fn assistant_skill_enabled(&self, id: &str) -> bool {
        !self.assistant.skills_off.iter().any(|held| held == id)
    }

    pub fn set_assistant_plan_only(&mut self, plan_only: bool) {
        self.assistant.plan_only = plan_only;
    }

    pub(crate) fn assistant_precheck(&self, prompt: &str) -> Result<Ready, String> {
        let prompt = prompt.trim();
        if prompt.is_empty() {
            return Err("Type a prompt first".into());
        }
        if self.assistant.running {
            return Err("The assistant is still working on the last prompt".into());
        }
        let ai = self.cloud_settings().ai;
        if !ai.is_configured() {
            return Err("Configure an AI provider in Settings to send this prompt.".into());
        }
        let provider = provider_from_ai(&ai)?;
        let (extensions, notes) = self.assistant_extensions();
        Ok(Ready {
            provider,
            context: self.assistant_context(),
            extensions,
            notes,
            history: self.assistant.history.clone(),
            prompt: prompt.to_string(),
            label: format!("Assistant · {}", ai.model.trim()),
            dry_run: self.assistant.plan_only,
            autonomy: ai.autonomy,
        })
    }

    pub fn send_agent_prompt(&mut self, prompt: String, cx: &mut Context<Self>) {
        let mut ready = match self.assistant_precheck(&prompt) {
            Ok(ready) => ready,
            Err(refusal) => {
                // An unconfigured provider is the one refusal with somewhere to
                // go, so it opens the place that fixes it. The others are about
                // this panel and stay in it.
                let unconfigured = refusal.contains("Settings");
                if !prompt.trim().is_empty() {
                    self.push_agent_message(AgentEntryKind::You, prompt.trim());
                }
                self.push_agent_message(AgentEntryKind::Error, refusal.clone());
                self.status_message = Some(refusal);
                if unconfigured {
                    self.open_dialog(crate::app::DialogKind::Settings);
                }
                return;
            }
        };
        self.push_agent_message(AgentEntryKind::You, &ready.prompt);
        for note in std::mem::take(&mut ready.notes) {
            self.push_agent_message(AgentEntryKind::Error, note);
        }
        self.assistant.remember(&ready.prompt);
        self.assistant.prompt.clear();
        self.assistant.running = true;
        self.assistant.streaming = None;
        self.assistant.reasoning = None;
        self.status_message = Some("Asking the assistant".into());

        let cancel = Arc::new(AtomicBool::new(false));
        self.assistant.cancel = Some(Arc::clone(&cancel));
        let engine = Arc::clone(&self.engine);
        let project_host = Arc::clone(&self.host);
        let (wake, wakes) = crate::handoff::wakes::<Beat>();
        let label = ready.label.clone();
        // Taken on this thread: a screenshot renders on the process's device, not
        // on one the turn brings up for itself.
        let gpu = crate::app::render_device::shared_device(&self.ctx);
        let plugins = aria_plugin::PluginContext::clone(&self.ctx);
        self.spawn_job(label, move |context| {
            let Ready {
                provider,
                context: editor,
                extensions,
                history,
                prompt,
                dry_run,
                autonomy,
                ..
            } = ready;
            let config = AgentConfig {
                dry_run,
                ..AgentConfig::default()
            };
            let mut bridge = SessionBridge::new(
                project_host,
                crate::handoff::Wake::clone(&wake),
                Arc::clone(&cancel),
            );
            // The tool host reads the same session engine the bridge edits: a
            // screenshot taken after two edits must show those edits.
            let mut host = ShellHost::new(
                engine,
                crate::handoff::Wake::clone(&wake),
                autonomy,
                gpu,
                plugins,
            );
            let mut edits = 0_usize;
            let outcome = {
                let tx = crate::handoff::Wake::clone(&wake);
                let cancel = Arc::clone(&cancel);
                let mut on_event = |event: AgentEvent| {
                    // The Jobs board's cancel and the panel's Stop are one
                    // signal: every beat is a chance to notice either, so a turn
                    // cancelled from the board stops at the next delta rather
                    // than running to completion unwatched.
                    if context.cancelled() {
                        cancel.store(true, Ordering::Relaxed);
                    }
                    let beat = match event {
                        AgentEvent::TextDelta(text) => Beat::Text(text),
                        AgentEvent::ReasoningDelta(text) => Beat::Reasoning(text),
                        AgentEvent::Action(action) => {
                            edits += 1;
                            Beat::Action(action.description)
                        }
                        AgentEvent::HostAction { name, summary } => {
                            Beat::Action(format!("{name}: {summary}"))
                        }
                        AgentEvent::Image(image) => Beat::Action(format!("image: {}", image.label)),
                    };
                    tx.wake(beat);
                };
                run_prompt_with_host(
                    &provider,
                    &mut bridge,
                    &mut host,
                    &editor,
                    &extensions,
                    &history,
                    &prompt,
                    &config,
                    &cancel,
                    &mut on_event,
                )
            };
            let summary = match &outcome.status {
                PromptStatus::Completed => counted(edits, "edit", "edits").to_string(),
                PromptStatus::DryRun => counted(edits, "planned edit", "planned edits").to_string(),
                PromptStatus::Aborted(reason) => format!("stopped: {reason}"),
            };
            let status = outcome.status.clone();
            let plan = outcome
                .actions
                .into_iter()
                .map(|action| PlanStep {
                    description: action.description,
                    command: action.command,
                    outcome: action.outcome,
                })
                .collect();
            wake.wake(Beat::Done(Box::new(Finished {
                text: outcome.text,
                status: outcome.status,
                messages: outcome.turn_messages,
                edits,
                plan,
            })));
            match status {
                // A stopped turn is a failed job: the board must not report
                // "done" for interrupted work.
                PromptStatus::Aborted(reason) => Err(reason),
                _ => Ok(JobCompletion::new(summary)),
            }
        });

        cx.spawn(async move |this, cx| {
            loop {
                let Some(beat) = wakes.next_from_a_thread(cx.background_executor()).await else {
                    // Every sender is gone without a Done beat: the job thread
                    // died. A panel left saying "working" forever is worse than
                    // an honest failure, so the turn is closed here.
                    let _ = this.update(cx, |state, cx| {
                        state.finish_agent_turn(None);
                        cx.notify();
                    });
                    return;
                };
                let done = matches!(beat, Beat::Done(_));
                if this
                    .update(cx, |state, cx| state.apply_agent_beat(beat, cx))
                    .is_err()
                {
                    return;
                }
                if done {
                    return;
                }
            }
        })
        .detach();
    }

    pub fn cancel_agent_prompt(&mut self) {
        if let Some(cancel) = &self.assistant.cancel {
            cancel.store(true, Ordering::Relaxed);
            self.status_message = Some("Stopping the assistant".into());
        }
        // A question nobody is going to answer must not hold the job thread: a
        // dropped reply channel denies the call, which is the safe reading.
        self.assistant.pending_ask = None;
    }

    pub(crate) fn run_host_request(&mut self, request: HostRequest) -> Result<String, String> {
        match request {
            HostRequest::SaveProject => {
                self.save_project().map_err(|err| err.to_string())?;
                Ok(match &self.project_path {
                    Some(path) => format!("saved {}", path.display()),
                    None => "saved".to_string(),
                })
            }
            HostRequest::Seek { seconds } => {
                let rate = self.playhead.rate;
                let ticks =
                    (seconds.max(0.0) * f64::from(rate.num) / f64::from(rate.den)).round() as i64;
                self.set_playhead(ticks);
                Ok(format!(
                    "the playhead is at {}",
                    crate::timecode::format_ticks(self.playhead.value, rate.num, rate.den)
                ))
            }
            HostRequest::ClearCache { key } => {
                self.clear_registered_cache(&key)?;
                Ok(self
                    .status_message
                    .clone()
                    .unwrap_or_else(|| format!("cleared the {key} cache")))
            }
            HostRequest::RenderClip { clip } => {
                let name = self
                    .ui_clip_ref(clip)
                    .map(|clip| clip.name.clone())
                    .unwrap_or_else(|| format!("clip {}", clip.raw()).into());
                // Two honest answers, and they are different: a render already
                // on disk is adopted before this returns, and a render that has
                // to be made is a job the turn cannot wait for. Reporting the
                // second as done is exactly what this path exists to avoid.
                match self.render_clip(clip)? {
                    Some(_) => Ok(format!(
                        "started rendering {name}; it replaces the clip when the render finishes, \
                         and the Jobs board shows its progress"
                    )),
                    None => Ok(format!(
                        "Kept the render of {name} already on disk, so the clip decodes \
                         through that file"
                    )),
                }
            }
        }
    }

    pub fn answer_agent_ask(&mut self, approve: bool) {
        let Some(ask) = self.assistant.pending_ask.take() else {
            return;
        };
        let _ = ask.reply.send(approve);
        let verdict = if approve { "allowed" } else { "refused" };
        self.push_agent_message(
            AgentEntryKind::Action,
            format!("you {verdict} {}", ask.name),
        );
        self.status_message = Some(format!("The assistant's request was {verdict}"));
    }

    pub(super) fn apply_agent_beat(&mut self, beat: Beat, cx: &mut Context<Self>) {
        self.fold_beat(beat);
        cx.notify();
    }

    pub(crate) fn fold_beat(&mut self, beat: Beat) {
        match beat {
            Beat::Text(text) => {
                let index = match self.assistant.streaming {
                    Some(index) => index,
                    None => {
                        let index = self.push_agent_message(AgentEntryKind::Assistant, "");
                        self.assistant.streaming = Some(index);
                        index
                    }
                };
                if let Some(entry) = std::sync::Arc::make_mut(&mut self.transcript).get_mut(index) {
                    entry.append(&text);
                }
            }
            Beat::Reasoning(text) => {
                let index = match self.assistant.reasoning {
                    Some(index) => index,
                    None => {
                        let index = self.push_agent_message(AgentEntryKind::Reasoning, "");
                        self.assistant.reasoning = Some(index);
                        index
                    }
                };
                if let Some(entry) = std::sync::Arc::make_mut(&mut self.transcript).get_mut(index) {
                    entry.append(&text);
                }
            }
            Beat::Action(description) => {
                self.push_agent_message(AgentEntryKind::Action, description);
                // An edit is in the engine now, so the timeline has to show it
                // before the model's next sentence, not after the whole turn.
                self.sync_from_engine();
                // A later text delta belongs to a new bubble: an answer split
                // around an edit reads as two statements, which is what it is.
                self.assistant.streaming = None;
                self.assistant.reasoning = None;
            }
            Beat::Apply { request, reply } => {
                let line = request.summary();
                let result = self.run_host_request(request);
                let note = match &result {
                    Ok(detail) => detail.clone(),
                    Err(err) => format!("{line} failed: {err}"),
                };
                self.push_agent_message(AgentEntryKind::Action, note);
                // A dropped receiver means the turn stopped between the request
                // and the answer, which is not a failure of the request.
                let _ = reply.send(result);
                self.assistant.streaming = None;
                self.assistant.reasoning = None;
            }
            Beat::Ask(ask) => {
                self.push_agent_message(
                    AgentEntryKind::Action,
                    format!("waiting for you: {}", ask.summary),
                );
                // One question at a time: the loop calls one tool at a time, so a
                // second ask can only mean the first was already answered.
                self.assistant.pending_ask = Some(ask);
            }
            Beat::Done(finished) => self.finish_agent_turn(Some(*finished)),
        }
        // A streaming answer that scrolled off the bottom is an answer nobody is
        // reading. Only while a turn runs: scrolling back through an old
        // conversation must not be yanked forward by a late beat.
        if self.assistant.running {
            self.assistant.follow_at = Some(std::time::Instant::now());
        }
    }

    pub(crate) fn finish_agent_turn(&mut self, finished: Option<Finished>) {
        self.assistant.running = false;
        self.assistant.cancel = None;
        self.assistant.streaming = None;
        self.assistant.reasoning = None;
        // The turn is over, so nothing is waiting on the question any more.
        self.assistant.pending_ask = None;
        match finished {
            Some(finished) => {
                let trimmed = finished.text.trim();
                if !trimmed.is_empty() && !self.transcript_ends_with(trimmed) {
                    self.push_agent_message(AgentEntryKind::Assistant, trimmed);
                }
                match &finished.status {
                    PromptStatus::Completed => {
                        self.assistant.history.extend(finished.messages);
                        self.assistant.plan = std::sync::Arc::default();
                        self.status_message = Some(if finished.edits == 0 {
                            "The assistant answered".into()
                        } else {
                            format!(
                                "The assistant made {}",
                                counted(finished.edits, "edit", "edits")
                            )
                        });
                    }
                    PromptStatus::DryRun => {
                        self.assistant.history.extend(finished.messages);
                        self.push_agent_message(
                            AgentEntryKind::Action,
                            format!(
                                "{}, nothing applied",
                                counted(finished.plan.len(), "planned edit", "planned edits")
                            ),
                        );
                        // The validated commands are kept so Apply replays this
                        // exact plan. Asking the model again would be a second
                        // plan, and the first one was approved.
                        self.assistant.plan = finished.plan.into();
                        self.status_message = Some("The assistant planned without editing".into());
                    }
                    // An aborted turn contributes no conversation: `aria-ai`
                    // returns no messages for it, and inventing a memory of a
                    // turn the model never finished would poison the next one.
                    // Its edits are gone too — the loop rolls the group back —
                    // so the action rows above are now describing a timeline
                    // that no longer has them, and the panel says so rather
                    // than leaving it to be found by inspection.
                    PromptStatus::Aborted(reason) => {
                        self.push_agent_message(AgentEntryKind::Error, reason.clone());
                        let undone = if finished.edits == 0 {
                            String::new()
                        } else {
                            let note =
                                format!("{} undone", counted(finished.edits, "edit", "edits"));
                            self.push_agent_message(AgentEntryKind::Action, note.clone());
                            format!(", {note}")
                        };
                        self.status_message =
                            Some(format!("The assistant stopped: {reason}{undone}"));
                    }
                }
            }
            None => {
                self.push_agent_message(AgentEntryKind::Error, "The assistant turn was lost");
                self.status_message = Some("The assistant turn was lost".into());
            }
        }
        // A finished turn is worth keeping even if the project is never saved
        // again: the transcript and the provider history are what the next
        // prompt is built from.
        self.persist_conversation();
        self.sync_from_engine();
    }
}
