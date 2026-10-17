//! `scrub` scenario: warm scrub (decoder open, within one GOP) and cold seek latency.

use std::time::Instant;

use aria_compositor::GpuContext;
use aria_decoder::{OutputMode, open_video_decoder};
use aria_models::{MediaSource, Project, Rational, RationalTime, TimeRange, TrackKind};
use aria_render::{Renderer, ResolveOverrides, SeekPolicy};

use crate::fixtures::{FixtureId, ensure_fixture};
use crate::scenarios::{BenchContext, GpuTextureSink};
use crate::{Comparison, SampleSeries, ScenarioId, ScenarioReport, TargetEvaluation};

pub(crate) fn run(ctx: &BenchContext) -> Result<ScenarioReport, String> {
    let h264 = ensure_fixture(&ctx.fixtures_dir, FixtureId::H264_1080p60)?;
    let hevc = ensure_fixture(&ctx.fixtures_dir, FixtureId::Hevc2160p60)?;
    let prores = ensure_fixture(&ctx.fixtures_dir, FixtureId::ProRes2160p30)?;
    let long_gop = ensure_fixture(&ctx.fixtures_dir, FixtureId::LongGopScrub)?;

    let rate60 = Rational::FPS_60;
    let rate30 = Rational::FPS_30;

    // 1. Warm scrub: decoder already open on h264-1080p60, scrubbing across
    // nearby frames and GOPs using `frame_at_nearest` and `frame_at`.
    let mut warm_dec = open_video_decoder(&h264.path, OutputMode::Cpu)
        .map_err(|err| format!("failed to open {}: {err}", h264.path.display()))?;
    let _ = warm_dec
        .frame_at(RationalTime::new(60, rate60))
        .map_err(|err| format!("initial warm seek failed: {err}"))?;

    let mut warm_scrub_samples = Vec::with_capacity(40);
    for step in 1..=20i64 {
        let tick = 60 + step * 2;
        let t0 = Instant::now();
        let frame = warm_dec
            .frame_at(RationalTime::new(tick, rate60))
            .map_err(|err| format!("warm scrub frame_at({tick}) failed: {err}"))?;
        std::hint::black_box(frame.map(|f| f.pts));
        warm_scrub_samples.push(t0.elapsed().as_secs_f64() * 1000.0);
    }
    for step in 1..=20i64 {
        let tick = (step * 90) % 3_500;
        let t0 = Instant::now();
        let frame = warm_dec
            .frame_at_nearest(RationalTime::new(tick, rate60))
            .map_err(|err| format!("warm scrub frame_at_nearest({tick}) failed: {err}"))?;
        std::hint::black_box(frame.map(|f| f.pts));
        warm_scrub_samples.push(t0.elapsed().as_secs_f64() * 1000.0);
    }

    // Also measure full Renderer interactive scrub (`SeekPolicy::NearestSync`) into GPU texture.
    let gpu = GpuContext::new_headless_blocking()
        .map_err(|err| format!("failed to create GpuContext: {err}"))?;
    let mut project = Project::new("scrub", rate60);
    let media = project.add_media(MediaSource::new(
        &h264.path, 1920, 1080, rate60, 3_600, true,
    ));
    let v1 = project.add_track(TrackKind::Video, "V1");
    project
        .add_clip(
            v1,
            media,
            TimeRange::at_rate(0, 3_600, rate60),
            RationalTime::new(0, rate60),
        )
        .map_err(|err| format!("failed to add clip: {err}"))?;
    let mut renderer = Renderer::with_gpu(gpu.share());
    let mut sink = GpuTextureSink::new(gpu);
    sink.preallocate(960, 540);
    renderer
        .render_frame_fit_into_with(
            &project,
            RationalTime::new(60, rate60),
            960,
            540,
            ResolveOverrides::default(),
            SeekPolicy::NearestSync,
            &mut sink,
        )
        .map_err(|err| format!("renderer warm-up failed: {err}"))?;

    let mut warm_renderer_samples = Vec::with_capacity(20);
    for step in 1..=20i64 {
        let tick = 60 + step * 3;
        let t0 = Instant::now();
        renderer
            .render_frame_fit_into_with(
                &project,
                RationalTime::new(tick, rate60),
                960,
                540,
                ResolveOverrides::default(),
                SeekPolicy::NearestSync,
                &mut sink,
            )
            .map_err(|err| format!("renderer scrub failed at {tick}: {err}"))?;
        warm_renderer_samples.push(t0.elapsed().as_secs_f64() * 1000.0);
    }

    // 2. Cold seek: open decoder + seek to distant targets across all 4 media fixtures.
    let mut cold_seek_samples = Vec::new();
    for &tick in &[300i64, 1_200, 2_700] {
        let t0 = Instant::now();
        let mut dec = open_video_decoder(&h264.path, OutputMode::Cpu)
            .map_err(|err| format!("cold open {} failed: {err}", h264.path.display()))?;
        let f = dec
            .frame_at(RationalTime::new(tick, rate60))
            .map_err(|err| format!("cold seek h264 at {tick} failed: {err}"))?;
        std::hint::black_box(f.map(|x| x.pts));
        cold_seek_samples.push(t0.elapsed().as_secs_f64() * 1000.0);
    }
    for &tick in &[300i64, 1_500] {
        let t0 = Instant::now();
        let mut dec = open_video_decoder(&hevc.path, OutputMode::Cpu)
            .map_err(|err| format!("cold open {} failed: {err}", hevc.path.display()))?;
        let f = dec
            .frame_at(RationalTime::new(tick, rate60))
            .map_err(|err| format!("cold seek hevc at {tick} failed: {err}"))?;
        std::hint::black_box(f.map(|x| x.pts));
        cold_seek_samples.push(t0.elapsed().as_secs_f64() * 1000.0);
    }
    for &tick in &[150i64, 600] {
        let t0 = Instant::now();
        let mut dec = open_video_decoder(&prores.path, OutputMode::Cpu)
            .map_err(|err| format!("cold open {} failed: {err}", prores.path.display()))?;
        let f = dec
            .frame_at_nearest(RationalTime::new(tick, rate30))
            .map_err(|err| format!("cold seek prores at {tick} failed: {err}"))?;
        std::hint::black_box(f.map(|x| x.pts));
        cold_seek_samples.push(t0.elapsed().as_secs_f64() * 1000.0);
    }

    // Also measure distant jumps in long-gop-scrub (10 s GOP = 300 frames).
    let mut long_gop_dec = open_video_decoder(&long_gop.path, OutputMode::Cpu)
        .map_err(|err| format!("open {} failed: {err}", long_gop.path.display()))?;
    let mut long_gop_samples = Vec::new();
    for &tick in &[915i64, 3_620, 9_030, 14_425] {
        let t0 = Instant::now();
        let f = long_gop_dec
            .frame_at(RationalTime::new(tick, rate30))
            .map_err(|err| format!("long-gop seek at {tick} failed: {err}"))?;
        std::hint::black_box(f.map(|x| x.pts));
        long_gop_samples.push(t0.elapsed().as_secs_f64() * 1000.0);
    }

    let warm_series = SampleSeries::from_values("warm_scrub_ms", "ms", warm_scrub_samples);
    let warm_render_series =
        SampleSeries::from_values("warm_renderer_scrub_ms", "ms", warm_renderer_samples);
    let cold_series = SampleSeries::from_values("cold_seek_ms", "ms", cold_seek_samples);
    let long_gop_series =
        SampleSeries::from_values("long_gop_exact_seek_ms", "ms", long_gop_samples);

    let targets = vec![
        TargetEvaluation::measured(
            "warm_scrub_ms",
            "Warm scrub (decoder open, frame in cache or one GOP away): next display frame (16.67 ms)",
            Comparison::AtMost,
            16.67,
            "ms",
            warm_series.p95,
        ),
        TargetEvaluation::measured(
            "cold_seek_ms",
            "Cold seek: 150 ms",
            Comparison::AtMost,
            150.0,
            "ms",
            cold_series.p50,
        ),
    ];

    let notes = vec![format!(
        "warm decoder scrub p50={:.2} ms (p95={:.2} ms), warm renderer scrub p50={:.2} ms, cold seek p50={:.2} ms (p95={:.2} ms), long-gop exact seek p50={:.2} ms",
        warm_series.p50,
        warm_series.p95,
        warm_render_series.p50,
        cold_series.p50,
        cold_series.p95,
        long_gop_series.p50
    )];

    Ok(ScenarioReport::new(
        ScenarioId::Scrub,
        ctx.env.clone(),
        targets,
        Vec::new(),
        vec![
            warm_series,
            warm_render_series,
            cold_series,
            long_gop_series,
        ],
        notes,
    ))
}
