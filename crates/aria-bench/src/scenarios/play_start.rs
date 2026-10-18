//! `play-start` scenario: Space to the first new frame on screen (warm vs cold).

use std::time::Instant;

use aria_compositor::GpuContext;
use aria_models::{MediaSource, Project, Rational, RationalTime, TimeRange, TrackKind};
use aria_render::{ExportAudioMixer, Renderer, ResolveOverrides, SeekPolicy};

use crate::fixtures::{FixtureId, ensure_fixture};
use crate::scenarios::{BenchContext, GpuTextureSink};
use crate::{Comparison, SampleSeries, ScenarioId, ScenarioReport, TargetEvaluation};

pub(crate) fn run(ctx: &BenchContext) -> Result<ScenarioReport, String> {
    // The app starts the decode device warm-up with its preview worker, before
    // the GPU context and the window; the paused frame below then measures
    // what a session waits for after the warm-up had the GPU set-up to run in.
    #[cfg(target_os = "linux")]
    aria_decoder::warm_up();
    let h264 = ensure_fixture(&ctx.fixtures_dir, FixtureId::H264_1080p60)?;
    let rate = Rational::FPS_60;
    let mut project = Project::new("play-start", rate);
    let media = project.add_media(MediaSource::new(&h264.path, 1920, 1080, rate, 3_600, true));
    let v_track = project.add_track(TrackKind::Video, "V1");
    let a_track = project.add_track(TrackKind::Audio, "A1");
    project
        .add_clip(
            v_track,
            media,
            TimeRange::at_rate(0, 3_600, rate),
            RationalTime::new(0, rate),
        )
        .map_err(|err| format!("failed to add video clip: {err}"))?;
    project
        .add_clip(
            a_track,
            media,
            TimeRange::at_rate(0, 3_600, rate),
            RationalTime::new(0, rate),
        )
        .map_err(|err| format!("failed to add audio clip: {err}"))?;

    let gpu = GpuContext::new_headless_blocking()
        .map_err(|err| format!("failed to create GpuContext: {err}"))?;
    let mut sink = GpuTextureSink::new(gpu.share());
    sink.preallocate(1920, 1080);

    // The paused frame a session shows after opening the project, before any
    // Space. It opens the process's first decoder and with it the hardware
    // decode device, which every later pipeline shares; reported on its own
    // so the cold samples below measure Space, not device bring-up.
    let opened = Instant::now();
    Renderer::with_gpu(gpu.share())
        .render_frame_into_with(
            &project,
            RationalTime::new(0, rate),
            ResolveOverrides::default(),
            SeekPolicy::Exact,
            &mut sink,
        )
        .map_err(|err| format!("paused-frame render failed: {err}"))?;
    let paused_frame_ms = opened.elapsed().as_secs_f64() * 1000.0;

    // Cold play-start ("with no decoder open"): each sample constructs a fresh
    // Renderer (no open video decoder in its pool) and renders the first frame
    // at a non-zero playhead tick into the display GPU texture target.
    let cold_ticks = [60i64, 180, 300, 420, 540, 660, 780];
    let mut cold_samples = Vec::with_capacity(cold_ticks.len());
    for &tick in &cold_ticks {
        let mut renderer = Renderer::with_gpu(gpu.share());
        let t0 = Instant::now();
        renderer
            .render_frame_into_with(
                &project,
                RationalTime::new(tick, rate),
                ResolveOverrides::default(),
                SeekPolicy::Exact,
                &mut sink,
            )
            .map_err(|err| format!("cold play-start render failed at tick {tick}: {err}"))?;
        cold_samples.push(t0.elapsed().as_secs_f64() * 1000.0);
    }

    // Warm play-start: decoder and audio mixer are already open at the paused
    // playhead tick; pressing Space renders the next frame (tick + 1) into the
    // GPU texture target and fills the first 256-sample audio block.
    let mut renderer = Renderer::with_gpu(gpu);
    renderer
        .render_frame_into_with(
            &project,
            RationalTime::new(120, rate),
            ResolveOverrides::default(),
            SeekPolicy::Exact,
            &mut sink,
        )
        .map_err(|err| format!("warm-up render failed: {err}"))?;
    let mut audio_mixer = ExportAudioMixer::for_project(&project)
        .map_err(|err| format!("failed to open ExportAudioMixer: {err}"))?
        .ok_or_else(|| "ExportAudioMixer::for_project returned None".to_string())?;
    let mut audio_block = vec![0.0f32; 256 * 2];
    audio_mixer
        .mix_into(120 * 800, &mut audio_block)
        .map_err(|err| format!("warmup audio mix_into failed: {err}"))?;
    let mut warm_samples = Vec::with_capacity(30);
    for step in 1..=30i64 {
        let tick = 120 + step;
        let t0 = Instant::now();
        renderer
            .render_frame_into_with(
                &project,
                RationalTime::new(tick, rate),
                ResolveOverrides::default(),
                SeekPolicy::Exact,
                &mut sink,
            )
            .map_err(|err| format!("warm play-start render failed at tick {tick}: {err}"))?;
        audio_mixer
            .mix_into(tick * 800, &mut audio_block)
            .map_err(|err| format!("warm play-start audio mix_into failed: {err}"))?;
        std::hint::black_box(audio_block[0]);
        warm_samples.push(t0.elapsed().as_secs_f64() * 1000.0);
    }

    let warm_series = SampleSeries::from_values("warm_first_frame_ms", "ms", warm_samples);
    let cold_series = SampleSeries::from_values("cold_first_frame_ms", "ms", cold_samples);
    let paused_series =
        SampleSeries::from_values("paused_frame_after_open_ms", "ms", vec![paused_frame_ms]);

    let targets = vec![
        TargetEvaluation::measured(
            "warm_first_frame_ms",
            "Space to the first new frame on screen: 100 ms warm",
            Comparison::AtMost,
            100.0,
            "ms",
            warm_series.p95,
        ),
        TargetEvaluation::measured(
            "cold_first_frame_ms",
            "Space to the first new frame on screen: 300 ms with no decoder open",
            Comparison::AtMost,
            300.0,
            "ms",
            cold_series.p95,
        ),
    ];

    let stats = renderer.last_frame_stats();
    let notes = vec![format!(
        "warm p50={:.2} ms (p95={:.2} ms), cold p50={:.2} ms (p95={:.2} ms), paused frame after open {paused_frame_ms:.2} ms; last warm frame decode={:.2} ms composite={:.2} ms, GPU readbacks={}",
        warm_series.p50,
        warm_series.p95,
        cold_series.p50,
        cold_series.p95,
        stats.decode_ms,
        stats.composite_ms,
        renderer.readback_count()
    )];

    Ok(ScenarioReport::new(
        ScenarioId::PlayStart,
        ctx.env.clone(),
        targets,
        Vec::new(),
        vec![warm_series, cold_series, paused_series],
        notes,
    ))
}
