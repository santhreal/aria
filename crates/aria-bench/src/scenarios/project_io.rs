//! `project-io` scenario: `project-10k` open to interactive, save latency, and
//! what an autosave of it costs the UI thread.

use std::path::Path;
use std::time::Instant;

use aria_commands::{Command, EditCommand};
use aria_models::{MarkerColor, Project, Rational, RationalTime};
use aria_plugin::PluginContext;
use aria_ui::app::AppState;
use aria_ui::app::recovery::AUTOSAVE_INTERVAL;

use crate::fixtures::{FixtureId, ensure_fixture};
use crate::scenarios::BenchContext;
use crate::{
    Comparison, SampleSeries, ScenarioId, ScenarioReport, TargetEvaluation, mount_builtin_plugins,
};

pub(crate) fn run(ctx: &BenchContext) -> Result<ScenarioReport, String> {
    let fixture = ensure_fixture(&ctx.fixtures_dir, FixtureId::Project10k)?;
    let scratch = ctx.ensure_scratch_subdir("project-io")?;
    let save_path = scratch.join("project-10k-save.aria");

    let playhead = RationalTime::new(0, Rational::FPS_24);
    let mut deserialize_samples = Vec::with_capacity(7);
    let mut open_interactive_samples = Vec::with_capacity(7);
    let mut save_samples = Vec::with_capacity(7);

    let mut loaded_project = None;
    for _ in 0..7usize {
        let t0 = Instant::now();
        let project = Project::load_from_file(&fixture.path)
            .map_err(|err| format!("failed to load {}: {err}", fixture.path.display()))?;
        let deser_ms = t0.elapsed().as_secs_f64() * 1000.0;
        let store = aria_ui::project_to_ui(&project, playhead, &[]);
        let snap_index = aria_ui::snap::index_for(&store);
        let open_ms = t0.elapsed().as_secs_f64() * 1000.0;
        std::hint::black_box(snap_index.rows().len());

        deserialize_samples.push(deser_ms);
        open_interactive_samples.push(open_ms);
        loaded_project = Some(project);
    }

    let project = loaded_project.ok_or_else(|| "no project loaded".to_string())?;
    for _ in 0..7usize {
        let t0 = Instant::now();
        project
            .save_to_file(&save_path)
            .map_err(|err| format!("failed to save {}: {err}", save_path.display()))?;
        let save_ms = t0.elapsed().as_secs_f64() * 1000.0;
        save_samples.push(save_ms);
    }

    let (autosave_tick_samples, autosave_edit_samples) = autosave_samples(&fixture.path, &scratch)?;

    let open_series =
        SampleSeries::from_values("open_interactive_ms", "ms", open_interactive_samples);
    let deser_series = SampleSeries::from_values("deserialize_only_ms", "ms", deserialize_samples);
    let save_series = SampleSeries::from_values("save_ms", "ms", save_samples);
    let autosave_tick_series =
        SampleSeries::from_values("autosave_tick_ms", "ms", autosave_tick_samples);
    let autosave_edit_series =
        SampleSeries::from_values("autosave_edit_ms", "ms", autosave_edit_samples);

    let targets = vec![
        TargetEvaluation::measured(
            "open_interactive_ms",
            "project-10k open to interactive in 500 ms",
            Comparison::AtMost,
            500.0,
            "ms",
            open_series.p50,
        ),
        TargetEvaluation::measured(
            "save_ms",
            "project-10k save in 200 ms",
            Comparison::AtMost,
            200.0,
            "ms",
            save_series.p50,
        ),
        TargetEvaluation::measured(
            "autosave_tick_ms",
            "project-10k autosave holds the UI thread at most 4 ms",
            Comparison::AtMost,
            4.0,
            "ms",
            autosave_tick_series.p50,
        ),
    ];

    let clip_count: usize = project
        .timeline()
        .tracks_ordered()
        .map(|t| t.clips_ordered().len())
        .sum();
    let notes = vec![format!(
        "project-10k verified: {} tracks, {} clips, {} markers; deserialize p50={:.2} ms, open+project+snap p50={:.2} ms, save p50={:.2} ms, autosave tick p50={:.2} ms, edit during the autosave write p50={:.2} ms",
        project.timeline().tracks_ordered().count(),
        clip_count,
        project.timeline().markers().len(),
        deser_series.p50,
        open_series.p50,
        save_series.p50,
        autosave_tick_series.p50,
        autosave_edit_series.p50
    )];

    Ok(ScenarioReport::new(
        ScenarioId::ProjectIo,
        ctx.env.clone(),
        targets,
        Vec::new(),
        vec![
            open_series,
            deser_series,
            save_series,
            autosave_tick_series,
            autosave_edit_series,
        ],
        notes,
    ))
}

/// What a due autosave of `project-10k` costs the thread that calls it, and
/// what an edit made right after it costs, over seven copies.
///
/// The session is the shipped one: [`AppState::autosave_tick`] on a dirty
/// `project-10k`, called with a clock one [`AUTOSAVE_INTERVAL`] later each
/// round so every call is due. The edit lands while the copy can still be
/// being written: a project still held by the writer is cloned by the next
/// edit. Each round waits for its copy before the next, so no write overlaps
/// another round's samples.
fn autosave_samples(fixture: &Path, scratch: &Path) -> Result<(Vec<f64>, Vec<f64>), String> {
    let data_dir = scratch.join("autosave");
    std::fs::create_dir_all(&data_dir)
        .map_err(|err| format!("could not create {}: {err}", data_dir.display()))?;
    let plugin_ctx = PluginContext::new();
    mount_builtin_plugins(&plugin_ctx, &data_dir)?;
    let mut state =
        AppState::new(plugin_ctx, data_dir).map_err(|err| format!("AppState::new: {err}"))?;
    // The decode warm-up maps the codec plugins on a thread of its own; it is
    // awaited so it does not compete with the first rounds.
    #[cfg(target_os = "linux")]
    aria_decoder::await_warm_up();
    state
        .open_project(fixture.to_path_buf())
        .map_err(|err| format!("open_project(project-10k) failed: {err}"))?;

    let start = Instant::now();
    let mut ticks = Vec::with_capacity(7);
    let mut edits = Vec::with_capacity(7);
    for round in 0..7u32 {
        add_marker(&mut state, 2 * i64::from(round))?;
        let t0 = Instant::now();
        let started = state.autosave_tick(start + AUTOSAVE_INTERVAL * (round + 1));
        ticks.push(t0.elapsed().as_secs_f64() * 1000.0);
        if !started {
            return Err(format!(
                "autosave round {round} started no recovery copy of a dirty project-10k"
            ));
        }
        let t0 = Instant::now();
        add_marker(&mut state, 2 * i64::from(round) + 1)?;
        edits.push(t0.elapsed().as_secs_f64() * 1000.0);
        state.finish_autosave();
    }
    Ok((ticks, edits))
}

/// One edit through the session, which leaves it dirty.
fn add_marker(state: &mut AppState, index: i64) -> Result<(), String> {
    let rate = state.playhead.rate;
    state
        .apply_command(Command::Edit(EditCommand::AddMarker {
            at: RationalTime::new(20_000 + index * 24, rate),
            name: format!("A{index}"),
            color: Some(MarkerColor::Green),
        }))
        .map(drop)
        .map_err(|err| format!("AddMarker {index} failed: {err}"))
}
