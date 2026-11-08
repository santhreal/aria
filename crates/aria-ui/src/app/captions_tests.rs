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

use aria_commands::{Command, EditCommand};
use aria_core::Rational;
use aria_models::{Clip, MediaId, TimeRange};
use aria_transcription::{WHISPER_SAMPLE_RATE, WhisperModel};

use super::captions::{
    CaptionCommit, THREAD_CHOICES, UiCaption, UiCaptionWord, caption_span, sample_at,
};
use super::models::TimeoutField;
use super::premiere_tests::{with_clip, with_media};

/// A sound file's source window `from..from + length` milliseconds played over
/// `ticks` of a 24 fps sequence from tick 100.
fn placed(from: i64, length: i64, ticks: i64, reversed: bool) -> Clip {
    let mut clip = Clip::from_media(
        MediaId::from_raw(1),
        TimeRange::at_rate(from, length, Rational::new(1000, 1)),
        TimeRange::at_rate(100, ticks, Rational::new(24, 1)),
    );
    clip.reversed = reversed;
    clip
}

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
    let rate = WHISPER_SAMPLE_RATE as usize;
    assert_eq!(sample_at(1.0, f64::floor), rate);
    assert_eq!(sample_at(0.0, f64::ceil), 0);
    // An out point rounds up so the tail of a clip is decoded rather than
    // clipped; an in point rounds down onto the sample that is playing.
    assert_eq!(sample_at(1.0 / 24.0, f64::ceil), 667);
    assert_eq!(sample_at(1.0 / 24.0, f64::floor), 666);
    // A negative or non-finite time cannot name audio, and must not wrap into
    // a huge allocation.
    assert_eq!(sample_at(-5.0, f64::floor), 0);
    assert_eq!(sample_at(f64::NAN, f64::ceil), 0);

    // Two seconds of a sound file played over two seconds of the sequence.
    let even = placed(0, 2000, 48, false);
    assert_eq!(caption_span(&even, 0, 100), (100, 124));
    assert_eq!(caption_span(&even, 50, 100), (112, 124));
    // A stretch too short to cover a tick lands on the tick it starts on.
    assert_eq!(caption_span(&even, 3, 3), (101, 101));
    // A time past the window lands on the clip's end, never past it.
    assert_eq!(caption_span(&even, 150, 900), (136, 148));
    assert_eq!(caption_span(&even, 300, 400), (148, 148));
    // A window trimmed half a second in starts at its own in point.
    assert_eq!(
        caption_span(&placed(500, 1500, 36, false), 0, 100),
        (100, 124)
    );
    // Played at twice the speed, a second of source is half a second of timeline.
    assert_eq!(
        caption_span(&placed(0, 2000, 24, false), 100, 200),
        (112, 124)
    );
    // Reversed, the window's start plays last.
    assert_eq!(caption_span(&placed(0, 2000, 48, true), 0, 50), (136, 148));
}

/// A run decodes the clip's own source window, counted in seconds of source.
///
/// WHY: the window was read from the clip's source ticks as if they were
/// sequence frames. A sound file counts a thousand ticks a second, so a
/// two-second clip asked for over a minute of audio and a clip trimmed half a
/// second in started its window past the end of the file, and the run failed
/// with "the clip covers no decoded audio"; captions were placed at sequence
/// frames counted from the clip's start, past where a retimed clip plays them.
/// Covered: a sound file at a 1000:24 source-to-sequence ratio, untrimmed and
/// trimmed at the head, with a line placed through the trimmed clip, and a
/// video whose rate is the sequence's. Not covered: Whisper's output on the
/// window, and a speed ramp, which the clip mapping approximates.
#[test]
fn a_run_decodes_the_clips_own_source_window_at_any_source_rate() {
    let fixtures =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures");
    let model = (WhisperModel::catalog()[0], fixtures.join("no-model.bin"));
    let window = |state: &mut super::AppState| {
        state.settings_form.installed_model = Some(model.clone());
        let request = state
            .transcription_precheck()
            .unwrap_or_else(|err| panic!("the run was refused: {err}"));
        let source = request.clip.source_range().expect("a media clip");
        (
            request.start_frame,
            request.frames,
            request.clip.timeline.duration.value,
            source.duration.seconds(),
        )
    };
    let rate = WHISPER_SAMPLE_RATE as usize;

    let (mut sound, clip) = with_media(fixtures.join("fixture-sine-48k-2s.wav"));
    let at = sound.playhead.rate;
    assert_eq!(
        at,
        aria_core::Rational::new(24, 1),
        "a head trim of 12 ticks below is half a second of a 24 fps sequence"
    );
    let (start, frames, ticks, seconds) = window(&mut sound);
    assert!(
        (1.9..=2.1).contains(&seconds),
        "the whole sound file is its two seconds, not {seconds} s"
    );
    assert_eq!(
        (start, frames, ticks),
        (
            0,
            sample_at(seconds, f64::ceil),
            (seconds * 24.0).round() as i64
        ),
        "the whole sound file"
    );
    sound
        .apply_command(Command::Edit(EditCommand::TrimClip {
            clip,
            timeline: aria_models::TimeRange::at_rate(12, ticks - 12, at),
        }))
        .expect("trim the head off the sound clip");
    sound.sync_from_engine();
    let (start, trimmed, short, rest) = window(&mut sound);
    assert_eq!(
        (start, trimmed, short),
        (rate / 2, frames - rate / 2, ticks - 12),
        "the sound clip trimmed half a second in"
    );
    assert!(
        (rest - (seconds - 0.5)).abs() < 1e-9,
        "{rest} s left of {seconds}"
    );
    // A line over the trimmed window's first second plays over the clip's
    // first second: ticks 12 to 36.
    let request = sound
        .transcription_precheck()
        .expect("the trimmed clip runs");
    assert_eq!(caption_span(&request.clip, 0, 100), (12, 36));

    let (mut video, _) = with_media(fixtures.join("fixture-640x360-24fps-2s-av.mp4"));
    let (start, frames, ticks, seconds) = window(&mut video);
    assert_eq!(
        (start, frames, ticks),
        (
            0,
            sample_at(seconds, f64::ceil),
            (seconds * 24.0).round() as i64
        ),
        "the video clip"
    );
}

/// Every model in the catalog is listed with a size, a description, and a state, and a
/// fresh session has none of them installed. Enumerated from the catalog so a new model
/// fails here rather than being silently unreachable.
#[test]
fn every_cataloged_model_is_listed_and_starts_uninstalled() {
    let (state, _) = with_clip();
    let rows = state.model_rows();
    assert_eq!(rows.len(), WhisperModel::catalog().len());
    assert!(!rows.is_empty(), "the catalog offers no model at all");
    for row in &rows {
        assert!(row.bytes > 0, "{} has no cataloged size", row.id);
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
