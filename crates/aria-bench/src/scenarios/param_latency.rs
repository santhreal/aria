//! `param-latency` scenario: dragging an effect value while paused updates the
//! Viewer within 2 display frames.

use std::time::Instant;

use aria_commands::{Command, EditCommand};
use aria_compositor::GpuContext;
use aria_engine::{Engine, EngineConfig};
use aria_models::{
    ColorAdjustments, EffectInstance, MediaSource, Param, Project, Rational, RationalTime,
    TimeRange, TrackKind,
};
use aria_render::SeekPolicy;

use crate::fixtures::{FixtureId, build_stack_4k, ensure_fixture};
use crate::scenarios::{BenchContext, GpuTextureSink};
use crate::{Comparison, SampleSeries, ScenarioId, ScenarioReport, TargetEvaluation};

pub(crate) fn run(ctx: &BenchContext) -> Result<ScenarioReport, String> {
    let h264 = ensure_fixture(&ctx.fixtures_dir, FixtureId::H264_1080p60)?;
    let _ = ensure_fixture(&ctx.fixtures_dir, FixtureId::Stack4k)?;

    let gpu = GpuContext::new_headless_blocking()
        .map_err(|err| format!("failed to create GpuContext: {err}"))?;
    let rate = Rational::FPS_60;
    let frame_period_ms = 1000.0 / rate.as_f64();

    // 1. Paused parameter drag on a 1080p clip with Gaussian Blur and Lumetri basic.
    let mut project = Project::new("param-latency-1080p", rate);
    let media = project.add_media(MediaSource::new(&h264.path, 1920, 1080, rate, 3_600, true));
    let v1 = project.add_track(TrackKind::Video, "V1");
    let clip_id = project
        .add_clip(
            v1,
            media,
            TimeRange::at_rate(0, 3_600, rate),
            RationalTime::new(0, rate),
        )
        .map_err(|err| format!("failed to add clip: {err}"))?;
    if let Some(clip) = project.timeline_mut().clip_mut(clip_id) {
        clip.effects.push(EffectInstance::new("gaussian_blur"));
    }
    project
        .set_effect_param(clip_id, 0, 0, 4.0)
        .map_err(|err| format!("set_effect_param failed: {err}"))?;
    project
        .set_clip_adjustments(
            clip_id,
            ColorAdjustments {
                exposure: Param::Constant(0.1),
                contrast: Param::Constant(0.1),
                saturation: Param::Constant(0.05),
                ..ColorAdjustments::default()
            },
        )
        .map_err(|err| format!("set_clip_adjustments failed: {err}"))?;

    let mut engine = Engine::with_renderer(
        EngineConfig::default(),
        project,
        aria_render::Renderer::with_gpu(gpu.share()),
    );

    let mut sink = GpuTextureSink::new(gpu.share());
    let paused_time = RationalTime::new(60, rate);

    // Prime the paused Viewer frame so the decoder and GPU pipelines are open.
    engine
        .get_frame_fit_into(paused_time, 960, 540, SeekPolicy::Exact, &mut sink)
        .map_err(|err| format!("initial paused render failed: {err}"))?;

    let mut drag_ms_samples = Vec::with_capacity(20);
    let mut drag_frames_samples = Vec::with_capacity(20);
    for step in 1..=20usize {
        let radius = 1.0 + (step as f32) * 0.75;
        let t0 = Instant::now();
        engine
            .apply(Command::Edit(EditCommand::SetEffectParam {
                clip: clip_id,
                index: 0,
                param: 0,
                value: radius,
            }))
            .map_err(|err| format!("SetEffectParam failed at step {step}: {err}"))?;
        engine
            .get_frame_fit_into(paused_time, 960, 540, SeekPolicy::Exact, &mut sink)
            .map_err(|err| format!("paused Viewer render failed at step {step}: {err}"))?;
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        drag_ms_samples.push(ms);
        drag_frames_samples.push(ms / frame_period_ms);
    }

    // 2. Also measure parameter drag on the 4-layer `stack-4k` fixture at paused 1/2 resolution.
    let stack_proj = build_stack_4k(&ctx.fixtures_dir)?;
    let top_video_clip = stack_proj
        .timeline()
        .tracks_ordered()
        .find(|t| t.kind == TrackKind::Video)
        .and_then(|t| t.clips_ordered().next().map(|c| c.id))
        .ok_or_else(|| "stack-4k has no video clip".to_string())?;
    let mut stack_engine = Engine::with_renderer(
        EngineConfig::default(),
        stack_proj,
        aria_render::Renderer::with_gpu(gpu.share()),
    );
    let mut stack_sink = GpuTextureSink::new(gpu);
    stack_sink.preallocate(1920, 1080);
    stack_engine
        .get_frame_fit_into(paused_time, 1920, 1080, SeekPolicy::Exact, &mut stack_sink)
        .map_err(|err| format!("stack-4k initial paused render failed: {err}"))?;

    let mut stack_drag_ms = Vec::with_capacity(10);
    for step in 1..=10usize {
        let radius = 2.0 + (step as f32) * 0.5;
        let t0 = Instant::now();
        stack_engine
            .apply(Command::Edit(EditCommand::SetEffectParam {
                clip: top_video_clip,
                index: 0,
                param: 0,
                value: radius,
            }))
            .map_err(|err| format!("stack-4k SetEffectParam failed: {err}"))?;
        stack_engine
            .get_frame_fit_into(paused_time, 1920, 1080, SeekPolicy::Exact, &mut stack_sink)
            .map_err(|err| format!("stack-4k paused render failed: {err}"))?;
        stack_drag_ms.push(t0.elapsed().as_secs_f64() * 1000.0);
    }

    let frames_series =
        SampleSeries::from_values("param_drag_display_frames", "frames", drag_frames_samples);
    let ms_series = SampleSeries::from_values("param_drag_ms", "ms", drag_ms_samples);
    let stack_series = SampleSeries::from_values("stack_4k_param_drag_ms", "ms", stack_drag_ms);

    let targets = vec![TargetEvaluation::measured(
        "param_drag_display_frames",
        "Dragging an effect value while paused: the Viewer shows the new value within 2 display frames",
        Comparison::AtMost,
        2.0,
        "frames",
        frames_series.p95,
    )];

    let notes = vec![format!(
        "1080p paused effect drag p50={:.2} ms ({:.2} frames at 60 Hz), p95={:.2} ms ({:.2} frames); stack-4k paused drag p50={:.2} ms",
        ms_series.p50, frames_series.p50, ms_series.p95, frames_series.p95, stack_series.p50
    )];

    Ok(ScenarioReport::new(
        ScenarioId::ParamLatency,
        ctx.env.clone(),
        targets,
        Vec::new(),
        vec![frames_series, ms_series, stack_series],
        notes,
    ))
}
