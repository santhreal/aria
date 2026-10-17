//! `startup` scenario: cold and warm time to an interactive editor window.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use aria_plugin::PluginContext;
use aria_ui::app::AppState;
use aria_ui::shell::Shell;
use gpui::AppContext as _;

use crate::process::{measure_process_window_launch, start_private_xvfb};
use crate::scenarios::BenchContext;
use crate::{
    Comparison, SampleSeries, ScenarioId, ScenarioReport, TargetEvaluation, mount_builtin_plugins,
};
fn find_aria_desktop_binary() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let mut candidates = Vec::new();
    if let Some(dir) = exe.parent() {
        candidates.push(dir.join("aria-desktop"));
        if let Some(parent) = dir.parent() {
            candidates.push(parent.join("aria-desktop"));
        }
    }
    candidates.push(PathBuf::from(
        "/var/santh-cargo-target/release/aria-desktop",
    ));
    candidates.into_iter().find(|p| p.is_file())
}
fn measure_inproc_interactive(data_dir: &Path) -> Result<f64, String> {
    let started = Instant::now();
    let mut app = gpui::TestApp::with_text_system_and_assets(
        Arc::new(gpui::NoopTextSystem),
        Arc::new(aria_ui::icons::Assets),
    );
    let data_dir_owned = data_dir.to_path_buf();
    let state = app.update(|cx| {
        aria_ui::theme::Theme::install(
            aria_ui::theme::Appearance::Dark,
            Default::default(),
            None,
            cx,
        );
        let plugin_ctx = PluginContext::new();
        mount_builtin_plugins(&plugin_ctx, &data_dir_owned).expect("built-in plugins mount");
        cx.new(|_| AppState::new(plugin_ctx, data_dir_owned).expect("editor state"))
    });
    let owned = gpui::Entity::clone(&state);
    let mut window = app.open_window(|_, cx| Shell::new(owned, cx));
    window.draw();
    let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;

    drop(state);
    drop(window);
    app.update(|cx| cx.shutdown());
    Ok(elapsed_ms)
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
    if let (Some(xvfb), Some(bin)) = (start_private_xvfb(), find_aria_desktop_binary()) {
        notes.push(format!(
            "Measured full aria-desktop process spawn to mapped X11 window on private Xvfb {}",
            xvfb.display
        ));
        for i in 0..3usize {
            let d = scratch.join(format!("proc-cold-data-{i}"));
            let c = scratch.join(format!("proc-cold-cache-{i}"));
            if let Ok(ms) = measure_process_window_launch(&bin, &xvfb.display, &d, &c) {
                cold_process_samples.push(ms);
            }
        }
        let warm_proc_data = scratch.join("proc-warm-data");
        let warm_proc_cache = scratch.join("proc-warm-cache");
        let _ =
            measure_process_window_launch(&bin, &xvfb.display, &warm_proc_data, &warm_proc_cache);
        for _ in 0..3usize {
            if let Ok(ms) = measure_process_window_launch(
                &bin,
                &xvfb.display,
                &warm_proc_data,
                &warm_proc_cache,
            ) {
                warm_process_samples.push(ms);
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

    let cold_measured = if !cold_process_samples.is_empty() {
        let proc_series =
            SampleSeries::from_values("cold_process_window_ms", "ms", cold_process_samples);
        let p50 = proc_series.p50;
        samples.push(proc_series);
        p50
    } else {
        cold_inproc_series.p50
    };

    if !warm_process_samples.is_empty() {
        let warm_proc_series =
            SampleSeries::from_values("warm_process_window_ms", "ms", warm_process_samples);
        notes.push(format!(
            "Warm process spawn-to-window p50={:.2} ms; warm in-process session+Shell draw p50={:.2} ms",
            warm_proc_series.p50, warm_inproc_series.p50
        ));
        samples.push(warm_proc_series);
    }

    let warm_measured = warm_inproc_series.p50;
    samples.push(cold_inproc_series);
    samples.push(warm_inproc_series);

    let targets = vec![
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

    Ok(ScenarioReport::new(
        ScenarioId::Startup,
        ctx.env.clone(),
        targets,
        Vec::new(),
        samples,
        notes,
    ))
}
