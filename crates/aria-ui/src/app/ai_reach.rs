//! What the AI and cloud backends can answer, as verbs the editor can reach.
//!
//! `aria-ai`, `aria-cloud` and `aria-settings` could each answer a question the
//! editor never asked. Settings let you name a model and a key and told you
//! nothing about whether the endpoint had that model or would accept that key,
//! so the first symptom of a wrong answer was a failed prompt. The assistant
//! read `commands/*.md` at send time and expanded nothing, so a slash command
//! was a file with no effect. The tool vocabulary the model is handed was a
//! count in one settings row and a list nowhere.
//!
//! What this module owns:
//!
//! - **A provider is asked, not assumed.** [`AppState::probe_ai_models`] and
//!   [`AppState::test_ai_connection`] run the provider's own `GET /models` on a
//!   background task and keep the answer in [`AiProbe`], so the model list is a
//!   fact about the configured endpoint rather than a curated guess.
//! - **A model row states its availability.** For a local server the curated
//!   catalog is crossed with what the endpoint reported
//!   ([`aria_ai::catalog::local_models_availability`]), so a model that is not
//!   installed says so in Settings instead of failing at the first prompt.
//! - **A refusal is shown before it costs a round trip.** An OpenRouter model
//!   list without a key, and a stock search with no reachable provider, are both
//!   named as such rather than offered and then refused.
//! - **Nothing here blocks the thread that draws.** Every provider call is a
//!   background task; the state it writes back is the probe, and the panels read
//!   the probe.

// The rest of the lane's verbs, beside this file rather than in a directory of
// its own. Split only for length: the module tree is still `app::ai_reach`.
#[path = "ai_reach_tools.rs"]
mod tools;

use aria_ai::catalog::{
    LocalModel, OPENROUTER_MODELS, local_model, local_models_availability,
    resolve_local_installed_id,
};
use aria_ai::config::provider_from_ai;
use aria_ai::providers::openrouter_compat_extras;
use aria_cloud::{DirectStockProvider, download::sha256_hex};
use aria_settings::{AiSource, schema_report};
use gpui::Context;

use super::AppState;
use super::picker::{Answer, Ask, FileSpec};
use super::settings_form::SettingsField;
use crate::cloud::{STOCK_PROVIDERS, StockRoute};
use crate::types::AgentEntryKind;
use gpui::SharedString;

/// What the configured AI endpoint last answered.
///
/// Session state, deliberately not persisted: a model list describes a server at
/// a moment, and a stale list read from disk is worse than an empty one, because
/// it looks like an answer.
#[derive(Debug, Default, Clone)]
pub struct AiProbe {
    /// Model ids the endpoint reported, in the order it reported them. Empty
    /// until it is asked.
    pub installed: Vec<String>,
    /// Whether a probe or a connection test is in flight, so a second click does
    /// not start a second request.
    pub busy: bool,
    /// The last result line: what the endpoint said, or why it could not be
    /// asked. Shown verbatim — a provider's own error names the endpoint and the
    /// status, which is what an operator needs.
    pub result: Option<String>,
}

/// One offerable model, as a settings row needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiModelRow {
    /// The id the setting stores when this row is picked.
    pub id: SharedString,
    /// What the row shows: the catalog's display name, or the id itself when the
    /// endpoint reported a model the catalog does not know.
    pub label: SharedString,
    /// The second line: what the model is for, where the request routes, and
    /// whether it is installed.
    pub detail: SharedString,
    /// Whether this is the configured model.
    pub selected: bool,
    /// Whether the model can be used right now. A curated local model that the
    /// endpoint did not report is offered but marked, because installing it is
    /// one `ollama pull` away and hiding it would hide the instruction.
    pub available: bool,
}

impl AppState {
    /// The models the operator can choose from, for the configured source.
    ///
    /// Three sources, three honest answers, none of them a hardcoded list:
    ///
    /// - **Local**: the curated catalog crossed with what the endpoint reported,
    ///   so every row says installed or not, and an installed model whose id
    ///   differs from the catalog's names it.
    /// - **OpenRouter**: the curated slugs, each with the upstream the request
    ///   would be pinned to.
    /// - **Custom**: whatever the endpoint reported, because there is no catalog
    ///   for an endpoint the operator named.
    ///
    /// Empty before the endpoint is asked for the sources that need asking;
    /// [`AppState::ai_models_hint`] is the line that says so.
    pub fn ai_model_rows(&self) -> Vec<AiModelRow> {
        let ai = self.cloud_settings().ai;
        let configured = ai.model.trim().to_string();
        match ai.source {
            AiSource::Local => local_models_availability(&self.ai_probe.installed)
                .into_iter()
                .map(|(entry, available, resolved)| {
                    local_row(entry, available, resolved, &configured)
                })
                .collect(),
            AiSource::OpenRouter => OPENROUTER_MODELS
                .iter()
                .map(|model| AiModelRow {
                    id: model.id.to_string().into(),
                    label: format!("{} · {}", model.display, model.vendor).into(),
                    detail: format!("{} — {}", model.role, routing_of(model.id)).into(),
                    selected: configured == model.id,
                    // A curated slug is always offerable; whether the key works
                    // is what Test connection answers.
                    available: true,
                })
                .collect(),
            AiSource::Custom => self
                .ai_probe
                .installed
                .iter()
                .map(|id| AiModelRow {
                    id: id.clone().into(),
                    // A custom endpoint has no catalog, so the endpoint's own id
                    // is the only honest label.
                    label: id.clone().into(),
                    detail: "reported by the endpoint".to_string().into(),
                    selected: &configured == id,
                    available: true,
                })
                .collect(),
        }
    }

    /// The line above the model list: what asking would cost, or why it cannot
    /// be asked.
    ///
    /// The OpenRouter case is the one that matters: a cloud model list with no
    /// key is a list of things that will refuse, so the row says to add the key
    /// instead of offering the request.
    pub fn ai_models_hint(&self) -> String {
        let ai = self.cloud_settings().ai;
        if let Some(line) = &self.ai_probe.result {
            return line.clone();
        }
        match ai.source {
            AiSource::OpenRouter if !ai.has_api_key() => {
                "No OpenRouter key is stored, so every model here would refuse. \
                 Add a key or name a variable that holds one."
                    .to_string()
            }
            AiSource::OpenRouter => {
                "Curated cloud models. Test connection checks the key.".to_string()
            }
            AiSource::Local => {
                "Curated local models. List models asks the server which are installed.".to_string()
            }
            AiSource::Custom => {
                "This endpoint has no catalog. List models asks it what it serves.".to_string()
            }
        }
    }

    /// What the curated catalog knows about the configured model, in one line.
    ///
    /// A model id in a settings field is opaque: `gpt-oss:120b` does not say
    /// whether this build knows it, and a typo reads exactly like a model that
    /// is merely not installed. The catalog answers the first half of that.
    pub fn ai_model_detail(&self) -> String {
        let ai = self.cloud_settings().ai;
        let id = ai.model.trim();
        if id.is_empty() {
            return "no model chosen".to_string();
        }
        match ai.source {
            AiSource::Local => match local_model(id) {
                Some(entry) => {
                    let installed = resolve_local_installed_id(entry.id, &self.ai_probe.installed);
                    match installed {
                        Some(actual) if actual == id => {
                            format!("{} — {}", entry.display, entry.role)
                        }
                        Some(actual) => format!(
                            "{} — {}; the server serves it as {actual}",
                            entry.display, entry.role
                        ),
                        None if self.ai_probe.installed.is_empty() => {
                            format!("{} — {}; not asked yet", entry.display, entry.role)
                        }
                        None => format!(
                            "{} — {}; the server did not report it as installed",
                            entry.display, entry.role
                        ),
                    }
                }
                None => format!("{id} is not in the local catalog this build allows"),
            },
            AiSource::OpenRouter => match aria_ai::catalog::openrouter_model(id) {
                Some(model) => format!(
                    "{} · {} — {}; {}",
                    model.display,
                    model.vendor,
                    model.role,
                    routing_of(model.id)
                ),
                None => format!("{id} is not in the cloud catalog this build allows"),
            },
            AiSource::Custom => format!("{id}, sent to the endpoint as written"),
        }
    }

    /// Choose a model, through the same write every typed settings field uses.
    ///
    /// Not a second writer: the draft is committed by
    /// [`AppState::commit_setting`], so a picked model is validated and persisted
    /// exactly as a typed one is.
    pub fn pick_ai_model(&mut self, id: &str) -> Result<(), String> {
        self.focus_setting(Some(SettingsField::AiModel));
        self.settings_form.draft = id.trim().to_string();
        self.commit_setting()
    }

    /// Ask the configured endpoint which models it serves.
    ///
    /// The answer is the endpoint's, not the catalog's: a curated model the
    /// server does not have is the failure this exists to make visible, and it
    /// is invisible if the list is drawn from the catalog alone.
    pub fn probe_ai_models(&mut self, cx: &mut Context<Self>) {
        if self.ai_probe.busy {
            return;
        }
        let ai = self.cloud_settings().ai;
        let provider = match provider_from_ai(&ai) {
            Ok(provider) => provider,
            Err(err) => {
                self.ai_probe.result = Some(err.clone());
                self.status_message = Some(err);
                return;
            }
        };
        self.ai_probe.busy = true;
        self.ai_probe.result = Some("Asking the endpoint for its models…".into());
        cx.spawn(async move |this, cx| {
            let listed = crate::handoff::on_a_thread(
                cx.background_executor(),
                "aria-ai-models",
                move || provider.list_models(),
            )
            .await;
            let _ = this.update(cx, |state, _| {
                state.ai_probe.busy = false;
                match listed {
                    Some(Ok(ids)) => {
                        state.ai_probe.result = Some(match ids.len() {
                            0 => "The endpoint answered with no models.".to_string(),
                            1 => "The endpoint serves 1 model.".to_string(),
                            n => format!("The endpoint serves {n} models."),
                        });
                        state.ai_probe.installed = ids;
                    }
                    Some(Err(err)) => {
                        // The list is not cleared: a failed refresh must not erase
                        // an answer that was true a minute ago and turn "the
                        // network blinked" into "nothing is installed".
                        state.ai_probe.result = Some(err.to_string());
                    }
                    None => {
                        state.ai_probe.result = Some("The endpoint did not answer.".into());
                    }
                }
            });
        })
        .detach();
    }

    /// Check that the configured endpoint and key work, without spending tokens.
    ///
    /// `aria-ai`'s own check, so a pass here and a refusal on the next prompt
    /// cannot disagree about what "connected" means.
    pub fn test_ai_connection(&mut self, cx: &mut Context<Self>) {
        if self.ai_probe.busy {
            return;
        }
        let ai = self.cloud_settings().ai;
        let provider = match provider_from_ai(&ai) {
            Ok(provider) => provider,
            Err(err) => {
                self.ai_probe.result = Some(err.clone());
                self.status_message = Some(err);
                return;
            }
        };
        self.ai_probe.busy = true;
        self.ai_probe.result = Some("Testing the connection…".into());
        cx.spawn(async move |this, cx| {
            let tested = crate::handoff::on_a_thread(
                cx.background_executor(),
                "aria-ai-connect",
                move || provider.test_connection(),
            )
            .await;
            let _ = this.update(cx, |state, _| {
                state.ai_probe.busy = false;
                let line = match tested {
                    Some(Ok(summary)) => summary,
                    Some(Err(err)) => err.to_string(),
                    None => "The endpoint did not answer.".to_string(),
                };
                state.status_message = Some(line.clone());
                state.ai_probe.result = Some(line);
            });
        })
        .detach();
    }
}

/// One curated local model as a row: what it is for, and whether the endpoint
/// has it.
fn local_row(
    entry: &'static LocalModel,
    available: bool,
    resolved: Option<String>,
    configured: &str,
) -> AiModelRow {
    let detail = match (&resolved, available) {
        // The server's id differs from the catalog's: worth saying, because that
        // is the id a request carries and the one a server log shows.
        (Some(actual), true) if actual != entry.id => {
            format!("{} — installed as {actual}", entry.role)
        }
        (_, true) => format!("{} — installed", entry.role),
        (_, false) => format!("{} — not installed on this server", entry.role),
    };
    AiModelRow {
        id: entry.id.to_string().into(),
        label: entry.display.to_string().into(),
        detail: detail.into(),
        selected: configured == entry.id,
        available,
    }
}

/// Where a cloud request for `id` would actually go.
///
/// Read from the extras the provider is built with, not from a second copy of
/// the pin table: what this says and what the request sends are the same value.
fn routing_of(id: &str) -> String {
    let extras = openrouter_compat_extras(id);
    match extras.provider_order.as_deref() {
        None | Some([]) => "routed by OpenRouter".to_string(),
        Some(order) if extras.allow_fallbacks => format!("prefers {}", order.join(", ")),
        Some(order) => format!("pinned to {}, no fallbacks", order.join(", ")),
    }
}
