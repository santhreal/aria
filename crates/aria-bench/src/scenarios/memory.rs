//! `memory` scenario: resident memory (`VmRSS`) idle with `project-10k` open
//! (frame cache excluded), across 1,000 undo steps, and after project close.

use aria_commands::{Command, EditCommand};
use aria_models::{MarkerColor, RationalTime};
use aria_plugin::PluginContext;
use aria_ui::app::AppState;

use crate::env_probe::current_rss_mb;
use crate::fixtures::{FixtureId, ensure_fixture};
use crate::scenarios::BenchContext;
use crate::{
    Comparison, SampleSeries, ScenarioId, ScenarioReport, TargetEvaluation, mount_builtin_plugins,
    read_report,
};

pub(crate) fn run(ctx: &BenchContext) -> Result<ScenarioReport, String> {
    // When invoked from `aria-bench all`, run the measurement in a fresh child
    // process so earlier scenarios (4K decode, export, audio-load) do not
    // contaminate `/proc/self/status` VmRSS.
    if std::env::var_os("ARIA_BENCH_MEMORY_ISOLATED").is_none()
        && let Ok(exe) = std::env::current_exe()
    {
        let child_results = ctx.scratch_dir.join("memory-isolated-results");
        let _ = std::fs::remove_dir_all(&child_results);
        let _ = crate::process::run_isolated_memory_child(
            &exe,
            ctx.env.machine.as_str(),
            &ctx.fixtures_dir,
            &child_results,
        );
        let report_path = child_results.join("memory.json");
        if let Ok(report) = read_report(&report_path) {
            return Ok(report);
        }
    }

    let fixture = ensure_fixture(&ctx.fixtures_dir, FixtureId::Project10k)?;
    let scratch = ctx.ensure_scratch_subdir("memory")?;
    let baseline_rss_mb = current_rss_mb()?;

    let data_dir = scratch.join("data");
    std::fs::create_dir_all(&data_dir).map_err(|err| format!("mkdir failed: {err}"))?;
    let plugin_ctx = PluginContext::new();
    mount_builtin_plugins(&plugin_ctx, &data_dir)?;
    let mut state =
        AppState::new(plugin_ctx, data_dir).map_err(|err| format!("AppState::new: {err}"))?;
    // `AppState::new` starts the decode warm-up, which maps the codec plugin
    // libraries on a thread of its own. Awaiting it here counts those in the
    // empty session, not in the `project-10k` delta of whichever checkpoint
    // the thread happens to finish before.
    #[cfg(target_os = "linux")]
    aria_decoder::await_warm_up();

    let empty_session_rss_mb = current_rss_mb()?;

    state
        .open_project(fixture.path)
        .map_err(|err| format!("load_project(project-10k) failed: {err}"))?;

    let idle_project_10k_rss_mb = current_rss_mb()?;
    let project_10k_delta_mb = (idle_project_10k_rss_mb - empty_session_rss_mb).max(0.0);
    let rate = state.playhead.rate;
    for i in 0..1_000i64 {
        state
            .apply_command(Command::Edit(EditCommand::AddMarker {
                at: RationalTime::new(10_000 + i * 24, rate),
                name: format!("U{i}"),
                color: Some(MarkerColor::Blue),
            }))
            .map_err(|err| format!("AddMarker {i} failed: {err}"))?;
    }
    let after_1000_edits_rss_mb = current_rss_mb()?;
    let undo_1000_delta_mb = (after_1000_edits_rss_mb - idle_project_10k_rss_mb).max(0.0);

    state
        .new_project()
        .map_err(|err| format!("new_project failed: {err}"))?;
    let after_close_rss_mb = current_rss_mb()?;

    drop(state);

    let rss_series = SampleSeries::from_values(
        "rss_checkpoints_mb",
        "mb",
        vec![
            baseline_rss_mb,
            empty_session_rss_mb,
            idle_project_10k_rss_mb,
            after_1000_edits_rss_mb,
            after_close_rss_mb,
        ],
    );
    let delta_series = SampleSeries::from_values(
        "memory_deltas_mb",
        "mb",
        vec![project_10k_delta_mb, undo_1000_delta_mb],
    );

    let targets = vec![TargetEvaluation::measured(
        "idle_project_10k_rss_mb",
        "Idle with project-10k open, frame cache excluded: 500 MB resident",
        Comparison::AtMost,
        500.0,
        "mb",
        idle_project_10k_rss_mb,
    )];

    let notes = vec![format!(
        "VmRSS: baseline={:.1} MiB, empty AppState={:.1} MiB, idle project-10k open={:.1} MiB (+{:.1} MiB), after 1,000 undoable edits={:.1} MiB (+{:.1} MiB), after new_project={:.1} MiB",
        baseline_rss_mb,
        empty_session_rss_mb,
        idle_project_10k_rss_mb,
        project_10k_delta_mb,
        after_1000_edits_rss_mb,
        undo_1000_delta_mb,
        after_close_rss_mb
    )];

    Ok(ScenarioReport::new(
        ScenarioId::Memory,
        ctx.env.clone(),
        targets,
        Vec::new(),
        vec![rss_series, delta_series],
        notes,
    ))
}
