//! Contracts for transcription: what refuses a run, what a transcript projects to, and
//! what a commit writes.
//!
//! The class closed here is "a surface offers a run that cannot work, or a commit that
//! writes something other than what the panel showed". Nothing here opens a socket or
//! needs a model on disk: the run itself is a job, and the decisions worth pinning are
//! the ones made before it starts.
//!
//! Not covered: Whisper's own output. That needs a model file and a machine willing to
//! spend a minute on inference, and it is `aria-transcription`'s contract, not this
//! crate's.

use aria_core::Rational;
use aria_transcription::{WHISPER_SAMPLE_RATE, WhisperModel};

use super::captions::{
    CaptionCommit, THREAD_CHOICES, UiCaption, UiCaptionWord, frames_for_ticks,
    ticks_for_centiseconds,
};
use super::models::TimeoutField;
use super::premiere_tests::with_clip;

const R24: Rational = Rational::new(24, 1);

fn caption(start: i64, end: i64, text: &str, words: &[(i64, i64, &str)]) -> UiCaption {
    UiCaption {
        start_tick: start,
        end_tick: end,
        text: text.to_string().into(),
        confidence: None,
        words: words
            .iter()
            .map(|(start, end, text)| UiCaptionWord {
                start_tick: *start,
                end_tick: *end,
                text: text.to_string().into(),
                confidence: 1.0,
            })
            .collect(),
    }
}

/// Sample windows and caption times are the two places a rounding error turns into
/// audio the run never sees or a caption under the wrong frame.
#[test]
fn the_decode_window_and_caption_times_convert_exactly() {
    // One second at 24 fps is one second of 16 kHz mono.
    assert_eq!(frames_for_ticks(24, R24), WHISPER_SAMPLE_RATE as usize);
    assert_eq!(frames_for_ticks(0, R24), 0);
    // A partial frame rounds up: the tail of a clip is decoded rather than clipped.
    assert_eq!(frames_for_ticks(1, R24), 667);
    // Negative ticks cannot name audio, and must not wrap into a huge allocation.
    assert_eq!(frames_for_ticks(-5, R24), 0);
    // A drop-frame rate is not 24: the conversion must use the real rate, not a
    // rounded one.
    let ntsc = Rational::new(30_000, 1001);
    assert_eq!(frames_for_ticks(30, ntsc), 16_016);

    assert_eq!(ticks_for_centiseconds(0, R24), 0);
    assert_eq!(ticks_for_centiseconds(100, R24), 24);
    assert_eq!(ticks_for_centiseconds(50, R24), 12);
    // Sub-frame offsets floor onto the frame that is showing, never the next one.
    assert_eq!(ticks_for_centiseconds(3, R24), 0);
    assert!(ticks_for_centiseconds(u64::MAX, R24) > 0);
}

/// Every model in the catalog is listed with a size, a description, and a state, and a
/// fresh session has none of them installed. Enumerated from the catalog so a new model
/// fails here rather than being silently unreachable.
#[test]
fn every_catalogued_model_is_listed_and_starts_uninstalled() {
    let (state, _) = with_clip();
    let rows = state.model_rows();
    assert_eq!(rows.len(), WhisperModel::catalog().len());
    assert!(!rows.is_empty(), "the catalog offers no model at all");
    for row in &rows {
        assert!(row.bytes > 0, "{} has no catalogued size", row.id);
        assert!(!row.description.is_empty(), "{} has no description", row.id);
        assert_eq!(row.status, "Not installed", "{} claims a state", row.id);
        assert!(!row.reinstall, "{} offered a re-download", row.id);
    }
    assert!(
        state.installed_model().is_none(),
        "a fresh session claimed a verified model"
    );
}

/// Every download timeout is editable, persists, and is clamped to what
/// `HttpDownloader` accepts. The class closed here is "a stepper writes a value the
/// downloader refuses, so the next install fails with no visible cause": the fields are
/// enumerated from `TimeoutField::ALL`, so a new one is covered on the day it is added.
///
/// Not covered: the download itself. No test here opens a socket.
#[test]
fn download_timeouts_persist_and_clamp_to_a_usable_range() {
    let (mut state, _) = with_clip();
    let defaults = aria_settings::TranscriptionSettings::default();
    assert_eq!(state.model_timeouts(), defaults);

    for (field, id, label) in TimeoutField::ALL {
        assert!(!id.is_empty() && !label.is_empty());
        state.set_model_timeout(field, 42).expect("a plain value");
        assert_eq!(
            field.secs(&state.model_timeouts()),
            42,
            "{id} did not persist"
        );

        // Halving past the floor stops at one second rather than reaching zero, which
        // the downloader would refuse.
        state
            .set_model_timeout(field, 0)
            .expect("clamped, not refused");
        assert_eq!(field.secs(&state.model_timeouts()), 1, "{id} floor");

        // Doubling past the ceiling stops at the ceiling.
        state
            .set_model_timeout(field, u64::MAX)
            .expect("clamped, not refused");
        assert_eq!(
            field.secs(&state.model_timeouts()),
            aria_settings::MAX_DOWNLOAD_TIMEOUT_SECS,
            "{id} ceiling"
        );
    }

    // Every field ended at the ceiling, and nothing else in the config moved.
    let mut expected = aria_settings::TranscriptionSettings::default();
    expected.connect_timeout_secs = aria_settings::MAX_DOWNLOAD_TIMEOUT_SECS;
    expected.read_timeout_secs = aria_settings::MAX_DOWNLOAD_TIMEOUT_SECS;
    expected.total_timeout_secs = aria_settings::MAX_DOWNLOAD_TIMEOUT_SECS;
    assert_eq!(state.model_timeouts(), expected);
    assert_eq!(
        aria_settings::load(&state.config_path())
            .expect("the written config still loads")
            .transcription,
        expected
    );
}

/// Options come from the box, and the validator has the last word. A language Whisper
/// does not know must be reported rather than quietly detected.
#[test]
fn language_and_thread_choices_are_validated_before_a_run() {
    let (mut state, _) = with_clip();
    let detect = state.transcription_options().expect("defaults validate");
    assert_eq!(detect.language, None, "an empty box is automatic detection");
    assert!(
        detect.token_timestamps,
        "word timings must come back, or per-word captions need a second run"
    );

    state.captions.language = " en ".into();
    let named = state.transcription_options().expect("en validates");
    assert_eq!(named.language.as_deref(), Some("en"));

    state.captions.language = "klingon".into();
    let err = state
        .transcription_options()
        .expect_err("an unknown language must be refused");
    assert!(
        err.contains("klingon"),
        "the refusal did not name it: {err}"
    );

    state.captions.language.clear();
    for threads in THREAD_CHOICES {
        state.set_caption_threads(threads);
        let options = state
            .transcription_options()
            .unwrap_or_else(|err| panic!("{threads} threads: {err}"));
        assert_eq!(options.thread_count, threads);
    }
    // Zero means "whatever this machine has", which is the default, not a refusal.
    state.set_caption_threads(0);
    assert!(state.transcription_options().unwrap().thread_count >= 1);
    // Out of the validator's range is refused before a model file is opened.
    state.set_caption_threads(9_000);
    assert!(state.transcription_options().is_err());
}

/// Every reason a run cannot start, in the order they can be fixed.
#[test]
fn a_run_is_refused_by_name_and_costs_no_job() {
    let (mut state, _) = with_clip();
    let jobs_before = state.job_view.len();

    // The fixture is a still image: no audio track to transcribe.
    let err = state
        .transcription_precheck()
        .expect_err("a still image was accepted");
    assert!(err.contains("no audio"), "{err}");

    state.selection.clear();
    let err = state
        .transcription_precheck()
        .expect_err("nothing selected was accepted");
    assert!(err.contains("Select a clip"), "{err}");

    assert_eq!(
        state.job_view.len(),
        jobs_before,
        "a refused run queued a job anyway"
    );
}

/// Granularity is a projection: the same transcript commits per segment or per word,
/// and a segment the model gave no word timings for survives either way.
#[test]
fn caption_units_follow_the_word_switch_without_losing_speech() {
    let (mut state, _) = with_clip();
    state.captions.segments = vec![
        caption(0, 48, "hello there", &[(0, 24, "hello"), (24, 48, "there")]),
        caption(48, 72, "unsplit", &[]),
    ];

    let segments = state.caption_units();
    assert_eq!(segments.len(), 2);
    assert_eq!(segments[0], (0, 48, "hello there".to_string()));

    state.set_caption_word_level(true);
    let words = state.caption_units();
    assert_eq!(
        words.len(),
        3,
        "word mode dropped the segment with no word timings"
    );
    assert_eq!(words[0], (0, 24, "hello".to_string()));
    assert_eq!(words[2], (48, 72, "unsplit".to_string()));
}

/// A commit writes exactly what the panel listed, to the target the panel chose.
#[test]
fn committing_writes_markers_or_caption_clips_and_nothing_when_empty() {
    let (mut state, _) = with_clip();

    // Nothing transcribed: a commit says so rather than writing an empty caption.
    let text_lanes = |state: &crate::app::AppState| {
        state
            .store
            .sequence
            .tracks
            .iter()
            .filter(|track| track.kind == crate::types::UiTrackKind::Text)
            .count()
    };
    state.commit_captions().expect("empty commit");
    assert!(state.store.sequence.markers.is_empty());
    assert_eq!(text_lanes(&state), 0, "an empty commit made a lane");

    state.captions.segments = vec![caption(0, 48, "one", &[]), caption(48, 96, "two", &[])];

    state.set_caption_commit(CaptionCommit::Markers);
    state.commit_captions().expect("markers");
    let names: Vec<_> = state
        .store
        .sequence
        .markers
        .iter()
        .map(|marker| marker.name.clone())
        .collect();
    assert_eq!(names, vec!["one".to_string(), "two".to_string()]);

    state.set_caption_commit(CaptionCommit::TextClips);
    state.commit_captions().expect("clips");
    let captions: Vec<_> = state
        .store
        .sequence
        .tracks
        .iter()
        .filter(|track| track.kind == crate::types::UiTrackKind::Text)
        .flat_map(|track| track.clips.iter())
        .map(|clip| (clip.timeline_start_ticks, clip.timeline_end_ticks))
        .collect();
    assert_eq!(
        captions,
        vec![(0, 48), (48, 96)],
        "caption clips did not land on the transcript's own times"
    );
    // The markers are untouched by the second commit: two targets, not one that
    // replaces the other.
    assert_eq!(state.store.sequence.markers.len(), 2);
}

/// The tab is reachable without knowing it exists.
#[test]
fn the_captions_tab_is_in_the_palette() {
    assert!(
        crate::commands::BUILTIN_COMMANDS
            .iter()
            .any(|command| command.label == "Captions"),
        "the palette has no way to reach captions"
    );
}
