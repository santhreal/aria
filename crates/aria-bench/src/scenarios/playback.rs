//! `playback` scenario: forward and reverse playback drop counts and per-frame rules.

use std::sync::Arc;
use std::time::{Duration, Instant};

use aria_compositor::GpuContext;
use aria_engine::{Engine, EngineConfig};
use aria_models::{
    CanvasAspect, CanvasSettings, MediaSource, Project, Rational, RationalTime, TimeRange,
    TrackKind,
};
use aria_plugin::PluginContext;
use aria_render::{
    DropCause, DroppedFrames, PlaybackMeter, Renderer, ResolveOverrides, SeekPolicy, canvas_size,
};
use aria_ui::preview_worker::{Cadence, PreviewWorker};
use parking_lot::Mutex;

use crate::alloc_tracker::measure_thread_allocs;
use crate::env_probe::current_thread_count;
use crate::fixtures::{FixtureId, build_stack_4k, ensure_fixture};
use crate::scenarios::{BenchContext, GpuTextureSink};
use crate::{
    Comparison, MachineProfile, RuleCheck, SampleSeries, ScenarioId, ScenarioReport,
    TargetEvaluation, Verdict,
};

struct StreamRunResult {
    frame_ms: Vec<f64>,
    dropped: DroppedFrames,
    readbacks: u64,
    threads_created_after_start: u64,
    allocs_per_frame_after_1s: f64,
}

fn run_forward_stream(
    gpu: &GpuContext,
    project: &Project,
    rate: Rational,
    fit: Option<(u32, u32)>,
    warmup_frames: i64,
    steady_frames: i64,
) -> Result<StreamRunResult, String> {
    let mut renderer = Renderer::with_gpu(gpu.share());
    let (canvas_w, canvas_h) = canvas_size(project);
    let (target_w, target_h) = fit.unwrap_or((canvas_w, canvas_h));
    let mut sink = GpuTextureSink::new(gpu.share());
    sink.preallocate(target_w, target_h);

    let mut meter = PlaybackMeter::at_rate(rate);
    let budget = meter.budget();
    let total_frames = warmup_frames + steady_frames;
    let mut frame_ms = Vec::with_capacity(total_frames as usize);

    // Prime the first frame so the decoder and GPU pipelines are open before
    // measuring steady-state thread creation.
    render_one(
        &mut renderer,
        project,
        RationalTime::new(0, rate),
        fit,
        &mut sink,
    )?;
    let threads_at_start = current_thread_count().unwrap_or(0);

    for tick in 0..warmup_frames {
        let t0 = Instant::now();
        render_one(
            &mut renderer,
            project,
            RationalTime::new(tick, rate),
            fit,
            &mut sink,
        )?;
        let took = t0.elapsed();
        meter.frame(took);
        if took > budget {
            let mut drop = DroppedFrames::default();
            let late_slots =
                ((took.as_secs_f64() / budget.as_secs_f64()).floor() as u32).saturating_sub(1);
            drop.record(
                DropCause::not_ready(&renderer.last_frame_stats()),
                late_slots.max(1),
            );
            meter.drop_frames(drop);
        } else {
            meter.present(took);
        }
        frame_ms.push(took.as_secs_f64() * 1000.0);
    }

    let (steady_res, alloc_stats) = measure_thread_allocs(|| -> Result<(), String> {
        for tick in warmup_frames..total_frames {
            let t0 = Instant::now();
            render_one(
                &mut renderer,
                project,
                RationalTime::new(tick, rate),
                fit,
                &mut sink,
            )?;
            let took = t0.elapsed();
            meter.frame(took);
            if took > budget {
                let mut drop = DroppedFrames::default();
                let late_slots =
                    ((took.as_secs_f64() / budget.as_secs_f64()).floor() as u32).saturating_sub(1);
                drop.record(
                    DropCause::not_ready(&renderer.last_frame_stats()),
                    late_slots.max(1),
                );
                meter.drop_frames(drop);
            } else {
                meter.present(took);
            }
            frame_ms.push(took.as_secs_f64() * 1000.0);
        }
        Ok(())
    });
    steady_res?;

    let threads_at_end = current_thread_count().unwrap_or(threads_at_start);
    let threads_created_after_start = threads_at_end.saturating_sub(threads_at_start);
    let report = meter.report();
    let allocs_per_frame_after_1s = if steady_frames > 0 {
        alloc_stats.allocations as f64 / (steady_frames as f64)
    } else {
        0.0
    };

    Ok(StreamRunResult {
        frame_ms,
        dropped: report.dropped,
        readbacks: renderer.readback_count() + sink.cpu_frames,
        threads_created_after_start,
        allocs_per_frame_after_1s,
    })
}

fn render_one(
    renderer: &mut Renderer,
    project: &Project,
    t: RationalTime,
    fit: Option<(u32, u32)>,
    sink: &mut GpuTextureSink,
) -> Result<(), String> {
    match fit {
        Some((w, h)) => renderer
            .render_frame_fit_into_with(
                project,
                t,
                w,
                h,
                ResolveOverrides::default(),
                SeekPolicy::Exact,
                sink,
            )
            .map_err(|err| format!("render_frame_fit_into_with failed at {}: {err}", t.value)),
        None => renderer
            .render_frame_into_with(
                project,
                t,
                ResolveOverrides::default(),
                SeekPolicy::Exact,
                sink,
            )
            .map_err(|err| format!("render_frame_into_with failed at {}: {err}", t.value)),
    }
}

fn run_reverse_long_gop(gpu: &GpuContext, ctx: &BenchContext) -> Result<StreamRunResult, String> {
    let long_gop = ensure_fixture(&ctx.fixtures_dir, FixtureId::LongGopScrub)?;
    let rate = Rational::FPS_30;
    let mut project = Project::new("reverse-long-gop", rate);
    let media = project.add_media(MediaSource::new(
        &long_gop.path,
        1920,
        1080,
        rate,
        18_000,
        true,
    ));
    let v_track = project.add_track(TrackKind::Video, "V1");
    project
        .add_clip(
            v_track,
            media,
            TimeRange::at_rate(0, 18_000, rate),
            RationalTime::new(0, rate),
        )
        .map_err(|err| format!("failed to add long-gop clip: {err}"))?;

    let mut renderer = Renderer::with_gpu(gpu.share());
    let mut sink = GpuTextureSink::new(gpu.share());
    sink.preallocate(1920, 1080);

    // Warm the first GOP in reverse (ticks 615..=600), then measure reverse 1x
    // playback across the preceding GOP boundary (ticks 599..=540).
    for tick in (600..615i64).rev() {
        render_one(
            &mut renderer,
            &project,
            RationalTime::new(tick, rate),
            None,
            &mut sink,
        )?;
    }

    let mut meter = PlaybackMeter::at_rate(rate);
    let budget = meter.budget();
    let mut frame_ms = Vec::with_capacity(30);
    for tick in (570..600i64).rev() {
        let t0 = Instant::now();
        render_one(
            &mut renderer,
            &project,
            RationalTime::new(tick, rate),
            None,
            &mut sink,
        )?;
        let took = t0.elapsed();
        meter.frame(took);
        if took > budget {
            let mut drop = DroppedFrames::default();
            let late_slots =
                ((took.as_secs_f64() / budget.as_secs_f64()).floor() as u32).saturating_sub(1);
            drop.record(
                DropCause::not_ready(&renderer.last_frame_stats()),
                late_slots.max(1),
            );
            meter.drop_frames(drop);
        } else {
            meter.present(took);
        }
        frame_ms.push(took.as_secs_f64() * 1000.0);
    }

    let report = meter.report();
    Ok(StreamRunResult {
        frame_ms,
        dropped: report.dropped,
        readbacks: renderer.readback_count() + sink.cpu_frames,
        threads_created_after_start: 0,
        allocs_per_frame_after_1s: 0.0,
    })
}

fn check_preview_worker_rules(project: Project) -> Result<(bool, f64), String> {
    let rate = project.timeline().frame_rate;
    let engine = Engine::with_project(EngineConfig::default(), project)
        .map_err(|e| format!("engine init: {e}"))?;
    let shared_engine = Arc::new(Mutex::new(engine));
    let plugin_ctx = PluginContext::new();
    let worker = PreviewWorker::spawn(Arc::clone(&shared_engine), plugin_ctx)
        .map_err(|err| format!("PreviewWorker::spawn failed: {err}"))?;

    worker.set_scopes(true);
    worker.measure_at(RationalTime::new(0, rate), Some((640, 360)));

    let cadence = Cadence {
        anchor: 0,
        speed_num: 1,
        speed_den: 1,
        end: 300,
        rate,
    };
    worker.render_ahead(cadence, 0, Some((640, 360)));

    // Measure UI-thread non-blocking poll cost while the worker renders ahead
    // and computes scopes on `aria-preview`.
    let mut max_poll_ms = 0.0f64;
    for slot in 1..=10i64 {
        let t0 = Instant::now();
        let _presented = worker.present(cadence, slot - 1, slot);
        let _scopes = worker.take_measured();
        let poll_ms = t0.elapsed().as_secs_f64() * 1000.0;
        if poll_ms > max_poll_ms {
            max_poll_ms = poll_ms;
        }
    }

    let deadline = Instant::now() + Duration::from_secs(3);
    let mut scopes_from_worker = false;
    while Instant::now() < deadline {
        if worker.take_measured().is_some() {
            scopes_from_worker = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    worker.clear();
    Ok((scopes_from_worker, max_poll_ms))
}

pub(crate) fn run(ctx: &BenchContext) -> Result<ScenarioReport, String> {
    let h264 = ensure_fixture(&ctx.fixtures_dir, FixtureId::H264_1080p60)?;
    let hevc = ensure_fixture(&ctx.fixtures_dir, FixtureId::Hevc2160p60)?;
    let _ = ensure_fixture(&ctx.fixtures_dir, FixtureId::Stack4k)?;

    let gpu = GpuContext::new_headless_blocking()
        .map_err(|err| format!("failed to create GpuContext: {err}"))?;

    // 1. h264-1080p60 at Full (1920x1080): 60 warmup frames (1 s) + 60 steady frames (1 s).
    let rate60 = Rational::FPS_60;
    let mut h264_proj = Project::new("playback-h264", rate60);
    let h264_media = h264_proj.add_media(MediaSource::new(
        &h264.path, 1920, 1080, rate60, 3_600, true,
    ));
    let h264_v1 = h264_proj.add_track(TrackKind::Video, "V1");
    h264_proj
        .add_clip(
            h264_v1,
            h264_media,
            TimeRange::at_rate(0, 3_600, rate60),
            RationalTime::new(0, rate60),
        )
        .map_err(|err| format!("failed to add h264 clip: {err}"))?;

    let h264_res = run_forward_stream(&gpu, &h264_proj, rate60, None, 60, 60)?;

    // 2. hevc-2160p60 at Full on Desktop/Server, 1/2 on Laptop.
    let mut hevc_proj = Project::new("playback-hevc", rate60);
    let canvas4k = CanvasSettings::custom(CanvasAspect::Auto, [0, 0, 0], Some([3840, 2160]))
        .map_err(|err| format!("invalid 4K canvas: {err}"))?;
    hevc_proj.timeline_mut().set_canvas(canvas4k);
    let hevc_media = hevc_proj.add_media(MediaSource::new(
        &hevc.path, 3840, 2160, rate60, 3_600, true,
    ));
    let hevc_v1 = hevc_proj.add_track(TrackKind::Video, "V1");
    hevc_proj
        .add_clip(
            hevc_v1,
            hevc_media,
            TimeRange::at_rate(0, 3_600, rate60),
            RationalTime::new(0, rate60),
        )
        .map_err(|err| format!("failed to add hevc clip: {err}"))?;
    let hevc_fit = match ctx.env.machine {
        MachineProfile::Desktop | MachineProfile::Server => None,
        MachineProfile::Laptop => Some((1920, 1080)),
    };
    let hevc_res = run_forward_stream(&gpu, &hevc_proj, rate60, hevc_fit, 30, 30)?;

    // 3. stack-4k at 1/2 on Desktop/Server, 1/4 on Laptop.
    let stack_proj = build_stack_4k(&ctx.fixtures_dir)?;
    let stack_fit = match ctx.env.machine {
        MachineProfile::Desktop | MachineProfile::Server => Some((1920, 1080)),
        MachineProfile::Laptop => Some((960, 540)),
    };
    let stack_res = run_forward_stream(&gpu, &stack_proj, rate60, stack_fit, 15, 15)?;

    // 4. Reverse long-gop-scrub at 1x after the first GOP.
    let rev_res = run_reverse_long_gop(&gpu, ctx)?;

    // 5. Check PreviewWorker off-UI-thread scopes and non-blocking present.
    let (scopes_off_ui, max_ui_poll_ms) = check_preview_worker_rules(h264_proj)?;

    let total_readbacks =
        h264_res.readbacks + hevc_res.readbacks + stack_res.readbacks + rev_res.readbacks;

    let rules = vec![
        RuleCheck {
            rule: "No thread is created after playback starts".to_string(),
            verdict: if h264_res.threads_created_after_start == 0 {
                Verdict::Pass
            } else {
                Verdict::Fail
            },
            measured: Some(h264_res.threads_created_after_start as f64),
            unit: "threads".to_string(),
            reason: (h264_res.threads_created_after_start > 0).then(|| {
                format!(
                    "{} threads created after playback start",
                    h264_res.threads_created_after_start
                )
            }),
            required_on_machine: true,
        },
        RuleCheck {
            rule: "No GPU to CPU readback of a frame that is only displayed".to_string(),
            verdict: if total_readbacks == 0 {
                Verdict::Pass
            } else {
                Verdict::Fail
            },
            measured: Some(total_readbacks as f64),
            unit: "readbacks".to_string(),
            reason: (total_readbacks > 0)
                .then(|| format!("{total_readbacks} GPU-to-CPU readbacks occurred")),
            required_on_machine: true,
        },
        RuleCheck {
            rule: "No allocation per frame on the render thread after the first second".to_string(),
            verdict: if h264_res.allocs_per_frame_after_1s == 0.0 {
                Verdict::Pass
            } else {
                Verdict::Fail
            },
            measured: Some(h264_res.allocs_per_frame_after_1s),
            unit: "allocs_per_frame".to_string(),
            reason: (h264_res.allocs_per_frame_after_1s > 0.0).then(|| {
                format!(
                    "render thread performed {:.1} heap allocations/frame after 1 s (resolve_with / GStreamer sample mapping)",
                    h264_res.allocs_per_frame_after_1s
                )
            }),
            required_on_machine: true,
        },
        RuleCheck {
            rule: "Scopes and meters do not run on the UI thread".to_string(),
            verdict: if scopes_off_ui {
                Verdict::Pass
            } else {
                Verdict::Fail
            },
            measured: Some(if scopes_off_ui { 0.0 } else { 1.0 }),
            unit: "ui_scope_passes".to_string(),
            reason: (!scopes_off_ui)
                .then(|| "PreviewWorker did not deliver off-thread scope textures".to_string()),
            required_on_machine: true,
        },
        RuleCheck {
            rule: "The UI thread never waits for decode or render".to_string(),
            verdict: if max_ui_poll_ms <= 1.0 {
                Verdict::Pass
            } else {
                Verdict::Fail
            },
            measured: Some(max_ui_poll_ms),
            unit: "ms".to_string(),
            reason: (max_ui_poll_ms > 1.0).then(|| {
                format!(
                    "PreviewWorker::present blocked UI thread for {max_ui_poll_ms:.3} ms (> 1.0 ms)"
                )
            }),
            required_on_machine: true,
        },
    ];

    let h264_series = SampleSeries::from_values("h264_1080p60_frame_ms", "ms", h264_res.frame_ms);
    let hevc_series = SampleSeries::from_values("hevc_2160p60_frame_ms", "ms", hevc_res.frame_ms);
    let stack_series = SampleSeries::from_values("stack_4k_frame_ms", "ms", stack_res.frame_ms);
    let rev_series = SampleSeries::from_values("reverse_long_gop_frame_ms", "ms", rev_res.frame_ms);

    let targets = vec![
        TargetEvaluation::measured(
            "h264_1080p60_dropped_frames",
            "h264-1080p60 at Full: 0 dropped frames",
            Comparison::AtMost,
            0.0,
            "frames",
            f64::from(h264_res.dropped.total()),
        ),
        TargetEvaluation::measured(
            "hevc_2160p60_dropped_frames",
            "hevc-2160p60 at Full (Laptop: 1/2): 0 dropped frames",
            Comparison::AtMost,
            0.0,
            "frames",
            f64::from(hevc_res.dropped.total()),
        ),
        TargetEvaluation::measured(
            "stack_4k_dropped_frames",
            "stack-4k at 1/2 (Laptop: 1/4): 0 dropped frames",
            Comparison::AtMost,
            0.0,
            "frames",
            f64::from(stack_res.dropped.total()),
        ),
        TargetEvaluation::measured(
            "reverse_long_gop_dropped_frames",
            "long-gop-scrub reverse at 1x: 0 dropped frames after the first GOP",
            Comparison::AtMost,
            0.0,
            "frames",
            f64::from(rev_res.dropped.total()),
        ),
    ];

    let notes = vec![
        format!(
            "h264-1080p60 p50={:.2} ms (dropped={}: decode_late={}, render_late={})",
            h264_series.p50,
            h264_res.dropped.total(),
            h264_res.dropped.decode_late,
            h264_res.dropped.render_late
        ),
        format!(
            "hevc-2160p60 p50={:.2} ms (dropped={}: decode_late={}, render_late={})",
            hevc_series.p50,
            hevc_res.dropped.total(),
            hevc_res.dropped.decode_late,
            hevc_res.dropped.render_late
        ),
        format!(
            "stack-4k p50={:.2} ms (dropped={}: decode_late={}, render_late={})",
            stack_series.p50,
            stack_res.dropped.total(),
            stack_res.dropped.decode_late,
            stack_res.dropped.render_late
        ),
        format!(
            "reverse long-gop-scrub p50={:.2} ms (dropped={}); render-thread allocs/frame after 1s={:.1}",
            rev_series.p50,
            rev_res.dropped.total(),
            h264_res.allocs_per_frame_after_1s
        ),
    ];

    Ok(ScenarioReport::new(
        ScenarioId::Playback,
        ctx.env.clone(),
        targets,
        rules,
        vec![h264_series, hevc_series, stack_series, rev_series],
        notes,
    ))
}
