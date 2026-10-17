//! Audio analyzers: plugins that inspect mixed sound over a time range and
//! return structured findings (beats, markers, silence ranges, loudness) to
//! apply as one undo step.
//!
//! A plugin registers an [`Analyzer`] during [`mount`](crate::Plugin::mount)
//! with [`PluginContext::register_analyzer`]. Running an analyzer through
//! [`PluginContext::run_analyzer`] mixes the requested [`AudioSource`] over
//! [`AudioHost::mix`], feeds each block to [`AnalyzerRun::block`], honours
//! per-block cancellation, and converts sample frames in [`AnalyzerResult`]
//! into timeline or clip-source ticks when applied via
//! [`PluginContext::apply_analyzer`] or [`apply_analyzer_result`].
//!
//! A panic in [`Analyzer::begin`], [`AnalyzerRun::block`], or
//! [`AnalyzerRun::finish`] is caught: the plugin is failed for the session via
//! [`PluginContext::fail`], its registrations are dropped, and the project is
//! left unchanged.

use std::borrow::Cow;
use std::ops::ControlFlow;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use aria_commands::{Command, EditCommand, EditOutcome};
use aria_core::{Rational, RationalTime, TimeRange};
use aria_engine::ApplyOutcome;
use aria_models::{ClipId, MarkerColor, MarkerKind, Project};
use aria_render::{EXPORT_AUDIO_CHANNELS, EXPORT_AUDIO_RATE, sample_boundary};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::audio_host::{AUDIO_HOST_BLOCK, AudioHost, AudioSource};
use crate::context::PluginContext;
use crate::error::PluginError;
use crate::events::panic_message;
use crate::listed::Listed;
use crate::project_host::{Origin, ProjectHost, Receipt, TransactError};
use crate::registration::{Contribution, ContributionKind, Registration};
use crate::render_host::sequence_holding;

/// What target an analyzer operates on and which menu offers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnalyzerScope {
    /// Runs on a selected clip (`Clip > Analyze`).
    #[default]
    Clip,
    /// Runs on the open sequence (`Sequence > Analyze`).
    Sequence,
    /// Runs on the active In-to-Out range, or the whole sequence when unset
    /// (`Sequence > Analyze`).
    InOut,
}

impl AnalyzerScope {
    pub const ALL: [Self; 3] = [Self::Clip, Self::Sequence, Self::InOut];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Clip => "clip",
            Self::Sequence => "sequence",
            Self::InOut => "in_out",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Clip => "Clip",
            Self::Sequence => "Sequence",
            Self::InOut => "In to Out",
        }
    }

    pub const fn code(self) -> u32 {
        match self {
            Self::Clip => 0,
            Self::Sequence => 1,
            Self::InOut => 2,
        }
    }

    pub const fn from_code(code: u32) -> Option<Self> {
        match code {
            0 => Some(Self::Clip),
            1 => Some(Self::Sequence),
            2 => Some(Self::InOut),
            _ => None,
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "clip" => Some(Self::Clip),
            "sequence" => Some(Self::Sequence),
            "in_out" | "inout" | "in-out" => Some(Self::InOut),
            _ => None,
        }
    }
}

/// Catalog metadata for a registered audio analyzer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnalyzerSpec {
    /// Namespaced identifier, such as `"aria.beats"` or `"com.example.tempo"`.
    pub id: String,
    /// Human-readable label shown in `Clip > Analyze`, `Sequence > Analyze`,
    /// the command palette, and the undo history step.
    pub label: String,
    /// Optional submenu / grouping category.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// Whether this analyzer targets a clip, the whole sequence, or In to Out.
    #[serde(default)]
    pub scope: AnalyzerScope,
}

impl AnalyzerSpec {
    pub fn new(id: impl Into<String>, label: impl Into<String>, scope: AnalyzerScope) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            category: None,
            scope,
        }
    }

    #[must_use]
    pub fn with_category(mut self, category: impl Into<String>) -> Self {
        let category = category.into();
        self.category = (!category.trim().is_empty()).then_some(category);
        self
    }
}

/// A point or range marker produced by an analyzer, in sample frames at the
/// mix rate ([`EXPORT_AUDIO_RATE`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnalyzerMarker {
    /// Start position in sample frames at the mix rate.
    pub frame: i64,
    /// Span in sample frames (`0` for a point marker).
    #[serde(default)]
    pub duration_frames: i64,
    /// Short label shown beside the marker flag.
    pub name: String,
    /// Marker flag color.
    pub color: MarkerColor,
}

impl AnalyzerMarker {
    pub fn new(
        frame: i64,
        duration_frames: i64,
        name: impl Into<String>,
        color: MarkerColor,
    ) -> Self {
        Self {
            frame,
            duration_frames,
            name: name.into(),
            color,
        }
    }
}

/// A labelled sample-frame span (such as a detected silence region).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnalyzerRange {
    /// Inclusive start sample frame at the mix rate.
    pub start: i64,
    /// Exclusive end sample frame at the mix rate.
    pub end: i64,
    /// Label applied when converting the range into a sequence marker.
    pub label: String,
}

impl AnalyzerRange {
    pub fn new(start: i64, end: i64, label: impl Into<String>) -> Self {
        Self {
            start,
            end,
            label: label.into(),
        }
    }
}

/// Integrated loudness summary produced by an analyzer.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AnalyzerLoudness {
    /// Integrated programme loudness in LUFS (EBU R128 / ITU-R BS.1770).
    pub integrated_lufs: f64,
    /// Maximum sample peak across channels in dBFS.
    pub true_peak_db: f64,
    /// Loudness range (LRA) in LU.
    pub range_lu: f64,
}

/// Structured findings returned by [`AnalyzerRun::finish`].
///
/// Sample positions (`beats`, `markers`, `ranges`) are expressed in sample
/// frames at the mix rate ([`EXPORT_AUDIO_RATE`], 48 kHz) as handed to
/// [`AnalyzerRun::block`]. When applied to a project, frames are converted to
/// ticks at the sequence or clip-source rate.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct AnalyzerResult {
    /// Beat positions in sample frames at the mix rate.
    #[serde(default)]
    pub beats: Vec<i64>,
    /// Markers in sample frames at the mix rate.
    #[serde(default)]
    pub markers: Vec<AnalyzerMarker>,
    /// Silence or segmentation ranges in sample frames at the mix rate.
    #[serde(default)]
    pub ranges: Vec<AnalyzerRange>,
    /// Optional loudness measurement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loudness: Option<AnalyzerLoudness>,
}

impl AnalyzerResult {
    /// Whether this result holds any project edits (`beats`, `markers`, or
    /// `ranges`).
    pub fn has_edits(&self) -> bool {
        !self.beats.is_empty() || !self.markers.is_empty() || !self.ranges.is_empty()
    }

    /// Whether every field in this result is empty.
    pub fn is_empty(&self) -> bool {
        !self.has_edits() && self.loudness.is_none()
    }

    /// Convert `self.beats` (sample frames at [`EXPORT_AUDIO_RATE`]) to ticks
    /// at `tick_rate`, sorted and deduplicated.
    pub fn beat_ticks(&self, tick_rate: Rational) -> Vec<i64> {
        let mut ticks: Vec<i64> = self
            .beats
            .iter()
            .map(|&frame| frame_to_tick(frame, EXPORT_AUDIO_RATE, tick_rate))
            .collect();
        ticks.sort_unstable();
        ticks.dedup();
        ticks
    }

    /// Convert `self.beats` (sequence sample frames from mixing
    /// `AudioSource::Clip(clip_id)`) into clip source ticks on `clip_id`,
    /// clamped to the clip's source window, sorted and deduplicated.
    pub fn clip_beat_ticks(&self, project: &Project, clip_id: ClipId) -> Option<Vec<i64>> {
        let seq_id = sequence_holding(project, clip_id).ok()?;
        let timeline = match seq_id {
            None => project.root_timeline(),
            Some(id) => &project.sequence(id)?.timeline,
        };
        let clip = timeline.clip(clip_id)?;
        let source = clip.source_range()?;
        let seq_rate = timeline.frame_rate;
        let clip_start_sample =
            sample_boundary(clip.timeline.start.value, seq_rate.num, seq_rate.den);
        let linear = !clip.reversed
            && clip.timeline.duration.value == source.duration.value
            && !clip.has_speed_curve();
        let lo = source.start.value;
        let hi = source.end_tick();
        let mut beats: Vec<i64> = self
            .beats
            .iter()
            .filter_map(|&frame| {
                let tick = if linear {
                    let delta = frame - clip_start_sample;
                    source.start.value + frame_to_tick(delta, EXPORT_AUDIO_RATE, source.start.rate)
                } else {
                    let seq_tick = frame_to_tick(frame, EXPORT_AUDIO_RATE, seq_rate);
                    clip.clip_time_at_timeline_tick(seq_tick)?.value
                };
                (lo..hi).contains(&tick).then_some(tick)
            })
            .collect();
        beats.sort_unstable();
        beats.dedup();
        Some(beats)
    }

    /// Convert `self.ranges` (sample frames at [`EXPORT_AUDIO_RATE`]) into
    /// sequence [`TimeRange`]s at `tick_rate`.
    pub fn range_spans(&self, tick_rate: Rational) -> Vec<TimeRange> {
        self.ranges
            .iter()
            .filter_map(|range| {
                let start = frame_to_tick(range.start, EXPORT_AUDIO_RATE, tick_rate);
                let end = frame_to_tick(range.end, EXPORT_AUDIO_RATE, tick_rate);
                (end > start)
                    .then(|| {
                        TimeRange::new(
                            RationalTime::new(start, tick_rate),
                            RationalTime::new(end - start, tick_rate),
                        )
                        .ok()
                    })
                    .flatten()
            })
            .collect()
    }

    /// One-line human-readable summary for the Jobs list and status strip.
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if !self.beats.is_empty() {
            let n = self.beats.len();
            parts.push(format!("{n} beat{}", if n == 1 { "" } else { "s" }));
        }
        if !self.markers.is_empty() {
            let n = self.markers.len();
            parts.push(format!("{n} marker{}", if n == 1 { "" } else { "s" }));
        }
        if !self.ranges.is_empty() {
            let n = self.ranges.len();
            parts.push(format!(
                "{n} silence range{}",
                if n == 1 { "" } else { "s" }
            ));
        }
        if let Some(loudness) = self.loudness {
            if loudness.integrated_lufs.is_finite() {
                parts.push(format!(
                    "{:.1} LUFS, {:.1} dBTP, {:.1} LU",
                    loudness.integrated_lufs, loudness.true_peak_db, loudness.range_lu
                ));
            } else {
                parts.push("digital silence (-inf LUFS)".to_owned());
            }
        }
        if parts.is_empty() {
            "No features detected".to_owned()
        } else {
            parts.join(", ")
        }
    }
}

/// Convert sample `frame` at `sample_rate` Hz to the nearest tick at
/// `tick_rate`.
pub fn frame_to_tick(frame: i64, sample_rate: u32, tick_rate: Rational) -> i64 {
    if sample_rate == 0 || !tick_rate.is_valid() {
        return 0;
    }
    let num = i128::from(frame) * i128::from(tick_rate.num);
    let den = i128::from(sample_rate) * i128::from(tick_rate.den);
    if num >= 0 {
        ((num + den / 2) / den) as i64
    } else {
        ((num - den / 2) / den) as i64
    }
}

/// One in-flight run of an [`Analyzer`] over a sequence or clip mix.
pub trait AnalyzerRun: Send {
    /// Inspect one interleaved block starting at `first_frame` (in sample
    /// frames at the mix rate from the start of the sequence). Returning
    /// [`ControlFlow::Break`] stops the mix early and proceeds to [`Self::finish`].
    fn block(&mut self, first_frame: i64, samples: &[f32]) -> ControlFlow<()>;

    /// Complete the analysis and return structured findings.
    fn finish(self: Box<Self>) -> Result<AnalyzerResult, String>;
}

/// Factory that starts an [`AnalyzerRun`] at `rate` Hz and `channels` channels.
pub trait Analyzer: Send + Sync + 'static {
    fn begin(&self, rate: u32, channels: u16) -> Box<dyn AnalyzerRun>;
}

impl<F> Analyzer for F
where
    F: Fn(u32, u16) -> Box<dyn AnalyzerRun> + Send + Sync + 'static,
{
    fn begin(&self, rate: u32, channels: u16) -> Box<dyn AnalyzerRun> {
        self(rate, channels)
    }
}

/// Why an analyzer run did not produce a result.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AnalyzerError {
    #[error("unknown analyzer: {0}")]
    UnknownAnalyzer(String),
    #[error("analyzer cancelled")]
    Cancelled,
    #[error("audio mix failed: {0}")]
    Mix(String),
    #[error("analyzer {id} failed: {reason}")]
    Failed { id: String, reason: String },
    #[error("analyzer {id} panicked: {reason}")]
    Panicked { id: String, reason: String },
}

/// A registered analyzer entry held by [`PluginContext`].
pub struct RegisteredAnalyzer {
    pub spec: AnalyzerSpec,
    /// Owning plugin id, or `""` when registered outside [`PluginContext::mount`].
    pub plugin: Cow<'static, str>,
    analyzer: Box<dyn Analyzer>,
}

impl std::fmt::Debug for RegisteredAnalyzer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RegisteredAnalyzer")
            .field("spec", &self.spec)
            .field("plugin", &self.plugin)
            .finish_non_exhaustive()
    }
}

impl RegisteredAnalyzer {
    /// Specification registered for this analyzer.
    pub fn spec(&self) -> &AnalyzerSpec {
        &self.spec
    }

    /// Run this analyzer over `range` of `source` in `project`, reporting
    /// `(mixed_frames, total_frames)` after each block and checking `cancel`
    /// before and after every block.
    pub fn run_with_progress(
        &self,
        ctx: &PluginContext,
        project: &Project,
        source: AudioSource,
        range: TimeRange,
        cancel: &AtomicBool,
        mut on_progress: impl FnMut(u64, u64),
    ) -> Result<AnalyzerResult, AnalyzerError> {
        if cancel.load(Ordering::Acquire) {
            return Err(AnalyzerError::Cancelled);
        }

        let rate = range.start.rate;
        let from = sample_boundary(range.start.value, rate.num, rate.den);
        let to = sample_boundary(range.end_tick(), rate.num, rate.den);
        let total_frames = (to - from).max(0) as u64;
        on_progress(0, total_frames);

        let started_begin = Instant::now();
        let mut run = match catch_unwind(AssertUnwindSafe(|| {
            self.analyzer
                .begin(EXPORT_AUDIO_RATE, EXPORT_AUDIO_CHANNELS)
        })) {
            Ok(run) => {
                if !self.plugin.is_empty() {
                    ctx.record_block_cost(&self.plugin, started_begin.elapsed());
                }
                run
            }
            Err(payload) => {
                let what = panic_message(payload.as_ref());
                let reason = format!("analyzer {} panicked: {what}", self.spec.id);
                if !self.plugin.is_empty() {
                    ctx.fail(&self.plugin, &reason);
                }
                return Err(AnalyzerError::Panicked {
                    id: self.spec.id.clone(),
                    reason: what,
                });
            }
        };

        let host = AudioHost::new();
        let channels = usize::from(EXPORT_AUDIO_CHANNELS).max(1);
        let mut mixed_frames = 0u64;
        let mut cancelled = false;
        let mut panicked: Option<String> = None;

        let mix_res = host.mix(project, source, range, |first_frame, samples| {
            if cancel.load(Ordering::Acquire) {
                cancelled = true;
                return ControlFlow::Break(());
            }
            let block_started = Instant::now();
            let step = catch_unwind(AssertUnwindSafe(|| run.block(first_frame, samples)));
            if !self.plugin.is_empty() {
                ctx.record_block_cost(&self.plugin, block_started.elapsed());
            }
            match step {
                Ok(flow) => {
                    let frames = (samples.len() / channels).min(AUDIO_HOST_BLOCK) as u64;
                    mixed_frames = mixed_frames.saturating_add(frames);
                    on_progress(mixed_frames.min(total_frames), total_frames);
                    if cancel.load(Ordering::Acquire) {
                        cancelled = true;
                        return ControlFlow::Break(());
                    }
                    flow
                }
                Err(payload) => {
                    panicked = Some(panic_message(payload.as_ref()));
                    ControlFlow::Break(())
                }
            }
        });

        if let Some(what) = panicked {
            // Drop `run` under `catch_unwind` before unmounting the plugin.
            let _ = catch_unwind(AssertUnwindSafe(|| drop(run)));
            let reason = format!("analyzer {} panicked: {what}", self.spec.id);
            if !self.plugin.is_empty() {
                ctx.fail(&self.plugin, &reason);
            }
            return Err(AnalyzerError::Panicked {
                id: self.spec.id.clone(),
                reason: what,
            });
        }

        if let Err(err) = mix_res {
            let _ = catch_unwind(AssertUnwindSafe(|| drop(run)));
            return Err(AnalyzerError::Mix(err.to_string()));
        }

        if cancelled || cancel.load(Ordering::Acquire) {
            let _ = catch_unwind(AssertUnwindSafe(|| drop(run)));
            return Err(AnalyzerError::Cancelled);
        }

        let finish_started = Instant::now();
        let finished = catch_unwind(AssertUnwindSafe(|| run.finish()));
        if !self.plugin.is_empty() {
            ctx.record_block_cost(&self.plugin, finish_started.elapsed());
        }
        match finished {
            Ok(Ok(result)) => {
                on_progress(total_frames, total_frames);
                Ok(result)
            }
            Ok(Err(message)) => {
                if !self.plugin.is_empty() {
                    ctx.report_fault(
                        &self.plugin,
                        format!("analyzer {} failed: {message}", self.spec.id),
                    );
                }
                Err(AnalyzerError::Failed {
                    id: self.spec.id.clone(),
                    reason: message,
                })
            }
            Err(payload) => {
                let what = panic_message(payload.as_ref());
                let reason = format!("analyzer {} panicked: {what}", self.spec.id);
                if !self.plugin.is_empty() {
                    ctx.fail(&self.plugin, &reason);
                }
                Err(AnalyzerError::Panicked {
                    id: self.spec.id.clone(),
                    reason: what,
                })
            }
        }
    }
}

#[derive(Default)]
pub(crate) struct Analyzers {
    entries: Arc<Listed<Arc<RegisteredAnalyzer>>>,
}

impl Analyzers {
    fn register(
        &self,
        plugin: Cow<'static, str>,
        mut spec: AnalyzerSpec,
        analyzer: Box<dyn Analyzer>,
    ) -> Result<Box<dyn std::any::Any + Send + Sync>, PluginError> {
        validate_id(&spec.id)?;
        let label = spec.label.trim().to_owned();
        if label.is_empty() {
            return Err(PluginError::AnalyzerRefused(format!(
                "{}: analyzer label cannot be empty",
                spec.id
            )));
        }
        spec.label = label;
        spec.category = spec
            .category
            .map(|c| c.trim().to_owned())
            .filter(|c| !c.is_empty());

        let id = spec.id.clone();
        let plugin_id: Arc<str> = Arc::from(plugin.as_ref());
        let entry = Arc::new(RegisteredAnalyzer {
            spec,
            plugin,
            analyzer,
        });
        let guard = self
            .entries
            .insert_unique(&plugin_id, entry, |existing| existing.spec.id == id)
            .ok_or_else(|| {
                PluginError::AnalyzerRefused(format!("analyzer {id} is already registered"))
            })?;
        Ok(Box::new(guard))
    }

    fn entries(&self) -> Vec<Arc<RegisteredAnalyzer>> {
        self.entries.items()
    }

    fn find(&self, id: &str) -> Option<Arc<RegisteredAnalyzer>> {
        self.entries.find(|entry| entry.spec.id == id)
    }

    fn revision(&self) -> u64 {
        self.entries.revision()
    }
}

fn validate_id(id: &str) -> Result<(), PluginError> {
    if id.is_empty() {
        return Err(PluginError::AnalyzerRefused(
            "an analyzer needs an id".to_owned(),
        ));
    }
    if !id.contains('.') {
        return Err(PluginError::AnalyzerRefused(format!(
            "{id}: an analyzer id needs a namespace, such as `example.{id}`"
        )));
    }
    if !id
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    {
        return Err(PluginError::AnalyzerRefused(format!(
            "{id}: analyzer ids use ASCII letters, digits, '.', '_', and '-'"
        )));
    }
    Ok(())
}

/// Apply `result` to `host` as a single undo step named `label`.
///
/// - `beats` -> [`EditCommand::SetClipBeats`] on `clip` when `source` is
///   [`AudioSource::Clip`].
/// - `markers` -> clip markers when `source` is [`AudioSource::Clip`], or
///   sequence markers otherwise.
/// - `ranges` -> sequence markers with duration (`MarkerKind::Segmentation`).
/// - `loudness` -> read-only measurement (does not record an undo step unless
///   paired with beats, markers, or ranges).
pub fn apply_analyzer_result(
    host: &ProjectHost,
    origin: Origin,
    label: impl Into<Cow<'static, str>>,
    source: AudioSource,
    result: &AnalyzerResult,
) -> Result<Receipt, TransactError> {
    let label = label.into();
    host.transact_with(origin, label, |tx| {
        let snapshot = tx.project();
        let seq_rate = snapshot.timeline().frame_rate;
        let seq_end_tick = snapshot.timeline().duration().value;
        let clip_beats = match source {
            AudioSource::Clip(clip_id)
                if !result.beats.is_empty()
                    || (result.markers.is_empty()
                        && result.ranges.is_empty()
                        && result.loudness.is_none()) =>
            {
                Some((
                    clip_id,
                    result.clip_beat_ticks(&snapshot, clip_id).ok_or_else(|| {
                        TransactError::Failed {
                            label: "Set beats",
                            reason: format!("clip {clip_id:?} not found or has no media source"),
                        }
                    })?,
                ))
            }
            _ => None,
        };
        drop(snapshot);

        if let Some((clip, beats)) = clip_beats {
            tx.apply(Command::Edit(EditCommand::SetClipBeats { clip, beats }))?;
        }

        for marker in &result.markers {
            let at_tick = frame_to_tick(marker.frame, EXPORT_AUDIO_RATE, seq_rate).max(0);
            let at = RationalTime::new(at_tick, seq_rate);
            let duration_ticks =
                frame_to_tick(marker.duration_frames.max(0), EXPORT_AUDIO_RATE, seq_rate).max(0);
            let outcome = tx.apply(Command::Edit(EditCommand::AddMarker {
                at,
                name: marker.name.clone(),
                color: Some(marker.color),
            }))?;
            if let ApplyOutcome::Edited(EditOutcome::CreatedMarker(marker_id)) = outcome {
                if let AudioSource::Clip(clip_id) = source {
                    let mut engine = host.engine().lock();
                    engine
                        .attach_marker_to_clip(marker_id, clip_id)
                        .map_err(|err| TransactError::Failed {
                            label: "Add marker",
                            reason: err.to_string(),
                        })?;
                    if marker.duration_frames > 0 {
                        if let Some(placed) = engine.project().timeline().marker(marker_id).cloned()
                        {
                            let clip_end = engine
                                .project()
                                .timeline()
                                .clip(clip_id)
                                .map(|c| c.clip_time_range().end_tick())
                                .unwrap_or(placed.tick.value);
                            let dur_ticks = frame_to_tick(
                                marker.duration_frames,
                                EXPORT_AUDIO_RATE,
                                placed.tick.rate,
                            )
                            .max(0)
                            .min((clip_end - placed.tick.value).max(0));
                            if dur_ticks > 0 {
                                let dur = RationalTime::new(dur_ticks, placed.tick.rate);
                                engine
                                    .edit_marker(
                                        marker_id,
                                        placed.tick,
                                        dur,
                                        placed.name,
                                        placed.color,
                                        placed.comment,
                                        placed.kind,
                                    )
                                    .map_err(|err| TransactError::Failed {
                                        label: "Edit marker",
                                        reason: err.to_string(),
                                    })?;
                            }
                        }
                    }
                } else if duration_ticks > 0 {
                    let clamped_dur = duration_ticks.min((seq_end_tick - at_tick).max(0));
                    if clamped_dur > 0 {
                        let dur = RationalTime::new(clamped_dur, seq_rate);
                        let mut engine = host.engine().lock();
                        engine
                            .edit_marker(
                                marker_id,
                                at,
                                dur,
                                marker.name.clone(),
                                marker.color,
                                String::new(),
                                MarkerKind::Comment,
                            )
                            .map_err(|err| TransactError::Failed {
                                label: "Edit marker",
                                reason: err.to_string(),
                            })?;
                    }
                }
            }
        }

        for range in &result.ranges {
            let start_tick = frame_to_tick(range.start, EXPORT_AUDIO_RATE, seq_rate).max(0);
            let end_tick = frame_to_tick(range.end, EXPORT_AUDIO_RATE, seq_rate).min(seq_end_tick);
            if end_tick <= start_tick {
                continue;
            }
            let at = RationalTime::new(start_tick, seq_rate);
            let dur = RationalTime::new(end_tick - start_tick, seq_rate);
            let name = if range.label.is_empty() {
                "Silence".to_owned()
            } else {
                range.label.clone()
            };
            let color = MarkerColor::Yellow;
            let outcome = tx.apply(Command::Edit(EditCommand::AddMarker {
                at,
                name: name.clone(),
                color: Some(color),
            }))?;
            if let ApplyOutcome::Edited(EditOutcome::CreatedMarker(marker_id)) = outcome {
                let mut engine = host.engine().lock();
                engine
                    .edit_marker(
                        marker_id,
                        at,
                        dur,
                        name,
                        color,
                        String::new(),
                        MarkerKind::Segmentation,
                    )
                    .map_err(|err| TransactError::Failed {
                        label: "Edit marker",
                        reason: err.to_string(),
                    })?;
            }
        }

        Ok(())
    })
    .map(|((), receipt)| receipt)
}

impl PluginContext {
    /// Register an audio analyzer in `Clip > Analyze` or `Sequence > Analyze`.
    ///
    /// Held on the terms of [`register_effect`](Self::register_effect): kept by
    /// the context during [`mount`](Self::mount), and removed when the returned
    /// [`Registration`] drops or the plugin unmounts or fails.
    ///
    /// Refused with [`PluginError::AnalyzerRefused`] when `spec.id` is empty,
    /// lacks a `.` namespace, contains characters outside ASCII
    /// `[A-Za-z0-9._-]`, is already registered on this context, or `spec.label`
    /// is blank.
    pub fn register_analyzer(
        &self,
        spec: AnalyzerSpec,
        analyzer: impl Analyzer,
    ) -> Result<Registration, PluginError> {
        let plugin = self.plugin_scope().unwrap_or(Cow::Borrowed(""));
        let id = spec.id.clone();
        let guard = self
            .analyzer_registry()
            .register(plugin, spec, Box::new(analyzer))?;
        let contribution = Contribution::new(ContributionKind::Analyzer, id);
        Ok(self.retain_if_mounting(Registration::holding(contribution, guard)))
    }

    /// Every registered analyzer, in registration order.
    pub fn analyzers(&self) -> Vec<Arc<RegisteredAnalyzer>> {
        self.analyzer_registry().entries()
    }

    /// Registered analyzers whose scope matches `scope`, in registration order.
    pub fn analyzers_for_scope(&self, scope: AnalyzerScope) -> Vec<Arc<RegisteredAnalyzer>> {
        self.analyzer_registry()
            .entries()
            .into_iter()
            .filter(|entry| entry.spec.scope == scope)
            .collect()
    }

    /// Look up one registered analyzer by `id`.
    pub fn analyzer(&self, id: &str) -> Option<Arc<RegisteredAnalyzer>> {
        self.analyzer_registry().find(id)
    }

    /// Monotonic counter incremented when an analyzer is registered or
    /// unmounted.
    pub fn analyzer_revision(&self) -> u64 {
        self.analyzer_registry().revision()
    }

    /// Run registered analyzer `id` over `range` of `source` in `project`.
    pub fn run_analyzer(
        &self,
        id: &str,
        project: &Project,
        source: AudioSource,
        range: TimeRange,
        cancel: &AtomicBool,
    ) -> Result<AnalyzerResult, AnalyzerError> {
        self.run_analyzer_with_progress(id, project, source, range, cancel, |_, _| {})
    }

    /// Run registered analyzer `id` over `range` of `source` in `project`,
    /// reporting `(mixed_frames, total_frames)` to `on_progress` as blocks are
    /// processed.
    pub fn run_analyzer_with_progress(
        &self,
        id: &str,
        project: &Project,
        source: AudioSource,
        range: TimeRange,
        cancel: &AtomicBool,
        on_progress: impl FnMut(u64, u64),
    ) -> Result<AnalyzerResult, AnalyzerError> {
        let entry = self
            .analyzer(id)
            .ok_or_else(|| AnalyzerError::UnknownAnalyzer(id.to_owned()))?;
        entry.run_with_progress(self, project, source, range, cancel, on_progress)
    }

    /// Apply `result` from analyzer `id` as one undo step named after the
    /// analyzer's label.
    pub fn apply_analyzer(
        &self,
        host: &ProjectHost,
        origin: Origin,
        id: &str,
        source: AudioSource,
        result: &AnalyzerResult,
    ) -> Result<Receipt, TransactError> {
        let label = self
            .analyzer(id)
            .map(|entry| Cow::Owned(entry.spec.label.clone()))
            .unwrap_or_else(|| Cow::Owned(id.to_owned()));
        apply_analyzer_result(host, origin, label, source, result)
    }
}

/// Shipped beat analyzer id (`Clip > Analyze > Detect Beats`).
pub const SHIPPED_BEAT_ANALYZER_ID: &str = "aria.beats";

/// Shipped silence analyzer id (`Sequence > Analyze > Detect Silence`).
pub const SHIPPED_SILENCE_ANALYZER_ID: &str = "aria.silence";

/// Shipped loudness analyzer id (`Sequence > Analyze > Measure Loudness`).
pub const SHIPPED_LOUDNESS_ANALYZER_ID: &str = "aria.loudness";

/// Shipped beat analyzer backed by [`aria_analysis::InterleavedPcm::detect_beats`].
#[derive(Debug, Clone, Copy, Default)]
pub struct BeatAnalyzer {
    pub config: aria_analysis::BeatConfig,
}

impl BeatAnalyzer {
    pub const fn new(config: aria_analysis::BeatConfig) -> Self {
        Self { config }
    }

    pub fn spec() -> AnalyzerSpec {
        AnalyzerSpec::new(
            SHIPPED_BEAT_ANALYZER_ID,
            "Detect Beats",
            AnalyzerScope::Clip,
        )
        .with_category("Rhythm")
    }
}

struct BeatAnalyzerRun {
    rate: u32,
    channels: u16,
    config: aria_analysis::BeatConfig,
    base_frame: Option<i64>,
    samples: Vec<f32>,
}

impl Analyzer for BeatAnalyzer {
    fn begin(&self, rate: u32, channels: u16) -> Box<dyn AnalyzerRun> {
        Box::new(BeatAnalyzerRun {
            rate,
            channels,
            config: self.config,
            base_frame: None,
            samples: Vec::new(),
        })
    }
}

impl AnalyzerRun for BeatAnalyzerRun {
    fn block(&mut self, first_frame: i64, samples: &[f32]) -> ControlFlow<()> {
        if self.base_frame.is_none() {
            self.base_frame = Some(first_frame);
        }
        self.samples
            .extend(samples.iter().map(|&s| if s.is_finite() { s } else { 0.0 }));
        ControlFlow::Continue(())
    }

    fn finish(self: Box<Self>) -> Result<AnalyzerResult, String> {
        let channels = usize::from(self.channels);
        if self.rate == 0 || channels == 0 || self.samples.is_empty() {
            return Ok(AnalyzerResult::default());
        }
        let base_frame = self.base_frame.unwrap_or(0);
        let pcm = aria_analysis::InterleavedPcm::new(&self.samples, self.rate, channels)
            .map_err(|err| err.to_string())?;
        let total_frames = pcm.frame_count();
        let window_frames = (self.config.loudness().window_seconds() * f64::from(self.rate))
            .round()
            .max(1.0) as usize;
        let refractory_frames =
            (self.config.refractory_seconds() * f64::from(self.rate)).round() as i64;

        let mut beats = Vec::new();

        // Check for an impulse at the very beginning of the window (which window 0
        // flux suppresses in `InterleavedPcm::detect_beats`): if the first window
        // has energy above the onset floor and decays by window 2, refine its
        // leading edge inside window 0.
        let contour = pcm.loudness_contour(self.config.loudness());
        if contour.len() >= 3 {
            let first_db = contour[0].dbfs;
            let third_db = contour[2].dbfs;
            if first_db - third_db >= self.config.minimum_onset_db() {
                let onset =
                    refine_onset_frame(&self.samples, channels, 0, window_frames.min(total_frames));
                beats.push(base_frame + onset as i64);
            }
        }

        for event in pcm.detect_beats(self.config) {
            let end_frame =
                ((event.time_seconds * f64::from(self.rate)).round() as usize).min(total_frames);
            let start_frame = end_frame.saturating_sub(window_frames);
            let onset = refine_onset_frame(&self.samples, channels, start_frame, end_frame);
            let candidate = base_frame + onset as i64;
            if beats
                .last()
                .is_none_or(|&prev| candidate - prev >= refractory_frames.max(1))
            {
                beats.push(candidate);
            }
        }

        Ok(AnalyzerResult {
            beats,
            ..Default::default()
        })
    }
}

fn refine_onset_frame(
    samples: &[f32],
    channels: usize,
    start_frame: usize,
    end_frame: usize,
) -> usize {
    if start_frame >= end_frame || channels == 0 {
        return start_frame;
    }
    let mut max_energy = 0.0f32;
    for frame in start_frame..end_frame {
        let base = frame * channels;
        let energy: f32 = samples[base..base + channels].iter().map(|&s| s * s).sum();
        if energy > max_energy {
            max_energy = energy;
        }
    }
    if max_energy <= 0.0 {
        return start_frame;
    }
    let threshold = max_energy * 0.25;
    for frame in start_frame..end_frame {
        let base = frame * channels;
        let energy: f32 = samples[base..base + channels].iter().map(|&s| s * s).sum();
        if energy >= threshold {
            return frame;
        }
    }
    start_frame
}

/// Shipped silence analyzer backed by [`aria_analysis::InterleavedPcm::detect_silence`].
#[derive(Debug, Clone, Copy, Default)]
pub struct SilenceAnalyzer {
    pub config: aria_analysis::SilenceConfig,
}

impl SilenceAnalyzer {
    pub const fn new(config: aria_analysis::SilenceConfig) -> Self {
        Self { config }
    }

    pub fn spec() -> AnalyzerSpec {
        AnalyzerSpec::new(
            SHIPPED_SILENCE_ANALYZER_ID,
            "Detect Silence",
            AnalyzerScope::Sequence,
        )
        .with_category("Segmentation")
    }
}

struct SilenceAnalyzerRun {
    rate: u32,
    channels: u16,
    config: aria_analysis::SilenceConfig,
    base_frame: Option<i64>,
    samples: Vec<f32>,
}

impl Analyzer for SilenceAnalyzer {
    fn begin(&self, rate: u32, channels: u16) -> Box<dyn AnalyzerRun> {
        Box::new(SilenceAnalyzerRun {
            rate,
            channels,
            config: self.config,
            base_frame: None,
            samples: Vec::new(),
        })
    }
}

impl AnalyzerRun for SilenceAnalyzerRun {
    fn block(&mut self, first_frame: i64, samples: &[f32]) -> ControlFlow<()> {
        if self.base_frame.is_none() {
            self.base_frame = Some(first_frame);
        }
        self.samples
            .extend(samples.iter().map(|&s| if s.is_finite() { s } else { 0.0 }));
        ControlFlow::Continue(())
    }

    fn finish(self: Box<Self>) -> Result<AnalyzerResult, String> {
        let channels = usize::from(self.channels);
        if self.rate == 0 || channels == 0 || self.samples.is_empty() {
            return Ok(AnalyzerResult::default());
        }
        let base_frame = self.base_frame.unwrap_or(0);
        let pcm = aria_analysis::InterleavedPcm::new(&self.samples, self.rate, channels)
            .map_err(|err| err.to_string())?;
        let ranges = pcm
            .detect_silence(self.config)
            .into_iter()
            .filter_map(|span| {
                let start =
                    base_frame + (span.span.start_seconds() * f64::from(self.rate)).round() as i64;
                let end =
                    base_frame + (span.span.end_seconds() * f64::from(self.rate)).round() as i64;
                (end > start).then(|| AnalyzerRange::new(start, end, "Silence"))
            })
            .collect();
        Ok(AnalyzerResult {
            ranges,
            ..Default::default()
        })
    }
}

/// Shipped loudness analyzer backed by [`aria_analysis::InterleavedPcm::loudness_contour`].
#[derive(Debug, Clone, Copy, Default)]
pub struct LoudnessAnalyzer {
    pub config: aria_analysis::LoudnessConfig,
}

impl LoudnessAnalyzer {
    pub const fn new(config: aria_analysis::LoudnessConfig) -> Self {
        Self { config }
    }

    pub fn spec() -> AnalyzerSpec {
        AnalyzerSpec::new(
            SHIPPED_LOUDNESS_ANALYZER_ID,
            "Measure Loudness",
            AnalyzerScope::InOut,
        )
        .with_category("Loudness")
    }
}

struct LoudnessAnalyzerRun {
    rate: u32,
    channels: u16,
    config: aria_analysis::LoudnessConfig,
    samples: Vec<f32>,
}

impl Analyzer for LoudnessAnalyzer {
    fn begin(&self, rate: u32, channels: u16) -> Box<dyn AnalyzerRun> {
        Box::new(LoudnessAnalyzerRun {
            rate,
            channels,
            config: self.config,
            samples: Vec::new(),
        })
    }
}

impl AnalyzerRun for LoudnessAnalyzerRun {
    fn block(&mut self, _first_frame: i64, samples: &[f32]) -> ControlFlow<()> {
        self.samples
            .extend(samples.iter().map(|&s| if s.is_finite() { s } else { 0.0 }));
        ControlFlow::Continue(())
    }

    fn finish(self: Box<Self>) -> Result<AnalyzerResult, String> {
        let channels = usize::from(self.channels);
        if self.rate == 0 || channels == 0 || self.samples.is_empty() {
            return Ok(AnalyzerResult {
                loudness: Some(AnalyzerLoudness {
                    integrated_lufs: f64::NEG_INFINITY,
                    true_peak_db: f64::NEG_INFINITY,
                    range_lu: 0.0,
                }),
                ..Default::default()
            });
        }
        let pcm = aria_analysis::InterleavedPcm::new(&self.samples, self.rate, channels)
            .map_err(|err| err.to_string())?;
        let peak = self.samples.iter().fold(0.0f32, |acc, &s| acc.max(s.abs()));
        let true_peak_db = if peak > 0.0 {
            20.0 * f64::from(peak).log10()
        } else {
            f64::NEG_INFINITY
        };
        let contour = pcm.loudness_contour(self.config);
        let active: Vec<f64> = contour
            .iter()
            .map(|p| f64::from(p.dbfs))
            .filter(|&db| db > f64::from(self.config.silence_floor_dbfs()) && db > -70.0)
            .collect();
        let (integrated_lufs, range_lu) = if active.is_empty() {
            (f64::NEG_INFINITY, 0.0)
        } else {
            let mean_power: f64 = active
                .iter()
                .map(|&db| 10.0_f64.powf(db / 10.0))
                .sum::<f64>()
                / active.len() as f64;
            let ungated_lufs = -0.691 + 10.0 * mean_power.log10();
            let rel_gate = ungated_lufs - 10.0;
            let mut gated: Vec<f64> = active
                .into_iter()
                .filter(|&db| db - 0.691 >= rel_gate)
                .collect();
            if gated.is_empty() {
                (ungated_lufs, 0.0)
            } else {
                let gated_power: f64 = gated
                    .iter()
                    .map(|&db| 10.0_f64.powf(db / 10.0))
                    .sum::<f64>()
                    / gated.len() as f64;
                let lufs = -0.691 + 10.0 * gated_power.log10();
                gated.sort_by(|a, b| a.total_cmp(b));
                let lra = if gated.len() >= 2 {
                    let lo = gated[(gated.len() - 1) * 10 / 100];
                    let hi = gated[(gated.len() - 1) * 95 / 100];
                    (hi - lo).max(0.0)
                } else {
                    0.0
                };
                (lufs, lra)
            }
        };
        Ok(AnalyzerResult {
            loudness: Some(AnalyzerLoudness {
                integrated_lufs,
                true_peak_db,
                range_lu,
            }),
            ..Default::default()
        })
    }
}

/// Register the shipped beat, silence, and loudness analyzers on `ctx`.
pub fn register_shipped_analyzers(
    ctx: &PluginContext,
    beats: aria_analysis::BeatConfig,
    silence: aria_analysis::SilenceConfig,
    loudness: aria_analysis::LoudnessConfig,
) -> Result<Vec<Registration>, PluginError> {
    Ok(vec![
        ctx.register_analyzer(BeatAnalyzer::spec(), BeatAnalyzer::new(beats))?,
        ctx.register_analyzer(SilenceAnalyzer::spec(), SilenceAnalyzer::new(silence))?,
        ctx.register_analyzer(LoudnessAnalyzer::spec(), LoudnessAnalyzer::new(loudness))?,
    ])
}

#[cfg(test)]
#[path = "analyzers_tests.rs"]
mod tests;
