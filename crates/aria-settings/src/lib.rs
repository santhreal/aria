//! Aria user settings: the typed model and IO for `~/.aria/config.toml`.
//!
//! This crate is the **single owner** of the user config file. Everything the
//! app persists between runs that isn't project data or the recents/autosave
//! sidecars (those live in the OS data dir, see `aria-desktop::paths`)
//! lives here: AI providers, the theme, cloud API endpoint, and storage
//! locations/quotas. Keys never live in project files — the `[ai]` table is
//! the historical home for the API key and stays here.
//!
//! Three design rules carried over from the rest of the app:
//!
//! - **Loading is tolerant, and says what it dropped.** A missing file is
//!   all-defaults — a fresh install is a normal state, never an error (the
//!   `recent.json` philosophy). Only a *malformed* file is an `Err`, so callers
//!   can choose to surface the parse problem (the agent does) or fall back to
//!   defaults (app startup does, via `unwrap_or_default`). A value the file
//!   holds that this build cannot use — a relative storage path, a wrong type,
//!   a number out of range — keeps the default and is recorded in
//!   [`Settings::rejected`] so the operator can be told which line had no
//!   effect. See the `read` module.
//! - **Writing is format-preserving.** [`save`] round-trips the existing file
//!   through `toml_edit`, so hand-written comments, key ordering, and any
//!   tables a newer build added all survive a save from an older one. We only
//!   ever touch the keys we own.
//! - **Meaning is versioned.** Tolerance covers a missing key, not a changed
//!   meaning. Every table this crate owns is a
//!   [`PersistedShape`](schema::PersistedShape) with a version recorded in the
//!   `[schema]` table, and a stale copy is migrated or refused rather than
//!   served as if it were current. See the [`schema`] module.
//!
//! ```toml
//! [ai]
//! source = "local"                 # "local" | "openrouter" | "custom"
//! base_url = "http://localhost:11434/v1"   # Local / Advanced
//! model = "qwen3:14b"              # curated id, OR slug, or freeform
//! # api_key = "sk-..."             # OpenRouter / Advanced, or:
//! # api_key_env = "OPENROUTER_API_KEY"
//! # api_protocol = "responses"      # Advanced only; default chat_completions
//! # reasoning_summary = "off"       # default: "auto" in Responses mode
//! # autonomy = "full"              # skip destructive-tool confirmations
//!
//! [appearance]
//! theme = "dark-blue"              # "default" | "ember" | "dark-blue"
//!
//! [storage]
//! root = "/Volumes/Media/Aria"  # optional; absolute paths only
//! download_quota_mib = 2048        # default: 2 GiB
//!
//! [storage.paths]
//! proxies = "/Volumes/Scratch/Aria/proxies"
//! # analysis = "/Volumes/Scratch/Aria/analysis"
//! # ai_models = "/Volumes/Scratch/Aria/ai-models"
//! # download = "/Volumes/Scratch/Aria/download"
//! # catalog = "/Volumes/Scratch/Aria/catalog"
//! # luts = "/Volumes/Scratch/Aria/luts"
//! # lottie = "/Volumes/Scratch/Aria/lottie"
//! # templates = "/Volumes/Scratch/Aria/templates"
//!
//! [transcription]
//! connect_timeout_secs = 15        # model download; default 15
//! read_timeout_secs = 30           # per read; default 30
//! total_timeout_secs = 1800        # whole request; default 30 min
//!
//! # BYOK provider keys (stock, generation, TTS) — same literal-or-env
//! # pattern as [ai]. A configured key routes calls direct to the provider,
//! # bypassing the Aria backend entirely.
//! [providers.pexels]
//! api_key_env = "PEXELS_API_KEY"
//!
//! [providers.elevenlabs]
//! api_key = "sk-..."
//!
//! # Every table above is a versioned shape; see the `schema` module.
//! [schema]
//! ai = 2
//! appearance = 1
//!
//! # Anonymous cloud API host (stock, catalogs, update check).
//! [cloud]
//! base_url = "https://api.aria.sh"     # API override; empty = default
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use toml_edit::DocumentMut;

#[path = "persist.rs"]
mod persist;
#[path = "read.rs"]
mod read;
pub mod schema;
#[path = "schema_report.rs"]
pub mod schema_report;
#[path = "write.rs"]
mod write;

pub use read::{Refusal, Rejected};

// The transaction's pieces are reached by name from `save` and from the tests
// that inject filesystem faults into it.
pub(crate) use persist::*;
// The document patchers, reached by name from `schema::record_versions`, which
// writes the version table through the same "only touch what changed" rule.
pub(crate) use write::*;

/// Which bundled theme the shell renders. The variant order matches the
/// `index()` the UI dropdown uses; `DarkBlue` is the shipped default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeChoice {
    /// Cool graphite + teal (`DefaultTheme`).
    Default,
    /// Warm ember (`EmberTheme`).
    Ember,
    /// The shipped look (`DarkBlueTheme`).
    #[default]
    DarkBlue,
}

impl ThemeChoice {
    /// Every choice, in dropdown order.
    pub const ALL: [ThemeChoice; 3] = [
        ThemeChoice::Default,
        ThemeChoice::Ember,
        ThemeChoice::DarkBlue,
    ];

    /// The stable string written to `config.toml`.
    pub fn key(self) -> &'static str {
        match self {
            ThemeChoice::Default => "default",
            ThemeChoice::Ember => "ember",
            ThemeChoice::DarkBlue => "dark-blue",
        }
    }

    /// Parse a `config.toml` value; `None` for anything unrecognized (the
    /// caller keeps the default rather than failing the whole load).
    pub fn from_key(s: &str) -> Option<Self> {
        match s {
            "default" => Some(ThemeChoice::Default),
            "ember" => Some(ThemeChoice::Ember),
            "dark-blue" | "dark_blue" => Some(ThemeChoice::DarkBlue),
            _ => None,
        }
    }

    /// 0-based index for the Slint dropdown.
    pub fn index(self) -> i32 {
        match self {
            ThemeChoice::Default => 0,
            ThemeChoice::Ember => 1,
            ThemeChoice::DarkBlue => 2,
        }
    }

    /// Inverse of [`index`](Self::index); out-of-range falls back to the
    /// shipped default.
    pub fn from_index(i: i32) -> Self {
        match i {
            0 => ThemeChoice::Default,
            1 => ThemeChoice::Ember,
            _ => ThemeChoice::DarkBlue,
        }
    }
}

/// How much the agent may do without asking. Read by the desktop's tool
/// host when it executes destructive (System-tier) agent tools; the
/// validated edit vocabulary is not affected (it has its own preview/undo
/// flow).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Autonomy {
    /// Destructive tools (clear caches, run scripts, overwrite files)
    /// require a per-call confirmation in the agent panel.
    #[default]
    Ask,
    /// Run everything without confirmations.
    Full,
}

impl Autonomy {
    /// Every level, in the order a picker should offer them.
    ///
    /// A caller that offers a choice enumerates this, so a level added here cannot be
    /// persistable and unofferable at the same time.
    pub const ALL: [Self; 2] = [Self::Ask, Self::Full];

    /// The stable string written to `config.toml`.
    pub fn key(self) -> &'static str {
        match self {
            Autonomy::Ask => "ask",
            Autonomy::Full => "full",
        }
    }

    /// Parse a `config.toml` value; `None` for anything unrecognized (the
    /// caller keeps the default rather than failing the whole load).
    /// "confirm" is accepted as an alias for [`Autonomy::Ask`].
    pub fn from_key(s: &str) -> Option<Self> {
        match s {
            "ask" | "confirm" => Some(Autonomy::Ask),
            "full" => Some(Autonomy::Full),
            _ => None,
        }
    }
}

/// OpenAI-compatible HTTP protocol used by the editing agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AiApiProtocol {
    /// Broadly supported by Ollama, LM Studio, llama.cpp, and older gateways.
    #[default]
    ChatCompletions,
    /// OpenAI Responses API, required for reasoning models that call tools.
    Responses,
}

impl AiApiProtocol {
    /// Every protocol, in picker order.
    pub const ALL: [Self; 2] = [Self::ChatCompletions, Self::Responses];

    pub fn key(self) -> &'static str {
        match self {
            Self::ChatCompletions => "chat_completions",
            Self::Responses => "responses",
        }
    }

    /// Parse a persisted value. Aliases tolerate hand-written configs while
    /// keeping one canonical key on save.
    pub fn from_key(value: &str) -> Option<Self> {
        match value {
            "chat_completions" | "chat-completions" | "chat" => Some(Self::ChatCompletions),
            "responses" | "response" => Some(Self::Responses),
            _ => None,
        }
    }
}

/// Whether Responses requests ask the provider for a safe reasoning summary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReasoningSummary {
    /// Ask for the most detailed provider-supported summary.
    #[default]
    Auto,
    /// Do not request or display reasoning summaries.
    Off,
}

impl ReasoningSummary {
    /// Every mode, in picker order.
    pub const ALL: [Self; 2] = [Self::Auto, Self::Off];

    pub fn key(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Off => "off",
        }
    }

    pub fn from_key(value: &str) -> Option<Self> {
        match value {
            "auto" | "on" => Some(Self::Auto),
            "off" | "none" => Some(Self::Off),
            _ => None,
        }
    }
}

/// Which AI path Settings / the agent should use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AiSource {
    /// Local OpenAI-compatible server (Ollama, LM Studio). Base URL + curated model.
    #[default]
    Local,
    /// OpenRouter cloud gateway. One key + curated OR slug.
    OpenRouter,
    /// Freeform OpenAI-compatible endpoint (escape hatch).
    Custom,
}

impl AiSource {
    /// Every source, in picker order.
    pub const ALL: [Self; 3] = [Self::Local, Self::OpenRouter, Self::Custom];

    pub fn key(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::OpenRouter => "openrouter",
            Self::Custom => "custom",
        }
    }

    pub fn from_key(value: &str) -> Option<Self> {
        match value {
            "local" => Some(Self::Local),
            "openrouter" | "or" => Some(Self::OpenRouter),
            "custom" | "advanced" | "endpoint" => Some(Self::Custom),
            _ => None,
        }
    }
}

/// The `[ai]` table: how the agent reaches an OpenAI-compatible endpoint.
/// Plain data — key *resolution* (the `api_key_env` indirection) is an
/// AI-domain concern and lives in `aria_ai::config`. The default (empty
/// endpoint + model) is the "not configured" state
/// [`is_configured`](Self::is_configured) reports.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct AiSettings {
    /// Local / OpenRouter / freeform Advanced.
    pub source: AiSource,
    /// OpenAI-compatible endpoint root. Used by Local and Custom; ignored for
    /// OpenRouter (fixed gateway URL in `aria-ai`).
    pub base_url: String,
    /// Model id: curated local / OR slug / freeform for Custom.
    pub model: String,
    /// HTTP API shape. Local and OpenRouter always use Chat Completions;
    /// Custom may select Responses.
    pub api_protocol: AiApiProtocol,
    /// Provider-generated summary visibility for Responses reasoning models.
    pub reasoning_summary: ReasoningSummary,
    /// Literal API key. Local servers usually need none; OpenRouter requires one.
    pub api_key: Option<String>,
    /// Name of an environment variable holding the key (preferred over a
    /// literal for cloud providers).
    pub api_key_env: Option<String>,
    /// Confirmation policy for destructive agent tools. Orthogonal to
    /// [`is_configured`](Self::is_configured) — it gates tool *execution*,
    /// not provider reachability.
    pub autonomy: Autonomy,
}

impl std::fmt::Debug for AiSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AiSettings")
            .field("source", &self.source)
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("api_protocol", &self.api_protocol)
            .field("reasoning_summary", &self.reasoning_summary)
            .field("api_key", &self.api_key.as_ref().map(|_| "<redacted>"))
            .field("api_key_env", &self.api_key_env)
            .field("autonomy", &self.autonomy)
            .finish()
    }
}

impl AiSettings {
    /// Whether enough is set to attempt a prompt for the active [`AiSource`].
    /// Allowlist membership is enforced by the desktop / `aria-ai` catalog
    /// at save and prompt time; this only checks structural readiness.
    pub fn is_configured(&self) -> bool {
        let model = !self.model.trim().is_empty();
        match self.source {
            AiSource::Local => !self.base_url.trim().is_empty() && model,
            AiSource::OpenRouter => model && self.has_api_key(),
            AiSource::Custom => !self.base_url.trim().is_empty() && model,
        }
    }

    /// Literal or env-named key present (not resolved).
    pub fn has_api_key(&self) -> bool {
        self.api_key
            .as_deref()
            .is_some_and(|k| !k.trim().is_empty())
            || self
                .api_key_env
                .as_deref()
                .is_some_and(|k| !k.trim().is_empty())
    }
}

/// The `[appearance]` table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AppearanceSettings {
    /// The active bundled theme.
    pub theme: ThemeChoice,
}

/// One `[providers.<name>]` entry: a BYOK key for a third-party service
/// (stock search, image/video generation, TTS). Same literal-or-env shape
/// as [`AiSettings`]; key *resolution* stays with the caller (the env
/// indirection is a use-site concern). A configured provider routes calls
/// direct — the Aria backend never sees the key.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct ProviderSettings {
    /// Literal API key.
    pub api_key: Option<String>,
    /// Name of an environment variable holding the key (preferred).
    pub api_key_env: Option<String>,
}

impl std::fmt::Debug for ProviderSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderSettings")
            .field("api_key", &self.api_key.as_ref().map(|_| "<redacted>"))
            .field("api_key_env", &self.api_key_env)
            .finish()
    }
}

impl ProviderSettings {
    /// Whether either key form is present (the BYOK routing predicate).
    pub fn is_configured(&self) -> bool {
        self.api_key
            .as_deref()
            .is_some_and(|k| !k.trim().is_empty())
            || self
                .api_key_env
                .as_deref()
                .is_some_and(|k| !k.trim().is_empty())
    }

    /// Resolve the key with the same precedence and blank handling as
    /// `aria_ai::config`: a named `api_key_env` wins, and its value counts only
    /// when it holds more than whitespace; otherwise the literal `api_key`.
    /// Whitespace is never trimmed off a key that does resolve, because some
    /// providers issue keys with significant characters at the edges.
    pub fn resolve_key(&self) -> Option<String> {
        let from_env = self
            .api_key_env
            .as_deref()
            .filter(|name| !name.trim().is_empty())
            .and_then(|name| std::env::var(name).ok())
            .filter(|value| !value.trim().is_empty());
        from_env.or_else(|| {
            self.api_key
                .as_deref()
                .filter(|key| !key.trim().is_empty())
                .map(str::to_owned)
        })
    }
}

/// The `[cloud]` table: anonymous API host override for stock, catalogs,
/// and the update check. Empty `base_url` means the shipped default.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CloudSettings {
    /// Backend (API) base URL override; empty = the shipped default.
    pub base_url: String,
}

/// Default quota for downloaded, re-fetchable assets: 2048 MiB (2 GiB).
pub const DEFAULT_DOWNLOAD_QUOTA_MIB: u64 = 2_048;

/// Smallest accepted download-cache quota.
pub const MIN_DOWNLOAD_QUOTA_MIB: u64 = 1;

/// Largest accepted download-cache quota: 1 TiB.
///
/// Keeping a finite upper bound makes conversion to bytes and downstream
/// accounting predictable even when the config file was hand-edited.
pub const MAX_DOWNLOAD_QUOTA_MIB: u64 = 1_048_576;

/// Known per-cache overrides in `[storage.paths]`.
///
/// Every populated path is absolute. An empty, relative, or wrongly-typed
/// value keeps the default and is reported in [`Settings::rejected`] rather
/// than failing the load.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StoragePathOverrides {
    /// Pool poster frames rendered once and read back on the next launch.
    pub library_thumbnails: Option<PathBuf>,
    /// Generated media proxies.
    pub proxies: Option<PathBuf>,
    /// Regenerable media-analysis state.
    pub analysis: Option<PathBuf>,
    /// Downloaded AI model weights for transcription, vision, and embeddings.
    pub ai_models: Option<PathBuf>,
    /// Downloaded stock and generated assets.
    pub download: Option<PathBuf>,
    /// Cached asset-catalog responses and metadata.
    pub catalog: Option<PathBuf>,
    /// Downloaded LUT packs.
    pub luts: Option<PathBuf>,
    /// Downloaded Lottie assets.
    pub lottie: Option<PathBuf>,
    /// Downloaded and installed template bundles.
    pub templates: Option<PathBuf>,
}

impl StoragePathOverrides {
    /// Every override key this table understands, in `[storage.paths]` order.
    ///
    /// A caller that edits overrides enumerates this rather than writing its own
    /// list, so a new override cannot be settable in one place and invisible in
    /// another.
    pub const KEYS: [&'static str; 9] = [
        "library_thumbnails",
        "proxies",
        "analysis",
        "ai_models",
        "download",
        "catalog",
        "luts",
        "lottie",
        "templates",
    ];

    /// Return a known override by its exact TOML key.
    ///
    /// Unknown keys return `None`; they are still preserved in the TOML
    /// document when [`save`] patches a file.
    pub fn get(&self, key: &str) -> Option<&Path> {
        self.slot_ref(key).and_then(|slot| slot.as_deref())
    }

    /// The writable slot for one known override key.
    ///
    /// Setting an override goes through here rather than through a field so the key
    /// the config file uses and the field it lands in cannot disagree.
    pub fn slot(&mut self, key: &str) -> Option<&mut Option<PathBuf>> {
        match key {
            "library_thumbnails" => Some(&mut self.library_thumbnails),
            "proxies" => Some(&mut self.proxies),
            "analysis" => Some(&mut self.analysis),
            "ai_models" => Some(&mut self.ai_models),
            "download" => Some(&mut self.download),
            "catalog" => Some(&mut self.catalog),
            "luts" => Some(&mut self.luts),
            "lottie" => Some(&mut self.lottie),
            "templates" => Some(&mut self.templates),
            _ => None,
        }
    }

    fn slot_ref(&self, key: &str) -> Option<&Option<PathBuf>> {
        match key {
            "library_thumbnails" => Some(&self.library_thumbnails),
            "proxies" => Some(&self.proxies),
            "analysis" => Some(&self.analysis),
            "ai_models" => Some(&self.ai_models),
            "download" => Some(&self.download),
            "catalog" => Some(&self.catalog),
            "luts" => Some(&self.luts),
            "lottie" => Some(&self.lottie),
            "templates" => Some(&self.templates),
            _ => None,
        }
    }
}

/// The `[storage]` table: optional storage roots and the download-cache quota.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageSettings {
    /// Optional absolute root for Aria-managed storage.
    pub root: Option<PathBuf>,
    /// Download-cache quota in MiB.
    ///
    /// The default is [`DEFAULT_DOWNLOAD_QUOTA_MIB`] (2048 MiB); accepted
    /// values are [`MIN_DOWNLOAD_QUOTA_MIB`] through
    /// [`MAX_DOWNLOAD_QUOTA_MIB`], inclusive.
    pub download_quota_mib: u64,
    /// Absolute per-cache overrides from `[storage.paths]`.
    pub paths: StoragePathOverrides,
}

impl StorageSettings {
    /// Whether `value` is safe to use as a download-cache quota.
    pub fn is_valid_download_quota_mib(value: u64) -> bool {
        (MIN_DOWNLOAD_QUOTA_MIB..=MAX_DOWNLOAD_QUOTA_MIB).contains(&value)
    }
}

impl Default for StorageSettings {
    fn default() -> Self {
        Self {
            root: None,
            download_quota_mib: DEFAULT_DOWNLOAD_QUOTA_MIB,
            paths: StoragePathOverrides::default(),
        }
    }
}

/// The `[transcription]` table: model-download timeouts.
///
/// Seconds rather than durations: the file is hand-editable, and a TOML duration would
/// be a second syntax for the same number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TranscriptionSettings {
    /// Connect timeout for a model download.
    pub connect_timeout_secs: u64,
    /// Per-read timeout for a model download: how long a stalled mirror is tolerated.
    pub read_timeout_secs: u64,
    /// Whole-request deadline for a model download.
    pub total_timeout_secs: u64,
}

/// Longest any download timeout may be set to (24 hours). A deadline further out is
/// indistinguishable from none, which is the failure mode a deadline exists to avoid.
pub const MAX_DOWNLOAD_TIMEOUT_SECS: u64 = 24 * 60 * 60;

impl TranscriptionSettings {
    /// Whether `value` is usable as a download timeout. Zero is refused because
    /// `HttpDownloader` refuses it, and a config the downloader rejects would fail at
    /// the click rather than at the edit.
    pub fn is_valid_timeout_secs(value: u64) -> bool {
        (1..=MAX_DOWNLOAD_TIMEOUT_SECS).contains(&value)
    }
}

impl Default for TranscriptionSettings {
    fn default() -> Self {
        // The same values `HttpDownloader::default` uses, so an absent table and an
        // absent config behave identically.
        Self {
            connect_timeout_secs: 15,
            read_timeout_secs: 30,
            total_timeout_secs: 30 * 60,
        }
    }
}

/// The whole user config, one struct per table. [`Settings::default`] is the
/// state of a fresh install (no file on disk).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Settings {
    /// `[appearance]`.
    pub appearance: AppearanceSettings,
    /// `[ai]`.
    pub ai: AiSettings,
    /// `[providers.<name>]` — BYOK keys by provider name ("pexels",
    /// "pixabay", "elevenlabs", …). Sorted map so saves are deterministic.
    pub providers: BTreeMap<String, ProviderSettings>,
    /// `[cloud]`.
    pub cloud: CloudSettings,
    /// `[storage]`.
    pub storage: StorageSettings,
    /// `[transcription]`.
    pub transcription: TranscriptionSettings,
    /// `[plugins]` — values a plugin declared, by the dotted key it named
    /// (`notes.directory`). This crate knows nothing about them beyond that they
    /// are strings: the plugin that declared a key is the only thing that reads
    /// it. Sorted map so saves are deterministic.
    pub plugins: BTreeMap<String, String>,
    /// Every value the file held that this build could not use, in the order
    /// the loader met them. Empty for a fresh install, for a file this build
    /// understands, and for anything [`save`] wrote. A dropped value is a
    /// message for the operator, not a load failure: see [`Rejected`].
    pub rejected: Vec<Rejected>,
}

impl Settings {
    /// The named provider's settings, defaulting to unconfigured.
    pub fn provider(&self, name: &str) -> ProviderSettings {
        self.providers.get(name).cloned().unwrap_or_default()
    }

    /// The value a plugin stored under `key`.
    pub fn plugin_value(&self, key: &str) -> Option<&str> {
        self.plugins.get(key).map(String::as_str)
    }

    /// Store or clear a plugin value. An empty value removes the key, so "unset"
    /// and "set to nothing" do not collapse into the same state.
    pub fn set_plugin_value(&mut self, key: &str, value: Option<&str>) {
        match value.map(str::trim).filter(|value| !value.is_empty()) {
            Some(value) => {
                self.plugins.insert(key.to_string(), value.to_string());
            }
            None => {
                self.plugins.remove(key);
            }
        }
    }
}

/// `~/.aria/config.toml` — the user's home dir on every platform
/// (`C:\Users\<name>\.aria\config.toml` on Windows, where `HOME` is
/// unset). Falls back to the temp dir only if the home dir can't be resolved;
/// never the working directory, which is the read-only install folder on
/// Windows.
pub fn default_config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join(".aria")
        .join("config.toml")
}

/// `~/.aria/agent/` — the user's AI-assistant extension dir
/// (`rules/*.md`, `skills/<id>/SKILL.md`, `commands/*.md`), reloaded by
/// the desktop before every prompt so edits apply without a restart.
pub fn agent_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join(".aria")
        .join("agent")
}

/// Load settings from `path`. A missing file yields [`Settings::default`]
/// (not an error); a malformed file is an `Err` naming the problem so the
/// caller can surface it. Unknown keys/tables are ignored, and any key we
/// don't recognize keeps its default — a partially-written file still loads.
///
/// A table whose recorded schema version this build cannot read is an `Err`
/// naming the table and both versions: a shape whose meaning changed is
/// refused rather than served. A table recording an older version is migrated
/// to the current shape. See the [`schema`] module.
pub fn load(path: &Path) -> Result<Settings, String> {
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Settings::default()),
        Err(e) => return Err(format!("could not read {}: {e}", path.display())),
    };
    let doc = raw
        .parse::<DocumentMut>()
        .map_err(|e| format!("could not parse {}: {e}", path.display()))?;
    Settings::from_document(&doc).map_err(|e| format!("could not read {}: {e}", path.display()))
}

/// Persist `settings` to `path`, preserving everything we don't own.
///
/// Reads the existing file (if any) into a `toml_edit` document, patches the
/// keys this crate manages, and writes it back. Comments, blank lines, key
/// order, and unknown tables survive. The parent directory is created on
/// demand. A malformed existing file is an `Err` rather than silently
/// clobbered — refusing to overwrite a file we couldn't understand. So is a
/// file recording a schema version this build cannot read: an older build must
/// not rewrite a newer build's meaning.
pub fn save(path: &Path, settings: &Settings) -> Result<(), String> {
    let mut doc = match std::fs::read_to_string(path) {
        Ok(raw) => raw.parse::<DocumentMut>().map_err(|_| {
            format!(
                "could not parse {}: the existing configuration is malformed",
                path.display()
            )
        })?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => DocumentMut::new(),
        Err(e) => return Err(format!("could not read {}: {e}", path.display())),
    };

    schema::validate(&doc).map_err(|e| {
        format!(
            "could not replace {}: {e}; the existing configuration was left in place",
            path.display()
        )
    })?;

    settings.write_into(&mut doc)?;

    // Materialize and validate the complete output before creating directories,
    // temporary files, or otherwise changing filesystem state.
    let serialized = doc.to_string();
    let _: DocumentMut = serialized.parse().map_err(|_| {
        format!(
            "could not validate generated configuration for {}",
            path.display()
        )
    })?;

    persist_serialized(path, serialized.as_bytes())
}

/// An absolute path for fixtures on any host: `/x` is absolute on unix but
/// root-relative on Windows, where [`Path::is_absolute`] is false for it and
/// the reader rightly rejects it as non-absolute. Fixture roots go through
/// here so the stale/round-trip/rejection suites assert the same acceptances
/// on both platforms.
#[cfg(test)]
pub(crate) fn fixture_abs(rest: &str) -> String {
    if cfg!(windows) {
        format!("C:/fixtures/{rest}")
    } else {
        format!("/{rest}")
    }
}

#[cfg(test)]
mod tests;
