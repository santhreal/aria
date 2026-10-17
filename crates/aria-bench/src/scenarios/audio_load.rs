//! `audio-load` scenario: 32 stereo tracks with four shipped effects each at
//! 48 kHz, 256-sample blocks across 60 seconds (0 underruns).

use std::path::Path;
use std::time::Instant;

use aria_models::{
    AudioEffectInstance, AudioInsertTarget, MediaSource, Param, Project, Rational, RationalTime,
    TimeRange, TrackKind,
};
use aria_render::ExportAudioMixer;

use crate::scenarios::BenchContext;
use crate::{Comparison, SampleSeries, ScenarioId, ScenarioReport, TargetEvaluation};

fn write_stereo_wav_48k(path: &Path, duration_secs: u32) -> Result<(), String> {
    let sample_rate = 48_000u32;
    let channels = 2u16;
    let bits_per_sample = 16u16;
    let total_frames = sample_rate * duration_secs;
    let data_bytes = total_frames * u32::from(channels) * u32::from(bits_per_sample / 8);
    let byte_rate = sample_rate * u32::from(channels) * u32::from(bits_per_sample / 8);
    let block_align = channels * (bits_per_sample / 8);

    let mut buf = Vec::with_capacity(44 + data_bytes as usize);
    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    buf.extend_from_slice(b"WAVE");
    buf.extend_from_slice(b"fmt ");
    buf.extend_from_slice(&16u32.to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes());
    buf.extend_from_slice(&channels.to_le_bytes());
    buf.extend_from_slice(&sample_rate.to_le_bytes());
    buf.extend_from_slice(&byte_rate.to_le_bytes());
    buf.extend_from_slice(&block_align.to_le_bytes());
    buf.extend_from_slice(&bits_per_sample.to_le_bytes());
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&data_bytes.to_le_bytes());

    // Precompute one 480-frame period (10 ms at 48 kHz) of a 400 Hz stereo wave
    // and repeat it so synthesizing 62 s of PCM takes < 5 ms.
    let period_frames = 480usize;
    let mut period_bytes = Vec::with_capacity(period_frames * 4);
    for i in 0..period_frames {
        let phase = (i as f32) * std::f32::consts::TAU * 4.0 / (period_frames as f32);
        let l = (phase.sin() * 12_000.0) as i16;
        let r = (phase.cos() * 12_000.0) as i16;
        period_bytes.extend_from_slice(&l.to_le_bytes());
        period_bytes.extend_from_slice(&r.to_le_bytes());
    }
    let repeats = (total_frames as usize) / period_frames;
    for _ in 0..repeats {
        buf.extend_from_slice(&period_bytes);
    }

    std::fs::write(path, &buf)
        .map_err(|err| format!("failed to write {}: {err}", path.display()))?;
    Ok(())
}

fn build_four_shipped_inserts() -> Result<Vec<AudioEffectInstance>, String> {
    let mut gain = AudioEffectInstance::new("gain");
    gain.set_param(0, Param::Constant(-3.0))
        .map_err(|err| format!("gain param: {err}"))?;

    let mut eq = AudioEffectInstance::new("parametric_eq");
    eq.set_param(1, Param::Constant(2.5))
        .map_err(|err| format!("parametric_eq band1_gain: {err}"))?;
    eq.set_param(4, Param::Constant(-2.0))
        .map_err(|err| format!("parametric_eq band2_gain: {err}"))?;
    eq.set_param(22, Param::Constant(1.5))
        .map_err(|err| format!("parametric_eq band8_gain: {err}"))?;

    let mut comp = AudioEffectInstance::new("compressor");
    comp.set_param(0, Param::Constant(-18.0))
        .map_err(|err| format!("compressor threshold: {err}"))?;
    comp.set_param(1, Param::Constant(4.0))
        .map_err(|err| format!("compressor ratio: {err}"))?;

    let mut limiter = AudioEffectInstance::new("limiter");
    limiter
        .set_param(0, Param::Constant(-1.0))
        .map_err(|err| format!("limiter ceiling: {err}"))?;

    Ok(vec![gain, eq, comp, limiter])
}

pub(crate) fn run(ctx: &BenchContext) -> Result<ScenarioReport, String> {
    let scratch = ctx.ensure_scratch_subdir("audio-load")?;
    let wav_path = scratch.join("audio-load-48k.wav");
    write_stereo_wav_48k(&wav_path, 62)?;

    let rate = Rational::FPS_60;
    let duration_ticks = 60 * 60;
    let mut project = Project::new("audio-load-32x4", rate);
    let media = project.add_media(MediaSource::new(
        &wav_path,
        0,
        0,
        rate,
        duration_ticks + 120,
        true,
    ));

    let inserts = build_four_shipped_inserts()?;
    for idx in 0..32usize {
        let track = project.add_track(TrackKind::Audio, format!("A{}", idx + 1));
        project
            .add_clip(
                track,
                media,
                TimeRange::at_rate(0, duration_ticks, rate),
                RationalTime::new(0, rate),
            )
            .map_err(|err| format!("failed to add audio clip on track {idx}: {err}"))?;
        project
            .set_audio_inserts(AudioInsertTarget::Track(track), inserts.clone())
            .map_err(|err| format!("failed to set 4 audio inserts on track {idx}: {err}"))?;
    }

    let sample_rate = 48_000u32;
    let block_frames = 256usize;
    let block_budget_ms = (block_frames as f64) * 1000.0 / f64::from(sample_rate);
    let total_blocks = (60 * sample_rate as usize) / block_frames; // 11,250 blocks = 60.0 s

    let mut mixer = ExportAudioMixer::for_project(&project)
        .map_err(|err| format!("ExportAudioMixer::for_project failed: {err}"))?
        .ok_or_else(|| "ExportAudioMixer::for_project returned None".to_string())?;

    let mut out = vec![0.0f32; block_frames * 2];

    // Warm the first block so all 32 GStreamer audio readers are open and
    // prerolled before the 60 s real-time callback loop begins.
    mixer
        .mix_into(0, &mut out)
        .map_err(|err| format!("warmup mix_into failed: {err}"))?;
    std::hint::black_box(out[0]);

    let mut block_ms_samples = Vec::with_capacity(total_blocks);
    let mut underruns = 0u64;
    for block_idx in 0..total_blocks {
        let pos = (block_idx * block_frames) as i64;
        let t0 = Instant::now();
        mixer
            .mix_into(pos, &mut out)
            .map_err(|err| format!("mix_into failed at block {block_idx}: {err}"))?;
        let elapsed_ms = t0.elapsed().as_secs_f64() * 1000.0;
        std::hint::black_box(out[0]);
        if elapsed_ms > block_budget_ms {
            underruns += 1;
        }
        block_ms_samples.push(elapsed_ms);
    }

    let block_series = SampleSeries::from_values("audio_block_256_ms", "ms", block_ms_samples);
    let targets = vec![TargetEvaluation::measured(
        "audio_underruns_60s",
        "32 tracks with four shipped effects each at 48 kHz, 256-sample blocks: 0 underruns in 60 s",
        Comparison::AtMost,
        0.0,
        "count",
        underruns as f64,
    )];

    let notes = vec![format!(
        "Mixed {} blocks (60.0 s at 48 kHz, 256 samples/block, budget {:.3} ms) across 32 tracks x 4 effects (gain, parametric_eq, compressor, limiter = 128 processors): p50={:.3} ms, p95={:.3} ms, p99={:.3} ms, max={:.3} ms, underruns={}",
        total_blocks,
        block_budget_ms,
        block_series.p50,
        block_series.p95,
        block_series.p99,
        block_series.max,
        underruns
    )];

    Ok(ScenarioReport::new(
        ScenarioId::AudioLoad,
        ctx.env.clone(),
        targets,
        Vec::new(),
        vec![block_series],
        notes,
    ))
}
