//! The Whisper model store: what is on disk, and the job that installs it.
//!
//! `aria-transcription::ModelManager` owns verification and the atomic install; nothing
//! here reimplements a hash or a temporary file. What this module owns:
//!
//! - **Install is a job with determinate progress.** The catalog knows the exact byte
//!   count, so a wrapping downloader reports a real fraction instead of a spinner.
//! - **A corrupt file is a state, not a crash.** `ModelStatus::Invalid` carries the
//!   integrity failure, and the row offers the re-download that fixes it.

use std::path::PathBuf;
use std::time::Duration;

use aria_core::count::{counted, noun};
use aria_jobs::JobCompletion;
use aria_transcription::{
    HttpDownloader, ModelDownloader, ModelManager, ModelStatus, WhisperModel,
};

use super::AppState;

/// Which download timeout a control writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeoutField {
    Connect,
    Read,
    Total,
}

impl TimeoutField {
    /// The fields, as the settings rows. A data table so a new timeout shows up as a
    /// missing row rather than as one nobody can edit.
    pub const ALL: [(Self, &'static str, &'static str); 3] = [
        (Self::Connect, "model-timeout-connect", "Connect timeout"),
        (Self::Read, "model-timeout-read", "Read timeout"),
        (Self::Total, "model-timeout-total", "Total timeout"),
    ];

    /// The configured value for this field.
    pub fn secs(self, settings: &aria_settings::TranscriptionSettings) -> u64 {
        match self {
            Self::Connect => settings.connect_timeout_secs,
            Self::Read => settings.read_timeout_secs,
            Self::Total => settings.total_timeout_secs,
        }
    }
}
/// One catalogued model as the settings row needs it.
#[derive(Debug, Clone, PartialEq)]
pub struct UiModel {
    pub model: WhisperModel,
    pub id: &'static str,
    pub description: &'static str,
    pub bytes: u64,
    /// "Ready", "Not installed", "Corrupt", or "Unreadable".
    pub status: &'static str,
    /// The integrity or filesystem failure behind a non-Ready status.
    pub detail: String,
    /// Whether the action is a re-download rather than a first install.
    pub reinstall: bool,
}

impl AppState {
    /// The model store: the layout's `ai_models` entry, so the storage root and
    /// an override move it.
    pub fn model_manager(&self) -> Result<ModelManager, String> {
        let root = self
            .storage_layout()
            .resolve(aria_storage::CacheId::AiModels)
            .expect("ai_models is a disk cache");
        ModelManager::new(root).map_err(|err| err.to_string())
    }

    /// Every catalogued model with its held on-disk state.
    ///
    /// Held on `SettingsForm` and refreshed at startup, when Settings opens,
    /// when the Captions panel comes forward, and when a model job finishes so
    /// a draw does not stat model files per frame.
    pub fn model_rows(&self) -> Vec<UiModel> {
        self.settings_form.model_rows.to_vec()
    }

    /// The first verified model, which is what a run uses.
    pub fn installed_model(&self) -> Option<(WhisperModel, PathBuf)> {
        self.settings_form.installed_model.clone()
    }

    /// Re-read [`Self::model_rows`] and [`Self::installed_model`] from the
    /// model store.
    pub fn refresh_models(&mut self) {
        let manager = self.model_manager();
        let mut installed = None;
        let rows: Vec<UiModel> = WhisperModel::catalog()
            .iter()
            .map(|model| {
                let spec = model.spec();
                let (status, detail, reinstall) = match &manager {
                    Err(err) => ("Unreadable", err.clone(), false),
                    Ok(manager) => match manager.status(*model) {
                        Ok(ModelStatus::Ready { path }) => {
                            if installed.is_none() {
                                installed = Some((*model, path));
                            }
                            ("Ready", String::new(), false)
                        }
                        Ok(ModelStatus::Missing) => ("Not installed", String::new(), false),
                        Ok(ModelStatus::Invalid { reason, .. }) => {
                            ("Corrupt", reason.to_string(), true)
                        }
                        Err(err) => ("Unreadable", err.to_string(), false),
                    },
                };
                UiModel {
                    model: *model,
                    id: spec.id(),
                    description: spec.resource_description(),
                    bytes: spec.exact_bytes(),
                    status,
                    detail,
                    reinstall,
                }
            })
            .collect();
        self.settings_form.model_rows = rows.into();
        self.settings_form.installed_model = installed;
    }

    /// Install (or re-install) a model as a job.
    ///
    /// A corrupt file is replaced rather than removed first: the manager verifies what
    /// is on disk before reusing it, so the download is the repair.
    ///
    /// The timeouts come from the config, and an unusable set is refused here rather
    /// than inside the job: a job row that fails instantly says less than a message
    /// naming the setting.
    pub fn install_model(&mut self, model: WhisperModel) -> Result<u64, String> {
        let manager = self.model_manager()?;
        let downloader = self.model_downloader()?;
        let spec = model.spec();
        let total = spec.exact_bytes();
        let label = format!("Downloading {}", spec.id());
        let id = self.spawn_job(
            super::jobs::JobSpec::new(super::jobs::JobKind::Download, label)
                .with_target(format!("model:{}", spec.id())),
            move |context| {
                // The reader is `Box<dyn Read + 'static>` and cannot borrow the context, so
                // bytes travel over a channel and one scoped thread turns them into
                // progress. No polling: the reporter blocks on the receiver.
                let (tx, rx) = crossbeam_channel::unbounded::<u64>();
                let outcome = std::thread::scope(|scope| {
                    scope.spawn(|| {
                        while let Ok(read) = rx.recv() {
                            let fraction = if total == 0 {
                                0.0
                            } else {
                                read as f32 / total as f32
                            };
                            context.set_progress(
                                fraction,
                                format!("{} / {} MiB", read >> 20, total >> 20),
                            );
                        }
                    });
                    let cancelled = || context.cancelled();
                    // The job registry, not the manager, owns the moment the
                    // install stops being cancellable. `try_begin_commit` flips
                    // that state atomically and answers whether it won; the
                    // manager calls it immediately before the first rename, so a
                    // Cancel pressed in the same instant either wins outright (no
                    // rename happens, the old file survives) or loses outright (the
                    // rename completes and the row stops offering Cancel). Without
                    // the handshake the two race and Cancel can hit a model that
                    // is already installed.
                    let begin_commit = || context.try_begin_commit();
                    let result = {
                        let downloader = ProgressDownloader {
                            inner: downloader.clone(),
                            bytes: tx.clone(),
                        };
                        manager.ensure_with_cancellation_and_commit(
                            model,
                            &downloader,
                            &cancelled,
                            &begin_commit,
                        )
                    };
                    // Every sender is gone once this one is: the reporter ends and the
                    // scope joins it.
                    drop(tx);
                    result
                });
                let path = outcome.map_err(|err| err.to_string())?;
                JobCompletion::new("verified")
                    .with_output("path", path.display().to_string())
                    .map_err(String::from)
            },
        );
        self.status_message = Some(format!("Downloading {}", spec.id()));
        Ok(id.raw())
    }

    /// Re-hash every model file on disk and report the ones that no longer match.
    ///
    /// `status` is cheap and answers "is there a plausible file here"; `verify`
    /// reads the whole file and checks its digest. Bit rot, a truncated copy
    /// onto a full disk, and a half-restored backup all leave a file that
    /// `status` is happy with and a run that fails deep inside Whisper with a
    /// message about tensors. This is the sweep that names the file instead.
    ///
    /// A job because a large model is gigabytes of hashing; cancellable because
    /// a sweep started by mistake should not have to run to the end.
    ///
    /// # Errors
    ///
    /// Returns the store error when the model directory cannot be opened. Per
    /// model failures are the job's failure detail, not this call's.
    pub fn verify_models(&mut self) -> Result<u64, String> {
        let manager = self.model_manager()?;
        let id = self.spawn_job(
            super::jobs::JobSpec::new(super::jobs::JobKind::Other, "Verifying models")
                .with_target("verify_models"),
            move |context| {
                let catalog = WhisperModel::catalog();
                let mut checked = 0usize;
                let mut failures = Vec::new();
                for (index, model) in catalog.iter().enumerate() {
                    if context.cancelled() {
                        return Err("cancelled".to_string());
                    }
                    let spec = model.spec();
                    context.set_progress(
                        (index + 1) as f32 / catalog.len() as f32,
                        format!("hashing {}", spec.id()),
                    );
                    // A model nobody installed is not a failure, and re-hashing
                    // one already known corrupt is how the row confirms a
                    // re-download fixed it.
                    let present = match manager.status(*model) {
                        Ok(ModelStatus::Missing) => false,
                        Ok(ModelStatus::Ready { .. } | ModelStatus::Invalid { .. }) => true,
                        Err(err) => {
                            failures.push(format!("{}: {err}", spec.id()));
                            continue;
                        }
                    };
                    if !present {
                        continue;
                    }
                    checked += 1;
                    if let Err(err) = manager.verify(*model) {
                        failures.push(format!("{}: {err}", spec.id()));
                    }
                }
                if failures.is_empty() {
                    return Ok(JobCompletion::new(format!(
                        "{} still {} {} digest",
                        counted(checked, "installed model", "installed models"),
                        noun(checked, "matches", "match"),
                        noun(checked, "its", "their")
                    )));
                }
                Err(failures.join("; "))
            },
        );
        self.set_job_status_line("Verifying installed models");
        Ok(id.raw())
    }

    /// The HTTP downloader the configured timeouts describe.
    fn model_downloader(&self) -> Result<HttpDownloader, String> {
        let timeouts = self.cloud_settings().transcription;
        HttpDownloader::with_timeouts(
            Duration::from_secs(timeouts.connect_timeout_secs),
            Duration::from_secs(timeouts.read_timeout_secs),
            Duration::from_secs(timeouts.total_timeout_secs),
        )
        .map_err(|err| format!("transcription download timeouts: {err}"))
    }

    /// Persist one download timeout, clamped to what the downloader accepts.
    ///
    /// Clamping rather than refusing: the control steps, and a step that lands out of
    /// range should stop at the edge instead of reporting an error nothing asked
    /// for.
    pub fn set_model_timeout(&mut self, field: TimeoutField, secs: u64) -> Result<(), String> {
        let path = self.config_path();
        let mut settings = aria_settings::load(&path)?;
        let clamped = secs.clamp(1, aria_settings::MAX_DOWNLOAD_TIMEOUT_SECS);
        let slot = match field {
            TimeoutField::Connect => &mut settings.transcription.connect_timeout_secs,
            TimeoutField::Read => &mut settings.transcription.read_timeout_secs,
            TimeoutField::Total => &mut settings.transcription.total_timeout_secs,
        };
        if *slot == clamped {
            return Ok(());
        }
        *slot = clamped;
        aria_settings::save(&path, &settings)?;
        self.refresh_cloud_settings();
        Ok(())
    }

    /// The download timeouts, for the settings rows.
    pub fn model_timeouts(&self) -> aria_settings::TranscriptionSettings {
        self.cloud_settings().transcription
    }
}

/// A downloader that forwards byte counts so the job can report a real fraction.
///
/// The progress lives here rather than in `aria-transcription`: the catalog already
/// knows the exact byte count, and that crate has no opinion about job rows.
struct ProgressDownloader {
    inner: HttpDownloader,
    bytes: crossbeam_channel::Sender<u64>,
}

impl ModelDownloader for ProgressDownloader {
    fn download(
        &self,
        spec: &aria_transcription::ModelSpec,
    ) -> Result<aria_transcription::DownloadReader, aria_transcription::DownloadError> {
        let reader = self.inner.download(spec)?;
        Ok(Box::new(CountingReader {
            inner: reader,
            read: 0,
            bytes: self.bytes.clone(),
        }))
    }
}

/// Wraps a download stream and reports the running total per read.
struct CountingReader {
    inner: aria_transcription::DownloadReader,
    read: u64,
    bytes: crossbeam_channel::Sender<u64>,
}

impl std::io::Read for CountingReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let count = self.inner.read(buf)?;
        self.read = self.read.saturating_add(count as u64);
        // A full channel is impossible (unbounded) and a closed one means the job is
        // over: either way the read itself must still succeed.
        let _ = self.bytes.send(self.read);
        Ok(count)
    }
}
