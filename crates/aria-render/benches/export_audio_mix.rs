//! Production export audio mixing hot path (`ExportAudioMixer::mix_into`).
//!
//! Run: `cargo bench -p aria-render --bench export_audio_mix`

use aria_core::{AudioReader, DecodeError};
use aria_models::Param;
use aria_render::export_audio_bench::{BusNode, ExportAudioMixer, Span, SpanWarp};
use aria_render::{EXPORT_AUDIO_CHANNELS, EXPORT_AUDIO_RATE};
use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};

const CHANNELS: usize = EXPORT_AUDIO_CHANNELS as usize;

/// Pre-allocated in-memory audio reader holding synthesized PCM samples.
/// Avoids any disk I/O or decode overhead during benchmark measurement.
struct BufferAudioReader {
    samples: Vec<f32>,
    pos: i64,
}

impl BufferAudioReader {
    fn new(total_frames: usize, freq: f32) -> Self {
        let mut samples = vec![0.0; total_frames * CHANNELS];
        for f in 0..total_frames {
            let t = f as f32 / EXPORT_AUDIO_RATE as f32;
            let sample = (2.0 * std::f32::consts::PI * freq * t).sin() * 0.5;
            for ch in 0..CHANNELS {
                samples[f * CHANNELS + ch] = sample;
            }
        }
        Self { samples, pos: 0 }
    }
}

impl AudioReader for BufferAudioReader {
    fn read(&mut self, out: &mut [f32]) -> Result<usize, DecodeError> {
        let total_frames = self.samples.len() / CHANNELS;
        let start_frame = self.pos as usize;
        if start_frame >= total_frames {
            return Ok(0);
        }
        let available_frames = total_frames - start_frame;
        let want_frames = out.len() / CHANNELS;
        let copy_frames = want_frames.min(available_frames);
        out[..copy_frames * CHANNELS].copy_from_slice(
            &self.samples[start_frame * CHANNELS..(start_frame + copy_frames) * CHANNELS],
        );
        self.pos += copy_frames as i64;
        Ok(copy_frames)
    }

    fn seek_to_frame(&mut self, frame: i64) -> Result<(), DecodeError> {
        self.pos = frame;
        Ok(())
    }

    fn position(&self) -> Option<i64> {
        Some(self.pos)
    }
}

/// Build a representative multi-track and multi-bus mixer.
///
/// Graph layout:
/// - Bus 0: Dialogue Submix (gain 0.9, post-fader send to Bus 3, output: Bus 4)
/// - Bus 1: Music Submix (gain 0.8, output: Bus 4)
/// - Bus 2: SFX Submix (gain 1.0, post-fader send to Bus 3, output: Bus 4)
/// - Bus 3: Reverb Aux Bus (gain 0.7, output: Bus 4)
/// - Bus 4: Master Bus (gain 1.0, output: None -> programme out)
fn build_multitrack_mixer(
    track_count: usize,
    total_frames: usize,
    automated: bool,
) -> ExportAudioMixer {
    let buses = vec![
        // Bus 0: Dialogue Submix
        BusNode::new(0.9, Some(4), vec![(3, 0.25, false)]),
        // Bus 1: Music Submix
        BusNode::new(0.8, Some(4), Vec::new()),
        // Bus 2: SFX Submix
        BusNode::new(1.0, Some(4), vec![(3, 0.35, false)]),
        // Bus 3: Reverb Aux Send
        BusNode::new(0.7, Some(4), Vec::new()),
        // Bus 4: Master Bus
        BusNode::new(1.0, None, Vec::new()),
    ];

    let mut spans = Vec::with_capacity(track_count);
    for i in 0..track_count {
        // Distribute tracks across submix buses 0..3
        let bus = i % 3;
        let freq = 220.0 + (i as f32) * 55.0;
        let reader = Box::new(BufferAudioReader::new(total_frames, freq));

        let (volume, pan, fade_in, fade_out) = if automated {
            // Alternating pan (-0.8 .. +0.8), volume levels, and edge fades
            let pan_val = -0.8 + 1.6 * ((i % 5) as f32 / 4.0);
            let vol_val = 0.7 + 0.3 * ((i % 3) as f32 / 2.0);
            (
                Param::Constant(vol_val),
                Param::Constant(pan_val),
                480, // 10ms fade in at 48kHz
                480, // 10ms fade out at 48kHz
            )
        } else {
            // Unity passthrough
            (Param::Constant(1.0), Param::Constant(0.0), 0, 0)
        };

        spans.push(Span::linear(
            0,
            total_frames as i64,
            0,
            volume,
            pan,
            fade_in,
            fade_out,
            bus,
            Some(reader),
        ));
    }

    ExportAudioMixer::of_graph(spans, buses)
}

/// Build a mixer with retimed / warped and reversed spans.
fn build_warped_mixer(track_count: usize, total_frames: usize) -> ExportAudioMixer {
    let mut spans = Vec::with_capacity(track_count);
    for i in 0..track_count {
        let freq = 300.0 + (i as f32) * 100.0;
        let reader = Box::new(BufferAudioReader::new(total_frames * 2, freq));

        let warp = match i % 3 {
            0 => SpanWarp::FlatSpeed { num: 2, den: 1 }, // 2x speed
            1 => SpanWarp::FlatSpeed { num: 1, den: 2 }, // 0.5x speed
            _ => SpanWarp::Linear,
        };

        let reversed = i % 4 == 3;

        let mut span = Span::linear(
            0,
            total_frames as i64,
            0,
            Param::Constant(0.9),
            Param::Constant(0.0),
            0,
            0,
            0,
            Some(reader),
        );
        span.warp = warp;
        span.reversed = reversed;
        spans.push(span);
    }

    ExportAudioMixer::of_spans(spans)
}

fn bench_export_audio_multitrack(c: &mut Criterion) {
    const BLOCK_FRAMES: usize = 1024;
    const TOTAL_FRAMES: usize = 48_000 * 10;
    let mut group = c.benchmark_group("export_audio/multitrack");
    group.throughput(Throughput::Elements(BLOCK_FRAMES as u64));

    for &track_count in &[4, 8, 16] {
        let mut mixer = build_multitrack_mixer(track_count, TOTAL_FRAMES, true);
        let mut out = vec![0.0; BLOCK_FRAMES * CHANNELS];

        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{track_count}_tracks")),
            &track_count,
            |b, _| {
                let mut pos = 0i64;
                b.iter(|| {
                    mixer.mix_into(black_box(pos), black_box(&mut out)).unwrap();
                    pos = (pos + BLOCK_FRAMES as i64) % (TOTAL_FRAMES as i64 - BLOCK_FRAMES as i64);
                    black_box(&out);
                });
            },
        );
    }

    group.finish();
}

fn bench_export_audio_block_sizes(c: &mut Criterion) {
    const TOTAL_FRAMES: usize = 48_000 * 10;
    let mut group = c.benchmark_group("export_audio/block_sizes");

    for &block_frames in &[256, 512, 1024, 2048, 4096] {
        let mut mixer = build_multitrack_mixer(8, TOTAL_FRAMES, true);
        let mut out = vec![0.0; block_frames * CHANNELS];
        group.throughput(Throughput::Elements(block_frames as u64));

        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{block_frames}_frames")),
            &block_frames,
            |b, &frames| {
                let mut pos = 0i64;
                b.iter(|| {
                    mixer.mix_into(black_box(pos), black_box(&mut out)).unwrap();
                    pos = (pos + frames as i64) % (TOTAL_FRAMES as i64 - frames as i64);
                    black_box(&out);
                });
            },
        );
    }

    group.finish();
}

fn bench_export_audio_passthrough_vs_automated(c: &mut Criterion) {
    const BLOCK_FRAMES: usize = 1024;
    const TOTAL_FRAMES: usize = 48_000 * 10;
    let mut group = c.benchmark_group("export_audio/automation");
    group.throughput(Throughput::Elements(BLOCK_FRAMES as u64));

    let mut unity_mixer = build_multitrack_mixer(8, TOTAL_FRAMES, false);
    let mut out_unity = vec![0.0; BLOCK_FRAMES * CHANNELS];
    group.bench_function("8_tracks_unity_passthrough", |b| {
        let mut pos = 0i64;
        b.iter(|| {
            unity_mixer
                .mix_into(black_box(pos), black_box(&mut out_unity))
                .unwrap();
            pos = (pos + BLOCK_FRAMES as i64) % (TOTAL_FRAMES as i64 - BLOCK_FRAMES as i64);
            black_box(&out_unity);
        });
    });

    let mut auto_mixer = build_multitrack_mixer(8, TOTAL_FRAMES, true);
    let mut out_auto = vec![0.0; BLOCK_FRAMES * CHANNELS];
    group.bench_function("8_tracks_automated_pan_vol_fade", |b| {
        let mut pos = 0i64;
        b.iter(|| {
            auto_mixer
                .mix_into(black_box(pos), black_box(&mut out_auto))
                .unwrap();
            pos = (pos + BLOCK_FRAMES as i64) % (TOTAL_FRAMES as i64 - BLOCK_FRAMES as i64);
            black_box(&out_auto);
        });
    });

    group.finish();
}

fn bench_export_audio_warped(c: &mut Criterion) {
    const BLOCK_FRAMES: usize = 1024;
    const TOTAL_FRAMES: usize = 48_000 * 10;
    let mut group = c.benchmark_group("export_audio/warped");
    group.throughput(Throughput::Elements(BLOCK_FRAMES as u64));

    let mut mixer = build_warped_mixer(4, TOTAL_FRAMES);
    let mut out = vec![0.0; BLOCK_FRAMES * CHANNELS];

    group.bench_function("4_tracks_retimed_and_reversed", |b| {
        let mut pos = 0i64;
        b.iter(|| {
            mixer.mix_into(black_box(pos), black_box(&mut out)).unwrap();
            pos = (pos + BLOCK_FRAMES as i64) % (TOTAL_FRAMES as i64 - BLOCK_FRAMES as i64);
            black_box(&out);
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_export_audio_multitrack,
    bench_export_audio_block_sizes,
    bench_export_audio_passthrough_vs_automated,
    bench_export_audio_warped,
);
criterion_main!(benches);
