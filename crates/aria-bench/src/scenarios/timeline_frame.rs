//! `timeline-frame` scenario: `project-10k` scroll, zoom, and drag UI frame build latency.

use std::sync::Arc;
use std::time::Instant;

use aria_plugin::PluginContext;
use aria_ui::app::AppState;
use aria_ui::shell::{Moved, Shell};
use gpui::AppContext as _;

use crate::fixtures::{FixtureId, ensure_fixture};
use crate::scenarios::BenchContext;
use crate::{
    Comparison, SampleSeries, ScenarioId, ScenarioReport, TargetEvaluation, mount_builtin_plugins,
};

pub(crate) fn run(ctx: &BenchContext) -> Result<ScenarioReport, String> {
    let fixture = ensure_fixture(&ctx.fixtures_dir, FixtureId::Project10k)?;
    let scratch = ctx.ensure_scratch_subdir("timeline-frame")?;

    let mut app = gpui::TestApp::with_text_system_and_assets(
        Arc::new(gpui::NoopTextSystem),
        Arc::new(aria_ui::icons::Assets),
    );
    let data_dir = scratch.join("data");
    std::fs::create_dir_all(&data_dir).map_err(|err| format!("mkdir failed: {err}"))?;

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

    // Each step below changes what the matching pointer listener changes and
    // redraws the way that listener does: `Shell::redraw` with what moved.

    // 1. Scroll 30 frames across horizontal and vertical lanes on project-10k.
    let mut scroll_samples = Vec::with_capacity(30);
    for step in 0..30usize {
        let t0 = Instant::now();
        window.update(|shell, _, cx| {
            shell.state.update(cx, |s, _| {
                s.zoom_bar_pan(0.025);
                s.video_scroll_y = ((step % 15) as f32) * 24.0;
                s.audio_scroll_y = ((step % 15) as f32) * 24.0;
            });
            shell.redraw(Moved::View, cx);
        });
        window.draw();
        scroll_samples.push(t0.elapsed().as_secs_f64() * 1000.0);
    }

    // 2. Zoom 30 frames on project-10k: ctrl+wheel notches under a pointer a
    // third of the way into the lanes.
    let mut zoom_samples = Vec::with_capacity(30);
    for step in 0..30usize {
        let factor = if (step / 5).is_multiple_of(2) {
            1.15
        } else {
            1.0 / 1.15
        };
        let t0 = Instant::now();
        window.update(|shell, _, cx| {
            let (left, width) = {
                let s = shell.state.read(cx);
                (
                    s.timeline_content_left().unwrap_or(0.0),
                    s.timeline_viewport_px(),
                )
            };
            shell.zoom_at(left + width / 3.0, factor, cx);
        });
        window.draw();
        zoom_samples.push(t0.elapsed().as_secs_f64() * 1000.0);
    }

    // 3. Drag / scrub with snap index lookup across 30 frames on project-10k.
    let threshold = aria_ui::snap::threshold_ticks(80.0, 24.0);
    let mut drag_samples = Vec::with_capacity(30);
    for step in 0..30i64 {
        let desired = (step * 73) % 4_800;
        let t0 = Instant::now();
        window.update(|shell, _, cx| {
            shell.state.update(cx, |s, _| {
                let (snapped, _) = s.snap_at(desired, Some(1), threshold, false);
                s.set_playhead(snapped);
            });
            shell.redraw(Moved::Playhead, cx);
        });
        window.draw();
        drag_samples.push(t0.elapsed().as_secs_f64() * 1000.0);
    }

    // 4. The floor under every frame above: the window chrome redrawn with
    // every panel body replaying its last drawing. Not part of the target.
    let mut chrome_samples = Vec::with_capacity(30);
    for _ in 0..30 {
        let t0 = Instant::now();
        window.update(|shell, _, cx| shell.redraw_chrome(cx));
        window.draw();
        chrome_samples.push(t0.elapsed().as_secs_f64() * 1000.0);
    }

    let paint_stats = app.update(|cx| state.read(cx).timeline_paint_stats);

    drop(state);
    drop(window);
    app.update(|cx| cx.shutdown());

    let mut all_frames =
        Vec::with_capacity(scroll_samples.len() + zoom_samples.len() + drag_samples.len());
    all_frames.extend_from_slice(&scroll_samples);
    all_frames.extend_from_slice(&zoom_samples);
    all_frames.extend_from_slice(&drag_samples);

    let all_series = SampleSeries::from_values("ui_frame_build_ms", "ms", all_frames);
    let scroll_series = SampleSeries::from_values("scroll_frame_ms", "ms", scroll_samples);
    let zoom_series = SampleSeries::from_values("zoom_frame_ms", "ms", zoom_samples);
    let drag_series = SampleSeries::from_values("drag_frame_ms", "ms", drag_samples);
    let chrome_series = SampleSeries::from_values("chrome_frame_ms", "ms", chrome_samples);

    let targets = vec![TargetEvaluation::measured(
        "ui_frame_build_ms",
        "project-10k: scroll, zoom, and drag at display refresh, UI frame build at most 4 ms",
        Comparison::AtMost,
        4.0,
        "ms",
        all_series.p95,
    )];

    let notes = vec![format!(
        "project-10k GPUI Shell window.draw(): scroll p50={:.2} ms, zoom p50={:.2} ms, drag p50={:.2} ms, overall p95={:.2} ms, chrome alone p50={:.2} ms; TimelinePaintStats: total_lanes={}, visible_lanes={}, total_clips={}, visible_clips={}, canvas_elements={}, paint_calls={}",
        scroll_series.p50,
        zoom_series.p50,
        drag_series.p50,
        all_series.p95,
        chrome_series.p50,
        paint_stats.total_lanes,
        paint_stats.visible_lanes,
        paint_stats.total_clips,
        paint_stats.visible_clips,
        paint_stats.canvas_elements,
        paint_stats.paint_calls
    )];

    Ok(ScenarioReport::new(
        ScenarioId::TimelineFrame,
        ctx.env.clone(),
        targets,
        Vec::new(),
        vec![
            all_series,
            scroll_series,
            zoom_series,
            drag_series,
            chrome_series,
        ],
        notes,
    ))
}
