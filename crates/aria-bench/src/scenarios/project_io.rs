//! `project-io` scenario: `project-10k` open to interactive and save latency.

use std::time::Instant;

use aria_models::{Project, Rational, RationalTime};

use crate::fixtures::{FixtureId, ensure_fixture};
use crate::scenarios::BenchContext;
use crate::{Comparison, SampleSeries, ScenarioId, ScenarioReport, TargetEvaluation};

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

    let open_series =
        SampleSeries::from_values("open_interactive_ms", "ms", open_interactive_samples);
    let deser_series = SampleSeries::from_values("deserialize_only_ms", "ms", deserialize_samples);
    let save_series = SampleSeries::from_values("save_ms", "ms", save_samples);

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
    ];

    let clip_count: usize = project
        .timeline()
        .tracks_ordered()
        .map(|t| t.clips_ordered().len())
        .sum();
    let notes = vec![format!(
        "project-10k verified: {} tracks, {} clips, {} markers; deserialize p50={:.2} ms, open+project+snap p50={:.2} ms, save p50={:.2} ms",
        project.timeline().tracks_ordered().count(),
        clip_count,
        project.timeline().markers().len(),
        deser_series.p50,
        open_series.p50,
        save_series.p50
    )];

    Ok(ScenarioReport::new(
        ScenarioId::ProjectIo,
        ctx.env.clone(),
        targets,
        Vec::new(),
        vec![open_series, deser_series, save_series],
        notes,
    ))
}
