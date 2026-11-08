//! The analysis axis: one detector per row, and the shapes the panel draws.
//!
//! Each type and its `impl` blocks are defined here and import parent module
//! items through `use super::*`.

use super::*;

/// Confidence thresholds the panel offers.
pub const CONFIDENCE_CHOICES: [f32; 4] = [0.0, 0.25, 0.5, 0.75];

/// One detector: what it is called, what it writes, and which version wrote it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnalysisKind {
    /// Hard-cut candidates from sampled video frames.
    Shots,
    /// Silent spans from windowed RMS.
    Silence,
    /// Onset points from short-time energy change.
    Beats,
}

impl AnalysisKind {
    /// Every detector, as the panel's buttons and the tests' enumeration.
    ///
    /// A data table rather than three call sites: a new detector shows up as a missing
    /// row here, not as a feature nobody can reach.
    pub const ALL: [Self; 3] = [Self::Shots, Self::Silence, Self::Beats];

    /// The element id prefix and the stable analyzer identity stem.
    pub fn id(self) -> &'static str {
        match self {
            Self::Shots => "shots",
            Self::Silence => "silence",
            Self::Beats => "beats",
        }
    }

    /// What the button says.
    pub fn label(self) -> &'static str {
        match self {
            Self::Shots => "Shots",
            Self::Silence => "Silence",
            Self::Beats => "Beats",
        }
    }

    /// The storage identity of this detector's results.
    ///
    /// Panics only on a malformed constant, which is a build-time mistake: the grammar
    /// is lowercase ASCII and every value here is a literal checked by the tests.
    pub fn analyzer_identity(self) -> AnalyzerIdentity {
        AnalyzerIdentity::new(match self {
            Self::Shots => "aria.shots",
            Self::Silence => "aria.silence",
            Self::Beats => "aria.beats",
        })
        .expect("analyzer identity constants are valid")
    }

    /// The result schema version.
    ///
    /// Bump this when a detector's configuration or output meaning changes, so every
    /// stored batch from the previous behavior reads as stale instead of current.
    pub fn analyzer_version(self) -> AnalyzerVersion {
        let version = match self {
            Self::Shots => 2,
            Self::Silence | Self::Beats => 1,
        };
        AnalyzerVersion::new(version).expect("analyzer version constants are non-zero")
    }

    /// The moment kind this detector writes.
    pub fn moment_kind(self) -> MomentKind {
        match self {
            Self::Shots => MomentKind::SHOT_BOUNDARY,
            Self::Silence => MomentKind::SILENCE,
            Self::Beats => MomentKind::BEAT,
        }
    }

    /// Whether this detector reads audio (rather than video frames).
    pub fn needs_audio(self) -> bool {
        matches!(self, Self::Silence | Self::Beats)
    }

    /// The replacement scope for one media file.
    pub fn batch_key(self, content_key: MediaContentKey) -> MomentBatchKey {
        MomentBatchKey::new(
            content_key,
            self.analyzer_identity(),
            self.analyzer_version(),
        )
    }
}

/// One analyzed moment of the selected clip's media.
#[derive(Debug, Clone, PartialEq)]
pub struct UiMoment {
    /// The storage kind, so a row can say what it is without a second lookup.
    pub kind: String,
    /// Start and end in seconds of the media; equal for a point event.
    pub seconds: (f64, f64),
    /// Where the selected clip showed it when the rows were read, in timeline
    /// ticks, or `None` when the clip's source window leaves it out. An edit
    /// or a seek places it again through the clip as it stands.
    pub at: Option<(i64, i64)>,
    /// Detector confidence in `[0, 1]`.
    pub confidence: f32,
    /// Which analyzer version produced it.
    pub analyzer_version: u32,
    /// Whether that version is behind the one this build would write.
    pub stale: bool,
}

/// One loudness window of the measured media, in file order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UiLoudness {
    /// Window RMS in dBFS.
    pub dbfs: f32,
}

/// What the Analysis panel is holding.
#[derive(Debug, Clone, Default)]
pub struct AnalysisForm {
    /// The rows last read from the index, in canonical order.
    pub moments: Vec<UiMoment>,
    /// The kind filter, or `None` for every kind.
    pub kind_filter: Option<AnalysisKind>,
    /// Inclusive confidence floor.
    pub minimum_confidence: f32,
    /// The loudness contour of the selected clip's media, if measured.
    ///
    /// Behind a handle because the Audio tab reduces the whole contour to its
    /// columns every frame, and a measurement replaces it wholesale rather than
    /// growing it, so nothing ever needs it back as a `Vec`.
    pub loudness: std::sync::Arc<[UiLoudness]>,
    /// The detectors that already hold current results for the selected media,
    /// recorded by the refresh that had the index open. A panel drawing a Detect
    /// button asks this rather than the database.
    pub stored: Vec<AnalysisKind>,
    /// Bytes the analysis cache occupies on disk, refreshed when Settings
    /// opens and whenever the moments index is read or cleared.
    pub cache_bytes: u64,
    /// Media entry the current `moments` and `stored` rows were projected for.
    pub(crate) media_id: Option<u64>,
    /// Media entry the current `loudness` contour was measured from.
    pub(crate) loudness_media_id: Option<u64>,
    /// The moments index this session has open, so a read or a write costs
    /// no open, no schema check, and no close-time checkpoint.
    pub(crate) index: HeldIndex,
}

/// One open [`MomentsIndex`], shared with the jobs that write it.
///
/// Closing a written index checkpoints its write-ahead log, which costs four
/// `fsync` calls, so an index opened per call paid them on every clear and
/// every detector run. A clone holds nothing: the copy opens its own.
#[derive(Default)]
pub(crate) struct HeldIndex(parking_lot::Mutex<Option<Arc<MomentsIndex>>>);

impl HeldIndex {
    /// The held index if it is open at `path` and its file is still there,
    /// else a fresh open at `path`, held from now on.
    pub(crate) fn held_or_open(&self, path: &Path) -> Result<Arc<MomentsIndex>, String> {
        let mut held = self.0.lock();
        if let Some(index) = held
            .as_ref()
            .filter(|index| index.path() == path && path.exists())
        {
            return Ok(Arc::clone(index));
        }
        *held = None;
        let index = MomentsIndex::open(path)
            .map(Arc::new)
            .map_err(|err| format!("opening the moments index: {err}"))?;
        *held = Some(Arc::clone(&index));
        Ok(index)
    }

    /// Close the held index, before its directory is cleared or moved.
    pub(crate) fn release(&self) {
        self.0.lock().take();
    }
}

impl Clone for HeldIndex {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl std::fmt::Debug for HeldIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("HeldIndex")
    }
}

/// Everything a run needs, resolved before it costs anything.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AnalysisRequest {
    pub(crate) kind: AnalysisKind,
    pub(crate) media_path: PathBuf,
    pub(crate) content_key: MediaContentKey,
    /// The clip the run was started from, so a beat grid lands on that clip even
    /// if the selection moved while the detector ran.
    pub(crate) clip: ClipId,
    pub(crate) label: String,
    /// How the detector is tuned, resolved from the analysis plugin before the
    /// job starts so the run cannot pick up a different setting mid-flight.
    pub(crate) tuning: crate::plugins::DetectorTuning,
}
