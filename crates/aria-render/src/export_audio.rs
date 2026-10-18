//! Export-side audio: collect every audible clip and mix it, streamed, into
//! interleaved stereo `f32` blocks for the encoder's AAC track.
//!
//! Decodes straight from the original source files (same rule as export video:
//! no cache, no proxies). The mix policy is the MVP subset of the desktop
//! mixer — sum overlapping spans, apply per-sample volume + fades + pan, clamp
//! to `[-1, 1]`, silence where nothing is audible. It is fail-loud: a source
//! that can't be opened or read aborts the export.
//!
//! Pan uses the industry-standard constant-power law scaled to unity at
//! center (`0 dB` center, `+3 dB` at the hard-left / hard-right edges):
//! `angle = (pan + 1) · π/4`, then `left = cos(angle)·√2`,
//! `right = sin(angle)·√2`. Extreme pan + full volume can therefore exceed
//! unity before the mixer's final `[-1, 1]` clamp — acceptable clipping at
//! the edges. For stereo sources this is a *balance* (same L/R multipliers),
//! not a true stereo pan that redistributes mid/side energy; mono sources
//! (identical L/R after decode) are distributed across the field.
//!
//! Retimed clips (speed / speed-curve ramps) hold their pitch: the source is
//! read at unit rate inside each overlap-add frame and the frames are laid down
//! at the retimed spacing (WSOLA), so a 2× clip is twice as fast at the same
//! pitch. Clearing a clip's pitch lock mixes it varispeed instead, where pitch
//! follows rate. Denoise-flagged clips run through RNNoise before mixing.
//! Reversed clips walk the source window backward (same mapping as video
//! [`aria_models::Clip::source_time_at`]) by resampling a decoded window.
//!
//! Preview (desktop + mobile) and export share this mixer: both open an
//! [`ExportAudioMixer`] over a project snapshot.
//!
//! The export loop drives [`ExportAudioMixer::mix_into`] with monotonically
//! advancing timeline positions (one block per video frame). Unity-speed
//! forward spans seek once and stream; reversed and retimed spans resample
//! a source window.

use std::f32::consts::{FRAC_PI_4, SQRT_2};
use std::path::PathBuf;

use aria_core::{AudioReader, DecodeError};
use aria_models::{
    AudioEffectInstance, AutomationMode, ClipParam, ModelError, Param, Project, Rational, TrackId,
    audio_gain_at,
};

use crate::driver::driven_envelope;
use crate::error::RenderError;

use crate::audio_dsp::{
    DenoiseReader, OLA_FRAME, OLA_HOP, OLA_PAD, OLA_WARMUP, SpanWarp, ola_aligned_offset,
    ola_frames, ola_window, warped_source_frame,
};

/// Export audio sample rate: the broadcast/web standard for video files.
pub const EXPORT_AUDIO_RATE: u32 = 48_000;
/// Export channel count (interleaved stereo).
pub const EXPORT_AUDIO_CHANNELS: u16 = 2;
/// Highest shuttle multiple (`|speed| <= 4x`) at which preview audio plays.
const MAX_SHUTTLE_AUDIO_SPEED: i32 = 4;
const CHANNELS: usize = EXPORT_AUDIO_CHANNELS as usize;

/// One audible clip resolved to output sample frames at [`EXPORT_AUDIO_RATE`].
///
/// Visible to this module's children because the mixing arithmetic lives in a
/// sibling file: the type stays inside the crate.
pub struct Span {
    pub path: PathBuf,
    /// Timeline placement in output sample frames.
    pub start: i64,
    pub end: i64,
    /// Start of the source window in output sample frames.
    pub source_start: i64,
    /// Length of the source window in output sample frames.
    pub source_len: i64,
    /// Walk the source window backward (video `Clip::reversed`).
    pub reversed: bool,
    /// How output samples map into the source window.
    pub warp: SpanWarp,
    /// Hold the source's pitch while retiming it (`Clip::preserve_pitch`).
    /// Ignored at unity speed, where there is nothing to stretch.
    pub preserve_pitch: bool,
    /// Run RNNoise on this clip's audio before mixing.
    pub denoise: bool,
    /// Clip gain envelope, ticks rebased to clip-relative output sample frames.
    pub volume: Param<f32>,
    /// Clip pan envelope (−1…+1), ticks rebased like `volume`.
    pub pan: Param<f32>,
    /// Fade ramp lengths in output sample frames, anchored at the span edges.
    pub fade_in: i64,
    pub fade_out: i64,
    /// Crossfade ramps from audio transitions at this span's cuts. Each one
    /// replaces the fade on its side.
    pub crossfade_in: Option<Crossfade>,
    pub crossfade_out: Option<Crossfade>,
    /// Opened on first overlap, dropped with the mixer.
    pub reader: Option<Box<dyn AudioReader>>,
    /// Source ran out before the span's out-point: the rest pads as silence.
    pub exhausted: bool,
    /// Which bus this span sums into, as an index into the mixer's own
    /// [`Timeline::audio_buses`] order.
    ///
    /// [`Timeline::audio_buses`]: aria_models::Timeline::audio_buses
    pub bus: usize,
}

impl std::fmt::Debug for Span {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Span")
            .field("path", &self.path)
            .field("start", &self.start)
            .field("end", &self.end)
            .field("source_start", &self.source_start)
            .field("source_len", &self.source_len)
            .field("reversed", &self.reversed)
            .field("warp", &self.warp)
            .field("preserve_pitch", &self.preserve_pitch)
            .field("denoise", &self.denoise)
            .field("crossfade_in", &self.crossfade_in)
            .field("crossfade_out", &self.crossfade_out)
            .field("has_reader", &self.reader.is_some())
            .field("exhausted", &self.exhausted)
            .field("bus", &self.bus)
            .finish_non_exhaustive()
    }
}

impl Span {
    /// Create a linear (unwarped) span with unity pitch.
    pub fn linear(
        start: i64,
        end: i64,
        source_start: i64,
        volume: Param<f32>,
        pan: Param<f32>,
        fade_in: i64,
        fade_out: i64,
        bus: usize,
        reader: Option<Box<dyn AudioReader>>,
    ) -> Self {
        let source_len = end.saturating_sub(start).max(0);
        Self {
            path: PathBuf::new(),
            start,
            end,
            source_start,
            source_len,
            reversed: false,
            warp: SpanWarp::Linear,
            preserve_pitch: true,
            denoise: false,
            volume,
            pan,
            fade_in,
            fade_out,
            crossfade_in: None,
            crossfade_out: None,
            reader,
            exhausted: false,
            bus,
        }
    }
}

/// Source bounds for a monotonic output interval. Positive speed curves and
/// reverse playback both remain monotonic, so only the interval endpoints can
/// define its minimum and maximum.
fn monotonic_source_bounds(
    span: &Span,
    rel_start: i64,
    rel_end: i64,
    span_len: i64,
) -> (f64, f64, u32) {
    debug_assert!(rel_start < rel_end);
    let first = span_source_offset(span, rel_start, span_len);
    let last = span_source_offset(span, rel_end - 1, span_len);
    (first.min(last), first.max(last), 2)
}

/// Size scratch that the following loop overwrites. A warm buffer keeps its
/// existing bytes instead of being cleared and zero-filled first.
fn size_overwritten_scratch(scratch: &mut Vec<f32>, len: usize) {
    scratch.resize(len, 0.0);
}

/// One lane of the mix, resolved to indices: what to multiply it by, where it
/// goes, and which buses take a copy.
///
/// The routing itself is decided by the model
/// ([`Timeline::audio_buses`]); this is that answer with track ids replaced by
/// positions, so folding a block is array arithmetic.
///
/// [`Timeline::audio_buses`]: aria_models::Timeline::audio_buses
#[derive(Debug)]
pub struct BusNode {
    /// The lane's fader, as linear gain.
    pub gain: f32,
    /// The lane's stereo pan (`-1.0..=1.0`).
    pub pan: f32,
    /// The lane's automation mode.
    pub automation: AutomationMode,
    /// Sequence-tick volume automation curve.
    pub volume_envelope: Param<f32>,
    /// Sequence-tick pan automation curve.
    pub pan_envelope: Param<f32>,
    /// Where this lane's own sum goes: another bus, or the programme mix.
    pub output: Option<usize>,
    /// `(bus, level, pre_fader)` for each copy of this lane.
    pub sends: Vec<(usize, f32, bool)>,
    /// Whether anything this lane carries reaches the programme mix. A lane
    /// that does not, such as one behind a muted bus, is skipped by the fold
    /// and meters silence.
    pub live: bool,
}

impl BusNode {
    pub fn new(gain: f32, output: Option<usize>, sends: Vec<(usize, f32, bool)>) -> Self {
        Self {
            gain,
            pan: 0.0,
            automation: AutomationMode::Off,
            volume_envelope: Param::Constant(gain),
            pan_envelope: Param::Constant(0.0),
            output,
            sends,
            live: true,
        }
    }

    pub(crate) fn apply_fader_and_pan(&self, buf: &mut [f32], start_sample: i64, fps: Rational) {
        apply_strip_gain_pan(
            buf,
            start_sample,
            fps,
            false,
            self.gain,
            self.pan,
            self.automation,
            &self.volume_envelope,
            &self.pan_envelope,
        );
    }
}

/// Streamed mixer over every audible span of a project's timeline.
pub struct ExportAudioMixer {
    spans: Vec<Span>,
    /// The mix graph in feed order: a bus is always folded after everything
    /// that feeds it, so one forward pass needs no lookahead.
    buses: Vec<BusNode>,
    /// One interleaved stereo accumulator per bus, reused every block.
    bus_buffers: Vec<Vec<f32>>,
    scratch: Vec<f32>,
    warp_scratch: Vec<f32>,
    /// Per-output-sample window weight for a stretched block, so the overlap-add
    /// can be normalised at the block's edges.
    weight_scratch: Vec<f32>,
    /// Decoded timeline window reused across varispeed shuttle blocks.
    shuttle_scratch: Vec<f32>,
    /// The lane each bus position belongs to, in feed order: what a meter is a
    /// meter *of*. Read straight out of the mix graph, so a reading cannot be
    /// attributed to the wrong strip.
    lanes: Vec<TrackId>,
    /// Each lane's post-fader peak from the last block folded, per channel.
    /// Sized once; the fold overwrites it in place, so metering costs no
    /// allocation per block.
    peaks: Vec<[f32; 2]>,
    /// Sequence frame rate for converting sample positions to automation ticks.
    fps: Rational,
    /// Master strip fader, pan, mute, automation mode, envelopes, and peak.
    master_gain: f32,
    master_pan: f32,
    master_muted: bool,
    master_automation: AutomationMode,
    master_volume: Param<f32>,
    master_pan_envelope: Param<f32>,
    master_peak: [f32; 2],
    /// The insert chains and their alignment, when any insert in the project
    /// processes. `None` mixes on the plain path.
    inserts: Option<Box<inserts::InsertGraph>>,
}

impl std::fmt::Debug for ExportAudioMixer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExportAudioMixer")
            .field("spans", &self.spans.len())
            .field("buses", &self.buses.len())
            .field("lanes", &self.lanes.len())
            .finish_non_exhaustive()
    }
}

impl ExportAudioMixer {
    /// Audible spans: clips on lanes whose sound still arrives at the master,
    /// summed through the project's own mix graph. CapCut-style, a video clip
    /// keeps its own sound, so video lanes are audible too; only a clip
    /// detached to a linked audio lane defers its audio there. `None` when the
    /// timeline is silent, so callers can skip the audio track entirely.
    ///
    /// Routing is read once, from [`Timeline::audio_buses`]: a lane behind a
    /// muted bus is dropped here rather than decoded and multiplied by zero,
    /// and a project whose routing is cyclic is refused by name instead of
    /// being mixed in whatever order the fold happened to take.
    ///
    /// Reversed clips mix backward. Other retimed and denoised clips mix
    /// through the same warped-resample path.
    ///
    /// [`Timeline::audio_buses`]: aria_models::Timeline::audio_buses
    pub fn for_project(project: &Project) -> Result<Option<Self>, RenderError> {
        let timeline = project.timeline();
        let fps = timeline.frame_rate;
        let graph = timeline.audio_buses()?;
        let index_of = |track| graph.iter().position(|node| node.track == track);
        // Feed order is what the fold below relies on: a lane only ever writes
        // forward, so its destinations have to sit after it. The model
        // guarantees that; this turns the guarantee into a checked precondition
        // instead of an index the fold would have to second-guess per block.
        let mut buses: Vec<BusNode> = Vec::with_capacity(graph.len());
        for (i, node) in graph.iter().enumerate() {
            let forward = |target| match index_of(target) {
                Some(j) if j > i => Ok(j),
                _ => Err(RenderError::Model(ModelError::RoutingCycle(node.track))),
            };
            let mut sends = Vec::with_capacity(node.sends.len());
            for send in node.sends {
                sends.push((forward(send.to)?, send.level, send.pre_fader));
            }
            buses.push(BusNode {
                gain: node.gain,
                pan: node.pan,
                automation: node.automation,
                volume_envelope: node.volume_envelope.clone(),
                pan_envelope: node.pan_envelope.clone(),
                output: node.output.bus().map(forward).transpose()?,
                sends,
                live: node.reaches_master,
            });
        }
        let mut spans = Vec::new();
        // Each span's clip chain, by span.
        let mut clip_inserts: Vec<&[AudioEffectInstance]> = Vec::new();
        // One lane's spans, for its crossfades.
        let mut lane = Vec::new();
        for track in timeline.tracks_ordered() {
            let Some(bus) = index_of(track.id) else {
                continue;
            };
            if !graph[bus].reaches_master {
                continue;
            }
            lane.clear();
            for clip in track.clips_ordered() {
                if clip.is_silent() {
                    continue;
                }
                if !timeline.carries_own_audio(clip.id) {
                    continue;
                }
                let Some(media_id) = clip.media() else {
                    continue;
                };
                let Some(media) = project.media(media_id) else {
                    continue;
                };
                if !media.has_audio {
                    continue;
                };
                let Some(source) = clip.source_range() else {
                    continue;
                };
                let warp = if clip.has_speed_curve() {
                    SpanWarp::Curved {
                        curve: clip.speed_curve.clone(),
                        source_len: ticks_to_samples(
                            source.duration.value,
                            source.start.rate.num,
                            source.start.rate.den,
                        ),
                        curve_total: clip.speed_curve_average(),
                    }
                } else if clip.speed.num != clip.speed.den {
                    SpanWarp::FlatSpeed {
                        num: clip.speed.num,
                        den: clip.speed.den,
                    }
                } else {
                    SpanWarp::Linear
                };
                // A driven envelope replaces the keyframed one, evaluated once
                // per frame of the clip.
                let volume = driven_envelope(project, clip, ClipParam::Volume);
                let pan = driven_envelope(project, clip, ClipParam::Pan);
                spans.push(Span {
                    path: media.path().to_path_buf(),
                    start: ticks_to_samples(clip.timeline.start.value, fps.num, fps.den),
                    end: ticks_to_samples(clip.timeline.end_tick(), fps.num, fps.den),
                    source_start: ticks_to_samples(
                        source.start.value,
                        source.start.rate.num,
                        source.start.rate.den,
                    ),
                    source_len: ticks_to_samples(
                        source.duration.value,
                        source.start.rate.num,
                        source.start.rate.den,
                    ),
                    reversed: clip.reversed,
                    warp,
                    preserve_pitch: clip.preserve_pitch,
                    denoise: clip.denoise,
                    volume: volume
                        .as_ref()
                        .unwrap_or(&clip.volume)
                        .map_ticks(|tick| ticks_to_samples(tick, fps.num, fps.den)),
                    pan: pan
                        .as_ref()
                        .unwrap_or(&clip.pan)
                        .map_ticks(|tick| ticks_to_samples(tick, fps.num, fps.den)),
                    fade_in: ticks_to_samples(clip.fade_in, fps.num, fps.den),
                    fade_out: ticks_to_samples(clip.fade_out, fps.num, fps.den),
                    crossfade_in: None,
                    crossfade_out: None,
                    reader: None,
                    exhausted: false,
                    bus,
                });
                lane.push(xfade::LaneSpan {
                    clip: clip.id,
                    span: spans.len() - 1,
                    media_len: ticks_to_samples(
                        media.duration.value,
                        media.duration.rate.num,
                        media.duration.rate.den,
                    ),
                });
                clip_inserts.push(&clip.audio_effects);
            }
            xfade::crossfade_lane(track, &lane, &mut spans, fps);
        }
        if spans.is_empty() {
            return Ok(None);
        }
        let inserts = inserts::InsertGraph::build(
            &graph,
            &buses,
            timeline.master_audio_effects(),
            &clip_inserts,
            fps,
        )
        .map(Box::new);
        Ok(Some(Self {
            bus_buffers: vec![Vec::new(); buses.len()],
            lanes: graph.iter().map(|node| node.track).collect(),
            peaks: vec![[0.0; 2]; buses.len()],
            fps,
            master_gain: timeline.master_gain(),
            master_pan: timeline.master_pan(),
            master_muted: timeline.master_muted(),
            master_automation: timeline.master_automation(),
            master_volume: timeline.master_volume_envelope().clone(),
            master_pan_envelope: timeline.master_pan_envelope().clone(),
            master_peak: [0.0; 2],
            buses,
            spans,
            scratch: Vec::new(),
            warp_scratch: Vec::new(),
            weight_scratch: Vec::new(),
            shuttle_scratch: Vec::new(),
            inserts,
        }))
    }

    /// A mixer over hand-built spans, all summing straight to the master at
    /// unity: the shape a project with no routing resolves to. Lets a case
    /// exercise one span's own arithmetic without a project behind it.
    pub fn of_spans(spans: Vec<Span>) -> Self {
        Self {
            spans,
            buses: vec![BusNode::new(1.0, None, Vec::new())],
            bus_buffers: vec![Vec::new()],
            lanes: vec![TrackId::from_raw(0)],
            peaks: vec![[0.0; 2]],
            fps: Rational::FPS_30,
            master_gain: 1.0,
            master_pan: 0.0,
            master_muted: false,
            master_automation: AutomationMode::Off,
            master_volume: Param::Constant(1.0),
            master_pan_envelope: Param::Constant(0.0),
            master_peak: [0.0; 2],
            scratch: Vec::new(),
            warp_scratch: Vec::new(),
            weight_scratch: Vec::new(),
            shuttle_scratch: Vec::new(),
            inserts: None,
        }
    }

    /// A mixer over hand-built spans and a hand-built graph, for cases about
    /// the fold itself rather than about one span's arithmetic.
    pub fn of_graph(spans: Vec<Span>, buses: Vec<BusNode>) -> Self {
        Self {
            bus_buffers: vec![Vec::new(); buses.len()],
            lanes: (0..buses.len())
                .map(|at| TrackId::from_raw(at as u64))
                .collect(),
            peaks: vec![[0.0; 2]; buses.len()],
            fps: Rational::FPS_30,
            master_gain: 1.0,
            master_pan: 0.0,
            master_muted: false,
            master_automation: AutomationMode::Off,
            master_volume: Param::Constant(1.0),
            master_pan_envelope: Param::Constant(0.0),
            master_peak: [0.0; 2],
            buses,
            spans,
            scratch: Vec::new(),
            warp_scratch: Vec::new(),
            shuttle_scratch: Vec::new(),
            weight_scratch: Vec::new(),
            inserts: None,
        }
    }

    /// Mix every span overlapping `[pos, pos + out.len()/2)` into `out`
    /// (interleaved stereo; cleared to silence first).
    ///
    /// A span sums into its own lane's accumulator, never straight into `out`.
    /// The lanes are then folded in feed order — pre-fader sends, then the
    /// fader, then post-fader sends, then the output — so a bus carries
    /// everything routed into it before it is itself faded and passed on. Only
    /// what reaches the master lands in `out`.
    ///
    /// A project with inserts that process mixes through its insert graph
    /// instead: see the `inserts` module for the signal order and latency.
    pub fn mix_into(&mut self, pos: i64, out: &mut [f32]) -> Result<(), DecodeError> {
        if self.inserts.is_some() {
            return self.mix_inserted(pos, out);
        }
        out.fill(0.0);
        let block_end = pos + (out.len() / CHANNELS) as i64;
        for buffer in self.bus_buffers.iter_mut() {
            buffer.clear();
            buffer.resize(out.len(), 0.0);
        }

        for span in &mut self.spans {
            let bus = span.bus;
            Self::mix_span_range(
                span,
                &mut self.scratch,
                &mut self.warp_scratch,
                &mut self.weight_scratch,
                pos,
                block_end,
                &mut self.bus_buffers[bus],
            )?;
        }

        self.fold_buses(pos, out);
        for sample in out.iter_mut() {
            *sample = sample.clamp(-1.0, 1.0);
        }
        Ok(())
    }

    /// Mix `out.len() / 2` stereo frames for shuttle playback anchored at
    /// timeline sample `anchor_sample`, at output stream frame `stream_frame`,
    /// and signed rational speed `speed_num / speed_den`.
    ///
    /// Holds pitch via WSOLA for forward and reverse speeds in `(0, 4x]`, and
    /// writes silence when stopped or above `MAX_SHUTTLE_AUDIO_SPEED`.
    pub fn mix_shuttle_into(
        &mut self,
        anchor_sample: i64,
        stream_frame: i64,
        speed_num: i32,
        speed_den: i32,
        out: &mut [f32],
    ) -> Result<(), DecodeError> {
        let out_frames = out.len() / CHANNELS;
        if out_frames == 0
            || speed_num == 0
            || speed_den <= 0
            || i64::from(speed_num).abs()
                > i64::from(MAX_SHUTTLE_AUDIO_SPEED) * i64::from(speed_den)
        {
            out.fill(0.0);
            self.peaks.fill([0.0; 2]);
            self.master_peak = [0.0; 2];
            return Ok(());
        }
        let rel_start = stream_frame.max(0);
        if speed_num == speed_den {
            return self.mix_into(anchor_sample.saturating_add(rel_start), out);
        }
        if speed_num == -speed_den {
            out.fill(0.0);
            for span in &mut self.spans {
                span.exhausted = false;
            }
            let src_end = (anchor_sample - rel_start).max(0);
            let src_start = (anchor_sample - rel_start - out_frames as i64).max(0);
            let need = (src_end - src_start) as usize;
            if need == 0 {
                self.peaks.fill([0.0; 2]);
                self.master_peak = [0.0; 2];
                return Ok(());
            }
            let mut window = std::mem::take(&mut self.shuttle_scratch);
            window.resize(need * CHANNELS, 0.0);
            let mix_res = self.mix_into(src_start, &mut window);
            if let Err(err) = mix_res {
                self.shuttle_scratch = window;
                return Err(err);
            }
            for i in 0..need {
                let out_idx = (anchor_sample - 1 - rel_start - (src_start + i as i64)) as usize;
                out[out_idx * CHANNELS] = window[i * CHANNELS];
                out[out_idx * CHANNELS + 1] = window[i * CHANNELS + 1];
            }
            self.shuttle_scratch = window;
            return Ok(());
        }
        let direction = if speed_num < 0 { -1 } else { 1 };
        let abs_speed = (f64::from(speed_num) / f64::from(speed_den)).abs();
        let rel_end = rel_start + out_frames as i64;
        let mut anchor_at = |rel_out: i64| -> f64 {
            if direction > 0 {
                anchor_sample as f64 + (rel_out as f64) * abs_speed
            } else {
                (anchor_sample - 1) as f64 - (rel_out as f64) * abs_speed
            }
        };
        let (src_floor, src_ceil) = ola_source_bounds(rel_start, rel_end, &mut anchor_at);
        let need = (src_ceil - src_floor).max(0) as usize;
        if need == 0 {
            out.fill(0.0);
            self.peaks.fill([0.0; 2]);
            self.master_peak = [0.0; 2];
            return Ok(());
        }
        for span in &mut self.spans {
            span.exhausted = false;
        }
        let mut window = std::mem::take(&mut self.shuttle_scratch);
        window.resize(need * CHANNELS, 0.0);
        let mix_res = self.mix_into(src_floor, &mut window);
        if let Err(err) = mix_res {
            self.shuttle_scratch = window;
            return Err(err);
        }
        ola_synthesize(
            &window,
            need,
            src_floor,
            rel_start,
            rel_end,
            direction,
            &mut anchor_at,
            &mut self.weight_scratch,
            out,
        );
        for sample in out.iter_mut() {
            *sample = sample.clamp(-1.0, 1.0);
        }
        self.shuttle_scratch = window;
        Ok(())
    }

    /// Hold insert processors to playback deadlines: a block that takes longer
    /// than the real time it covers counts as a miss. Off for export.
    pub fn set_realtime(&mut self, realtime: bool) {
        if let Some(graph) = &mut self.inserts {
            graph.set_realtime(realtime);
        }
    }

    /// Add the part of `span` inside `[from, to)` into `out`, whose first
    /// frame is `from`. A span outside the range, or whose source ran out,
    /// adds nothing.
    fn mix_span_range(
        span: &mut Span,
        scratch: &mut Vec<f32>,
        warp_scratch: &mut Vec<f32>,
        weight_scratch: &mut Vec<f32>,
        from: i64,
        to: i64,
        out: &mut [f32],
    ) -> Result<(), DecodeError> {
        if span.start >= to || span.end <= from || span.exhausted {
            return Ok(());
        }
        let s = span.start.max(from);
        let e = span.end.min(to);
        if matches!(span.warp, SpanWarp::Linear) && !span.reversed {
            Self::mix_linear_span(span, scratch, s, e, from, out)
        } else {
            Self::mix_warped_span(span, scratch, warp_scratch, weight_scratch, s, e, from, out)
        }
    }

    /// Fold every lane into its destination, in feed order.
    ///
    /// Every destination sits after its source, which `for_project` checked, so
    /// a lane only ever writes forward and the split borrow is exact: the lane
    /// being folded is read out of `upstream`, everything it feeds is written
    /// through `downstream`.
    fn fold_buses(&mut self, pos: i64, out: &mut [f32]) {
        for i in 0..self.buses.len() {
            // Nothing a dead lane carries reaches the programme mix, including
            // what a live lane routed into it.
            if !self.buses[i].live {
                self.peaks[i] = [0.0; 2];
                continue;
            }
            // Pre-fader copies are taken before the lane's own fader, so riding
            // the lane down does not thin them.
            let (upstream, downstream) = self.bus_buffers.split_at_mut(i + 1);
            for (target, level, pre_fader) in self.buses[i].sends.iter().copied() {
                if pre_fader {
                    add_scaled(&mut downstream[target - i - 1], &upstream[i], level);
                }
            }
            self.buses[i].apply_fader_and_pan(&mut self.bus_buffers[i], pos, self.fps);
            // Post-fader, before anything this lane feeds is written: what the
            // lane is sending on, which is what a strip's meter is showing.
            self.peaks[i] = stereo_peak(&self.bus_buffers[i]);
            let (upstream, downstream) = self.bus_buffers.split_at_mut(i + 1);
            let lane = &upstream[i];
            for (target, level, pre_fader) in self.buses[i].sends.iter().copied() {
                if !pre_fader {
                    add_scaled(&mut downstream[target - i - 1], lane, level);
                }
            }
            match self.buses[i].output {
                Some(target) => add_scaled(&mut downstream[target - i - 1], lane, 1.0),
                None => add_scaled(out, lane, 1.0),
            }
        }
        apply_strip_gain_pan(
            out,
            pos,
            self.fps,
            self.master_muted,
            self.master_gain,
            self.master_pan,
            self.master_automation,
            &self.master_volume,
            &self.master_pan_envelope,
        );
        self.master_peak = stereo_peak(out);
    }

    /// Each lane's post-fader peak from the last block, keyed by the lane it
    /// belongs to and in the order the mix folds them.
    ///
    /// One iterator for both the id and the level, so a caller cannot pair a
    /// reading with the wrong lane.
    pub fn lane_peaks(&self) -> impl Iterator<Item = (TrackId, [f32; 2])> + '_ {
        self.lanes.iter().copied().zip(self.peaks.iter().copied())
    }

    /// The master strip's post-fader peak from the last block folded.
    pub fn master_peak(&self) -> [f32; 2] {
        self.master_peak
    }

    fn mix_linear_span(
        span: &mut Span,
        scratch: &mut Vec<f32>,
        s: i64,
        e: i64,
        pos: i64,
        out: &mut [f32],
    ) -> Result<(), DecodeError> {
        let reader = match &mut span.reader {
            Some(reader) => reader,
            None => span.reader.insert(open_span_reader(span)?),
        };

        let src_from = span.source_start + (s - span.start);
        reader
            .seek_to_frame(src_from)
            .map_err(|err| audio_err("seek audio source", &span.path, err))?;
        let lead = reader
            .position()
            .map_or(0, |p| (p - src_from).clamp(0, e - s));

        let want = ((e - s) - lead) as usize;
        if want == 0 {
            return Ok(());
        }
        scratch.resize(want * CHANNELS, 0.0);
        let got = reader
            .read(&mut scratch[..want * CHANNELS])
            .map_err(|err| audio_err("decode audio source", &span.path, err))?;
        if got < want {
            span.exhausted = true;
        }

        let offset = ((s - pos + lead) as usize) * CHANNELS;
        accumulate_span_samples(span, s + lead, got, offset, out, scratch);
        Ok(())
    }

    #[allow(clippy::too_many_arguments)] // three scratch buffers, all reused per block
    fn mix_warped_span(
        span: &mut Span,
        scratch: &mut Vec<f32>,
        warp_scratch: &mut Vec<f32>,
        weight_scratch: &mut Vec<f32>,
        s: i64,
        e: i64,
        pos: i64,
        out: &mut [f32],
    ) -> Result<(), DecodeError> {
        let span_len = span.end - span.start;
        let rel_start = s - span.start;
        let rel_end = e - span.start;
        let out_frames = (rel_end - rel_start) as usize;
        if out_frames == 0 {
            return Ok(());
        }

        // A stretched span reads whole frames at unit rate, so the window it
        // touches is anchored on the frames that overlap this block, not on the
        // block's own samples.
        let stretch = span.preserve_pitch && !span.warp.is_unity();
        let (src_floor, src_ceil) = if stretch {
            ola_source_bounds(rel_start, rel_end, &mut |rel| {
                span_source_offset(span, rel, span_len)
            })
        } else {
            let (src_min, src_max, _evaluations) =
                monotonic_source_bounds(span, rel_start, rel_end, span_len);
            ((src_min.floor() as i64).max(0), src_max.ceil() as i64 + 1)
        };
        let need = (src_ceil - src_floor).max(0) as usize;
        if need == 0 {
            return Ok(());
        }

        let source_start = span.source_start;
        let path = span.path.clone();
        if span.reader.is_none() {
            span.reader = Some(open_span_reader(span)?);
        }
        let got = {
            let reader = span.reader.as_mut().expect("span reader");
            reader
                .seek_to_frame(source_start + src_floor)
                .map_err(|err| audio_err("seek audio source", &path, err))?;
            warp_scratch.resize(need * CHANNELS, 0.0);
            reader
                .read(&mut warp_scratch[..need * CHANNELS])
                .map_err(|err| audio_err("decode audio source", &path, err))?
        };
        if (got as i64) < src_ceil - src_floor {
            span.exhausted = true;
        }

        size_overwritten_scratch(scratch, out_frames * CHANNELS);
        if stretch {
            let window = &warp_scratch[..got * CHANNELS];
            let direction = if span.reversed { -1 } else { 1 };
            ola_synthesize(
                window,
                got,
                src_floor,
                rel_start,
                rel_end,
                direction,
                &mut |rel| span_source_offset(span, rel, span_len),
                weight_scratch,
                scratch,
            );
        } else {
            for (out_idx, rel) in (rel_start..rel_end).enumerate() {
                let src = span_source_offset(span, rel, span_len);
                let src_rel = src - src_floor as f64;
                let base = src_rel.floor() as usize;
                let frac = src_rel - base as f64;
                if base >= got {
                    let at = out_idx * CHANNELS;
                    scratch[at..at + CHANNELS].fill(0.0);
                    continue;
                }
                for ch in 0..CHANNELS {
                    let a = warp_scratch[base * CHANNELS + ch];
                    let b = if base + 1 < got {
                        warp_scratch[(base + 1) * CHANNELS + ch]
                    } else {
                        a
                    };
                    scratch[out_idx * CHANNELS + ch] = a + (b - a) * frac as f32;
                }
            }
        }

        let offset = ((s - pos) as usize) * CHANNELS;
        accumulate_span_samples(span, s, out_frames, offset, out, scratch);
        Ok(())
    }
}

/// Constant-power pan gains scaled to unity at center (`0 dB` center,
/// `+3 dB` hard edges). See the module docs.
fn pan_channel_gains(pan: f32) -> (f32, f32) {
    let angle = (pan + 1.0) * FRAC_PI_4;
    (angle.cos() * SQRT_2, angle.sin() * SQRT_2)
}

/// `dst += src * level`, one interleaved block into another.
///
/// Unity skips the multiply: a lane routed straight through, which is every
/// lane in a project nobody has mixed, costs one add per sample and nothing
/// else. A silent send costs nothing at all.
fn add_scaled(dst: &mut [f32], src: &[f32], level: f32) {
    if level == 0.0 {
        return;
    }
    if level == 1.0 {
        for (d, s) in dst.iter_mut().zip(src) {
            *d += *s;
        }
    } else {
        for (d, s) in dst.iter_mut().zip(src) {
            *d += *s * level;
        }
    }
}

/// The loudest magnitude per channel in an interleaved stereo block.
///
/// Reads the buffer the fold already holds and allocates nothing, so a meter on
/// every lane costs one pass per lane per block. A source that decoded to NaN is
/// skipped rather than poisoning the reading: a bar stuck at NaN would draw as
/// silence and read as a broken meter rather than a broken clip.
fn stereo_peak(block: &[f32]) -> [f32; 2] {
    let mut peak = [0.0f32; 2];
    for frame in block.chunks_exact(CHANNELS) {
        for (bar, sample) in peak.iter_mut().zip(frame) {
            let magnitude = sample.abs();
            if magnitude.is_finite() && magnitude > *bar {
                *bar = magnitude;
            }
        }
    }
    peak
}

// The rest of the per-span maths moved to a sibling to stay under the file
// ceiling. Re-exported at its own visibility, so `sample_boundary` is still
// `crate::export_audio::sample_boundary` and the helpers still resolve here.
#[path = "export_audio_mix.rs"]
mod mix;
pub use mix::*;

#[path = "export_audio_inserts.rs"]
mod inserts;

#[path = "export_audio_xfade.rs"]
mod xfade;
pub use xfade::Crossfade;

// The mix run ahead of an export's frame loop, on a thread of its own.
#[path = "export_audio_feed.rs"]
pub(crate) mod feed;

#[doc(hidden)]
pub mod export_audio_bench {
    pub use super::{BusNode, ExportAudioMixer, Span};
    pub use crate::audio_dsp::SpanWarp;
}
#[cfg(test)]
mod pitch_tests;
#[cfg(test)]
mod submix_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod xfade_tests;
