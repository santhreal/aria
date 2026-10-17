//! Analysis: shots, silence, beats, loudness, and the moments index they persist to.
//!
//! `aria-analysis` owns every detector, `aria-detect` owns the decode/sample seam that
//! feeds them, and `aria-analysis::moments` owns the SQLite index; nothing here
//! reimplements a threshold, a histogram, a pixel conversion, or a query. What this
//! module owns:
//!
//! - **One content key per media file**, derived from its path, length, and mtime. A
//!   re-encoded or replaced file gets a different key, so its stale analysis is never
//!   served for the new bytes.
//! - **One analyzer identity and version per detector** ([`AnalysisKind`]). The version
//!   is the staleness signal: a batch stored by an older version is labelled and offers
//!   a re-run rather than being silently trusted or silently dropped.
//! - **A run is a job.** Decode, detect, and `replace_batch` happen on the job thread;
//!   the rows come back over a channel. A detector is never run on the render thread.
//! - **The panel reads rows, not the database.** One query per refresh, filtered and
//!   ordered by `filter_and_sort_moments`, because a query per frame is a disk read per
//!   frame.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use aria_analysis::LoudnessPoint;
use aria_analysis::moments::{
    AnalyzerIdentity, AnalyzerVersion, MAX_MOMENTS_QUERY_LIMIT, MediaContentKey, MomentBatchKey,
    MomentConfidence, MomentKind, MomentQuery, MomentRecord, MomentsIndex,
    beat_events_to_moment_batch, filter_and_sort_moments, shot_boundaries_to_moment_batch,
    silence_spans_to_moment_batch,
};
use aria_commands::{Command, EditCommand};
use aria_core::{Rational, RationalTime};
use aria_engine::EngineError;
use aria_jobs::JobCompletion;
use aria_models::ClipId;
use gpui::Context;
use sha2::{Digest, Sha256};

use super::AppState;

#[path = "analysis_kind.rs"]
mod analysis_kind;
#[path = "analysis_loudness.rs"]
mod analysis_loudness;
#[path = "analysis_moments.rs"]
mod analysis_moments;
#[path = "analysis_project.rs"]
mod analysis_project;

pub(crate) use analysis_kind::AnalysisRequest;
pub use analysis_kind::{AnalysisForm, AnalysisKind, CONFIDENCE_CHOICES, UiLoudness, UiMoment};
pub(crate) use analysis_project::{
    AnalysisReport, content_key_for, decode_analysis_pcm, project_moment, ticks_for_seconds,
    validated_pcm,
};
use analysis_project::{measure_contour, run_analysis};
// Staleness is decided where the report is built, so no production reader asks;
// the facade path exists because the arithmetic is worth a test of its own.
#[cfg(test)]
pub(crate) use analysis_project::is_stale;

pub use aria_detect::{
    ANALYSIS_SAMPLE_RATE, MAX_ANALYSIS_FRAMES, MAX_SHOT_SECONDS, SHOT_SAMPLE_FPS,
};

impl AppState {
    /// Where the moments index and any cached analysis live.
    pub fn analysis_cache_root(&self) -> PathBuf {
        self.data_dir.join("analysis")
    }

    /// The moments database file, so a read can tell "nothing analyzed yet" from a
    /// failure without creating it.
    pub fn analysis_index_path(&self) -> PathBuf {
        self.analysis_cache_root()
            .join(aria_analysis::moments::MOMENTS_INDEX_DATABASE_FILENAME)
    }

    /// The moments index, opened under the analysis cache root.
    ///
    /// Opened per call rather than held: a click-driven read costs one open, and a
    /// cached connection would outlive a relocated cache root.
    pub(crate) fn analysis_index(&self) -> Result<Arc<MomentsIndex>, String> {
        MomentsIndex::open_in_cache_root(self.analysis_cache_root())
            .map(Arc::new)
            .map_err(|err| format!("opening the moments index: {err}"))
    }

    /// Bytes the analysis cache occupies on disk.
    ///
    /// Read on demand, not polled: it stats the index and its journals, and a
    /// per-frame reading would be a disk read per frame.
    pub fn analysis_cache_size_bytes(&self) -> u64 {
        let Ok(entries) = std::fs::read_dir(self.analysis_cache_root()) else {
            return 0;
        };
        entries
            .flatten()
            .filter_map(|entry| entry.metadata().ok())
            .filter(|meta| meta.is_file())
            .map(|meta| meta.len())
            .sum()
    }

    /// Delete every stored analysis for every media file.
    ///
    /// The whole index goes, rather than a batch at a time: the operator asking to
    /// clear a cache is asking to reclaim the disk, and a per-media clear already
    /// exists on the Analysis panel. Nothing in the project references it, so the next
    /// run rebuilds what is still wanted.
    pub fn clear_analysis_cache(&mut self) -> Result<u64, String> {
        let root = self.analysis_cache_root();
        let freed = self.analysis_cache_size_bytes();
        if root.exists() {
            std::fs::remove_dir_all(&root).map_err(|err| format!("{}: {err}", root.display()))?;
        }
        self.analysis.moments.clear();
        self.analysis.loudness = std::sync::Arc::from([]);
        self.status_message = Some(format!(
            "Cleared the analysis cache, {}",
            super::storage::format_bytes(freed)
        ));
        Ok(freed)
    }

    /// The selected clip's media, or why analysis cannot run on it.
    ///
    /// Split from the run so every refusal is reachable without a window.
    pub(crate) fn analysis_precheck(&self, kind: AnalysisKind) -> Result<AnalysisRequest, String> {
        let clip = self.selected_ui_clip().ok_or("Select a clip to analyze")?;
        let media_id = clip
            .media_id
            .ok_or("A generated clip has no media to analyze")?;
        let media = self
            .store
            .media
            .iter()
            .find(|item| item.id == media_id)
            .ok_or("The clip's media is no longer in the pool")?;
        if media.missing {
            return Err(format!("{} is offline. Relink it first", media.name));
        }
        if kind.needs_audio() && !media.has_audio {
            return Err(format!("{} has no audio track", media.name));
        }
        let path = PathBuf::from(&*media.path);
        let content_key = content_key_for(&path)?;
        Ok(AnalysisRequest {
            kind,
            media_path: path,
            content_key,
            rate: self.playhead.rate,
            clip: ClipId::from_raw(clip.id),
            media_rate: media.frame_rate,
            label: format!(
                "Analyzing {} for {}",
                media.name,
                kind.label().to_lowercase()
            ),
            tuning: self
                .ctx
                .try_get::<crate::plugins::DetectorTuning>()
                .map(|tuning| *tuning)
                .unwrap_or_default(),
        })
    }

    /// Run one detector as a job, persist its batch, and adopt the rows.
    pub fn analyze_selected(
        &mut self,
        kind: AnalysisKind,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let request = self.analysis_precheck(kind)?;
        let index = self.analysis_index()?;
        let (wake, wakes) = crate::handoff::wakes::<Result<AnalysisReport, String>>();
        let label = request.label.clone();
        let clip = request.clip;
        let media_rate = request.media_rate;
        self.spawn_job(label, move |context| {
            let outcome = run_analysis(context, &request, &index);
            let reported = match (&outcome, context.cancelled()) {
                (_, true) => Err(String::new()),
                (Ok(report), false) => Ok(AnalysisReport {
                    count: report.count,
                    beat_seconds: report.beat_seconds.clone(),
                }),
                (Err(err), false) => Err(err.clone()),
            };
            wake.wake(reported);
            let count = outcome?.count;
            JobCompletion::new(format!("{count} moment(s)"))
                .with_output("moments", count.to_string())
                .map_err(String::from)
        });

        cx.spawn(async move |this, cx| {
            let outcome = wakes.next_from_a_thread(cx.background_executor()).await;
            let _ = this.update(cx, |state, cx| {
                match outcome {
                    Some(Ok(report)) => {
                        state.status_message = Some(format!("Detected {} moment(s)", report.count));
                        if let Err(err) = state.refresh_moments() {
                            state.report_error(err);
                        }
                        if kind == AnalysisKind::Beats {
                            state.store_beats(clip, &report.beat_seconds, media_rate);
                        }
                    }
                    Some(Err(reason)) if reason.is_empty() => {
                        state.status_message = Some("Cancelled the analysis".into());
                    }
                    Some(Err(reason)) => state.report_error(reason),
                    None => state.report_error("the analysis did not report"),
                }
                cx.notify();
            });
        })
        .detach();
        self.status_message = Some(format!("Analyzing for {}…", kind.label().to_lowercase()));
        Ok(())
    }

    /// Run one detector and put its refusal where the operator will see it.
    ///
    /// The wrapper every surface spends: the palette row, the chord, and the
    /// panel button all want "start it, and if it will not start, say so", and
    /// three copies of that are three chances for one of them to swallow the
    /// reason.
    pub(crate) fn run_detector(&mut self, kind: AnalysisKind, cx: &mut Context<Self>) {
        if let Err(err) = self.analyze_selected(kind, cx) {
            self.report_error(err);
        }
    }

    /// Store a detected beat grid on the clip that was analysed.
    ///
    /// Beats are kept in source ticks at the media's own rate, so they ride the
    /// content through trims, splits, and retimes. The detector reports seconds,
    /// and this is the only place that conversion happens.
    ///
    /// A run that found nothing still writes: an empty grid is the honest result,
    /// and it clears a stale one from an earlier detector version.
    pub(crate) fn store_beats(&mut self, clip: ClipId, seconds: &[f64], media_rate: Rational) {
        let snapshot = self.host.snapshot();
        let seq_rate = snapshot.timeline().frame_rate;
        let (clip_start_sample, source_start_ticks, source_rate) = snapshot
            .timeline()
            .clip(clip)
            .and_then(|c| {
                let src = c.source_range()?;
                let start_sample = (i128::from(c.timeline.start.value)
                    * i128::from(aria_render::EXPORT_AUDIO_RATE)
                    * i128::from(seq_rate.den))
                    / (i128::from(seq_rate.num).max(1));
                Some((start_sample as i64, src.start.value, src.start.rate))
            })
            .unwrap_or((0, 0, media_rate));
        let beats: Vec<i64> = seconds
            .iter()
            .map(|value| {
                let target_tick = ticks_for_seconds(*value, source_rate);
                let delta_ticks = target_tick - source_start_ticks;
                let num = i128::from(source_rate.num).max(1);
                let delta_frames = (i128::from(delta_ticks)
                    * i128::from(aria_render::EXPORT_AUDIO_RATE)
                    * i128::from(source_rate.den)
                    + (num - 1))
                    / num;
                clip_start_sample + delta_frames as i64
            })
            .collect();
        let result = aria_plugin::AnalyzerResult {
            beats,
            ..Default::default()
        };
        if let Err(err) = self.apply_analyzer_output(
            aria_plugin::SHIPPED_BEAT_ANALYZER_ID,
            aria_plugin::AudioSource::Clip(clip),
            &result,
        ) {
            self.report_error(err);
            return;
        }
        let found = self.ui_clip_ref(clip).map_or(0, |c| c.beat_ticks.len());
        self.status_message = Some(match found {
            0 => "No beats found in this clip".to_string(),
            1 => "Stored 1 beat on the clip".to_string(),
            count => format!("Stored {count} beats on the clip"),
        });
    }

    /// Apply an [`aria_plugin::AnalyzerResult`] to the project as one undo step
    /// named after the analyzer, then refresh the editor projection.
    pub fn apply_analyzer_output(
        &mut self,
        analyzer_id: &str,
        source: aria_plugin::AudioSource,
        result: &aria_plugin::AnalyzerResult,
    ) -> Result<(), String> {
        let mutates_project = result.has_edits()
            || (matches!(source, aria_plugin::AudioSource::Clip(_)) && result.loudness.is_none());
        if mutates_project {
            self.ctx
                .apply_analyzer(
                    &self.host,
                    aria_plugin::Origin::Editor,
                    analyzer_id,
                    source,
                    result,
                )
                .map_err(|err| err.to_string())?;
            self.sync_from_engine();
            self.clear_error();
            self.republish_audio();
        }
        self.status_message = Some(result.summary());
        Ok(())
    }

    /// Run registered audio analyzer `analyzer_id` over `AudioHost::mix` on a
    /// background job, streaming progress and applying its result on completion
    /// as one undo step named after the analyzer.
    pub fn run_registered_analyzer(
        &mut self,
        analyzer_id: &str,
        cx: &mut Context<Self>,
    ) -> Result<u64, String> {
        let entry = self
            .ctx
            .analyzer(analyzer_id)
            .ok_or_else(|| format!("Analyzer {analyzer_id} is not registered"))?;
        let spec = entry.spec().clone();
        let project = self.host.snapshot();
        let seq_rate = project.timeline().frame_rate;
        let (source, range) = match spec.scope {
            aria_plugin::AnalyzerScope::Clip => {
                let clip = self
                    .sounding_selection()
                    .into_iter()
                    .next()
                    .ok_or_else(|| "Select a clip with sound.".to_string())?;
                let timeline = project
                    .timeline()
                    .clip(clip)
                    .map(|c| c.timeline)
                    .ok_or_else(|| "Select a clip with sound.".to_string())?;
                (aria_plugin::AudioSource::Clip(clip), timeline)
            }
            aria_plugin::AnalyzerScope::Sequence => {
                let duration = project
                    .timeline()
                    .duration()
                    .value
                    .max(self.store.sequence.duration_ticks);
                if duration <= 0 {
                    return Err("The sequence has no clips.".into());
                }
                (
                    aria_plugin::AudioSource::Open,
                    aria_core::TimeRange::at_rate(0, duration, seq_rate),
                )
            }
            aria_plugin::AnalyzerScope::InOut => {
                let (start, end) = self
                    .work_area_or_sequence_range()
                    .ok_or_else(|| "The sequence has no clips.".to_string())?;
                if end <= start {
                    return Err("The sequence has no clips.".into());
                }
                (
                    aria_plugin::AudioSource::Open,
                    aria_core::TimeRange::at_rate(start, end - start, seq_rate),
                )
            }
        };

        let (wake, wakes) = crate::handoff::wakes::<Result<aria_plugin::AnalyzerResult, String>>();
        let ctx_clone = Arc::clone(&self.ctx);
        let job_label = format!("Analyzing: {}", spec.label);
        let analyzer_id_owned = spec.id.clone();
        let id = self.spawn_job(job_label, move |context| {
            let cancel = std::sync::atomic::AtomicBool::new(context.cancelled());
            let res = entry.run_with_progress(
                &ctx_clone,
                &project,
                source,
                range,
                &cancel,
                |mixed, total| {
                    if context.cancelled() {
                        cancel.store(true, std::sync::atomic::Ordering::Release);
                    } else {
                        let frac = if total == 0 {
                            1.0
                        } else {
                            mixed as f32 / total as f32
                        };
                        context.set_progress(frac, format!("frame {mixed}/{total}"));
                    }
                },
            );
            match res {
                Ok(result) => {
                    crate::app::jobs::CommitGuard::new(context, "the analyzer result").begin()?;
                    let summary = result.summary();
                    wake.wake(Ok(result));
                    Ok(JobCompletion::new(summary))
                }
                Err(aria_plugin::AnalyzerError::Cancelled) if context.cancelled() => {
                    wake.wake(Err(String::new()));
                    Err(String::new())
                }
                Err(err) => {
                    let msg = err.to_string();
                    wake.wake(Err(msg.clone()));
                    Err(msg)
                }
            }
        });

        cx.spawn(async move |this, cx| {
            let outcome = wakes.next_from_a_thread(cx.background_executor()).await;
            let _ = this.update(cx, |state, cx| {
                match outcome {
                    Some(Ok(result)) => {
                        if let Err(err) =
                            state.apply_analyzer_output(&analyzer_id_owned, source, &result)
                        {
                            state.report_error(err);
                        }
                    }
                    Some(Err(reason)) if reason.is_empty() => {
                        state.status_message = Some("Cancelled the analysis".into());
                    }
                    Some(Err(reason)) => state.report_error(reason),
                    None => state.report_error("the analysis did not report"),
                }
                cx.notify();
            });
        })
        .detach();

        self.status_message = Some(format!("Analyzing with {}…", spec.label));
        Ok(id.raw())
    }

    /// Read the selected media's moments out of the index into panel rows.
    ///
    /// One query per refresh returns every version for this media; the filters are
    /// applied in memory by `filter_and_sort_moments`, so the kind chips and the
    /// confidence floor cost no further disk reads and the stale rows stay visible.
    ///
    /// With no index on disk this is a no-op rather than an open: selecting a clip must
    /// not create a database for a session that never analyzed anything.
    pub fn refresh_moments(&mut self) -> Result<(), String> {
        if !self.analysis_index_path().exists() {
            self.clear_analysis_rows();
            return Ok(());
        }
        let Some(clip) = self.selected_ui_clip() else {
            self.clear_analysis_rows();
            return Ok(());
        };
        let Some(path) = clip.media_id.and_then(|id| {
            self.store
                .media
                .iter()
                .find(|item| item.id == id)
                .map(|item| PathBuf::from(&*item.path))
        }) else {
            self.clear_analysis_rows();
            return Ok(());
        };
        let content_key = content_key_for(&path)?;
        let index = self.analysis_index()?;
        let stored = index
            .query_with_limit(&MomentQuery::new(content_key), MAX_MOMENTS_QUERY_LIMIT)
            .map_err(|err| format!("reading the moments index: {err}"))?
            .into_records();
        // Which detectors have run is asked while a panel draws, so it is
        // answered here, on the connection this refresh already opened, rather
        // than by opening the database again per button per frame. A batch with
        // no moments in it still counts: the detector ran and found nothing.
        self.analysis.stored = AnalysisKind::ALL
            .into_iter()
            .filter(|kind| {
                index
                    .batch_exists(&kind.batch_key(content_key))
                    .unwrap_or(false)
            })
            .collect();

        let rate = self.playhead.rate;
        let offset = clip.timeline_start_ticks - clip.source_start_ticks;
        self.analysis.moments = self
            .filtered_moments(&stored, content_key)
            .into_iter()
            .map(|record| project_moment(record, offset, rate))
            .collect();
        Ok(())
    }

    /// Forget what the last refresh read. Both the rows and the record of which
    /// detectors have run describe one selection, so neither outlives it.
    fn clear_analysis_rows(&mut self) {
        self.analysis.moments.clear();
        self.analysis.stored.clear();
    }

    /// The stored records the panel's filters accept, in canonical order.
    fn filtered_moments<'a>(
        &self,
        stored: &'a [MomentRecord],
        content_key: MediaContentKey,
    ) -> Vec<&'a MomentRecord> {
        let mut query = MomentQuery::new(content_key).with_minimum_confidence(
            MomentConfidence::new(self.analysis.minimum_confidence.clamp(0.0, 1.0))
                .unwrap_or(MomentConfidence::ZERO),
        );
        if let Some(kind) = self.analysis.kind_filter {
            query = query.with_kind(kind.moment_kind());
        }
        filter_and_sort_moments(stored, &query)
    }

    /// Drop one detector's stored results for the selected media.
    ///
    /// Deleting rather than re-running: a stale batch that cannot be re-measured (the
    /// media is offline, the detector changed) must still be removable, or the panel
    /// shows results nothing can correct.
    ///
    /// Clearing beats also clears the grid the run stored on the clip. Leaving it
    /// behind would keep the magnet snapping to positions the panel no longer
    /// shows, and the project copy is what the timeline reads. The grid goes
    /// first and without the precheck: it can outlive the media's audio (offline,
    /// relinked, replaced by a still), and a grid nothing can clear is worse than
    /// a cache row nothing can key.
    pub fn clear_analysis(&mut self, kind: AnalysisKind) -> Result<(), String> {
        let grid = (kind == AnalysisKind::Beats)
            .then(|| self.selected_ui_clip())
            .flatten()
            .filter(|clip| !clip.beat_ticks.is_empty())
            .map(|clip| ClipId::from_raw(clip.id));
        if let Some(clip) = grid {
            self.apply_command(Command::Edit(EditCommand::ClearBeats { clip }))
                .map_err(|err| err.to_string())?;
        }
        let existed = match self.delete_analysis_batch(kind) {
            Ok(existed) => existed,
            // No batch can be keyed without the media the precheck wants, and
            // the grid is already gone, so this is a clear that did something.
            Err(err) if grid.is_some() => {
                debug_assert!(!err.is_empty());
                false
            }
            Err(err) => return Err(err),
        };
        self.status_message = Some(if existed || grid.is_some() {
            format!("Cleared the {} analysis", kind.label().to_lowercase())
        } else {
            format!("No {} analysis to clear", kind.label().to_lowercase())
        });
        self.refresh_moments()
    }

    /// Delete one detector's cached batch for the selected media. Returns whether
    /// there was one.
    fn delete_analysis_batch(&self, kind: AnalysisKind) -> Result<bool, String> {
        let request = self.analysis_precheck(kind)?;
        self.analysis_index()?
            .delete_batch(&kind.batch_key(request.content_key))
            .map_err(|err| format!("clearing the moments index: {err}"))
    }

    /// Whether the selected media already has this detector's current results.
    ///
    /// Read from what the last [`Self::refresh_moments`] recorded. The index is
    /// a file to open, and this is asked once per Detect button per frame.
    pub fn analysis_exists(&self, kind: AnalysisKind) -> bool {
        self.analysis.stored.contains(&kind)
    }

    /// Split the selected clip at every detected shot boundary inside it.
    ///
    /// Boundaries outside the clip's own range are skipped rather than clamped: a cut
    /// at the clip edge is not an edit, and a cut past it belongs to media the clip
    /// does not show.
    pub fn split_at_shots(&mut self) -> Result<(), EngineError> {
        let ticks = self.moment_ticks_in_selection(MomentKind::SHOT_BOUNDARY);
        let Some(clip) = self.selected_clip() else {
            self.status_message = Some("Select a clip to split".into());
            return Ok(());
        };
        if ticks.is_empty() {
            self.status_message =
                Some("No shot boundaries inside the clip. Detect shots first".into());
            return Ok(());
        }
        let rate = self.playhead.rate;
        let mut split = 0_usize;
        // Later cuts first: splitting the head does not move the tail's ticks, and a
        // cut is addressed by the clip that still covers it.
        for tick in ticks.into_iter().rev() {
            let target = self
                .store
                .sequence
                .tracks
                .iter()
                .flat_map(|track| track.clips.iter())
                .find(|candidate| {
                    candidate.timeline_start_ticks < tick && candidate.timeline_end_ticks > tick
                })
                .map_or(clip, |candidate| {
                    aria_models::ClipId::from_raw(candidate.id)
                });
            self.apply_command(Command::Edit(EditCommand::SplitClip {
                clip: target,
                at: RationalTime::new(tick, rate),
            }))?;
            split += 1;
        }
        self.status_message = Some(format!("Split at {split} shot boundary(ies)"));
        Ok(())
    }

    /// Remove every detected silent span inside the selected clip, closing the gaps.
    ///
    /// Each span becomes two splits and one ripple delete. The engine has no batch, so
    /// each command is its own undo entry; the spans are walked from the end so an
    /// earlier removal never moves a later span's ticks.
    pub fn ripple_delete_silence(&mut self) -> Result<(), EngineError> {
        let spans = self.moment_spans_in_selection(MomentKind::SILENCE);
        if spans.is_empty() {
            self.status_message = Some("No silence inside the clip. Detect silence first".into());
            return Ok(());
        }
        let rate = self.playhead.rate;
        let mut removed = 0_usize;
        for (start, end) in spans.into_iter().rev() {
            let Some(head) = self.clip_covering(start) else {
                continue;
            };
            self.apply_command(Command::Edit(EditCommand::SplitClip {
                clip: head,
                at: RationalTime::new(start, rate),
            }))?;
            if let Some(middle) = self.clip_covering(end) {
                self.apply_command(Command::Edit(EditCommand::SplitClip {
                    clip: middle,
                    at: RationalTime::new(end, rate),
                }))?;
            }
            let Some(silent) = self.clip_starting_at(start) else {
                continue;
            };
            self.apply_command(Command::Edit(EditCommand::RippleDelete { clip: silent }))?;
            removed += 1;
        }
        self.clear_selection();
        self.status_message = Some(format!("Removed {removed} silent span(s)"));
        Ok(())
    }
}
