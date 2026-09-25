//! One detector run, and the projections that turn stored records into rows.
//!
//! A pure move of the free functions that sat below the `impl`. `run_analysis`
//! and `measure_contour` are `pub(super)`: private to this module tree, as
//! before, and reached from the parent and its siblings through it.

use super::*;

/// What one detector run produced: how many moments landed in the index, and,
/// for a beat run, where the beats are.
///
/// The seconds ride back with the count because the project — not the analysis
/// cache — owns a clip's beat grid, and the follow-up edit that stores it needs
/// the positions rather than a row count.
pub(crate) struct AnalysisReport {
    pub(crate) count: usize,
    pub(crate) beat_seconds: Vec<f64>,
}

/// One detector run: decode, detect, persist, and report what landed.
pub(super) fn run_analysis(
    context: &aria_jobs::JobContext,
    request: &AnalysisRequest,
    index: &MomentsIndex,
) -> Result<AnalysisReport, String> {
    let mut beat_seconds = Vec::new();
    let batch = match request.kind {
        AnalysisKind::Shots => {
            context.set_progress(0.0, "sampling frames");
            let boundaries = aria_detect::detect_shots_sampled(
                &request.media_path,
                SHOT_SAMPLE_FPS,
                MAX_SHOT_SECONDS,
                request.tuning.shots,
                || context.cancelled(),
            )?;
            shot_boundaries_to_moment_batch(
                &boundaries,
                request.content_key,
                request.kind.analyzer_identity(),
                request.kind.analyzer_version(),
            )
            .map_err(|err| format!("shot boundaries: {err}"))?
        }
        AnalysisKind::Silence => {
            let pcm = decode_analysis_pcm(context, &request.media_path)?;
            let view = validated_pcm(&pcm)?;
            context.set_progress(0.5, "detecting silence");
            let spans = view.detect_silence(request.tuning.silence);
            silence_spans_to_moment_batch(
                &spans,
                request.content_key,
                request.kind.analyzer_identity(),
                request.kind.analyzer_version(),
            )
            .map_err(|err| format!("silence spans: {err}"))?
        }
        AnalysisKind::Beats => {
            let pcm = decode_analysis_pcm(context, &request.media_path)?;
            let view = validated_pcm(&pcm)?;
            context.set_progress(0.5, "detecting beats");
            let events = view.detect_beats(request.tuning.beats);
            beat_seconds = events.iter().map(|event| event.time_seconds).collect();
            beat_events_to_moment_batch(
                &events,
                request.content_key,
                request.kind.analyzer_identity(),
                request.kind.analyzer_version(),
            )
            .map_err(|err| format!("beat events: {err}"))?
        }
    };
    let count = batch.len();
    // The index write is the irreversible step: past here a cancel must be refused
    // rather than leaving a half-replaced batch behind.
    crate::app::jobs::CommitGuard::new(context, "the moments batch").begin()?;
    index
        .replace_batch(&batch)
        .map_err(|err| format!("writing the moments index: {err}"))?;
    context.set_progress(1.0, "stored");
    Ok(AnalysisReport {
        count,
        beat_seconds,
    })
}

/// Measure a loudness contour and place its windows on the timeline.
pub(super) fn measure_contour(
    context: &aria_jobs::JobContext,
    request: &AnalysisRequest,
    offset: i64,
) -> Result<Vec<UiLoudness>, String> {
    let pcm = decode_analysis_pcm(context, &request.media_path)?;
    let view = validated_pcm(&pcm)?;
    context.set_progress(0.5, "measuring loudness");
    let contour = view.loudness_contour(request.tuning.loudness);
    context.set_progress(1.0, "done");
    Ok(contour
        .iter()
        .map(|point| project_loudness(*point, offset, request.rate))
        .collect())
}

/// Decode the whole file as analysis-rate mono, reporting progress on the job.
///
/// The decode belongs to `aria-detect`; what is left here is the job surface —
/// one progress line and the job's own cancel flag.
pub(crate) fn decode_analysis_pcm(
    context: &aria_jobs::JobContext,
    path: &Path,
) -> Result<Vec<f32>, String> {
    context.set_progress(0.0, "decoding audio");
    aria_detect::decode_mono(path, || context.cancelled())
}

pub(crate) use aria_detect::validated_pcm;

/// Turn one stored record into a panel row at the timeline rate.
pub(crate) fn project_moment(record: &MomentRecord, offset: i64, rate: Rational) -> UiMoment {
    let kind = record.kind().as_str().to_string();
    let current = AnalysisKind::ALL
        .iter()
        .find(|candidate| candidate.moment_kind().as_str() == kind)
        .map(|candidate| candidate.analyzer_version().get());
    let version = record.analyzer_version().get();
    UiMoment {
        kind,
        start_tick: ticks_for_seconds(record.span().start_seconds(), rate) + offset,
        end_tick: ticks_for_seconds(record.span().end_seconds(), rate) + offset,
        confidence: record.confidence().get(),
        analyzer_version: version,
        stale: current.is_some_and(|current| is_stale(version, current)),
    }
}

/// Whether a stored batch predates what this build would write.
///
/// A separate rule so it is testable while every detector is still at version 1: the
/// day one is bumped, the panel must label the old rows rather than serve them as
/// current, and a record from a *newer* build must not be labelled either — it came
/// from a version that knows more, not less.
pub(crate) fn is_stale(stored: u32, current: u32) -> bool {
    stored < current
}

/// Turn one loudness window into a drawable point.
pub(crate) fn project_loudness(point: LoudnessPoint, offset: i64, rate: Rational) -> UiLoudness {
    UiLoudness {
        start_tick: ticks_for_seconds(point.span.start_seconds(), rate) + offset,
        dbfs: point.dbfs,
    }
}

/// Seconds to ticks at the timeline rate, floored so a moment never lands late.
///
/// A NaN offset is zero (there is no position to land on), but a positive infinity
/// saturates: an unbounded span ends after the timeline, never at its start.
pub(crate) fn ticks_for_seconds(seconds: f64, rate: Rational) -> i64 {
    if seconds.is_nan() || seconds <= 0.0 {
        return 0;
    }
    let ticks = seconds * f64::from(rate.num) / f64::from(rate.den);
    if ticks >= i64::MAX as f64 {
        i64::MAX
    } else {
        ticks.floor() as i64
    }
}

/// The content key for one media file: its path, length, and modification time.
///
/// Not a digest of the bytes. Hashing a multi-gigabyte file to decide whether its
/// analysis is current would cost more than re-analyzing, and the three cheap facts
/// change together whenever the file is replaced, re-encoded, or edited in place.
/// The cost is that a byte-identical copy under a second path analyzes twice.
pub(crate) fn content_key_for(path: &Path) -> Result<MediaContentKey, String> {
    let metadata = std::fs::metadata(path).map_err(|err| format!("{}: {err}", path.display()))?;
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |since| since.as_nanos());
    let mut hasher = Sha256::new();
    hasher.update(path.to_string_lossy().as_bytes());
    hasher.update([0]);
    hasher.update(metadata.len().to_le_bytes());
    hasher.update(modified.to_le_bytes());
    let digest: [u8; 32] = hasher.finalize().into();
    Ok(MediaContentKey::from(digest))
}
