//! `export` scenario: `h264-1080p60` timeline exported to H.264 1080p.

use std::time::Instant;

use aria_compositor::GpuContext;
use aria_models::{MediaSource, Project, Rational, RationalTime, TimeRange, TrackKind};
use aria_render::{ExportSettings, Renderer, export_to_file_with_encoder_observed};

use crate::fixtures::{FixtureId, ensure_fixture};
use crate::scenarios::BenchContext;
use crate::{Comparison, SampleSeries, ScenarioId, ScenarioReport, TargetEvaluation};

const TARGET: &str =
    "h264-1080p60 timeline to H.264 1080p on the hardware encoder: at least 4x real time";

pub(crate) fn run(ctx: &BenchContext) -> Result<ScenarioReport, String> {
    // The app starts the decode device warm-up before its window opens, so an
    // export from it opens a warm device. Without this the
    // timed export carries the process's first CUDA initialisation (1.8 s on
    // the Desktop machine), a startup cost `play-start` measures cold. The
    // warm-up is awaited, not only started: a warm-up still running when the
    // timer starts contends with the encoder's own device open.
    #[cfg(target_os = "linux")]
    {
        aria_decoder::warm_up();
        aria_decoder::await_warm_up();
    }
    let h264 = ensure_fixture(&ctx.fixtures_dir, FixtureId::H264_1080p60)?;
    let scratch = ctx.ensure_scratch_subdir("export")?;
    let out_path = scratch.join("export-h264-1080p60.mp4");

    let rate = Rational::FPS_60;
    let export_frames = 120i64;
    let exported_duration_secs = (export_frames as f64) / rate.as_f64();

    let mut project = Project::new("export-h264-1080p60", rate);
    let media = project.add_media(MediaSource::new(&h264.path, 1920, 1080, rate, 3_600, true));
    let v1 = project.add_track(TrackKind::Video, "V1");
    let a1 = project.add_track(TrackKind::Audio, "A1");
    project
        .add_clip(
            v1,
            media,
            TimeRange::at_rate(0, export_frames, rate),
            RationalTime::new(0, rate),
        )
        .map_err(|err| format!("failed to add video clip: {err}"))?;
    project
        .add_clip(
            a1,
            media,
            TimeRange::at_rate(0, export_frames, rate),
            RationalTime::new(0, rate),
        )
        .map_err(|err| format!("failed to add audio clip: {err}"))?;

    let gpu = GpuContext::new_headless_blocking()
        .map_err(|err| format!("failed to create GpuContext: {err}"))?;
    let mut renderer = Renderer::with_gpu(gpu);
    let settings = ExportSettings::for_project(&project);

    let mut frame_step_ms = Vec::with_capacity(export_frames as usize);
    let started = Instant::now();
    let mut last_tick = started;
    match export_to_file_with_encoder_observed(
        &mut renderer,
        &project,
        &out_path,
        settings,
        &mut aria_encoder::open_encoder,
        &mut |_done, _total| {
            let now = Instant::now();
            frame_step_ms.push(now.duration_since(last_tick).as_secs_f64() * 1000.0);
            last_tick = now;
            true
        },
    ) {
        Ok(exported) => {
            let written_frames = exported.frames;
            let chosen = exported.encoder.to_string().replace('\n', "; ");
            let elapsed_secs = started.elapsed().as_secs_f64();
            let realtime_x = if elapsed_secs > 0.0 {
                exported_duration_secs / elapsed_secs
            } else {
                0.0
            };

            let frame_series = SampleSeries::from_values("export_frame_ms", "ms", frame_step_ms);
            let ratio_series =
                SampleSeries::from_values("export_realtime_x", "x_realtime", vec![realtime_x]);

            let target = match exported.encoder.hardware {
                Some(true) => TargetEvaluation::measured(
                    "hw_export_realtime_x",
                    TARGET,
                    Comparison::AtLeast,
                    4.0,
                    "x_realtime",
                    realtime_x,
                ),
                _ => TargetEvaluation::skipped(
                    "hw_export_realtime_x",
                    TARGET,
                    Comparison::AtLeast,
                    4.0,
                    "x_realtime",
                    format!(
                        "No hardware H.264 encoder opens on this host; exported with {chosen} at {realtime_x:.2}x realtime"
                    ),
                    false,
                ),
            };

            let notes = vec![format!(
                "Exported {written_frames} frames ({exported_duration_secs:.2} s) of h264-1080p60 with {chosen} in {elapsed_secs:.2} s ({realtime_x:.2}x realtime, frame p50={:.2} ms)",
                frame_series.p50
            )];

            Ok(ScenarioReport::new(
                ScenarioId::Export,
                ctx.env.clone(),
                vec![target],
                Vec::new(),
                vec![ratio_series, frame_series],
                notes,
            ))
        }
        Err(export_err) => {
            let mut render_only_ms = Vec::with_capacity(export_frames as usize);
            let render_started = Instant::now();
            for tick in 0..export_frames {
                let t0 = Instant::now();
                renderer
                    .render_frame(&project, RationalTime::new(tick, rate))
                    .map_err(|err| format!("fallback render_frame({tick}) failed: {err}"))?;
                render_only_ms.push(t0.elapsed().as_secs_f64() * 1000.0);
            }
            let render_elapsed = render_started.elapsed().as_secs_f64();
            let render_realtime_x = if render_elapsed > 0.0 {
                exported_duration_secs / render_elapsed
            } else {
                0.0
            };
            let render_series =
                SampleSeries::from_values("export_render_only_frame_ms", "ms", render_only_ms);

            let target = TargetEvaluation::skipped(
                "hw_export_realtime_x",
                TARGET,
                Comparison::AtLeast,
                4.0,
                "x_realtime",
                format!("export_to_file_with_encoder_observed failed: {export_err}"),
                false,
            );

            let notes = vec![format!(
                "export_to_file_with_encoder_observed returned '{export_err}'; render-only pass across {export_frames} frames completed in {render_elapsed:.2} s ({render_realtime_x:.2}x realtime, frame p50={:.2} ms)",
                render_series.p50
            )];

            Ok(ScenarioReport::new(
                ScenarioId::Export,
                ctx.env.clone(),
                vec![target],
                Vec::new(),
                vec![render_series],
                notes,
            ))
        }
    }
}
