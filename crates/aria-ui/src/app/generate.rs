//! AI generation: a prompt goes out, media comes back into the pool.
//!
//! `aria-cloud` owns the provider protocol ([`aria_cloud::generate::GenerationProvider`]);
//! this module owns three decisions the panel must not make:
//!
//! - Generation is a job, like export and download. It is a remote queue with no
//!   deadline of its own, so it needs a row, a state, and a way out.
//! - The key is checked before the request, and the refusal names the provider and
//!   where to put the key. A generation that fails on the first HTTP round trip because
//!   nothing was configured is a worse way to learn that.
//! - The result URL is downloaded through the same cache and pinned like any other
//!   cloud asset, then imported through the normal media path. A generated file is not
//!   a special kind of media.
//!
//! The poll loop is bounded. A provider that never reaches a terminal state must end as
//! a timeout on the job row rather than a thread that outlives the session.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use aria_cloud::dto::{GenerateRequest, JobStatus};
use aria_cloud::generate::{FalGenerationProvider, GenerationKind, GenerationProvider};
use gpui::Context;

use super::AppState;

/// The provider whose key generation needs. One name, used by the refusal message, the
/// settings row, and the request.
pub const GENERATE_PROVIDER: &str = "fal";

/// How long a generation may sit in the provider's queue before the job gives up.
///
/// A bound rather than none: an unbounded poll loop against a queue that lost the
/// request is indistinguishable from a hang, and a hung job cannot be told from a slow
/// one on the board.
const POLL_DEADLINE: Duration = Duration::from_secs(15 * 60);

/// How long between polls. The queue API is rate limited and a generation takes tens of
/// seconds at best, so a tighter interval buys nothing and risks a 429.
const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// What the Generate panel is holding.
#[derive(Debug, Clone, Default)]
pub struct GenerateForm {
    pub prompt: String,
    pub kind: GenerateKind,
    /// Whether the prompt box has keyboard focus. Explicit, not inferred from the
    /// visible tab, so the bare-letter tool shortcuts keep working with the tab open.
    pub focused: bool,
}

/// The kinds offered, as the panel's own enum.
///
/// `GenerationKind` is not `Default` and lives in another crate; keeping the UI's
/// selection here means the panel's table and the request agree without a second
/// mapping in the view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GenerateKind {
    #[default]
    Image,
    Video,
    Tts,
}

impl GenerateKind {
    /// Label, and the extension a result of this kind is saved with when the URL does
    /// not carry one.
    pub const ALL: [(Self, &'static str, &'static str); 3] = [
        (Self::Image, "Image", "png"),
        (Self::Video, "Video", "mp4"),
        (Self::Tts, "Voice", "mp3"),
    ];

    pub fn label(self) -> &'static str {
        Self::ALL
            .iter()
            .find(|(kind, _, _)| *kind == self)
            .map(|(_, label, _)| *label)
            .unwrap_or("Image")
    }

    pub fn fallback_extension(self) -> &'static str {
        Self::ALL
            .iter()
            .find(|(kind, _, _)| *kind == self)
            .map(|(_, _, ext)| *ext)
            .unwrap_or("bin")
    }

    fn provider_kind(self) -> GenerationKind {
        match self {
            Self::Image => GenerationKind::Image,
            Self::Video => GenerationKind::Video,
            Self::Tts => GenerationKind::Tts,
        }
    }
}

impl AppState {
    pub fn focus_generate_prompt(&mut self, focused: bool) {
        self.generate.focused = focused;
    }

    pub fn generate_prompt_push(&mut self, ch: char) {
        if self.generate.focused {
            self.generate.prompt.push(ch);
        }
    }

    pub fn generate_prompt_pop(&mut self) {
        if self.generate.focused {
            self.generate.prompt.pop();
        }
    }

    /// Switching kind keeps the prompt: "a dog on a skateboard" is the same idea as an
    /// image and as a video, and retyping it is the annoyance the tab exists to avoid.
    pub fn set_generate_kind(&mut self, kind: GenerateKind) {
        self.generate.kind = kind;
    }

    /// Whether a generation could run at all with the current config.
    pub fn generation_configured(&self) -> bool {
        self.cloud_settings()
            .provider(GENERATE_PROVIDER)
            .resolve_key()
            .is_some()
    }

    /// What a submit needs before it may cost anything: a prompt and a key.
    ///
    /// Split from [`Self::submit_generation`] so the refusals are reachable without a
    /// window: they are the whole reason nothing is queued, and a refusal that only a
    /// running app can observe goes untested.
    pub(crate) fn generation_precheck(&self) -> Result<(String, String), String> {
        let prompt = self.generate.prompt.trim().to_string();
        if prompt.is_empty() {
            return Err("Type what to generate first".into());
        }
        match self
            .cloud_settings()
            .provider(GENERATE_PROVIDER)
            .resolve_key()
        {
            Some(key) => Ok((prompt, key)),
            None => Err(format!(
                "Add a {GENERATE_PROVIDER} key in Settings to generate"
            )),
        }
    }

    /// Submit the prompt as a job.
    ///
    /// Refused, by name, when there is no prompt or no key: both are things the
    /// operator fixes in one step, and a queued job that fails for either reason costs
    /// a round trip to say the same thing.
    pub fn submit_generation(&mut self, cx: &mut Context<Self>) -> Result<(), String> {
        let (prompt, key) = self.generation_precheck()?;

        let kind = self.generate.kind;
        let request = GenerateRequest {
            prompt: prompt.clone(),
            // Empty means the provider's default model for this kind, which is the one
            // the backend ships too.
            model: String::new(),
            duration_seconds: None,
            idempotency_key: format!("{}-{}", kind.label(), self.next_generation_key()),
        };
        let dest = self.generation_path(kind, &request.idempotency_key)?;

        let label = format!("Generating {} — {prompt}", kind.label().to_lowercase());
        let (wake, wakes) = crate::handoff::wakes::<Result<(), String>>();
        let target = dest.clone();
        self.spawn_job(label, move |context| {
            let outcome = run_generation(context, &key, kind, &request, &target);
            let reported = match (&outcome, context.cancelled()) {
                (_, true) => Err(String::new()),
                (Ok(()), false) => Ok(()),
                (Err(err), false) => Err(err.clone()),
            };
            wake.wake(reported);
            outcome.map(|()| aria_jobs::JobCompletion::new("generated"))
        });

        cx.spawn(async move |this, cx| {
            let outcome = wakes.next_from_a_thread(cx.background_executor()).await;
            let _ = this.update(cx, |state, _| match outcome {
                Some(Ok(())) => match state.adopt_generated(&dest) {
                    Ok(()) => state.generate.prompt.clear(),
                    Err(err) => state.report_error(err),
                },
                Some(Err(reason)) if reason.is_empty() => {
                    state.status_message = Some("Cancelled the generation".into());
                }
                Some(Err(reason)) => state.report_error(reason),
                None => state.report_error("the generation did not report"),
            });
        })
        .detach();
        self.status_message = Some("Generating…".into());
        Ok(())
    }

    /// Pin the generated file and import it like any other media.
    fn adopt_generated(&mut self, path: &std::path::Path) -> Result<(), String> {
        self.download_cache
            .protect_path(path)
            .map_err(|err| format!("{}: {err}", path.display()))?;
        self.import_media(path.to_path_buf())
            .map_err(|err| format!("{}: {err:?}", path.display()))?;
        self.status_message = Some(format!("Imported {}", super::jobs::file_label(path)));
        Ok(())
    }

    /// Where a generation's result is written inside the download cache.
    fn generation_path(&self, kind: GenerateKind, key: &str) -> Result<std::path::PathBuf, String> {
        let cache_key = format!("generated/{key}.{}", kind.fallback_extension());
        self.download_cache
            .path_for(&cache_key)
            .map_err(|err| format!("{cache_key}: {err}"))
    }

    /// A per-session counter, so two identical prompts are two generations rather than
    /// one idempotency key the provider dedupes into a single result.
    pub(crate) fn next_generation_key(&mut self) -> u64 {
        self.generation_seq += 1;
        self.generation_seq
    }
}

/// Start the request, poll it to a terminal state, then download the result.
///
/// Split out of the closure so the control flow — including the deadline — reads as one
/// piece rather than as nesting inside a job spawn.
fn run_generation(
    context: &aria_jobs::JobContext,
    key: &str,
    kind: GenerateKind,
    request: &GenerateRequest,
    dest: &std::path::Path,
) -> Result<(), String> {
    let provider = FalGenerationProvider::new(key);
    context.set_progress(0.0, "submitting".to_string());
    let started = provider
        .start(kind.provider_kind(), request)
        .map_err(|err| err.to_string())?;

    let deadline = Instant::now() + POLL_DEADLINE;
    let mut job = started;
    loop {
        match job.status {
            JobStatus::Succeeded => break,
            JobStatus::Failed => {
                return Err(job
                    .error
                    .unwrap_or_else(|| "the provider reported a failure".into()));
            }
            // Pending, Running, and an unrecognized status all mean "not yet". Treating
            // an unknown status as terminal would drop a result the provider is about
            // to deliver.
            JobStatus::Pending | JobStatus::Running | JobStatus::Other => {}
        }
        if context.cancelled() {
            return Err("cancelled".into());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "the provider did not finish within {} minutes",
                POLL_DEADLINE.as_secs() / 60
            ));
        }
        std::thread::sleep(POLL_INTERVAL);
        context.set_progress(0.0, format!("queued — {}", job.id));
        job = provider.poll(&job.id).map_err(|err| err.to_string())?;
    }

    let url = job
        .result_url
        .filter(|url| !url.trim().is_empty())
        .ok_or_else(|| "the provider returned no result".to_string())?;

    context.set_progress(0.0, "downloading".to_string());
    let cancel = Arc::new(AtomicBool::new(false));
    let mirror = cancel.clone();
    aria_cloud::download::download_to(&url, dest, &cancel, |progress| {
        if context.cancelled() {
            mirror.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        match progress.total_bytes {
            total if total > 0 => context.set_progress(
                progress.bytes_downloaded as f32 / total as f32,
                format!(
                    "{} of {}",
                    crate::app::storage::format_bytes(progress.bytes_downloaded),
                    crate::app::storage::format_bytes(total)
                ),
            ),
            _ => context.set_progress(
                0.0,
                crate::app::storage::format_bytes(progress.bytes_downloaded),
            ),
        }
    })
    .map_err(|err| err.to_string())
}
