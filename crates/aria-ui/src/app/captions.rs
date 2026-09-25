//! Transcription: the model on disk, a run over the selected clip's audio, and the
//! commit that turns a transcript into project state.
//!
//! `aria-transcription` owns Whisper and the model store; nothing here reimplements a
//! download, a hash, or an inference loop. What this module owns:
//!
//! - **The model store is [`super::models`]'s**, not this module's.
//! - **Transcription runs off decoded audio**, bounded and cancellable, on the job's
//!   thread. The window comes from the clip's source range, so a trimmed clip is
//!   transcribed over what it actually shows.
//! - **A refusal is decided before anything costs time** (`AppState::transcription_precheck`),
//!   so every reason a run cannot start is reachable without a window.
//! - **Granularity is a projection, not a re-run.** Word timings come back with the
//!   segments, so switching between per-segment and per-word captions costs nothing.

use std::path::PathBuf;
use std::sync::Arc;

use aria_analysis::moments::{AnalyzerIdentity, AnalyzerVersion, MediaContentKey, MomentsIndex};
use aria_commands::{Command, EditCommand};
use aria_core::Rational;
use aria_engine::EngineError;
use aria_jobs::{CancellationToken, JobCompletion};
use aria_models::{Generator, RationalTime, TimeRange, TrackKind};
use aria_transcription::{
    CancellationCheck, Transcript, TranscriptionOptions, WHISPER_SAMPLE_RATE, transcribe_pcm,
    transcript_to_moment_batch,
};
use gpui::Context;

use super::AppState;
use gpui::SharedString;

/// Longest audio one run accepts: 30 minutes of 16 kHz mono.
///
/// A bound rather than none: the decoder allocates the whole stream, and an hours-long
/// file would be a multi-gigabyte allocation decided by whatever was imported.
pub const MAX_PCM_FRAMES: usize = WHISPER_SAMPLE_RATE as usize * 60 * 30;

/// Thread counts offered for a run. `TranscriptionOptions` accepts `1..=64`; these are
/// the choices worth a chip, and the validator still has the final say.
pub const THREAD_CHOICES: [usize; 4] = [1, 2, 4, 8];

/// The storage identity of a transcript in the moments index.
///
/// A transcript is analysis of the media, so it is keyed on the media's content, not on
/// the project: the same file transcribed in two projects has one batch, and a later
/// query by kind or confidence finds it without re-running Whisper.
pub const TRANSCRIPT_ANALYZER: &str = "aria.transcription";

/// The transcript result schema version. Bump it when the payload meaning changes.
pub const TRANSCRIPT_ANALYZER_VERSION: u32 = 1;

/// What a committed transcript becomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CaptionCommit {
    /// One text clip per caption on the text lane.
    #[default]
    TextClips,
    /// One timeline marker per caption.
    Markers,
}

impl CaptionCommit {
    /// The targets, as the panel's chips. A data table so a new target shows up as a
    /// missing row here rather than as one nobody can choose.
    pub const ALL: [(Self, &'static str, &'static str); 2] = [
        (Self::TextClips, "commit-clips", "Text clips"),
        (Self::Markers, "commit-markers", "Markers"),
    ];
}

/// What the Captions panel is holding.
#[derive(Debug, Clone, Default)]
pub struct CaptionsForm {
    /// Whisper language code, or empty for automatic detection.
    pub language: String,
    /// Whether typing goes to the language box.
    pub language_focused: bool,
    /// Whether a commit writes one unit per word instead of per segment.
    pub word_level: bool,
    /// Decoder threads. Zero means "the default for this machine".
    pub threads: usize,
    /// What a commit writes.
    pub commit: CaptionCommit,
    /// The last result, in timeline ticks.
    pub segments: Vec<UiCaption>,
}

impl CaptionsForm {
    /// Whether a commit writes this segment as its words rather than as one
    /// line.
    ///
    /// A segment whose model returned no word timings stays one unit even in
    /// word mode: dropping it would silently lose speech. One owner for that
    /// rule, because both the commit and the count the panel draws ask it.
    pub fn splits_into_words(&self, segment: &UiCaption) -> bool {
        self.word_level && !segment.words.is_empty()
    }
}

/// One transcript segment, already placed on the timeline.
#[derive(Debug, Clone, PartialEq)]
pub struct UiCaption {
    pub start_tick: i64,
    pub end_tick: i64,
    pub text: SharedString,
    /// How sure the model was, as the mean of this segment's word
    /// probabilities, or `None` when the run returned no word timings.
    ///
    /// `None` is not zero. A run with token timestamps off says nothing about
    /// certainty, and a readout that painted that as 0% would tell the operator
    /// to re-record a line the model heard perfectly well. The two states are
    /// distinct on the row too: a number, or no number.
    ///
    /// Read from `TranscriptSegment::mean_word_probability`, which is also what
    /// the moments index stores as the segment's confidence — the readout and
    /// the saved analysis cannot disagree.
    pub confidence: Option<f32>,
    /// Word timings, empty when the model returned none.
    pub words: Vec<UiCaptionWord>,
}

/// One word inside a caption.
#[derive(Debug, Clone, PartialEq)]
pub struct UiCaptionWord {
    pub start_tick: i64,
    pub end_tick: i64,
    pub text: SharedString,
    /// The model's probability for this word, `0.0..=1.0`.
    ///
    /// Always a real number here: a word only exists because the model emitted
    /// a token for it, and every token carries a probability.
    pub confidence: f32,
}

/// Everything a run needs, resolved before it costs anything.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TranscribeRequest {
    pub model_path: PathBuf,
    pub media_path: PathBuf,
    /// Sample offset into the decoded stream (the clip's source in point).
    pub start_frame: usize,
    /// Sample count (the clip's source duration), capped by [`MAX_PCM_FRAMES`].
    pub frames: usize,
    pub options: TranscriptionOptions,
    /// Where the clip sits on the timeline, so a caption lands under it.
    pub clip_start_tick: i64,
    pub rate: Rational,
    /// The media's content key, so the transcript joins the same index the detectors
    /// write to.
    pub content_key: MediaContentKey,
    pub label: String,
}

impl AppState {
    /// The options a run would use, or why they are refused.
    ///
    /// The validator in `aria-transcription` is the only authority: the language box is
    /// free text, and a code Whisper does not know has to be reported by name rather
    /// than silently detected.
    pub fn transcription_options(&self) -> Result<TranscriptionOptions, String> {
        let mut options = TranscriptionOptions {
            // Word timings always come back, so per-word captions never need a re-run.
            token_timestamps: true,
            ..TranscriptionOptions::default()
        };
        let language = self.captions.language.trim();
        if !language.is_empty() {
            options.language = Some(language.to_string());
        }
        if self.captions.threads > 0 {
            options.thread_count = self.captions.threads;
        }
        options.validate().map_err(|err| err.to_string())?;
        Ok(options)
    }

    /// Everything that decides whether a run may start.
    ///
    /// Split from [`Self::transcribe_selected`] so every refusal is testable without a
    /// window: they are the whole reason nothing is queued.
    pub(crate) fn transcription_precheck(&self) -> Result<TranscribeRequest, String> {
        let clip = self
            .selected_ui_clip()
            .ok_or("Select a clip with audio to transcribe")?;
        let media_id = clip
            .media_id
            .ok_or("A generated clip has no audio to transcribe")?;
        let media = self
            .store
            .media
            .iter()
            .find(|item| item.id == media_id)
            .ok_or("The clip's media is no longer in the pool")?;
        if !media.has_audio {
            return Err(format!("{} has no audio track", media.name));
        }
        if media.missing {
            return Err(format!("{} is offline. Relink it first", media.name));
        }
        let (_, model_path) = self
            .installed_model()
            .ok_or("Install a transcription model in Settings first")?;
        let options = self.transcription_options()?;

        let content_key = super::analysis::content_key_for(std::path::Path::new(&*media.path))?;
        let rate = self.playhead.rate;
        let start_frame = frames_for_ticks(clip.source_start_ticks.max(0), rate);
        let span = clip
            .source_end_ticks
            .saturating_sub(clip.source_start_ticks)
            .max(0);
        let frames = frames_for_ticks(span, rate).min(MAX_PCM_FRAMES);
        if frames == 0 {
            return Err("The clip is too short to transcribe".into());
        }
        Ok(TranscribeRequest {
            model_path,
            media_path: PathBuf::from(&*media.path),
            start_frame,
            frames,
            options,
            clip_start_tick: clip.timeline_start_ticks,
            rate,
            content_key,
            label: format!("Transcribing {}", media.name),
        })
    }

    /// Run transcription as a job and adopt the result into the panel.
    pub fn transcribe_selected(&mut self, cx: &mut Context<Self>) -> Result<(), String> {
        let request = self.transcription_precheck()?;
        let index = self.analysis_index()?;
        let (wake, wakes) = crate::handoff::wakes::<Result<Vec<UiCaption>, String>>();
        let label = request.label.clone();
        self.spawn_job(label, move |context| {
            let outcome = run_transcription(context, &request, &index);
            let reported = match (&outcome, context.cancelled()) {
                (_, true) => Err(String::new()),
                (Ok(captions), false) => Ok(captions.clone()),
                (Err(err), false) => Err(err.clone()),
            };
            wake.wake(reported);
            let captions = outcome?;
            JobCompletion::new(format!("{} segment(s)", captions.len()))
                .with_output("segments", captions.len().to_string())
                .map_err(String::from)
        });

        cx.spawn(async move |this, cx| {
            let outcome = wakes.next_from_a_thread(cx.background_executor()).await;
            let _ = this.update(cx, |state, cx| {
                match outcome {
                    Some(Ok(captions)) => {
                        state.status_message =
                            Some(format!("Transcribed {} segment(s)", captions.len()));
                        state.captions.segments = captions;
                    }
                    Some(Err(reason)) if reason.is_empty() => {
                        state.status_message = Some("Cancelled the transcription".into());
                    }
                    Some(Err(reason)) => state.report_error(reason),
                    None => state.report_error("the transcription did not report"),
                }
                cx.notify();
            });
        })
        .detach();
        self.status_message = Some("Transcribing…".into());
        Ok(())
    }

    /// The units a commit would write, honouring the per-word switch.
    ///
    /// A segment whose model returned no word timings stays one unit: dropping it
    /// because word mode is on would silently lose speech. The text is owned here
    /// because a unit becomes a command argument, and the model owns its own text.
    pub fn caption_units(&self) -> Vec<(i64, i64, String)> {
        let mut units = Vec::new();
        for segment in &self.captions.segments {
            if self.captions.splits_into_words(segment) {
                units.extend(
                    segment
                        .words
                        .iter()
                        .map(|word| (word.start_tick, word.end_tick, word.text.to_string())),
                );
            } else {
                units.push((
                    segment.start_tick,
                    segment.end_tick,
                    segment.text.to_string(),
                ));
            }
        }
        units
    }

    /// How many units a commit would write.
    ///
    /// The panel needs this every frame — it decides whether Commit is offered
    /// and what the hint line says — and the units themselves are only needed
    /// when a commit runs, where each one costs an owned string. Counting reads
    /// the same rule [`Self::caption_units`] writes by, so the two cannot
    /// disagree about what word mode does to a segment with no word timings.
    pub fn caption_unit_count(&self) -> usize {
        self.captions
            .segments
            .iter()
            .map(|segment| {
                if self.captions.splits_into_words(segment) {
                    segment.words.len()
                } else {
                    1
                }
            })
            .sum()
    }

    /// Write the transcript into the project.
    ///
    /// Each unit is its own command, so each is its own undo entry: the engine has no
    /// batch, and inventing one here would put a second history model in the surface.
    pub fn commit_captions(&mut self) -> Result<(), EngineError> {
        let units = self.caption_units();
        if units.is_empty() {
            self.status_message = Some("Nothing to commit. Transcribe first".into());
            return Ok(());
        }
        let target = self.captions.commit;
        let rate = self.playhead.rate;
        let written = units.len();
        match target {
            CaptionCommit::Markers => {
                for (start, _, text) in units {
                    let color = self.next_marker_color();
                    self.apply_command(Command::Edit(EditCommand::AddMarker {
                        at: RationalTime::new(start.max(0), rate),
                        name: text,
                        color: Some(color),
                    }))?;
                }
            }
            CaptionCommit::TextClips => {
                let track = self.ensure_kind_track(TrackKind::Text)?;
                for (start, end, text) in units {
                    let duration = end.saturating_sub(start).max(1);
                    self.apply_command(Command::Edit(EditCommand::AddGenerated {
                        track,
                        generator: Generator::text(text),
                        timeline: TimeRange::at_rate(start.max(0), duration, rate),
                    }))?;
                }
            }
        }
        self.status_message = Some(match target {
            CaptionCommit::Markers => format!("Wrote {written} marker(s)"),
            CaptionCommit::TextClips => format!("Wrote {written} caption clip(s)"),
        });
        Ok(())
    }

    pub fn focus_caption_language(&mut self, focused: bool) {
        self.captions.language_focused = focused;
    }

    pub fn caption_language_push(&mut self, ch: char) {
        if self.captions.language_focused {
            self.captions.language.push(ch);
        }
    }

    pub fn caption_language_pop(&mut self) {
        if self.captions.language_focused {
            self.captions.language.pop();
        }
    }

    pub fn set_caption_word_level(&mut self, word_level: bool) {
        self.captions.word_level = word_level;
    }

    pub fn set_caption_commit(&mut self, commit: CaptionCommit) {
        self.captions.commit = commit;
    }

    pub fn set_caption_threads(&mut self, threads: usize) {
        self.captions.threads = threads;
    }
}

/// Decode the clip's audio, then transcribe it.
///
/// Split out of the job closure so the whole flow — decode window, cancellation, and
/// projection — reads as one piece.
fn run_transcription(
    context: &aria_jobs::JobContext,
    request: &TranscribeRequest,
    index: &MomentsIndex,
) -> Result<Vec<UiCaption>, String> {
    context.set_progress(0.0, "decoding audio");
    let wanted = request.start_frame.saturating_add(request.frames);
    let pcm = aria_decoder::read_mono_pcm_with_cancel(
        &request.media_path,
        WHISPER_SAMPLE_RATE,
        wanted.min(MAX_PCM_FRAMES).max(1),
        || context.cancelled(),
    )
    .map_err(|err| err.to_string())?;
    if context.cancelled() {
        return Err("cancelled".into());
    }
    let start = request.start_frame.min(pcm.len());
    let end = wanted.min(pcm.len());
    let window = &pcm[start..end];
    if window.is_empty() {
        return Err("the clip covers no decoded audio".into());
    }

    context.set_progress(0.5, "transcribing");
    let transcript = transcribe_pcm(
        &request.model_path,
        window,
        &request.options,
        Arc::new(JobCancellation(context.cancellation_token())),
    )
    .map_err(|err| err.to_string())?;
    context.set_progress(0.9, "storing the transcript");
    // Storing the transcript is the irreversible step: the segments the panel lists are
    // a projection of what landed in the index, so a cancel accepted after this point
    // would leave the two disagreeing.
    super::jobs::CommitGuard::new(context, "the transcript").begin()?;
    store_transcript(index, request.content_key, &transcript)?;
    context.set_progress(1.0, "done");
    Ok(project_transcript(
        &transcript,
        request.clip_start_tick,
        request.rate,
    ))
}

/// Persist the transcript as one analyzer batch.
///
/// A failure here fails the run: a transcript the index silently dropped is one the
/// operator would keep re-measuring, and the sentence names the index so the cause is
/// the disk rather than the model.
fn store_transcript(
    index: &MomentsIndex,
    content_key: MediaContentKey,
    transcript: &Transcript,
) -> Result<(), String> {
    let identity = AnalyzerIdentity::new(TRANSCRIPT_ANALYZER)
        .map_err(|err| format!("transcript analyzer identity: {err}"))?;
    let version = AnalyzerVersion::new(TRANSCRIPT_ANALYZER_VERSION)
        .map_err(|err| format!("transcript analyzer version: {err}"))?;
    let batch = transcript_to_moment_batch(content_key, identity, version, transcript)
        .map_err(|err| format!("transcript moments: {err}"))?;
    index
        .replace_batch(&batch)
        .map_err(|err| format!("writing the moments index: {err}"))
}

/// Place a transcript on the timeline: centisecond offsets become ticks under the clip.
fn project_transcript(
    transcript: &Transcript,
    clip_start_tick: i64,
    rate: Rational,
) -> Vec<UiCaption> {
    transcript
        .segments()
        .iter()
        .map(|segment| UiCaption {
            start_tick: clip_start_tick
                + ticks_for_centiseconds(segment.start_centiseconds(), rate),
            end_tick: clip_start_tick + ticks_for_centiseconds(segment.end_centiseconds(), rate),
            text: segment.text().trim().to_string().into(),
            confidence: segment.mean_word_probability(),
            words: segment
                .words()
                .iter()
                .map(|word| UiCaptionWord {
                    start_tick: clip_start_tick
                        + ticks_for_centiseconds(word.start_centiseconds(), rate),
                    end_tick: clip_start_tick
                        + ticks_for_centiseconds(word.end_centiseconds(), rate),
                    text: word.text().trim().to_string().into(),
                    confidence: word.probability(),
                })
                .filter(|word| !word.text.is_empty())
                .collect(),
        })
        .filter(|caption| !caption.text.is_empty())
        .collect()
}

/// Sample frames covering `ticks` at the timeline rate, rounded up so a partial frame
/// is still decoded rather than clipped off the end of a caption.
pub(crate) fn frames_for_ticks(ticks: i64, rate: Rational) -> usize {
    let num = i64::from(rate.num.max(1));
    let den = i64::from(rate.den.max(1));
    let rate_hz = i64::from(WHISPER_SAMPLE_RATE);
    let frames = (ticks.max(0).saturating_mul(den).saturating_mul(rate_hz) + num - 1) / num;
    usize::try_from(frames).unwrap_or(usize::MAX)
}

/// Ticks covering a centisecond offset at the timeline rate.
pub(crate) fn ticks_for_centiseconds(centiseconds: u64, rate: Rational) -> i64 {
    let num = i64::from(rate.num.max(1));
    let den = i64::from(rate.den.max(1));
    let cs = i64::try_from(centiseconds).unwrap_or(i64::MAX);
    cs.saturating_mul(num) / (den.saturating_mul(100))
}

/// The job's cancellation flag, as the transcriber's trait.
struct JobCancellation(CancellationToken);

impl CancellationCheck for JobCancellation {
    fn is_cancelled(&self) -> bool {
        self.0.cancelled()
    }
}
