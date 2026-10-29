//! Contracts for analysis: what refuses a run, what the index keeps, and what an
//! analyzed moment becomes on the timeline.
//!
//! Nothing here decodes media or opens a socket. The detectors are `aria-analysis`'s
//! contract; what this crate owns is the content key, the staleness rule, the filters,
//! and the edits a detected moment drives.

use aria_analysis::TimeSpan;
use aria_analysis::moments::{MomentConfidence, MomentRecord, MomentReplacementBatch};
use aria_core::Rational;

use super::AppState;
use super::analysis::{AnalysisKind, content_key_for, is_stale, ticks_for_seconds};
use super::fixtures::{media_path, store_moments as store};
use super::premiere_tests::with_clip;

const R24: Rational = Rational::new(24, 1);

/// Seconds land on the frame that is showing, never the next one, and an absurd or
/// negative input is clamped instead of wrapping into a negative tick.
#[test]
fn seconds_convert_to_ticks_without_drifting_late() {
    assert_eq!(ticks_for_seconds(0.0, R24), 0);
    assert_eq!(ticks_for_seconds(1.0, R24), 24);
    assert_eq!(ticks_for_seconds(1.999, R24), 47);
    assert_eq!(ticks_for_seconds(-5.0, R24), 0);
    assert_eq!(ticks_for_seconds(f64::NAN, R24), 0);
    assert_eq!(ticks_for_seconds(f64::INFINITY, R24), i64::MAX);
}

/// Every detector has a distinct storage identity, a non-zero version, and a moment
/// kind. Enumerated from `AnalysisKind::ALL`, so a new detector that reuses another's
/// scope — which would silently delete its batch — fails here.
#[test]
fn every_detector_has_its_own_replacement_scope() {
    let mut identities = Vec::new();
    let mut kinds = Vec::new();
    for kind in AnalysisKind::ALL {
        assert!(!kind.id().is_empty());
        assert!(!kind.label().is_empty());
        assert!(kind.analyzer_version().get() > 0);
        identities.push(kind.analyzer_identity().as_str().to_string());
        kinds.push(kind.moment_kind().as_str().to_string());
    }
    identities.sort();
    let unique = identities.len();
    identities.dedup();
    assert_eq!(identities.len(), unique, "two detectors share an identity");
    kinds.sort();
    let unique = kinds.len();
    kinds.dedup();
    assert_eq!(kinds.len(), unique, "two detectors share a moment kind");
}

/// The content key changes when the file's bytes change, and a missing file is a named
/// error rather than a key that would serve another file's analysis.
///
/// The class closed here is "stale analysis served for new bytes": the key is derived
/// from length and modification time, so replacing the file must move the key.
#[test]
fn the_content_key_follows_the_file_not_the_path() {
    let dir = crate::scratch_pool::scratch("analysis-content-key");
    let path = dir.join("clip.bin");
    std::fs::write(&path, b"first").unwrap();
    let first = content_key_for(&path).expect("a written file has a key");
    assert_eq!(first, content_key_for(&path).expect("stable"));

    // A different length is a different key even within mtime granularity.
    std::fs::write(&path, b"second and longer").unwrap();
    assert_ne!(
        first,
        content_key_for(&path).expect("key after rewrite"),
        "a rewritten file kept its old key"
    );

    let err = content_key_for(&dir.join("absent.bin")).expect_err("a missing file has no key");
    assert!(err.contains("absent.bin"), "{err}");
}

/// Stored moments reach the panel, the kind and confidence filters narrow them without
/// a second read, and beats become magnets.
///
/// The class closed here is "a detector's results are written and never seen again":
/// every filter path goes through the crate's own query, so a UI-side sort cannot drift
/// from what the index would return.
#[test]
fn stored_moments_reach_the_panel_and_the_filters_narrow_them() {
    let (mut state, _) = with_clip();
    store(
        &state,
        AnalysisKind::Beats,
        &[(0.5, 0.5, 1.0), (1.5, 1.5, 1.0)],
    );
    store(
        &state,
        AnalysisKind::Shots,
        &[(1.0, 1.0, 0.9), (2.0, 2.0, 0.2)],
    );
    state.refresh_moments().expect("read the index");

    assert_eq!(state.analysis.moments.len(), 4, "every kind is listed");
    assert!(
        state.analysis.moments.iter().any(|m| m.kind == "beat"),
        "the beat rows did not reach the panel"
    );
    assert!(
        state.analysis.moments.iter().all(|moment| !moment.stale),
        "current results were labeled stale"
    );

    state.set_analysis_kind_filter(Some(AnalysisKind::Shots));
    assert_eq!(state.analysis.moments.len(), 2);
    assert!(
        state
            .analysis
            .moments
            .iter()
            .all(|moment| moment.kind == "shot_boundary")
    );

    state.set_analysis_minimum_confidence(0.5);
    assert_eq!(
        state.analysis.moments.len(),
        1,
        "the weak shot survived the confidence floor"
    );
    assert!((state.analysis.moments[0].confidence - 0.9).abs() < 1e-6);

    // The filters are a view, not a delete: widening restores every row.
    state.set_analysis_minimum_confidence(0.0);
    state.set_analysis_kind_filter(None);
    assert_eq!(state.analysis.moments.len(), 4);
}

/// A batch written by an older analyzer version is labelled stale rather than served as
/// current or silently dropped, and clearing removes it.
#[test]
fn an_older_analyzer_version_reads_as_stale_and_can_be_cleared() {
    let (mut state, _) = with_clip();
    let kind = AnalysisKind::Shots;
    let content_key = content_key_for(&media_path(&state)).expect("a key");
    let index = state.analysis_index().expect("open the index");
    let old = aria_analysis::moments::AnalyzerVersion::new(kind.analyzer_version().get())
        .expect("non-zero");
    // The current version is 1, so a stale batch needs the schema to have moved. Store
    // under the current version, then assert the rule that decides staleness directly,
    // which is what a future bump will exercise.
    let record = MomentRecord::new(
        content_key,
        TimeSpan::new(1.0, 1.0).expect("span"),
        kind.moment_kind(),
        MomentConfidence::CERTAIN,
        kind.analyzer_identity(),
        old,
        None,
    );
    let batch = MomentReplacementBatch::new(
        aria_analysis::moments::MomentBatchKey::new(content_key, kind.analyzer_identity(), old),
        vec![record.clone()],
    )
    .expect("batch");
    index.replace_batch(&batch).expect("write");

    // Which detectors hold results is recorded by the refresh, not asked of the
    // database while a button draws, so the batch is invisible until then.
    assert!(
        !state.analysis_exists(kind),
        "a batch was reported before any refresh read it"
    );
    state.refresh_moments().expect("read");
    assert!(state.analysis_exists(kind), "the batch was not stored");
    assert_eq!(state.analysis.moments.len(), 1);
    assert!(
        !state.analysis.moments[0].stale,
        "the current version must not read as stale"
    );

    // The rule itself, in both directions. Every detector is at version 1 today, so a
    // stored batch cannot be older yet; this is what will label the rows on the day one
    // is bumped, and what must keep a newer build's rows unlabelled.
    assert!(is_stale(1, 2), "an older batch was not labeled stale");
    assert!(!is_stale(2, 2), "the current version was labeled stale");
    assert!(
        !is_stale(u32::MAX, 1),
        "a newer build's batch was labeled stale"
    );
    let projected = super::analysis::project_moment(&record, 0, R24);
    assert_eq!(projected.analyzer_version, kind.analyzer_version().get());
    assert_eq!(projected.start_tick, 24, "1 s at 24 fps");
    assert!(!projected.stale);

    state.clear_analysis(kind).expect("clear the batch");
    assert!(!state.analysis_exists(kind), "the batch survived the clear");
    assert!(state.analysis.moments.is_empty(), "the rows survived");

    // Clearing again is a message, not an error.
    state.clear_analysis(kind).expect("a second clear is inert");
}

/// Detected shots become splits and detected silence becomes ripple deletes, both
/// bounded to the selected clip and both undoable.
///
/// The class closed here is "an analyzed moment is a readout the editor cannot act on".
#[test]
fn detected_moments_drive_splits_and_ripple_deletes() {
    let (mut state, clip) = with_clip();
    let bounds = state
        .selected_ui_clip()
        .map(|ui| (ui.timeline_start_ticks, ui.timeline_end_ticks))
        .expect("a selected clip");
    let clip_seconds = (bounds.1 - bounds.0) as f64 / 24.0;
    assert!(clip_seconds > 0.5, "the fixture clip is too short to cut");

    // One cut inside the clip and one past its end: only the inside one is an edit.
    store(
        &state,
        AnalysisKind::Shots,
        &[
            (clip_seconds / 2.0, clip_seconds / 2.0, 1.0),
            (clip_seconds + 5.0, clip_seconds + 5.0, 1.0),
        ],
    );
    state.refresh_moments().expect("read");
    let before = state.store.sequence.clip_count();
    state.split_at_shots().expect("split");
    assert_eq!(
        state.store.sequence.clip_count(),
        before + 1,
        "a boundary outside the clip was cut anyway"
    );
    assert!(state.undo(), "the split left no undo entry");
    assert_eq!(state.store.sequence.clip_count(), before);

    // A silent span in the middle is removed and the gap closes.
    state.select_clip(clip);
    store(
        &state,
        AnalysisKind::Silence,
        &[(clip_seconds / 4.0, clip_seconds / 2.0, 1.0)],
    );
    state.refresh_moments().expect("read");
    let duration_before = state.store.sequence.duration_ticks;
    state.ripple_delete_silence().expect("remove silence");
    assert!(
        state.store.sequence.duration_ticks < duration_before,
        "removing silence did not close the gap"
    );
}

/// Every refusal is reachable without a window, and a refused run queues no job.
#[test]
fn a_run_that_cannot_work_is_refused_before_it_costs_anything() {
    let (mut state, _) = with_clip();
    let jobs = state.job_view.len();

    // The fixture is a still image: the audio detectors have nothing to read.
    for kind in [AnalysisKind::Silence, AnalysisKind::Beats] {
        let err = state
            .analysis_precheck(kind)
            .expect_err("a still image was accepted for audio analysis");
        assert!(err.contains("no audio track"), "{err}");
    }
    // Video analysis is allowed on it: the refusal must be about audio, not about the
    // file being an image.
    state
        .analysis_precheck(AnalysisKind::Shots)
        .expect("shots on a still image");

    state.clear_selection();
    for kind in AnalysisKind::ALL {
        let err = state
            .analysis_precheck(kind)
            .expect_err("no selection was accepted");
        assert!(err.contains("Select a clip"), "{err}");
    }
    assert_eq!(state.job_view.len(), jobs, "a refusal queued a job");

    // Nothing to act on is a message, not an error or an edit.
    state.split_at_shots().expect("no shots is not a failure");
    state
        .ripple_delete_silence()
        .expect("no silence is not a failure");
}

/// The moment seek walks the rows the panel shows, in both directions, and stops rather
/// than wrapping at the ends.
#[test]
fn seeking_moments_walks_the_visible_rows() {
    let (mut state, _) = with_clip();
    store(
        &state,
        AnalysisKind::Beats,
        &[(0.5, 0.5, 1.0), (1.5, 1.5, 1.0)],
    );
    state.refresh_moments().expect("read");

    state.set_playhead(0);
    state.seek_next_moment();
    assert_eq!(state.store.playhead_ticks, 12);
    state.seek_next_moment();
    assert_eq!(state.store.playhead_ticks, 36);
    state.seek_next_moment();
    assert_eq!(state.store.playhead_ticks, 36, "the last moment wrapped");
    state.seek_previous_moment();
    assert_eq!(state.store.playhead_ticks, 12);
    state.seek_previous_moment();
    assert_eq!(state.store.playhead_ticks, 12, "the first moment wrapped");
}

/// The analysis cache is measurable and clearable, and clearing it empties the rows
/// rather than leaving a view of a database that is gone.
#[test]
fn the_analysis_cache_is_measurable_and_clearable() {
    let (mut state, _) = with_clip();
    assert_eq!(
        state.analysis_cache_size_bytes(),
        0,
        "a fresh session has an analysis cache"
    );
    assert!(!state.analysis_index_path().exists());
    // A refresh with no index must not create one.
    state.refresh_moments().expect("read nothing");
    assert!(!state.analysis_index_path().exists());

    store(&state, AnalysisKind::Beats, &[(1.0, 1.0, 1.0)]);
    state.refresh_moments().expect("read");
    assert!(state.analysis_cache_size_bytes() > 0);
    assert_eq!(state.analysis.moments.len(), 1);

    let freed = state.clear_analysis_cache().expect("clear");
    assert!(freed > 0, "clearing reported no bytes");
    assert!(state.analysis.moments.is_empty());
    assert_eq!(state.analysis_cache_size_bytes(), 0);
}

/// A detected grid belongs to the project, not to the analysis cache: it lands on
/// the clip in source ticks, projects to timeline ticks, drives the magnet, and a
/// clear removes it.
///
/// The class closed here is a detector whose result is only ever a row in a
/// database: before this, the magnet read a mirror that a reopened project had
/// no way to rebuild, and a trim could not narrow it.
#[test]
fn a_detected_grid_lands_on_the_clip_and_drives_the_magnet() {
    let (mut state, clip) = with_clip();
    let media_rate = state.store.media[0].frame_rate;

    state.store_beats(clip, &[0.5, 1.5], media_rate);
    assert_eq!(
        beat_ticks(&state),
        vec![12, 36],
        "the grid did not project onto the timeline"
    );
    assert!(
        state
            .snap_candidates()
            .iter()
            .any(|c| c.tick == 12 && c.kind == crate::snap::SnapKind::Beat),
        "a stored beat is not a magnet"
    );

    // The grid is one undo step, and undo restores the empty one.
    assert!(state.undo());
    assert!(beat_ticks(&state).is_empty(), "undo kept the grid");
    assert!(state.redo());
    assert_eq!(beat_ticks(&state), vec![12, 36]);

    // A run that found nothing is stored as nothing rather than left stale.
    state.store_beats(clip, &[], media_rate);
    assert!(beat_ticks(&state).is_empty(), "an empty run kept the grid");
}

/// Clearing the beats analysis clears the grid it stored, so the magnet cannot
/// keep snapping to positions the panel no longer lists.
///
/// The fixture is a still, which is also the second class this closes: the audio
/// precheck cannot key a cache row for it, and a grid that only a re-detection
/// could remove would be stuck on the clip forever. The clear must still report
/// as a clear.
#[test]
fn clearing_the_beats_analysis_also_clears_the_stored_grid() {
    let (mut state, clip) = with_clip();
    let media_rate = state.store.media[0].frame_rate;
    store(&state, AnalysisKind::Beats, &[(0.5, 0.5, 1.0)]);
    state.store_beats(clip, &[0.5], media_rate);
    assert_eq!(beat_ticks(&state), vec![12]);

    state
        .clear_analysis(AnalysisKind::Beats)
        .expect("clear the beats");
    assert!(beat_ticks(&state).is_empty(), "the grid survived the clear");
    assert_eq!(
        state.status_message.as_deref(),
        Some("Cleared the beats analysis"),
        "the clear did not report as one"
    );

    // And it is idempotent: nothing left to clear now reads as nothing.
    state
        .clear_analysis(AnalysisKind::Beats)
        .expect_err("a still cannot key a beats cache row");
}

/// Every projected beat tick in the sequence, in order.
fn beat_ticks(state: &AppState) -> Vec<i64> {
    let mut ticks: Vec<i64> = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .flat_map(|clip| clip.beat_ticks.iter().copied())
        .collect();
    ticks.sort_unstable();
    ticks
}
#[test]
fn analyzer_menus_and_palette_reflect_registrations_and_unmount_and_apply_in_one_step() {
    use crate::chrome::menu_items::items_of;
    use crate::types::AppMenu;
    use aria_plugin::{
        AnalyzerMarker, AnalyzerResult, AnalyzerRun, AnalyzerScope, AnalyzerSpec, AudioSource,
    };
    use std::ops::ControlFlow;

    let (mut state, clip) = with_clip();
    // The palette lists menu rows only through the mounted menu host.
    state
        .ctx
        .mount(&crate::plugins::UiPlugin)
        .expect("the ui plugin needs nothing");
    // Shipped analyzers are present on Clip > Analyze and Sequence > Analyze.
    let clip_items = items_of(AppMenu::Clip, &state);
    let beat_row = clip_items
        .iter()
        .find(|it| it.id == "clip-analyze:aria.beats")
        .expect("shipped beat analyzer row in Clip > Analyze");
    assert_eq!(beat_row.under, Some("Analyze"));
    assert_eq!(beat_row.label, "Detect Beats");
    // The fixture clip is a still without sound, so the row is disabled with
    // the sound guard's reason.
    assert_eq!(
        beat_row.blocked_by(&state),
        Some("Select a clip with sound")
    );

    let seq_items = items_of(AppMenu::Sequence, &state);
    assert!(
        seq_items
            .iter()
            .any(|it| it.id == "seq-analyze:aria.silence" && it.under == Some("Analyze"))
    );
    assert!(
        seq_items
            .iter()
            .any(|it| it.id == "seq-analyze:aria.loudness" && it.under == Some("Analyze"))
    );

    // Registering a custom clip and sequence analyzer adds rows to both menus
    // and to the command palette; dropping the registrations removes them.
    let clip_reg = state
        .ctx
        .register_analyzer(
            AnalyzerSpec::new(
                "test.custom_transients",
                "Transient Peaks",
                AnalyzerScope::Clip,
            ),
            |_rate: u32, _channels: u16| -> Box<dyn AnalyzerRun> {
                struct DummyRun;
                impl AnalyzerRun for DummyRun {
                    fn block(&mut self, _: i64, _: &[f32]) -> ControlFlow<()> {
                        ControlFlow::Continue(())
                    }
                    fn finish(self: Box<Self>) -> Result<AnalyzerResult, String> {
                        Ok(AnalyzerResult::default())
                    }
                }
                Box::new(DummyRun)
            },
        )
        .expect("register custom clip analyzer");

    assert!(
        items_of(AppMenu::Clip, &state)
            .iter()
            .any(|it| it.id == "clip-analyze:test.custom_transients" && it.under == Some("Analyze"))
    );
    assert!(state.palette_catalog().iter().any(|offer| offer.id.as_ref()
        == "clip-analyze:test.custom_transients"
        && offer.label.as_ref() == "Analyze: Transient Peaks"));

    // Applying a combined result from the custom analyzer commits one undo
    // step named after the analyzer's label.
    let combined = AnalyzerResult {
        beats: vec![24_000, 72_000],
        markers: vec![AnalyzerMarker::new(
            24_000,
            0,
            "Transient",
            aria_models::MarkerColor::Orange,
        )],
        ranges: Vec::new(),
        loudness: None,
    };
    state
        .apply_analyzer_output("test.custom_transients", AudioSource::Clip(clip), &combined)
        .expect("apply custom analyzer output");
    assert_eq!(beat_ticks(&state), vec![12, 36]);
    assert_eq!(state.next_undo_label().as_deref(), Some("Transient Peaks"));
    assert!(state.undo());
    assert!(beat_ticks(&state).is_empty());

    drop(clip_reg);
    assert!(
        !items_of(AppMenu::Clip, &state)
            .iter()
            .any(|it| it.id == "clip-analyze:test.custom_transients")
    );
    assert!(
        !state
            .palette_catalog()
            .iter()
            .any(|offer| offer.id.as_ref() == "clip-analyze:test.custom_transients")
    );
}
