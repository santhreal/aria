//! `edit-apply` scenario: single edit and undo latency on `project-10k` in the
//! engine and until visible on the next display frame.

use std::sync::Arc;
use std::time::Instant;

use aria_commands::{Command, EditCommand};
use aria_engine::{Engine, EngineConfig};
use aria_models::{ClipParam, Easing, MarkerColor, ParamValue, Project, RationalTime, TimeRange};
use aria_plugin::PluginContext;
use aria_ui::app::AppState;
use aria_ui::shell::Shell;
use gpui::AppContext as _;

use crate::fixtures::{FixtureId, ensure_fixture};
use crate::scenarios::BenchContext;
use crate::{
    Comparison, SampleSeries, ScenarioId, ScenarioReport, TargetEvaluation, mount_builtin_plugins,
};

pub(crate) fn run(ctx: &BenchContext) -> Result<ScenarioReport, String> {
    let fixture = ensure_fixture(&ctx.fixtures_dir, FixtureId::Project10k)?;
    let project = Project::load_from_file(&fixture.path)
        .map_err(|err| format!("failed to load {}: {err}", fixture.path.display()))?;
    let rate = project.timeline().frame_rate;

    let first_track = project
        .timeline()
        .tracks_ordered()
        .find(|t| t.kind == aria_models::TrackKind::Video)
        .ok_or_else(|| "project-10k has no video tracks".to_string())?;
    let first_track_id = first_track.id;
    let clips: Vec<_> = first_track.clips_ordered().collect();
    let tail_clip = clips
        .last()
        .ok_or_else(|| "first track has no clips".to_string())?;
    let tail_id = tail_clip.id;
    let tail_start = tail_clip.timeline.start;
    let tail_dur = tail_clip.timeline.duration.value;
    let mid_clip_id = clips
        .get(100)
        .map(|c| c.id)
        .ok_or_else(|| "missing clip 100".to_string())?;
    let mid_start = clips[100].timeline.start;

    let mut engine = Engine::with_project(EngineConfig::default(), project)
        .map_err(|err| format!("Engine::with_project failed: {err}"))?;
    let mut engine_edit_samples = Vec::new();
    let mut engine_undo_samples = Vec::new();

    for iter in 0..10usize {
        let edits = [
            Command::Edit(EditCommand::TrimClip {
                clip: tail_id,
                timeline: TimeRange::at_rate(tail_start.value, (tail_dur - 2).max(1), rate),
            }),
            Command::Edit(EditCommand::MoveClip {
                clip: tail_id,
                to_track: first_track_id,
                start: RationalTime::new(tail_start.value + 300, rate),
            }),
            Command::Edit(EditCommand::AddMarker {
                at: RationalTime::new(40_000 + (iter as i64) * 24, rate),
                name: format!("EditBench{iter}"),
                color: Some(MarkerColor::Green),
            }),
            Command::Edit(EditCommand::SetParamKeyframe {
                clip: mid_clip_id,
                param: ClipParam::Opacity,
                at: RationalTime::new(mid_start.value + 2, rate),
                value: ParamValue::Scalar(0.65),
                easing: Easing::Linear,
                tangents: None,
            }),
            Command::Edit(EditCommand::SplitClip {
                clip: mid_clip_id,
                at: RationalTime::new(mid_start.value + (tail_dur / 2).max(1), rate),
            }),
            Command::Edit(EditCommand::RippleDelete { clip: tail_id }),
        ];

        for cmd in edits {
            let t0 = Instant::now();
            engine
                .apply(cmd)
                .map_err(|err| format!("engine edit failed: {err}"))?;
            engine_edit_samples.push(t0.elapsed().as_secs_f64() * 1000.0);

            let t_undo = Instant::now();
            let undone = engine.undo();
            if !undone {
                return Err("engine.undo() returned false after edit".to_string());
            }
            engine_undo_samples.push(t_undo.elapsed().as_secs_f64() * 1000.0);

            let t_redo = Instant::now();
            let redone = engine.redo();
            if !redone {
                return Err("engine.redo() returned false after undo".to_string());
            }
            engine_undo_samples.push(t_redo.elapsed().as_secs_f64() * 1000.0);

            let _ = engine.undo();
        }
    }

    // Measure end-to-end edit + UI projection + GPUI Shell window.draw() latency on project-10k.
    let scratch = ctx.ensure_scratch_subdir("edit-apply")?;
    let data_dir = scratch.join("data");
    std::fs::create_dir_all(&data_dir).map_err(|err| format!("mkdir failed: {err}"))?;

    let mut app = gpui::TestApp::with_text_system_and_assets(
        Arc::new(gpui::NoopTextSystem),
        Arc::new(aria_ui::icons::Assets),
    );
    let state = app.update(|cx| {
        aria_ui::theme::Theme::install(
            aria_ui::theme::Appearance::Dark,
            Default::default(),
            None,
            cx,
        );
        let plugin_ctx = PluginContext::new();
        mount_builtin_plugins(&plugin_ctx, &data_dir).expect("built-in plugins mount");
        cx.new(|_| AppState::new(plugin_ctx, data_dir).expect("editor state"))
    });
    let owned = gpui::Entity::clone(&state);
    let mut window = app.open_window(|_, cx| Shell::new(owned, cx));

    let fixture_path = fixture.path;
    app.update(|cx| {
        state.update(cx, |s, cx| {
            s.open_project(fixture_path).expect("load project-10k");
            s.mark_in = Some(0);
            s.mark_out = Some(24);
            cx.notify();
        });
    });
    window.draw();

    let mut visible_frame_samples = Vec::with_capacity(14);
    for iter in 0..7usize {
        let cmd = Command::Edit(EditCommand::TrimClip {
            clip: tail_id,
            timeline: TimeRange::at_rate(
                tail_start.value,
                (tail_dur - 1 - (iter as i64 % 3)).max(1),
                rate,
            ),
        });
        let t0 = Instant::now();
        app.update(|cx| {
            state.update(cx, |s, cx| {
                s.apply_command(cmd).expect("apply_command on project-10k");
                cx.notify();
            });
        });
        window.draw();
        visible_frame_samples.push(t0.elapsed().as_secs_f64() * 1000.0);
        settle(&mut app, &mut window);

        let t_undo = Instant::now();
        app.update(|cx| {
            state.update(cx, |s, cx| {
                assert!(s.undo(), "undo on project-10k");
                cx.notify();
            });
        });
        window.draw();
        visible_frame_samples.push(t_undo.elapsed().as_secs_f64() * 1000.0);
        settle(&mut app, &mut window);
    }

    drop(state);
    drop(window);
    app.update(|cx| cx.shutdown());

    let mut all_engine = Vec::with_capacity(engine_edit_samples.len() + engine_undo_samples.len());
    all_engine.extend_from_slice(&engine_edit_samples);
    all_engine.extend_from_slice(&engine_undo_samples);

    let engine_series = SampleSeries::from_values("engine_edit_or_undo_ms", "ms", all_engine);
    let edit_only_series = SampleSeries::from_values("engine_edit_ms", "ms", engine_edit_samples);
    let undo_only_series =
        SampleSeries::from_values("engine_undo_redo_ms", "ms", engine_undo_samples);
    let visible_series =
        SampleSeries::from_values("visible_next_frame_ms", "ms", visible_frame_samples);

    let targets = vec![
        TargetEvaluation::measured(
            "engine_edit_or_undo_ms",
            "Any single edit or undo on project-10k: 1 ms in the engine",
            Comparison::AtMost,
            1.0,
            "ms",
            engine_series.p95,
        ),
        TargetEvaluation::measured(
            "visible_next_frame_ms",
            "Any single edit or undo on project-10k: visible on the next display frame (16.67 ms)",
            Comparison::AtMost,
            16.67,
            "ms",
            visible_series.p95,
        ),
    ];

    let notes = vec![format!(
        "project-10k engine edit p50={:.4} ms (p95={:.4} ms), undo/redo p50={:.4} ms (p95={:.4} ms); full AppState::apply_command + UI re-project + GPUI Shell window.draw() p50={:.2} ms (p95={:.2} ms)",
        edit_only_series.p50,
        edit_only_series.p95,
        undo_only_series.p50,
        undo_only_series.p95,
        visible_series.p50,
        visible_series.p95
    )];

    Ok(ScenarioReport::new(
        ScenarioId::EditApply,
        ctx.env.clone(),
        targets,
        Vec::new(),
        vec![
            engine_series,
            edit_only_series,
            undo_only_series,
            visible_series,
        ],
        notes,
    ))
}

/// Run the work an edit left behind outside the timed window: the still frame
/// the drawn change asked the preview worker for, and the draw that shows it.
///
/// The test app runs background tasks on its one thread, so without this the
/// next sample's `update` waits for the previous change's preview render. In the
/// editor that render runs on a pool thread and the next edit does not wait for
/// it. Each sample therefore starts from a settled editor and measures one edit.
fn settle(app: &mut gpui::TestApp, window: &mut gpui::TestAppWindow<Shell>) {
    app.run_until_parked();
    window.draw();
    app.run_until_parked();
}
