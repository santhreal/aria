//! Ducking music under voice: measure the voice, then write volume keyframes.
//!
//! There is no duck command, and there is deliberately not going to be one. A
//! dip is ordinary automation — [`EditCommand::SetParamKeyframe`] on
//! [`ClipParam::Volume`] — so what ships here is the measurement and the curve,
//! and the result is an envelope that drags, retimes, and undoes like any
//! other. A `DuckLanes` command would have produced automation nobody could edit
//! and a second owner of the volume curve.
//!
//! What this module owns:
//!
//! - **The voice is a flagged lane.** Every audio clip on a `duck_source` lane is
//!   measured; the selection is what gets ducked. Ducking the lane the voice is
//!   on is the one thing that cannot be asked for.
//! - **Silence detection runs backwards.** `aria-analysis` reports where the
//!   voice is *not*; voice spans are the complement, so nothing here reimplements
//!   a threshold.
//! - **The curve is pure.** `duck_curve` is geometry over ticks: no engine, no
//!   decode, no clock. It is the part a test can pin, and the only fixture in
//!   this tree is a silent still image.

use std::path::PathBuf;

use aria_commands::{Command, EditCommand};
use aria_core::{Rational, RationalTime};
use aria_jobs::JobCompletion;
use aria_models::{ClipId, ClipParam, Easing, MAX_CLIP_VOLUME, ParamValue};
use gpui::Context;

use super::AppState;
use super::analysis::{decode_analysis_pcm, ticks_for_seconds, validated_pcm};
use crate::types::UiTrackKind;

/// How deep and how fast a duck dips.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DuckForm {
    /// The share of its own level the music holds while voice plays.
    pub amount: f32,
    /// Seconds the dip takes to arrive, ending where the voice starts.
    pub attack_seconds: f64,
    /// Seconds the dip takes to let go, starting where the voice stops.
    pub release_seconds: f64,
}

impl Default for DuckForm {
    fn default() -> Self {
        // A dip to a third with a fast attack and a slower release is the
        // broadcast default: the music must be out of the way before the first
        // syllable, and coming back up fast sounds like a mistake.
        Self {
            amount: 0.3,
            attack_seconds: 0.15,
            release_seconds: 0.5,
        }
    }
}

impl DuckForm {
    /// Levels the panel offers, as a share of the clip's own volume.
    pub const AMOUNTS: [f32; 5] = [0.1, 0.2, 0.3, 0.5, 0.7];

    /// Ramp lengths the panel offers, in seconds.
    pub const TIMES: [f64; 5] = [0.05, 0.15, 0.25, 0.5, 1.0];

    pub fn amount_label(self) -> String {
        format!("{:.0}%", self.amount * 100.0)
    }

    pub fn attack_label(self) -> String {
        format!("{:.0} ms", self.attack_seconds * 1000.0)
    }

    pub fn release_label(self) -> String {
        format!("{:.0} ms", self.release_seconds * 1000.0)
    }
}

/// The next entry after `current`, wrapping. Comparison is by distance so a value
/// that is not in the list still advances to something sensible.
fn next_choice<T: Copy + Into<f64>>(choices: &[T], current: f64) -> T {
    let nearest = choices
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| {
            let (a, b): (f64, f64) = ((**a).into(), (**b).into());
            (a - current).abs().total_cmp(&(b - current).abs())
        })
        .map_or(0, |(index, _)| index);
    choices[(nearest + 1) % choices.len()]
}

/// One voice clip to measure: where its media is, and how its source is counted.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DuckVoice {
    pub(crate) clip: ClipId,
    pub(crate) path: PathBuf,
    pub(crate) media_rate: Rational,
}

/// Everything a duck needs, resolved before it costs a decode.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DuckRequest {
    /// Every audio clip on a duck-source lane, in timeline order.
    pub(crate) voice: Vec<DuckVoice>,
    /// The selected clips the dip is written onto.
    pub(crate) music: Vec<ClipId>,
    pub(crate) tuning: crate::plugins::DetectorTuning,
    pub(crate) label: String,
}

/// Where the voice is, per voice clip, in that clip's own source ticks.
type VoiceSpans = Vec<(ClipId, Vec<(i64, i64)>)>;

impl AppState {
    pub fn cycle_duck_amount(&mut self) {
        self.duck.amount = next_choice(&DuckForm::AMOUNTS, f64::from(self.duck.amount));
        self.status_message = Some(format!("Duck to {}", self.duck.amount_label()));
    }

    pub fn cycle_duck_attack(&mut self) {
        self.duck.attack_seconds = next_choice(&DuckForm::TIMES, self.duck.attack_seconds);
    }

    pub fn cycle_duck_release(&mut self) {
        self.duck.release_seconds = next_choice(&DuckForm::TIMES, self.duck.release_seconds);
    }

    /// The name of the lane the voice is measured from, when there is one.
    ///
    /// The panel says which lane it will listen to rather than "the duck source":
    /// the flag lives on a lane header two panels away.
    pub fn duck_source_name(&self) -> Option<String> {
        self.store
            .sequence
            .tracks
            .iter()
            .find(|track| track.duck_source)
            .map(|track| track.name.to_string())
    }

    /// Resolve a duck, or name the reason it cannot run.
    ///
    /// Split from the run so every refusal is reachable without a window, and so
    /// the panel can grey the button with the same sentence the button would
    /// have produced.
    pub(crate) fn duck_precheck(&self) -> Result<DuckRequest, String> {
        if self.selection.is_empty() {
            return Err("Select the music clip(s) to duck".into());
        }
        let mut music = Vec::new();
        let mut voice = Vec::new();
        let mut on_the_voice_lane = 0_usize;
        let media_by_id: std::collections::HashMap<_, _> = self
            .store
            .media
            .iter()
            .map(|item| (item.id, item))
            .collect();
        for track in &self.store.sequence.tracks {
            if track.kind != UiTrackKind::Audio {
                continue;
            }
            for clip in &track.clips {
                let selected = self.selection.contains(&ClipId::from_raw(clip.id));
                if !track.duck_source {
                    if selected {
                        music.push(ClipId::from_raw(clip.id));
                    }
                    continue;
                }
                if selected {
                    on_the_voice_lane += 1;
                }
                let Some(media) = clip.media_id.and_then(|id| media_by_id.get(&id).copied()) else {
                    continue;
                };
                if !media.has_audio || media.missing {
                    continue;
                }
                voice.push(DuckVoice {
                    clip: ClipId::from_raw(clip.id),
                    path: PathBuf::from(&*media.path),
                    media_rate: media.frame_rate,
                });
            }
        }
        let Some(lane) = self.duck_source_name() else {
            return Err("Flag the voice lane as the duck source first".into());
        };
        if voice.is_empty() {
            return Err(format!("{lane} has no audio to measure"));
        }
        if music.is_empty() {
            return Err(if on_the_voice_lane > 0 {
                format!("{lane} is the voice. Select the music to duck under it")
            } else {
                "Select an audio clip to duck".into()
            });
        }
        Ok(DuckRequest {
            label: format!("Ducking {} clip(s) under {lane}", music.len()),
            voice,
            music,
            tuning: self
                .ctx
                .try_get::<crate::plugins::DetectorTuning>()
                .map(|tuning| *tuning)
                .unwrap_or_default(),
        })
    }

    /// Measure the voice lane as a job, then write the dip onto the selection.
    pub fn duck_selected(&mut self, cx: &mut Context<Self>) -> Result<(), String> {
        let request = self.duck_precheck()?;
        let (wake, wakes) = crate::handoff::wakes::<Result<VoiceSpans, String>>();
        let label = request.label.clone();
        let music = request.music.clone();
        let form = self.duck;
        self.spawn_job(label, move |context| {
            let outcome = measure_voice(context, &request);
            let reported = match (&outcome, context.cancelled()) {
                (_, true) => Err(String::new()),
                (Ok(spans), false) => Ok(spans.clone()),
                (Err(err), false) => Err(err.clone()),
            };
            wake.wake(reported);
            let spans: usize = outcome?.iter().map(|(_, spans)| spans.len()).sum();
            JobCompletion::new(format!("{spans} voice span(s)"))
                .with_output("spans", spans.to_string())
                .map_err(String::from)
        });

        cx.spawn(async move |this, cx| {
            let outcome = wakes.next_from_a_thread(cx.background_executor()).await;
            let _ = this.update(cx, |state, cx| {
                match outcome {
                    Some(Ok(spans)) => state.apply_duck(&spans, &music, form),
                    Some(Err(reason)) if reason.is_empty() => {
                        state.status_message = Some("Cancelled the duck".into());
                    }
                    Some(Err(reason)) => state.report_error(reason),
                    None => state.report_error("the duck did not report"),
                }
                cx.notify();
            });
        })
        .detach();
        self.status_message = Some("Measuring the voice…".into());
        Ok(())
    }

    /// Write one dip per music clip as a single undo entry.
    ///
    /// The whole duck is one step: a run that touched four clips must come back
    /// in one press, or undoing a bad amount walks back through dozens of
    /// keyframes.
    fn apply_duck(&mut self, voice: &VoiceSpans, music: &[ClipId], form: DuckForm) {
        let rate = self.playhead.rate;
        let attack = ticks_for_seconds(form.attack_seconds, rate);
        let release = ticks_for_seconds(form.release_seconds, rate);
        let spans = self.voice_timeline_spans(voice);
        if spans.is_empty() {
            self.status_message = Some("No voice found on the duck source lane".into());
            return;
        }

        let mut plans = Vec::with_capacity(music.len());
        {
            let engine = self.engine.lock();
            let project = engine.project();
            for id in music {
                let Some(clip) = project.clip(*id) else {
                    continue;
                };
                // The un-ducked level is the top of whatever is there now, so a
                // second run with a different amount re-dips from the same
                // level instead of ducking the dip.
                let base = clip
                    .volume
                    .keyframes()
                    .iter()
                    .map(|key| key.value)
                    .fold(clip.volume.constant().unwrap_or(1.0), f32::max);
                let window = (
                    clip.timeline.start.value,
                    clip.timeline.start.value + clip.timeline.duration.value,
                );
                let points = duck_curve(
                    &spans,
                    window,
                    base,
                    (base * form.amount).clamp(0.0, MAX_CLIP_VOLUME),
                    attack,
                    release,
                );
                if !points.is_empty() {
                    plans.push((*id, base, clip.volume.is_animated(), points));
                }
            }
        }
        if plans.is_empty() {
            self.status_message = Some("No voice overlaps the selected clip(s)".into());
            return;
        }

        let clips = plans.len();
        let written: usize = plans.iter().map(|(_, _, _, points)| points.len()).sum();
        // A half-written envelope is worse than no duck: the clip would dip and
        // never come back up.
        match self.one_step(|this| this.duck_steps(plans, rate)) {
            Ok(()) => {
                self.status_message = Some(format!(
                    "Ducked {clips} clip(s) with {written} keyframe(s) under {} voice span(s)",
                    spans.len()
                ));
            }
            Err(err) => self.report_error(&err),
        }
    }

    /// Every command one duck is made of, so the group above is one match.
    fn duck_steps(
        &mut self,
        plans: Vec<(ClipId, f32, bool, Vec<(i64, f32)>)>,
        rate: Rational,
    ) -> Result<(), aria_engine::EngineError> {
        for (clip, base, animated, points) in plans {
            // An existing envelope is replaced rather than merged with: two
            // ducks written over each other multiply, and the request was
            // for this dip, not for the product of every dip so far.
            if animated {
                self.apply_command(Command::Edit(EditCommand::SetParamConstant {
                    clip,
                    param: ClipParam::Volume,
                    value: ParamValue::Scalar(base),
                }))?;
            }
            for (tick, value) in points {
                self.apply_command(Command::Edit(EditCommand::SetParamKeyframe {
                    clip,
                    param: ClipParam::Volume,
                    at: RationalTime::new(tick, rate),
                    value: ParamValue::Scalar(value),
                    easing: Easing::Linear,
                    tangents: None,
                }))?;
            }
        }
        Ok(())
    }

    /// Voice spans in timeline ticks, merged across every voice clip.
    ///
    /// Each clip maps its own source spans through its own geometry — a trimmed
    /// or retimed voice clip shows a different part of the file — which is why
    /// the mapping happens per clip and the union happens after.
    fn voice_timeline_spans(&self, voice: &VoiceSpans) -> Vec<(i64, i64)> {
        let engine = self.engine.lock();
        let project = engine.project();
        let mut spans = Vec::new();
        for (id, source_spans) in voice {
            let Some(clip) = project.clip(*id) else {
                continue;
            };
            for (start, end) in source_spans {
                if let Some(span) = clip.timeline_span_for_source(*start, *end) {
                    spans.push(span);
                }
            }
        }
        spans.sort_unstable();
        spans
    }
}

/// Decode every voice clip's media and report where the voice is, in that clip's
/// own source ticks.
fn measure_voice(
    context: &aria_jobs::JobContext,
    request: &DuckRequest,
) -> Result<VoiceSpans, String> {
    let mut out = Vec::with_capacity(request.voice.len());
    let total = request.voice.len().max(1) as f32;
    for (index, entry) in request.voice.iter().enumerate() {
        context.set_progress(index as f32 / total, "decoding the voice");
        let pcm = decode_analysis_pcm(context, &entry.path)?;
        let view = validated_pcm(&pcm)?;
        let seconds = aria_detect::seconds_of(&pcm);
        let silence = view.detect_silence(request.tuning.silence);
        let quiet: Vec<(f64, f64)> = silence
            .iter()
            .map(|span| (span.span.start_seconds(), span.span.end_seconds()))
            .collect();
        out.push((entry.clip, voice_spans(&quiet, seconds, entry.media_rate)));
        if context.cancelled() {
            return Err("cancelled".into());
        }
    }
    context.set_progress(1.0, "measured");
    Ok(out)
}

/// Invert silence into voice, in source ticks at the media's own rate.
///
/// The detector reports where nothing is, because that is the cheap measurement;
/// the complement over the decoded length is where the voice is. A file that is
/// silent end to end inverts to nothing, which is the honest answer rather than
/// one span covering everything.
pub(crate) fn voice_spans(silence: &[(f64, f64)], seconds: f64, rate: Rational) -> Vec<(i64, i64)> {
    let mut quiet: Vec<(f64, f64)> = silence.to_vec();
    quiet.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut spans = Vec::new();
    let mut cursor = 0.0_f64;
    for (start, end) in quiet {
        if start > cursor {
            spans.push((cursor, start));
        }
        cursor = cursor.max(end);
    }
    if cursor < seconds {
        spans.push((cursor, seconds));
    }
    spans
        .into_iter()
        .filter_map(|(start, end)| {
            let (start, end) = (ticks_for_seconds(start, rate), ticks_for_seconds(end, rate));
            (end > start).then_some((start, end))
        })
        .collect()
}

/// The volume envelope one dip needs, as `(timeline tick, value)` pairs.
///
/// Pure geometry, and the whole reason a duck is testable in a tree whose only
/// audio fixture is a silent still: spans in, keyframes out.
///
/// Every span becomes four points — base before the attack, the dip at the voice,
/// the dip held to the end of it, base again after the release. Two rules make
/// the result an envelope rather than a pile of keyframes:
///
/// - **Spans too close together are merged.** With less than `release + attack`
///   between them there is no room to come back up and dip again, and writing
///   both would raise the level between two sentences.
/// - **A point outside the clip clamps inward, and the deeper value wins a tie.**
///   Voice that starts before the clip does means the clip opens already ducked;
///   the alternative is a keyframe the engine would reject and a dip that never
///   arrives.
pub(crate) fn duck_curve(
    voice: &[(i64, i64)],
    window: (i64, i64),
    base: f32,
    dip: f32,
    attack: i64,
    release: i64,
) -> Vec<(i64, f32)> {
    let (start, end) = window;
    let last = end - 1;
    if last < start || voice.is_empty() {
        return Vec::new();
    }
    let mut inside: Vec<(i64, i64)> = voice
        .iter()
        .map(|(from, to)| ((*from).max(start), (*to).min(end)))
        .filter(|(from, to)| to > from)
        .collect();
    inside.sort_unstable();

    let mut merged: Vec<(i64, i64)> = Vec::with_capacity(inside.len());
    for (from, to) in inside {
        match merged.last_mut() {
            Some(previous) if from <= previous.1 + release + attack => {
                previous.1 = previous.1.max(to);
            }
            _ => merged.push((from, to)),
        }
    }

    let mut points: Vec<(i64, f32)> = Vec::with_capacity(merged.len() * 4);
    for (from, to) in merged {
        points.push(((from - attack).clamp(start, last), base));
        points.push((from.clamp(start, last), dip));
        points.push((to.clamp(start, last), dip));
        points.push(((to + release).clamp(start, last), base));
    }
    // Lower value first, so a tie between a clamped base and a dip keeps the dip.
    points.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
    points.dedup_by_key(|point| point.0);
    points
}
