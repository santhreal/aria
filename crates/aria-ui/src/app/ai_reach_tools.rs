//! The assistant capabilities, the slash-command seam, and the two diagnostics
//! rows that answer "what would this build do with my configuration".
//!
//! A child of [`super`] rather than a second module beside it: every verb here
//! is a question about the same AI and cloud surface, and the split is only so
//! neither file is too long to read. The parent owns the model list and the
//! endpoint probe; this owns what the model is handed, what the composer does
//! with a leading slash, and what the settings file records.

use super::*;
use gpui::SharedString;

impl AppState {
    /// The slash commands `commands/*.md` defines, by name.
    ///
    /// Read at call time rather than cached, for the same reason the rules are:
    /// a command file added between two prompts must reach the second one.
    pub fn assistant_slash_commands(&self) -> Vec<String> {
        aria_ai::load_agent_dir(&self.agent_dir())
            .commands
            .into_iter()
            .map(|command| command.name)
            .collect()
    }

    /// The composer hint: what typing a slash does, and which names exist.
    pub fn assistant_slash_hint(&self) -> Option<String> {
        let names = self.assistant_slash_commands();
        if names.is_empty() {
            return None;
        }
        Some(format!(
            "/{} — a slash command expands before it is sent",
            names.join("  /")
        ))
    }

    /// Send what the composer holds, expanding a slash command first.
    ///
    /// The expansion happens here rather than inside the agent loop because it is
    /// a client-side template: the loop must see the prompt the operator meant,
    /// and the transcript must show it too, so a `/name` that expanded to
    /// nothing useful is visible as what it became.
    ///
    /// An unknown `/name` is sent unchanged. That is `expand_slash_command`'s
    /// own answer, and it is the right one: refusing would make a prompt that
    /// happens to start with a slash unsendable.
    pub fn send_composer_prompt(&mut self, cx: &mut Context<Self>) {
        let typed = self.assistant.prompt.clone();
        let commands = aria_ai::load_agent_dir(&self.agent_dir()).commands;
        match aria_ai::extend::expand_slash_command(&typed, &commands) {
            Some(expanded) => {
                self.status_message = Some(format!(
                    "Expanded {} into {} character(s)",
                    typed.split_whitespace().next().unwrap_or("the command"),
                    expanded.len()
                ));
                self.send_agent_prompt(expanded, cx);
            }
            None => self.send_agent_prompt(typed, cx),
        }
    }

    /// Every tool the model is handed, grouped as the registry groups them.
    ///
    /// Enumerated from `aria_ai::wire::tool_groups` plus the two specs the agent
    /// appends per turn, so this is the vocabulary a prompt actually carries. The
    /// existing settings row counts them; a count is not a capability list, and
    /// "the assistant can edit the timeline" should not be something an operator
    /// takes on trust.
    pub fn ai_tool_groups(&self) -> Vec<(SharedString, Vec<SharedString>)> {
        let mut groups: Vec<(SharedString, Vec<SharedString>)> = aria_ai::wire::tool_groups()
            .iter()
            .map(|group| {
                (
                    group.name.into(),
                    group.tools.iter().map(|name| (*name).into()).collect(),
                )
            })
            .collect();
        // The two the agent appends itself, which no registry group holds: the
        // project reader and the skill reader. Named from the specs so a rename
        // there follows here.
        groups.push((
            "beyond the edit vocabulary".into(),
            vec![
                aria_ai::wire::describe_project_spec().name.into(),
                aria_ai::extend::read_skill_spec().name.into(),
            ],
        ));
        groups
    }

    /// Every tool name the edit registry offers, flattened.
    ///
    /// The flat form is what a search reads against: a group is how the list is
    /// arranged, not how a name is looked for.
    pub fn ai_tool_names(&self) -> Vec<String> {
        aria_ai::wire::tool_names()
            .map(str::to_string)
            .collect::<Vec<_>>()
    }

    /// The exact system prompt the next turn would carry.
    ///
    /// Composed the same way the turn composes it — the project summary, the
    /// send-time editor context, and the rules and skills loaded from the agent
    /// directory — so what this shows is what the model is told. A model that
    /// ignores a rule is either not being sent it or choosing not to follow it,
    /// and those are different problems.
    pub fn ai_system_prompt(&self) -> String {
        // The context first, and the lock after it: the window the turn would
        // use is centred on the playhead the turn would report.
        let context = self.assistant_context();
        let summary = aria_ai::summarize_for(
            self.engine().lock().project(),
            aria_ai::SummaryRequest::Session {
                playhead_seconds: context.playhead_seconds,
            },
        );
        let (extensions, _) = self.assistant_extensions();
        aria_ai::agent::system_prompt(&summary, &context, &extensions)
    }

    /// Put the system prompt in the transcript, where it can be read and copied.
    ///
    /// The transcript rather than a dialog: it belongs to the conversation it
    /// describes, and every transcript row already copies itself.
    pub fn show_ai_system_prompt(&mut self) {
        let prompt = self.ai_system_prompt();
        let bytes = prompt.len();
        self.push_agent_message(AgentEntryKind::Action, prompt);
        self.status_message = Some(format!(
            "The next turn carries {bytes} byte(s) of system prompt"
        ));
    }

    /// Whether stock search can actually reach a provider, and through what.
    ///
    /// [`crate::cloud::stock_route`] decides the route from whether a key is
    /// *configured*; whether that key *resolves* is a different question, and a
    /// `api_key_env` naming an unset variable answers yes to the first and no to
    /// the second. A direct search with no resolved key returns empty results
    /// rather than an error, which reads as "nothing found". The built provider
    /// is asked instead, so the row cannot say reachable when it is not.
    pub fn stock_search_reach(&self) -> String {
        let settings = self.cloud_settings();
        match self.stock_route() {
            StockRoute::Direct => {
                let provider = DirectStockProvider::new(
                    settings.provider(STOCK_PROVIDERS[0]).resolve_key(),
                    settings.provider(STOCK_PROVIDERS[1]).resolve_key(),
                );
                if provider.has_any_key() {
                    "your own keys, direct to the provider".to_string()
                } else {
                    format!(
                        "{} is configured but its key does not resolve, so a direct \
                         search would return nothing",
                        STOCK_PROVIDERS.join(" or ")
                    )
                }
            }
            StockRoute::Backend => "anonymous, through the Aria backend".to_string(),
        }
    }

    /// Every persisted settings table, the version the file records, and what
    /// this build does with it.
    ///
    /// A configuration this build refuses fails to load with one sentence about
    /// one table. This is the whole picture, enumerated from
    /// `aria_settings::schema::PersistedShape` at run time, which is what makes
    /// "the file was written by a newer build" an answerable question rather
    /// than a guess.
    pub fn settings_schema_rows(&self) -> Vec<(String, String)> {
        schema_report::report(&self.config_path())
            .into_iter()
            .map(|row| (format!("[{}]", row.table), row.summary()))
            .collect()
    }

    /// The one-line verdict for the whole configuration file.
    pub fn settings_schema_status(&self) -> String {
        let rows = schema_report::report(&self.config_path());
        let refused: Vec<&str> = rows
            .iter()
            .filter(|row| row.refused())
            .map(|row| row.table)
            .collect();
        if refused.is_empty() {
            format!("{} table(s), all readable by this build", rows.len())
        } else {
            format!("refused: {}", refused.join(", "))
        }
    }

    /// Hash a file the way the asset catalog does, so a download can be checked
    /// against a published checksum.
    ///
    /// A catalog entry carries `checksum_sha256`, and an operator holding a
    /// downloaded bundle has no way to compare it without a hasher. This app
    /// ships one; asking it is one row.
    pub fn checksum_picked_file(&mut self) {
        let start = self.download_cache_root();
        self.request_pick(
            Ask::OpenFile(FileSpec::new("Checksum a file").directory(start)),
            |state, answer: Answer| {
                let Some(path) = answer.one() else {
                    state.status_message = Some("Nothing to checksum".into());
                    return;
                };
                match sha256_hex(&path) {
                    Ok(hex) => {
                        state.status_message =
                            Some(format!("{} · sha256 {hex}", file_label(&path)));
                    }
                    Err(err) => state.report_error(format!("{}: {err}", path.display())),
                }
            },
        );
    }
}

/// The last component of a path, for a status line that has to fit.
fn file_label(path: &std::path::Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string())
}
