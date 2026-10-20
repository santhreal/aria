//! Audio analyzers: their registration metadata, the running session, and the
//! markers, ranges, and loudness a run returns.

use serde::{Deserialize, Serialize};

use crate::MarkerColor;

/// What target an audio analyzer operates on and which menu offers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnalyzerScope {
    /// The selected clip's sound, offered in Clip > Analyze.
    #[default]
    Clip,
    /// The whole sequence mix, offered in Sequence > Analyze.
    Sequence,
    /// The sequence mix from In to Out, or the whole sequence when no range is
    /// marked, offered in Sequence > Analyze.
    InOut,
}

/// Audio analyzer registration metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnalyzerSpec {
    /// Namespaced id with at least one `.`: ASCII letters, digits, `.`, `_` and
    /// `-`.
    pub id: String,
    /// The menu row, palette row, job title, and undo step name. Not blank.
    pub label: String,
    /// An optional grouping label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// What the analyzer reads and which menu lists it.
    #[serde(default)]
    pub scope: AnalyzerScope,
}

impl AnalyzerSpec {
    /// Build an analyzer specification for `id`, `label`, and `scope`.
    pub fn new(id: impl Into<String>, label: impl Into<String>, scope: AnalyzerScope) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            category: None,
            scope,
        }
    }

    /// Attach an optional grouping category.
    #[must_use]
    pub fn with_category(mut self, category: impl Into<String>) -> Self {
        let category = category.into();
        self.category = (!category.trim().is_empty()).then_some(category);
        self
    }
}

/// Ruler or clip marker returned by an [`AnalyzerRun`], in sample frames at the mix rate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnalyzerMarker {
    /// Start position in sample frames.
    pub frame: i64,
    /// Span in sample frames, 0 for a point marker. A span is clamped to the
    /// clip or sequence end.
    #[serde(default)]
    pub duration_frames: i64,
    /// The label beside the marker flag.
    pub name: String,
    /// The marker flag colour.
    pub color: MarkerColor,
}

/// Labelled sample-frame span (such as a detected silence region) returned by an [`AnalyzerRun`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnalyzerRange {
    /// First sample frame of the span.
    pub start: i64,
    /// The sample frame after the span.
    pub end: i64,
    /// Name of the segmentation marker the range becomes. Empty names it
    /// Silence.
    pub label: String,
}

/// Integrated loudness summary returned by an [`AnalyzerRun`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AnalyzerLoudness {
    /// Integrated programme loudness in LUFS (EBU R128, ITU-R BS.1770).
    pub integrated_lufs: f64,
    /// Peak level across channels in dBFS.
    pub true_peak_db: f64,
    /// Loudness range (LRA) in LU.
    pub range_lu: f64,
}

/// Output returned when an [`AnalyzerRun`] finishes.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AnalyzerResult {
    /// Beat positions in sample frames, set as the clip's beat grid. Clip scope
    /// only.
    #[serde(default)]
    pub beats: Vec<i64>,
    /// Markers attached to the clip for clip scope, and sequence markers
    /// otherwise.
    #[serde(default)]
    pub markers: Vec<AnalyzerMarker>,
    /// Spans that become yellow segmentation markers with a duration.
    #[serde(default)]
    pub ranges: Vec<AnalyzerRange>,
    /// A loudness measurement, shown in the job result and on the status line.
    /// It adds no undo step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loudness: Option<AnalyzerLoudness>,
}

/// One running audio analysis session opened by an [`Analyzer`].
pub trait AnalyzerRun: Send {
    /// Feed interleaved `f32` audio samples (`frames * channels`).
    fn push(&mut self, samples: &[f32]) -> Result<(), String>;

    /// Finish analysis and return the [`AnalyzerResult`].
    fn finish(self: Box<Self>) -> Result<AnalyzerResult, String>;
}

/// Audio analyzer registered with [`Host::register_analyzer`](crate::Host::register_analyzer).
pub trait Analyzer: Send + Sync + 'static {
    /// Open a new [`AnalyzerRun`] session for `sample_rate` Hz and `channels` channels.
    fn open(
        &self,
        host: &crate::Host,
        sample_rate: u32,
        channels: u16,
    ) -> Result<Box<dyn AnalyzerRun>, String>;
}
