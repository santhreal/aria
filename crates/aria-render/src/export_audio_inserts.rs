//! Insert chains in the mix: clip, lane and master inserts, sidechain keys,
//! and the delays that keep every path aligned.
//!
//! A project whose inserts would all pass their input through (none set,
//! every one bypassed, or none registered) builds no [`InsertGraph`] and mixes
//! on the plain path in the parent module.
//!
//! # Signal order
//!
//! A clip's sound runs through the clip's chain, then its volume, fades and
//! pan. A lane's sum runs through the lane's chain before its pre-fader sends
//! and its fader. The programme mix runs through the master chain before the
//! final clamp. A sidechain key is the key lane's sound after its own chain
//! and before its fader.
//!
//! # Latency
//!
//! Each live lane has an arrival time `A`: how many frames behind the source
//! its input runs. A lane fed by nothing has `A = 0`; a lane fed by others,
//! through outputs, sends or keys, takes the largest `A + L` among them, where
//! `L` is the feeding lane's chain latency. Each edge is delayed by the
//! difference, so every input of a lane lines up. The master's output runs
//! `T = A_master + L_master` behind the graph, so the graph runs `T` frames
//! ahead of the output position, and lane `i` reads its clips at
//! `position + T - A_i`.
//!
//! A clip chain with latency `l` reads its clip `l` frames ahead and puts the
//! result back in place, cut to the clip's span.
//!
//! A mix that does not continue the last block resets every processor and
//! delay and pre-rolls `T` frames, so the first block out is aligned.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::{Duration, Instant};

use aria_models::{AudioBus, AudioEffectInstance, Rational, TrackId};

use super::{
    BusNode, CHANNELS, DecodeError, EXPORT_AUDIO_RATE, ExportAudioMixer, Param, Span,
    accumulate_span_samples, add_scaled, stereo_peak, ticks_to_samples,
};
use crate::audio_processor::{
    AudioBlock, AudioProcessor, FaultKind, FaultSink, ProcessorFault, create_processor,
    panic_message,
};

/// Frames processed at once while pre-rolling.
const PREROLL_BLOCK: i64 = 4096;
/// Missed playback deadlines in a row that bypass an insert.
const DEADLINE_MISSES: u8 = 3;

/// A fixed delay over a stream of samples, applied in place.
#[derive(Debug, Default)]
struct Delay {
    ring: Vec<f32>,
    at: usize,
}

impl Delay {
    fn new(samples: usize) -> Self {
        Self {
            ring: vec![0.0; samples],
            at: 0,
        }
    }

    fn is_empty(&self) -> bool {
        self.ring.is_empty()
    }

    fn run(&mut self, buf: &mut [f32]) {
        let len = self.ring.len();
        if len == 0 {
            return;
        }
        for sample in buf {
            std::mem::swap(sample, &mut self.ring[self.at]);
            self.at += 1;
            if self.at == len {
                self.at = 0;
            }
        }
    }

    fn reset(&mut self) {
        self.ring.fill(0.0);
        self.at = 0;
    }
}

/// Interleaved samples in `frames` frames; none for a negative count.
fn samples_for(frames: i64) -> usize {
    usize::try_from(frames.max(0)).unwrap_or(0) * CHANNELS
}

/// What one insert does to its input.
enum Stage {
    /// Bypassed in the project, or its spec or processor is not registered:
    /// the input passes through undelayed.
    Through,
    /// Processing.
    Live {
        processor: Box<dyn AudioProcessor>,
        misses: u8,
    },
    /// Failed: the input passes through, delayed by the declared latency.
    Faulted { left: Delay, right: Delay },
}

/// One parameter of an insert, ready to sample.
struct SlotParam {
    /// Keys in sample frames.
    curve: Param<f32>,
    default: f32,
    min: f32,
    max: f32,
}

impl SlotParam {
    /// The value at `frame`, inside the declared range. A value that is not a
    /// number reads as the default.
    fn value(&self, frame: i64) -> f32 {
        let value = self.curve.sample(frame);
        if value.is_finite() {
            value.clamp(self.min, self.max)
        } else {
            self.default
        }
    }
}

/// The key one insert reads.
struct Key {
    /// The bus whose post-chain sound is the key. `None` for a key lane
    /// outside the mix, which reads as silence.
    source: Option<usize>,
    delay: Delay,
    interleaved: Vec<f32>,
    left: Vec<f32>,
    right: Vec<f32>,
}

/// One insert in a chain.
struct Slot {
    effect_id: String,
    stage: Stage,
    /// Declared latency in frames; zero for an insert that passes through.
    latency: i64,
    params: Vec<SlotParam>,
    /// This block's parameter table: a constant per parameter, or `None` with
    /// the values in `automated` at `index * frames`.
    constants: Vec<Option<f32>>,
    automated: Vec<f32>,
    key: Option<Key>,
    on_fault: Option<FaultSink>,
}

impl Slot {
    /// `key_source` resolves a key lane to its bus; `None` for a clip chain,
    /// which reads no key.
    fn build(
        instance: &AudioEffectInstance,
        fps: Rational,
        key_source: Option<&dyn Fn(TrackId) -> Option<usize>>,
    ) -> Self {
        let mut slot = Self {
            effect_id: instance.effect_id.clone(),
            stage: Stage::Through,
            latency: 0,
            params: Vec::new(),
            constants: Vec::new(),
            automated: Vec::new(),
            key: None,
            on_fault: None,
        };
        if instance.bypass {
            return slot;
        }
        let Some(spec) = instance.spec() else {
            return slot;
        };
        let Some((created, on_fault)) = create_processor(&spec.id, EXPORT_AUDIO_RATE) else {
            return slot;
        };
        slot.on_fault = on_fault;
        slot.latency = i64::from(spec.latency);
        slot.params = spec
            .params
            .iter()
            .enumerate()
            .map(|(index, pspec)| {
                let curve = match instance.param_curve(&spec, index) {
                    Some(Param::Keyframed { keyframes }) if keyframes.is_empty() => {
                        Param::Constant(pspec.default)
                    }
                    Some(curve) => curve.map_ticks(|tick| ticks_to_samples(tick, fps.num, fps.den)),
                    None => Param::Constant(pspec.default),
                };
                SlotParam {
                    curve,
                    default: pspec.default,
                    min: pspec.min,
                    max: pspec.max,
                }
            })
            .collect();
        slot.constants = vec![None; slot.params.len()];
        if let (true, Some(track), Some(resolve)) = (spec.sidechain, instance.sidechain, key_source)
        {
            slot.key = Some(Key {
                source: resolve(track),
                delay: Delay::default(),
                interleaved: Vec::new(),
                left: Vec::new(),
                right: Vec::new(),
            });
        }
        match created {
            Ok(processor) => {
                slot.stage = Stage::Live {
                    processor,
                    misses: 0,
                };
            }
            Err(fault) => slot.fault(fault.kind),
        }
        slot
    }

    fn passes_through(&self) -> bool {
        matches!(self.stage, Stage::Through)
    }

    fn key_source(&self) -> Option<usize> {
        self.key.as_ref().and_then(|key| key.source)
    }

    /// Stop processing: delay the input by the declared latency from now on,
    /// and report why.
    fn fault(&mut self, kind: FaultKind) {
        let frames = usize::try_from(self.latency).unwrap_or(0);
        let faulted = Stage::Faulted {
            left: Delay::new(frames),
            right: Delay::new(frames),
        };
        if let Stage::Live { processor, .. } = std::mem::replace(&mut self.stage, faulted) {
            // A processor that panicked may panic again while it is dropped.
            if catch_unwind(AssertUnwindSafe(move || drop(processor))).is_err() {
                tracing::warn!(effect = %self.effect_id, "audio processor panicked while dropped");
            }
        }
        let fault = ProcessorFault {
            effect_id: self.effect_id.clone(),
            kind,
        };
        tracing::warn!(%fault, "audio insert bypassed");
        if let Some(sink) = &self.on_fault {
            sink(&fault);
        }
    }

    fn reset(&mut self) {
        if let Some(key) = &mut self.key {
            key.delay.reset();
        }
        match &mut self.stage {
            Stage::Through => {}
            Stage::Faulted { left, right } => {
                left.reset();
                right.reset();
            }
            Stage::Live { processor, misses } => {
                *misses = 0;
                if let Err(payload) = catch_unwind(AssertUnwindSafe(|| processor.reset())) {
                    self.fault(FaultKind::Panicked(panic_message(payload.as_ref())));
                }
            }
        }
    }

    /// Run this insert over `left` and `right`, whose first frame was recorded
    /// at `at`. `frame_of` maps that time onto the parameter curves' frames;
    /// `taps` holds this block's key signals by bus.
    fn run(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        at: i64,
        frame_of: &dyn Fn(i64) -> i64,
        taps: &[Vec<f32>],
        deadline: Option<Duration>,
    ) {
        let (processor, misses) = match &mut self.stage {
            Stage::Through => return,
            Stage::Faulted {
                left: delay_left,
                right: delay_right,
            } => {
                delay_left.run(left);
                delay_right.run(right);
                return;
            }
            Stage::Live { processor, misses } => (processor, misses),
        };
        let frames = left.len();
        self.automated.resize(self.params.len() * frames, 0.0);
        for (index, param) in self.params.iter().enumerate() {
            self.constants[index] = if param.curve.constant().is_some() {
                Some(param.value(0))
            } else {
                let values = &mut self.automated[index * frames..(index + 1) * frames];
                for (offset, value) in values.iter_mut().enumerate() {
                    *value = param.value(frame_of(at + offset as i64));
                }
                None
            };
        }
        let key = match &mut self.key {
            None => None,
            Some(key) => {
                key.interleaved.clear();
                let tap = key
                    .source
                    .and_then(|bus| taps.get(bus))
                    .filter(|tap| tap.len() == frames * CHANNELS);
                match tap {
                    Some(tap) => key.interleaved.extend_from_slice(tap),
                    None => key.interleaved.resize(frames * CHANNELS, 0.0),
                }
                key.delay.run(&mut key.interleaved);
                deinterleave(&key.interleaved, &mut key.left, &mut key.right);
                Some((key.left.as_slice(), key.right.as_slice()))
            }
        };
        let mut block = AudioBlock::from_table(
            left,
            right,
            at,
            EXPORT_AUDIO_RATE,
            &self.constants,
            &self.automated,
            key,
        );
        let started = deadline.map(|_| Instant::now());
        let outcome = catch_unwind(AssertUnwindSafe(|| processor.process(&mut block)));
        if let Err(payload) = outcome {
            // What a panicking processor left in the block is not audio.
            left.fill(0.0);
            right.fill(0.0);
            self.fault(FaultKind::Panicked(panic_message(payload.as_ref())));
            return;
        }
        for sample in left.iter_mut().chain(right.iter_mut()) {
            if !sample.is_finite() {
                *sample = 0.0;
            }
        }
        let (Some(budget), Some(started)) = (deadline, started) else {
            return;
        };
        let took = started.elapsed();
        if took <= budget {
            *misses = 0;
            return;
        }
        *misses += 1;
        if *misses >= DEADLINE_MISSES {
            self.fault(FaultKind::MissedDeadlines { budget, took });
        }
    }
}

/// One insert chain, in signal order.
struct Chain {
    slots: Vec<Slot>,
    left: Vec<f32>,
    right: Vec<f32>,
}

impl Chain {
    fn build(
        inserts: &[AudioEffectInstance],
        fps: Rational,
        key_source: Option<&dyn Fn(TrackId) -> Option<usize>>,
    ) -> Self {
        Self {
            slots: inserts
                .iter()
                .map(|instance| Slot::build(instance, fps, key_source))
                .collect(),
            left: Vec::new(),
            right: Vec::new(),
        }
    }

    fn passes_through(&self) -> bool {
        self.slots.iter().all(Slot::passes_through)
    }

    fn latency(&self) -> i64 {
        self.slots.iter().map(|slot| slot.latency).sum()
    }

    fn keys_from(&self, bus: usize) -> bool {
        self.slots.iter().any(|slot| slot.key_source() == Some(bus))
    }

    fn reset(&mut self) {
        for slot in &mut self.slots {
            slot.reset();
        }
    }

    /// Delay every key so it lines up with the input of the insert reading
    /// it: the chain's input runs `arrival` frames behind the source, and a
    /// key lane's post-chain sound `arrival[k] + latency[k]`.
    fn align_keys(&mut self, arrival: i64, lane_arrival: &[i64], lane_latency: &[i64]) {
        let mut before = 0;
        for slot in &mut self.slots {
            if let Some(Key {
                source: Some(source),
                delay,
                ..
            }) = &mut slot.key
            {
                let ready = lane_arrival[*source] + lane_latency[*source];
                *delay = Delay::new(samples_for(arrival + before - ready));
            }
            before += slot.latency;
        }
    }

    /// Run every insert over the interleaved block `buf`, whose first frame
    /// was recorded at `at`.
    fn run(
        &mut self,
        buf: &mut [f32],
        at: i64,
        frame_of: &dyn Fn(i64) -> i64,
        taps: &[Vec<f32>],
        deadline: Option<Duration>,
    ) {
        if self.passes_through() {
            return;
        }
        deinterleave(buf, &mut self.left, &mut self.right);
        let mut at = at;
        for slot in &mut self.slots {
            slot.run(
                &mut self.left,
                &mut self.right,
                at,
                frame_of,
                taps,
                deadline,
            );
            at -= slot.latency;
        }
        let (frames, _) = buf.as_chunks_mut::<CHANNELS>();
        for ((frame, left), right) in frames.iter_mut().zip(&self.left).zip(&self.right) {
            frame[0] = *left;
            frame[1] = *right;
        }
    }
}

fn deinterleave(buf: &[f32], left: &mut Vec<f32>, right: &mut Vec<f32>) {
    left.clear();
    right.clear();
    let (frames, _) = buf.as_chunks::<CHANNELS>();
    for frame in frames {
        left.push(frame[0]);
        right.push(frame[1]);
    }
}

/// A clip's chain and where its input stream stands.
struct ClipChain {
    chain: Chain,
    /// The lane-time frame the chain's input continues from; `None` after a
    /// reset.
    next: Option<i64>,
    raw: Vec<f32>,
}

impl ClipChain {
    /// Mix the part of `span` inside `[pos, pos + frames)` into `out` through
    /// the clip's chain, reading the clip ahead by the chain's latency.
    #[allow(clippy::too_many_arguments)] // the parent's three scratch buffers
    fn mix(
        &mut self,
        span: &mut Span,
        scratch: &mut Vec<f32>,
        warp_scratch: &mut Vec<f32>,
        weight_scratch: &mut Vec<f32>,
        pos: i64,
        frames: usize,
        out: &mut [f32],
        deadline: Option<Duration>,
    ) -> Result<(), DecodeError> {
        let from = span.start.max(pos);
        let to = span.end.min(pos + frames as i64);
        if from >= to {
            return Ok(());
        }
        let ahead = from + self.chain.latency();
        let (start, last) = (span.start, (span.end - span.start - 1).max(0));
        let frame_of = move |time: i64| (time - start).clamp(0, last);
        if self.next != Some(ahead) {
            self.next = None;
            self.chain.reset();
            let mut at = from;
            while at < ahead {
                let until = (at + PREROLL_BLOCK).min(ahead);
                read_raw(
                    span,
                    scratch,
                    warp_scratch,
                    weight_scratch,
                    at,
                    until,
                    &mut self.raw,
                )?;
                self.chain.run(&mut self.raw, at, &frame_of, &[], None);
                at = until;
            }
        }
        let len = to - from;
        read_raw(
            span,
            scratch,
            warp_scratch,
            weight_scratch,
            ahead,
            ahead + len,
            &mut self.raw,
        )?;
        self.chain
            .run(&mut self.raw, ahead, &frame_of, &[], deadline);
        self.next = Some(ahead + len);
        accumulate_span_samples(
            span,
            from,
            len as usize,
            ((from - pos) as usize) * CHANNELS,
            out,
            &self.raw,
        );
        Ok(())
    }

    fn reset(&mut self) {
        self.next = None;
        self.chain.reset();
    }
}

/// The clip's own sound over `[from, to)`, before its volume, fades and pan,
/// with silence outside the clip.
fn read_raw(
    span: &mut Span,
    scratch: &mut Vec<f32>,
    warp_scratch: &mut Vec<f32>,
    weight_scratch: &mut Vec<f32>,
    from: i64,
    to: i64,
    raw: &mut Vec<f32>,
) -> Result<(), DecodeError> {
    raw.clear();
    raw.resize(samples_for(to - from), 0.0);
    let volume = std::mem::replace(&mut span.volume, Param::Constant(1.0));
    let pan = std::mem::replace(&mut span.pan, Param::Constant(0.0));
    let fades = (span.fade_in, span.fade_out);
    span.fade_in = 0;
    span.fade_out = 0;
    let read = ExportAudioMixer::mix_span_range(
        span,
        scratch,
        warp_scratch,
        weight_scratch,
        from,
        to,
        raw,
    );
    span.volume = volume;
    span.pan = pan;
    (span.fade_in, span.fade_out) = fades;
    read
}

/// Add `lane` into `dest` at `level`, through `delay`.
fn send_through(
    delay: &mut Delay,
    edge: &mut Vec<f32>,
    lane: &[f32],
    dest: &mut [f32],
    level: f32,
) {
    if delay.is_empty() {
        add_scaled(dest, lane, level);
        return;
    }
    edge.clear();
    edge.extend_from_slice(lane);
    delay.run(edge);
    add_scaled(dest, edge, level);
}

/// Every insert chain in a project's mix, with the delays that align them.
pub(super) struct InsertGraph {
    /// Frames each bus's input runs behind the source, in feed order.
    arrival: Vec<i64>,
    lanes: Vec<Chain>,
    master: Chain,
    master_arrival: i64,
    /// Frames the master's output runs behind the graph.
    total: i64,
    /// Chains on clips, by span; `None` for a clip with nothing to run.
    clips: Vec<Option<ClipChain>>,
    /// The delay on each bus's output, and on each of its sends.
    output_delays: Vec<Delay>,
    send_delays: Vec<Vec<Delay>>,
    /// Whether an insert reads each bus as its key.
    keyed: Vec<bool>,
    /// This block's post-chain, pre-fader sound of each keyed bus.
    taps: Vec<Vec<f32>>,
    edge: Vec<f32>,
    preroll: Vec<f32>,
    /// The output position the next block continues from.
    next: Option<i64>,
    /// Hold processors to playback deadlines.
    realtime: bool,
}

impl InsertGraph {
    /// The insert graph over the mix graph `graph` (feed order) and the
    /// mixer's own `buses`, or `None` when no insert would change the sound.
    /// `clip_inserts` holds each span's clip chain, by span.
    ///
    /// Feed order puts every key's lane before the lane it keys, and
    /// `Timeline::audio_buses` refuses a lane keyed by itself, so a key's tap
    /// is always written before the insert reading it runs.
    pub(super) fn build(
        graph: &[AudioBus<'_>],
        buses: &[BusNode],
        master_inserts: &[AudioEffectInstance],
        clip_inserts: &[&[AudioEffectInstance]],
        fps: Rational,
    ) -> Option<Self> {
        let bus_of: &dyn Fn(TrackId) -> Option<usize> =
            &|track| graph.iter().position(|node| node.track == track);
        let mut lanes: Vec<Chain> = graph
            .iter()
            .map(|node| Chain::build(node.inserts, fps, Some(bus_of)))
            .collect();
        let mut master = Chain::build(master_inserts, fps, Some(bus_of));
        let clips: Vec<Option<ClipChain>> = clip_inserts
            .iter()
            .map(|inserts| {
                let chain = Chain::build(inserts, fps, None);
                (!chain.passes_through()).then(|| ClipChain {
                    chain,
                    next: None,
                    raw: Vec::new(),
                })
            })
            .collect();
        if lanes.iter().all(Chain::passes_through)
            && master.passes_through()
            && clips.iter().all(Option::is_none)
        {
            return None;
        }

        let count = buses.len();
        let latency: Vec<i64> = lanes.iter().map(Chain::latency).collect();
        let mut arrival = vec![0_i64; count];
        let mut master_arrival = 0_i64;
        for lane in 0..count {
            if !buses[lane].live {
                continue;
            }
            let ready = arrival[lane] + latency[lane];
            match buses[lane].output {
                Some(target) => arrival[target] = arrival[target].max(ready),
                None => master_arrival = master_arrival.max(ready),
            }
            for &(target, _, _) in &buses[lane].sends {
                arrival[target] = arrival[target].max(ready);
            }
            for holder in lane + 1..count {
                if lanes[holder].keys_from(lane) {
                    arrival[holder] = arrival[holder].max(ready);
                }
            }
            if master.keys_from(lane) {
                master_arrival = master_arrival.max(ready);
            }
        }

        let ready = |lane: usize| arrival[lane] + latency[lane];
        let output_delays = (0..count)
            .map(|lane| {
                let due = buses[lane]
                    .output
                    .map_or(master_arrival, |target| arrival[target]);
                Delay::new(samples_for(due - ready(lane)))
            })
            .collect();
        let send_delays = (0..count)
            .map(|lane| {
                buses[lane]
                    .sends
                    .iter()
                    .map(|&(target, _, _)| Delay::new(samples_for(arrival[target] - ready(lane))))
                    .collect()
            })
            .collect();
        for (holder, chain) in lanes.iter_mut().enumerate() {
            chain.align_keys(arrival[holder], &arrival, &latency);
        }
        master.align_keys(master_arrival, &arrival, &latency);
        let keyed = (0..count)
            .map(|lane| lanes.iter().any(|chain| chain.keys_from(lane)) || master.keys_from(lane))
            .collect();

        Some(Self {
            total: master_arrival + master.latency(),
            arrival,
            lanes,
            master,
            master_arrival,
            clips,
            output_delays,
            send_delays,
            keyed,
            taps: vec![Vec::new(); count],
            edge: Vec::new(),
            preroll: Vec::new(),
            next: None,
            realtime: false,
        })
    }

    pub(super) fn set_realtime(&mut self, realtime: bool) {
        self.realtime = realtime;
    }

    fn reset(&mut self) {
        self.next = None;
        for chain in &mut self.lanes {
            chain.reset();
        }
        self.master.reset();
        for clip in self.clips.iter_mut().flatten() {
            clip.reset();
        }
        for delay in &mut self.output_delays {
            delay.reset();
        }
        for delay in self.send_delays.iter_mut().flatten() {
            delay.reset();
        }
    }

    /// Mix the output block at `pos` into `out`, pre-rolling first when `pos`
    /// does not continue the last block.
    fn mix(
        &mut self,
        mixer: &mut ExportAudioMixer,
        pos: i64,
        out: &mut [f32],
    ) -> Result<(), DecodeError> {
        if self.next != Some(pos) {
            self.reset();
            let mut preroll = std::mem::take(&mut self.preroll);
            let mut at = pos;
            let end = pos + self.total;
            while at < end {
                let until = (at + PREROLL_BLOCK).min(end);
                preroll.clear();
                preroll.resize(samples_for(until - at), 0.0);
                self.block(mixer, at, &mut preroll, false)?;
                at = until;
            }
            self.preroll = preroll;
        }
        self.block(mixer, pos + self.total, out, self.realtime)?;
        for sample in out.iter_mut() {
            *sample = sample.clamp(-1.0, 1.0);
        }
        self.next = Some(pos + (out.len() / CHANNELS) as i64);
        Ok(())
    }

    /// Run the graph over the block that starts at graph position `at`,
    /// leaving the master's output in `out`.
    fn block(
        &mut self,
        mixer: &mut ExportAudioMixer,
        at: i64,
        out: &mut [f32],
        realtime: bool,
    ) -> Result<(), DecodeError> {
        let frames = out.len() / CHANNELS;
        // The real time the block covers, in whole nanoseconds.
        let deadline = realtime.then(|| {
            Duration::from_nanos(frames as u64 * 1_000_000_000 / u64::from(EXPORT_AUDIO_RATE))
        });
        out.fill(0.0);
        let ExportAudioMixer {
            spans,
            buses,
            bus_buffers,
            scratch,
            warp_scratch,
            weight_scratch,
            peaks,
            ..
        } = mixer;
        for buffer in bus_buffers.iter_mut() {
            buffer.clear();
            buffer.resize(out.len(), 0.0);
        }
        for (span, clip) in spans.iter_mut().zip(&mut self.clips) {
            let bus = span.bus;
            let lane_pos = at - self.arrival[bus];
            match clip {
                None => ExportAudioMixer::mix_span_range(
                    span,
                    scratch,
                    warp_scratch,
                    weight_scratch,
                    lane_pos,
                    lane_pos + frames as i64,
                    &mut bus_buffers[bus],
                )?,
                Some(clip) => clip.mix(
                    span,
                    scratch,
                    warp_scratch,
                    weight_scratch,
                    lane_pos,
                    frames,
                    &mut bus_buffers[bus],
                    deadline,
                )?,
            }
        }

        let same = |time: i64| time;
        for lane in 0..buses.len() {
            if !buses[lane].live {
                peaks[lane] = [0.0; 2];
                if self.keyed[lane] {
                    self.taps[lane].clear();
                    self.taps[lane].resize(out.len(), 0.0);
                }
                continue;
            }
            self.lanes[lane].run(
                &mut bus_buffers[lane],
                at - self.arrival[lane],
                &same,
                &self.taps,
                deadline,
            );
            if self.keyed[lane] {
                self.taps[lane].clear();
                self.taps[lane].extend_from_slice(&bus_buffers[lane]);
            }
            let (upstream, downstream) = bus_buffers.split_at_mut(lane + 1);
            let sound = &mut upstream[lane];
            let sends = &buses[lane].sends;
            for (index, &(target, level, pre_fader)) in sends.iter().enumerate() {
                if pre_fader {
                    send_through(
                        &mut self.send_delays[lane][index],
                        &mut self.edge,
                        sound,
                        &mut downstream[target - lane - 1],
                        level,
                    );
                }
            }
            let gain = buses[lane].gain;
            if gain != 1.0 {
                for sample in sound.iter_mut() {
                    *sample *= gain;
                }
            }
            peaks[lane] = stereo_peak(sound);
            for (index, &(target, level, pre_fader)) in sends.iter().enumerate() {
                if !pre_fader {
                    send_through(
                        &mut self.send_delays[lane][index],
                        &mut self.edge,
                        sound,
                        &mut downstream[target - lane - 1],
                        level,
                    );
                }
            }
            let dest = match buses[lane].output {
                Some(target) => &mut downstream[target - lane - 1][..],
                None => &mut *out,
            };
            send_through(
                &mut self.output_delays[lane],
                &mut self.edge,
                sound,
                dest,
                1.0,
            );
        }
        self.master
            .run(out, at - self.master_arrival, &same, &self.taps, deadline);
        Ok(())
    }
}

impl ExportAudioMixer {
    /// [`mix_into`](Self::mix_into) for a project whose inserts process.
    pub(super) fn mix_inserted(&mut self, pos: i64, out: &mut [f32]) -> Result<(), DecodeError> {
        let mut graph = self
            .inserts
            .take()
            .expect("the insert path runs only with an insert graph");
        let mixed = graph.mix(self, pos, out);
        self.inserts = Some(graph);
        mixed
    }
}
