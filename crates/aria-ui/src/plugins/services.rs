//! Cloud, analysis, jobs, transcription, and the assistant.

use std::path::PathBuf;

use aria_ai::AgentConfig;
use aria_analysis::{BeatConfig, LoudnessConfig, ShotDetectionConfig, SilenceConfig};
use aria_cloud::{CloudClient, DEFAULT_BASE_URL};
use aria_jobs::JobManager;
use aria_plugin::{
    DropAction, Plugin, PluginContext, PluginError, PluginManifest, ResolveDroppedPath, Service,
    SettingsService, StorageService,
};
use aria_transcription::ModelManager;

use aria_storage::StorageLayout;

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
        &PluginManifest {
            id: "aria.cloud",
            name: "Cloud",
            version: "0.1.0",
            requires: &["aria.storage"],
            // Reads the cache directory and the configured base URL.
            uses: &["StorageService", "SettingsService"],
        }
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
        &PluginManifest {
            id: "aria.analysis",
            name: "Analysis",
            version: "0.1.0",
            requires: &["aria.decoder"],
            uses: &[],
        }
    }

    fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        ctx.register(DetectorTuning::default());
        Ok(())
    }
}

pub struct JobsPlugin;

impl Plugin for JobsPlugin {
    fn manifest(&self) -> &PluginManifest {
        &PluginManifest {
            id: "aria.jobs",
            name: "Jobs",
            version: "0.1.0",
            requires: &[],
            uses: &[],
        }
    }

    fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        ctx.register(JobManager::new());
        Ok(())
    }
}

/// Speech models, kept under the session's own root.
///
/// `root` empty means "wherever storage put this profile", which is what the
/// shipped app asks for; an absolute path overrides it, for a host that keeps
/// models on another volume.
#[derive(Default)]
pub struct TranscriptionPlugin {
    pub root: PathBuf,
}

impl Plugin for TranscriptionPlugin {
    fn manifest(&self) -> &PluginManifest {
        &PluginManifest {
            id: "aria.transcription",
            name: "Transcription",
            version: "0.1.0",
            requires: &["aria.decoder", "aria.storage"],
            uses: &["StorageLayout"],
        }
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
                    "aria.transcription",
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

pub struct AiPlugin;

/// The assistant's guardrails, owned by the plugin that provides the assistant.
///
/// Settings reads these, so the caps a run is held to are the caps shown, rather
/// than a copy of the defaults written next to the display.
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
        &PluginManifest {
            id: "aria.ai",
            name: "AI",
            version: "0.1.0",
            requires: &["aria.engine", "aria.jobs"],
            uses: &[],
        }
    }

    fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        ctx.register(AiHost {
            config: AgentConfig::default(),
        });
        // A dropped `SKILL.md` is an assistant skill, whatever directory it came
        // from. Asked last, so nothing else wants it by then.
        ctx.on_serial::<ResolveDroppedPath, DropAction, _>(|event| {
            event
                .path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.eq_ignore_ascii_case("SKILL.md"))
                .then_some(DropAction::InstallSkill)
        });
        Ok(())
    }
}
