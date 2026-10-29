//! Cloud, analysis, jobs, transcription, and the assistant.

use std::path::PathBuf;
use std::sync::Arc;

use aria_ai::AgentConfig;
use aria_analysis::{BeatConfig, LoudnessConfig, ShotDetectionConfig, SilenceConfig};
use aria_cloud::{CloudClient, DEFAULT_BASE_URL};
use aria_jobs::{JobManager, JobState};
use aria_plugin::{
    DropAction, EventBus, JobFinished, JobOutcome, JobProgress, Plugin, PluginContext, PluginError,
    PluginManifest, ResolveDropped, Service, SettingsService, StorageService,
};
use aria_transcription::ModelManager;

use aria_storage::StorageLayout;

#[derive(Debug)]
pub struct CloudPlugin {
    pub base_url: String,
}

impl Default for CloudPlugin {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_string(),
        }
    }
}

impl Plugin for CloudPlugin {
    fn manifest(&self) -> &PluginManifest {
        // `uses`: the cache directory and the configured base URL.
        static MANIFEST: PluginManifest = PluginManifest::new(
            "aria.cloud",
            "Cloud",
            "0.1.0",
            &["aria.storage"],
            &["StorageService", "SettingsService"],
        );
        &MANIFEST
    }

    fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        // No cache rather than a cache in the default profile: storage is a
        // declared requirement, so a context without it is a host that mounted
        // nothing, not a host with another directory in mind.
        let cache = ctx
            .try_get::<dyn StorageService>()
            .map(|s| s.cache_dir().join("catalog"));
        let base = ctx
            .try_get::<dyn SettingsService>()
            .and_then(|s| s.get("cloud.base_url"))
            .and_then(|v| v.as_str().map(str::to_string))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| self.base_url.clone());
        ctx.register(CloudClient::new(&base, cache));
        Ok(())
    }
}

#[derive(Debug)]
pub struct AnalysisPlugin;

/// The detector settings every analysis run uses.
///
/// One owner: the job reads these off the request rather than calling
/// `::default()` at the point of use, so the plugin that provides analysis is the
/// plugin that decides how it is tuned. A host without this plugin falls back to
/// the crate defaults, which is what the service holds anyway.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct DetectorTuning {
    pub shots: ShotDetectionConfig,
    pub silence: SilenceConfig,
    pub beats: BeatConfig,
    pub loudness: LoudnessConfig,
}

impl Service for DetectorTuning {}

impl Plugin for AnalysisPlugin {
    fn manifest(&self) -> &PluginManifest {
        static MANIFEST: PluginManifest =
            PluginManifest::new("aria.analysis", "Analysis", "0.1.0", &["aria.decoder"], &[]);
        &MANIFEST
    }

    fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        let tuning = DetectorTuning::default();
        ctx.register(tuning);
        aria_plugin::register_shipped_analyzers(
            ctx,
            tuning.beats,
            tuning.silence,
            tuning.loudness,
        )?;
        Ok(())
    }
}

#[derive(Debug)]
pub struct JobsPlugin;

impl Plugin for JobsPlugin {
    fn manifest(&self) -> &PluginManifest {
        static MANIFEST: PluginManifest =
            PluginManifest::new("aria.jobs", "Jobs", "0.1.0", &[], &[]);
        &MANIFEST
    }

    fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        let manager = JobManager::new();
        announce_jobs(&manager, ctx.event_bus());
        ctx.register(manager);
        Ok(())
    }
}

/// Post the board's transitions on `bus`: `JobProgress` while a job is queued
/// or running, `JobFinished` when it ends. The subscription lasts as long as
/// the registry and holds the bus weakly.
pub(crate) fn announce_jobs(manager: &JobManager, bus: &Arc<EventBus>) {
    let bus = Arc::downgrade(bus);
    manager.subscribe(move |snapshot| {
        let Some(bus) = bus.upgrade() else {
            return;
        };
        let job = snapshot.id.raw();
        let outcome = match snapshot.state {
            JobState::Queued | JobState::Running => {
                bus.post(JobProgress {
                    job,
                    label: snapshot.label,
                    progress: snapshot.progress,
                    detail: snapshot.detail,
                });
                return;
            }
            JobState::Done => JobOutcome::Done,
            JobState::Failed => JobOutcome::Failed,
            JobState::Cancelled => JobOutcome::Cancelled,
        };
        bus.post(JobFinished {
            job,
            label: snapshot.label,
            outcome,
            detail: snapshot.detail,
        });
    });
}

/// Speech models, kept under the session's own root.
///
/// `root` empty means "wherever storage put this profile", which is what the
/// shipped app asks for; an absolute path overrides it, for a host that keeps
/// models on another volume.
#[derive(Debug, Default)]
pub struct TranscriptionPlugin {
    pub root: PathBuf,
}

impl Plugin for TranscriptionPlugin {
    fn manifest(&self) -> &PluginManifest {
        static MANIFEST: PluginManifest = PluginManifest::new(
            "aria.transcription",
            "Transcription",
            "0.1.0",
            &["aria.decoder", "aria.storage"],
            &["StorageLayout"],
        );
        &MANIFEST
    }

    fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        // The profile's root, from the plugin that owns it. A directory resolved
        // here would be the default profile's, so a session pointed elsewhere
        // downloaded gigabytes of models into a directory it never reads.
        let root = if self.root.is_absolute() {
            self.root.to_path_buf()
        } else {
            let Some(layout) = ctx.try_get::<StorageLayout>() else {
                return Err(PluginError::MountFailed(
                    "aria.transcription".into(),
                    "storage did not mount, so there is no root to keep models under".into(),
                ));
            };
            layout.root().join("models")
        };
        match ModelManager::new(&root) {
            Ok(manager) => {
                ctx.register(manager);
            }
            Err(err) => {
                tracing::warn!(error = %err, "transcription model dir rejected");
            }
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct AiPlugin;

/// The assistant's guardrails, owned by the plugin that provides the assistant.
///
/// Settings reads these, so the caps a run is held to are the caps shown, rather
/// than a copy of the defaults written next to the display.
#[derive(Debug)]
pub struct AiHost {
    config: AgentConfig,
}

impl AiHost {
    pub fn agent_config(&self) -> &AgentConfig {
        &self.config
    }
}

impl Service for AiHost {}

impl Plugin for AiPlugin {
    fn manifest(&self) -> &PluginManifest {
        static MANIFEST: PluginManifest =
            PluginManifest::new("aria.ai", "AI", "0.1.0", &["aria.engine", "aria.jobs"], &[]);
        &MANIFEST
    }

    fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        ctx.register(AiHost {
            config: AgentConfig::default(),
        });
        // A dropped `SKILL.md` is an assistant skill, whatever directory it came
        // from. Asked last, so nothing else wants it by then.
        ctx.on_serial::<ResolveDropped, DropAction, _>(|event| {
            event
                .item
                .as_path()
                .and_then(|path| path.file_name())
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.eq_ignore_ascii_case("SKILL.md"))
                .then_some(DropAction::InstallSkill)
        });
        Ok(())
    }
}
