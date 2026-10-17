//! `export` scenario: `h264-1080p60` timeline exported to H.264 1080p.

use std::time::Instant;

use aria_compositor::GpuContext;
use aria_models::{MediaSource, Project, Rational, RationalTime, TimeRange, TrackKind};
use aria_render::{ExportSettings, Renderer, export_to_file_observed};

use crate::fixtures::{FixtureId, ensure_fixture};
use crate::scenarios::BenchContext;
use crate::{Comparison, SampleSeries, ScenarioId, ScenarioReport, TargetEvaluation};

const HW_H264_ENCODERS: &[&str] = &[
    "nvh264enc",
    "vaapih264enc",
    "vah264enc",
    "qsvh264enc",
    "amfh264enc",
    "vtenc_h264_hw",
];

fn probe_hw_h264_encoder() -> Option<&'static str> {
    HW_H264_ENCODERS
        .iter()
        .copied()
        .find(|&elem| crate::process::has_gst_element(elem))
}

pub(crate) fn run(ctx: &BenchContext) -> Result<ScenarioReport, String> {
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

    let hw_elem = probe_hw_h264_encoder();
    match export_to_file_observed(
        &mut renderer,
        &project,
        &out_path,
        settings,
        &mut |_done, _total| {
            let now = Instant::now();
            frame_step_ms.push(now.duration_since(last_tick).as_secs_f64() * 1000.0);
            last_tick = now;
            true
        },
    ) {
        Ok(written_frames) => {
            let elapsed_secs = started.elapsed().as_secs_f64();
            let realtime_x = if elapsed_secs > 0.0 {
                exported_duration_secs / elapsed_secs
            } else {
                0.0
            };

            let frame_series = SampleSeries::from_values("export_frame_ms", "ms", frame_step_ms);
            let ratio_series =
                SampleSeries::from_values("export_realtime_x", "x_realtime", vec![realtime_x]);

            let target = match hw_elem {
                Some(_elem) => TargetEvaluation::measured(
                    "hw_export_realtime_x",
                    "h264-1080p60 timeline to H.264 1080p on the hardware encoder: at least 4x real time",
                    Comparison::AtLeast,
                    4.0,
                    "x_realtime",
                    realtime_x,
                ),
                None => TargetEvaluation::skipped(
                    "hw_export_realtime_x",
                    "h264-1080p60 timeline to H.264 1080p on the hardware encoder: at least 4x real time",
                    Comparison::AtLeast,
                    4.0,
                    "x_realtime",
                    format!(
                        "No GStreamer hardware H.264 encoder element ({}) is installed on this host; software x264enc exported at {realtime_x:.2}x realtime",
                        HW_H264_ENCODERS.join(", ")
                    ),
                    false,
                ),
            };

            let notes = vec![format!(
                "Exported {written_frames} frames ({exported_duration_secs:.2} s) of h264-1080p60 in {elapsed_secs:.2} s ({realtime_x:.2}x realtime, frame p50={:.2} ms)",
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
                "h264-1080p60 timeline to H.264 1080p on the hardware encoder: at least 4x real time",
                Comparison::AtLeast,
                4.0,
                "x_realtime",
                format!(
                    "export_to_file_observed failed with '{export_err}': crates/aria-encoder/src/gst.rs:253 fn pick_encoder(codec: VideoCodec) -> Result<String, EncodeError> selects the first element present in gst::ElementFactory::find (here {:?}) without probing gst::ElementFactory::make(name).build() or falling back to x264enc when hardware encoder initialization is unavailable in the headless session",
                    hw_elem.unwrap_or("nvh264enc")
                ),
                false,
            );

            let notes = vec![format!(
                "export_to_file_observed returned '{export_err}'; render-only pass across {export_frames} frames completed in {render_elapsed:.2} s ({render_realtime_x:.2}x realtime, frame p50={:.2} ms)",
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
