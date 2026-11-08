//! The assistant capabilities, the slash-command seam, and the two diagnostics
//! rows that answer "what would this build do with my configuration".
//!
//! A child of [`super`] rather than a second module beside it: every verb here
//! is a question about the same AI and cloud surface, and the split is only so
//! neither file is too long to read. The parent owns the model list and the
//! endpoint probe; this owns what the model is handed, what the composer does
//! with a leading slash, and what the settings file records.

use super::*;
use aria_core::count::counted;
use gpui::SharedString;

impl AppState {
    /// The slash commands `commands/*.md` defines, by name.
    ///
    /// Held on `AssistantForm` and refreshed alongside
    /// [`Self::refresh_assistant_skills`], so the composer hint draws without
    /// reading `<data_dir>/agent/commands` per frame.
    pub fn assistant_slash_commands(&self) -> Vec<String> {
        self.assistant.slash_commands.to_vec()
    }

    /// The composer hint: what typing a slash does, and which names exist.
    pub fn assistant_slash_hint(&self) -> Option<String> {
        let names = &self.assistant.slash_commands;
        if names.is_empty() {
            return None;
        }
        Some(format!(
            "A slash command expands before it is sent: /{}",
            names.join(", /")
        ))
    }

    /// Send what the composer holds, expanding a slash command first.
    ///
    /// The expansion happens here rather than inside the agent loop because it is
    /// a client-side template: the loop must see the intended prompt,
    /// and the transcript must show it too, so a `/name` that expanded to
    /// nothing useful is visible as what it became.
    ///
    /// An unknown `/name` is sent unchanged. That is `expand_slash_command`'s
    /// own answer, and it is the right one: refusing would make a prompt that
    /// happens to start with a slash unsendable.
    pub fn send_composer_prompt(&mut self, cx: &mut Context<Self>) {
        let typed = self.assistant.prompt.to_string();
        self.refresh_assistant_skills();
        let commands = aria_ai::load_agent_dir(&self.agent_dir()).commands;
        match aria_ai::extend::expand_slash_command(&typed, &commands) {
            Some(expanded) => {
                self.status_message = Some(format!(
                    "Expanded {} into {}",
                    typed.split_whitespace().next().unwrap_or("the command"),
                    counted(expanded.chars().count(), "character", "characters")
                ));
                self.send_agent_prompt(expanded, cx);
            }
            None => self.send_agent_prompt(typed, cx),
        }
    }

    /// Every tool the model is handed, grouped as the registry groups them.
    ///
    /// Enumerated from `aria_ai::wire::tool_groups`, then the `app` group
    /// (`super::assistant_host::app_tools`, which Settings counts), then one
    /// group per plugin that offers tools, so this is the vocabulary a prompt
    /// receives.
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
        let app = crate::app::assistant_host::app_tools();
        groups.push((
            "app".into(),
            app.into_iter().map(|spec| spec.name.into()).collect(),
        ));
        for tool in self.ctx.ai_tools() {
            let group: SharedString = format!("from the {} plugin", tool.plugin_name).into();
            match groups.iter_mut().find(|(name, _)| *name == group) {
                Some((_, tools)) => tools.push(tool.call_name.into()),
                None => groups.push((group, vec![tool.call_name.into()])),
            }
        }
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
        // use is centered on the playhead the turn would report.
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
            "The next turn carries {} of system prompt",
            counted(bytes, "byte", "bytes")
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
                    "Your own keys, direct to the provider".to_string()
                } else {
                    format!(
                        "{} is configured but its key does not resolve, so a direct \
                         search would return nothing",
                        STOCK_PROVIDERS
                            .map(crate::cloud::provider_label)
                            .join(" or ")
                    )
                }
            }
            StockRoute::Backend if self.cloud_server_set() => format!(
                "Anonymous, through the cloud server at {}",
                settings.cloud.base_url.trim()
            ),
            StockRoute::Backend => "Off: no stock key and no cloud server is set".to_string(),
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
        self.settings_form
            .schema
            .iter()
            .map(|row| (format!("[{}]", row.table), row.summary()))
            .collect()
    }

    /// The one-line verdict for the whole configuration file.
    pub fn settings_schema_status(&self) -> String {
        let rows = &self.settings_form.schema;
        let refused: Vec<String> = rows
            .iter()
            .filter(|row| row.refused())
            .map(|row| format!("[{}]", row.table))
            .collect();
        if refused.is_empty() {
            format!(
                "{}, all readable by this build",
                counted(rows.len(), "table", "tables")
            )
        } else {
            format!("Unreadable by this build: {}", refused.join(", "))
        }
    }

    /// Hash a file the way the asset catalog does, so a download can be checked
    /// against a published checksum.
    ///
    /// A catalog entry contains `checksum_sha256`, and a downloaded bundle has
    /// no way to be compared against it without a hasher. This app
    /// ships one; asking it is one row.
    pub fn checksum_picked_file(&mut self) {
        let start = self.download_cache_root();
        self.request_pick(
            Ask::OpenFile(FileSpec::new("Checksum a Downloaded File").directory(start)),
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
