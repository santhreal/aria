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
use aria_models::{ModelError, Param, Project, TrackId, audio_gain_at};

use crate::error::RenderError;

use crate::audio_dsp::{
    DenoiseReader, OLA_FRAME, OLA_HOP, OLA_PAD, SpanWarp, ola_aligned_offset, ola_frames,
    ola_window, warped_source_frame,
};

/// Export audio sample rate: the broadcast/web standard for video files.
pub const EXPORT_AUDIO_RATE: u32 = 48_000;
/// Export channel count (interleaved stereo).
pub const EXPORT_AUDIO_CHANNELS: u16 = 2;

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
    /// Where this lane's own sum goes: another bus, or the programme mix.
    pub output: Option<usize>,
    /// `(bus, level, pre_fader)` for each copy of this lane.
    pub sends: Vec<(usize, f32, bool)>,
}

impl BusNode {
    pub fn new(gain: f32, output: Option<usize>, sends: Vec<(usize, f32, bool)>) -> Self {
        Self {
            gain,
            output,
            sends,
        }
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
    /// The lane each bus position belongs to, in feed order: what a meter is a
    /// meter *of*. Read straight out of the mix graph, so a reading cannot be
    /// attributed to the wrong strip.
    lanes: Vec<TrackId>,
    /// Each lane's post-fader peak from the last block folded, per channel.
    /// Sized once; the fold overwrites it in place, so metering costs no
    /// allocation per block.
    peaks: Vec<[f32; 2]>,
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
                output: node.output.bus().map(forward).transpose()?,
                sends,
            });
        }
        let mut spans = Vec::new();
        for track in timeline.tracks_ordered() {
            let Some(bus) = index_of(track.id) else {
                continue;
            };
            if !graph[bus].reaches_master {
                continue;
            }
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
                    volume: clip
                        .volume
                        .map_ticks(|tick| ticks_to_samples(tick, fps.num, fps.den)),
                    pan: clip
                        .pan
                        .map_ticks(|tick| ticks_to_samples(tick, fps.num, fps.den)),
                    fade_in: ticks_to_samples(clip.fade_in, fps.num, fps.den),
                    fade_out: ticks_to_samples(clip.fade_out, fps.num, fps.den),
                    reader: None,
                    exhausted: false,
                    bus,
                });
            }
        }
        if spans.is_empty() {
            Ok(None)
        } else {
            Ok(Some(Self {
                bus_buffers: vec![Vec::new(); buses.len()],
                lanes: graph.iter().map(|node| node.track).collect(),
                peaks: vec![[0.0; 2]; buses.len()],
                buses,
                spans,
                scratch: Vec::new(),
                warp_scratch: Vec::new(),
                weight_scratch: Vec::new(),
            }))
        }
    }

    /// A mixer over hand-built spans, all summing straight to the master at
    /// unity: the shape a project with no routing resolves to. Lets a case
    /// exercise one span's own arithmetic without a project behind it.
    pub fn of_spans(spans: Vec<Span>) -> Self {
        Self {
            spans,
            buses: vec![BusNode {
                gain: 1.0,
                output: None,
                sends: Vec::new(),
            }],
            bus_buffers: vec![Vec::new()],
            lanes: vec![TrackId::from_raw(0)],
            peaks: vec![[0.0; 2]],
            scratch: Vec::new(),
            warp_scratch: Vec::new(),
            weight_scratch: Vec::new(),
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
            buses,
            spans,
            scratch: Vec::new(),
            warp_scratch: Vec::new(),
            weight_scratch: Vec::new(),
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
    pub fn mix_into(&mut self, pos: i64, out: &mut [f32]) -> Result<(), DecodeError> {
        out.fill(0.0);
        let block_frames = (out.len() / CHANNELS) as i64;
        let block_end = pos + block_frames;
        for buffer in self.bus_buffers.iter_mut() {
            buffer.clear();
            buffer.resize(out.len(), 0.0);
        }

        for i in 0..self.spans.len() {
            let span = &self.spans[i];
            if span.start >= block_end || span.end <= pos || span.exhausted {
                continue;
            }
            let s = span.start.max(pos);
            let e = span.end.min(block_end);
            let bus = span.bus;
            if matches!(span.warp, SpanWarp::Linear) && !span.reversed {
                Self::mix_linear_span(
                    &mut self.spans[i],
                    &mut self.scratch,
                    s,
                    e,
                    pos,
                    &mut self.bus_buffers[bus],
                )?;
            } else {
                Self::mix_warped_span(
                    &mut self.spans[i],
                    &mut self.scratch,
                    &mut self.warp_scratch,
                    &mut self.weight_scratch,
                    s,
                    e,
                    pos,
                    &mut self.bus_buffers[bus],
                )?;
            }
        }

        self.fold_buses(out);
        for sample in out.iter_mut() {
            *sample = sample.clamp(-1.0, 1.0);
        }
        Ok(())
    }

    /// Fold every lane into its destination, in feed order.
    ///
    /// Every destination sits after its source, which `for_project` checked, so
    /// a lane only ever writes forward and the split borrow is exact: the lane
    /// being folded is read out of `upstream`, everything it feeds is written
    /// through `downstream`.
    fn fold_buses(&mut self, out: &mut [f32]) {
        for i in 0..self.buses.len() {
            // Pre-fader copies are taken before the lane's own fader, so riding
            // the lane down does not thin them.
            let (upstream, downstream) = self.bus_buffers.split_at_mut(i + 1);
            for (target, level, pre_fader) in self.buses[i].sends.iter().copied() {
                if pre_fader {
                    add_scaled(&mut downstream[target - i - 1], &upstream[i], level);
                }
            }
            let gain = self.buses[i].gain;
            if gain != 1.0 {
                for sample in self.bus_buffers[i].iter_mut() {
                    *sample *= gain;
                }
            }
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
    }

    /// Each lane's post-fader peak from the last block, keyed by the lane it
    /// belongs to and in the order the mix folds them.
    ///
    /// One iterator for both the id and the level, so a caller cannot pair a
    /// reading with the wrong lane.
    pub fn lane_peaks(&self) -> impl Iterator<Item = (TrackId, [f32; 2])> + '_ {
        self.lanes.iter().copied().zip(self.peaks.iter().copied())
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
        let (first_frame, last_frame) = ola_frames(rel_start, rel_end);
        let mut src_min = f64::MAX;
        let mut src_max = f64::MIN;
        if stretch {
            // One frame earlier as well: the alignment search compares against
            // where the previous frame would have carried on to.
            for k in (first_frame - 1).max(0)..=last_frame {
                let anchor = span_source_offset(span, k * OLA_HOP as i64, span_len);
                src_min = src_min.min(anchor);
                src_max = src_max.max(anchor);
            }
            src_min -= OLA_PAD as f64;
            src_max += OLA_PAD as f64;
        } else {
            let (minimum, maximum, _evaluations) =
                monotonic_source_bounds(span, rel_start, rel_end, span_len);
            src_min = minimum;
            src_max = maximum;
        }

        let src_floor = (src_min.floor() as i64).max(0);
        let src_ceil = src_max.ceil() as i64 + 1;
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
            scratch.fill(0.0);
            weight_scratch.resize(out_frames, 0.0);
            weight_scratch.fill(0.0);
            let window = &warp_scratch[..got * CHANNELS];
            // Reversed spans walk the source backward, so a frame plays backward
            // at unit rate rather than forward from a receding anchor.
            let direction = if span.reversed { -1 } else { 1 };
            let mut previous = None;
            for k in (first_frame - 1).max(0)..=last_frame {
                let anchor = span_source_offset(span, k * OLA_HOP as i64, span_len);
                if k < first_frame {
                    previous = Some(anchor);
                    continue;
                }
                let aligned = ola_aligned_offset(anchor, previous, direction, &mut |pos| {
                    mono_at(window, got, src_floor, pos)
                });
                previous = Some(anchor);
                let base_out = k * OLA_HOP as i64;
                for i in 0..OLA_FRAME {
                    let rel = base_out + i as i64;
                    if rel < rel_start || rel >= rel_end {
                        continue;
                    }
                    let weight = ola_window(i);
                    let pos = aligned + (direction * i as i64) as f64;
                    let out_idx = (rel - rel_start) as usize;
                    weight_scratch[out_idx] += weight;
                    for ch in 0..CHANNELS {
                        scratch[out_idx * CHANNELS + ch] +=
                            weight * sample_at(window, got, src_floor, pos, ch);
                    }
                }
            }
            // Divide by the weight that actually landed, not by the ideal sum of
            // two Hann halves. At the very start and end of a span only one
            // frame overlaps, and dividing by the ideal would fade the first and
            // last ten milliseconds of every retimed clip.
            for (out_idx, &weight) in weight_scratch.iter().enumerate() {
                if weight < 1e-4 {
                    for ch in 0..CHANNELS {
                        scratch[out_idx * CHANNELS + ch] = 0.0;
                    }
                    continue;
                }
                let inv = 1.0 / weight;
                for ch in 0..CHANNELS {
                    let s = &mut scratch[out_idx * CHANNELS + ch];
                    *s *= inv;
                    if s.is_subnormal() || !s.is_finite() {
                        *s = 0.0;
                    }
                }
            }
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
