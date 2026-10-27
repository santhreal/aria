//! `startup` scenario: cold and warm time to an interactive editor window.

use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::fixtures::{FixtureId, ensure_fixture};
use crate::process::{WindowLaunch, measure_process_window_launch, start_private_xvfb};
use crate::scenarios::{BenchContext, open_editor};
use crate::{Comparison, SampleSeries, ScenarioId, ScenarioReport, TargetEvaluation};
fn find_aria_desktop_binary() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let mut candidates = Vec::new();
    if let Some(dir) = exe.parent() {
        candidates.push(dir.join("aria-desktop"));
        if let Some(parent) = dir.parent() {
            candidates.push(parent.join("aria-desktop"));
        }
    }
    candidates.into_iter().find(|p| p.is_file())
}
fn measure_inproc_interactive(data_dir: &Path) -> Result<f64, String> {
    let started = Instant::now();
    let (mut app, state, mut window) = open_editor(data_dir);
    window.draw();
    let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;

    drop(state);
    drop(window);
    app.update(|cx| cx.shutdown());
    Ok(elapsed_ms)
}

/// Push the `<kind>_process_window_ms` and `<kind>_process_first_frame_ms`
/// series of `launches` onto `samples` and return the first frame's p50, the
/// time to a window that draws. `None` when no launch was measured.
fn launch_series(
    samples: &mut Vec<SampleSeries>,
    kind: &str,
    launches: &[WindowLaunch],
) -> Option<f64> {
    if launches.is_empty() {
        return None;
    }
    let mapped = launches.iter().map(|launch| launch.mapped_ms).collect();
    let drawn = launches.iter().map(|launch| launch.drawn_ms).collect();
    samples.push(SampleSeries::from_values(
        format!("{kind}_process_window_ms"),
        "ms",
        mapped,
    ));
    let drawn = SampleSeries::from_values(format!("{kind}_process_first_frame_ms"), "ms", drawn);
    let p50 = drawn.p50;
    samples.push(drawn);
    Some(p50)
}

pub(crate) fn run(ctx: &BenchContext) -> Result<ScenarioReport, String> {
    let scratch = ctx.ensure_scratch_subdir("startup")?;
    let mut notes = Vec::new();
    let mut samples = Vec::new();

    let mut cold_inproc_samples = Vec::new();
    for i in 0..3usize {
        let dir = scratch.join(format!("inproc-cold-{i}"));
        std::fs::create_dir_all(&dir).map_err(|err| format!("mkdir failed: {err}"))?;
        cold_inproc_samples.push(measure_inproc_interactive(&dir)?);
    }

    let warm_dir = scratch.join("inproc-warm");
    std::fs::create_dir_all(&warm_dir).map_err(|err| format!("mkdir failed: {err}"))?;
    let _ = measure_inproc_interactive(&warm_dir)?;
    let mut warm_inproc_samples = Vec::new();
    for _ in 0..5usize {
        warm_inproc_samples.push(measure_inproc_interactive(&warm_dir)?);
    }

    let mut cold_process_samples = Vec::new();
    let mut warm_process_samples = Vec::new();
    let mut media_process_samples = Vec::new();
    if let (Some(xvfb), Some(bin)) = (start_private_xvfb(), find_aria_desktop_binary()) {
        notes.push(format!(
            "Measured full aria-desktop process spawn to mapped X11 window and to its first frame \
             drawn on a refresh on private Xvfb {}",
            xvfb.display
        ));
        for i in 0..3usize {
            let d = scratch.join(format!("proc-cold-data-{i}"));
            let c = scratch.join(format!("proc-cold-cache-{i}"));
            match measure_process_window_launch(&bin, &xvfb.display, &d, &c, &[]) {
                Ok(launch) => cold_process_samples.push(launch),
                Err(err) => notes.push(format!("cold launch {i}: {err}")),
            }
        }
        let warm_proc_data = scratch.join("proc-warm-data");
        let warm_proc_cache = scratch.join("proc-warm-cache");
        let _ = measure_process_window_launch(
            &bin,
            &xvfb.display,
            &warm_proc_data,
            &warm_proc_cache,
            &[],
        );
        for i in 0..3usize {
            match measure_process_window_launch(
                &bin,
                &xvfb.display,
                &warm_proc_data,
                &warm_proc_cache,
                &[],
            ) {
                Ok(launch) => warm_process_samples.push(launch),
                Err(err) => notes.push(format!("warm launch {i}: {err}")),
            }
        }
        // The editor probes media named on its command line before its window
        // opens, so this is the wait for the window over three codecs.
        let media = [
            FixtureId::H264_1080p60,
            FixtureId::Hevc2160p60,
            FixtureId::ProRes2160p30,
        ]
        .into_iter()
        .map(|id| ensure_fixture(&ctx.fixtures_dir, id).map(|fixture| fixture.path))
        .collect::<Result<Vec<PathBuf>, String>>()?;
        let named: Vec<&Path> = media.iter().map(PathBuf::as_path).collect();
        for i in 0..5usize {
            let d = scratch.join(format!("proc-media-data-{i}"));
            let c = scratch.join(format!("proc-media-cache-{i}"));
            match measure_process_window_launch(&bin, &xvfb.display, &d, &c, &named) {
                Ok(launch) => media_process_samples.push(launch),
                Err(err) => notes.push(format!("media launch {i}: {err}")),
            }
        }
    } else {
        notes.push(
            "Private Xvfb or aria-desktop binary unavailable; measured in-process PluginContext + AppState + GPUI Shell window draw"
                .to_string(),
        );
    }

    let cold_inproc_series =
        SampleSeries::from_values("cold_inproc_interactive_ms", "ms", cold_inproc_samples);
    let warm_inproc_series =
        SampleSeries::from_values("warm_inproc_interactive_ms", "ms", warm_inproc_samples);

    let cold_measured = launch_series(&mut samples, "cold", &cold_process_samples)
        .unwrap_or(cold_inproc_series.p50);
    if let Some(warm_drawn) = launch_series(&mut samples, "warm", &warm_process_samples) {
        notes.push(format!(
            "Warm process spawn-to-first-frame p50={warm_drawn:.2} ms; warm in-process \
             session+Shell draw p50={:.2} ms",
            warm_inproc_series.p50
        ));
    }
    let media_measured = launch_series(&mut samples, "media", &media_process_samples);

    let warm_measured = warm_inproc_series.p50;
    samples.push(cold_inproc_series);
    samples.push(warm_inproc_series);

    let mut targets = vec![
        TargetEvaluation::measured(
            "cold_interactive_ms",
            "Window interactive in 1 s cold",
            Comparison::AtMost,
            1000.0,
            "ms",
            cold_measured,
        ),
        TargetEvaluation::measured(
            "warm_interactive_ms",
            "Window interactive in 500 ms warm",
            Comparison::AtMost,
            500.0,
            "ms",
            warm_measured,
        ),
    ];
    if let Some(measured) = media_measured {
        targets.push(TargetEvaluation::measured(
            "media_interactive_ms",
            "Window interactive in 1 s with H.264, HEVC, and ProRes media named on the command line",
            Comparison::AtMost,
            1000.0,
            "ms",
            measured,
        ));
    }

    Ok(ScenarioReport::new(
        ScenarioId::Startup,
        ctx.env.clone(),
        targets,
        Vec::new(),
        samples,
        notes,
    ))
}
