//! `scrub` scenario: warm scrub (decoder open, within one GOP) and cold seek latency.

use std::path::Path;
use std::time::{Duration, Instant};

use aria_compositor::GpuContext;
use aria_decoder::{OutputMode, open_video_decoder, open_video_decoder_at};
use aria_engine::{CellRenderer, Engine, EngineConfig};
use aria_models::{MediaSource, Project, Rational, RationalTime, TimeRange, TrackKind};
use aria_render::{Renderer, ResolveOverrides, SeekPolicy};
use aria_ui::app::{AppState, FilmstripChanged};
use aria_ui::shell::{Moved, Shell};

use crate::fixtures::{FixtureId, ensure_fixture};
use crate::scenarios::{BenchContext, GpuTextureSink, open_editor, timed_frame};
use crate::{Comparison, SampleSeries, ScenarioId, ScenarioReport, TargetEvaluation};

/// The time between two pointer moves of a hand scrubbing the ruler: a
/// 125 Hz mouse.
const MOVE_GAP: Duration = Duration::from_millis(8);

/// The pointer moves one ruler scrub makes.
const MOVES: u32 = 30;

/// Zooms in by two about the middle of the lanes that follow the filmstrip's
/// first fill.
const REFILL_ZOOMS: u32 = 4;

/// Scrolls a view along that follow the zooms.
const REFILL_SCROLLS: u32 = 3;

/// Draws a filmstrip refill may take before it counts as stuck: many more
/// than the cells on screen.
const REFILL_DRAWS: usize = 4_096;

pub(crate) fn run(ctx: &BenchContext) -> Result<ScenarioReport, String> {
    let h264 = ensure_fixture(&ctx.fixtures_dir, FixtureId::H264_1080p60)?;
    let hevc = ensure_fixture(&ctx.fixtures_dir, FixtureId::Hevc2160p60)?;
    let prores = ensure_fixture(&ctx.fixtures_dir, FixtureId::ProRes2160p30)?;
    let long_gop = ensure_fixture(&ctx.fixtures_dir, FixtureId::LongGopScrub)?;

    let rate60 = Rational::FPS_60;
    let rate30 = Rational::FPS_30;

    // 1. Warm scrub: decoder already open on h264-1080p60, scrubbing across
    // nearby frames and GOPs using `frame_at_nearest` and `frame_at`.
    let mut warm_dec = open_video_decoder(&h264.path, OutputMode::Cpu)
        .map_err(|err| format!("failed to open {}: {err}", h264.path.display()))?;
    let _ = warm_dec
        .frame_at(RationalTime::new(60, rate60))
        .map_err(|err| format!("initial warm seek failed: {err}"))?;

    let mut warm_scrub_samples = Vec::with_capacity(40);
    for step in 1..=20i64 {
        let tick = 60 + step * 2;
        let t0 = Instant::now();
        let frame = warm_dec
            .frame_at(RationalTime::new(tick, rate60))
            .map_err(|err| format!("warm scrub frame_at({tick}) failed: {err}"))?;
        std::hint::black_box(frame.map(|f| f.pts));
        warm_scrub_samples.push(t0.elapsed().as_secs_f64() * 1000.0);
    }
    for step in 1..=20i64 {
        let tick = (step * 90) % 3_500;
        let t0 = Instant::now();
        let frame = warm_dec
            .frame_at_nearest(RationalTime::new(tick, rate60))
            .map_err(|err| format!("warm scrub frame_at_nearest({tick}) failed: {err}"))?;
        std::hint::black_box(frame.map(|f| f.pts));
        warm_scrub_samples.push(t0.elapsed().as_secs_f64() * 1000.0);
    }

    // Also measure full Renderer interactive scrub (`SeekPolicy::NearestSync`) into GPU texture.
    let gpu = GpuContext::new_headless_blocking()
        .map_err(|err| format!("failed to create GpuContext: {err}"))?;
    let mut project = Project::new("scrub", rate60);
    let media = project.add_media(MediaSource::new(
        &h264.path, 1920, 1080, rate60, 3_600, true,
    ));
    let v1 = project.add_track(TrackKind::Video, "V1");
    project
        .add_clip(
            v1,
            media,
            TimeRange::at_rate(0, 3_600, rate60),
            RationalTime::new(0, rate60),
        )
        .map_err(|err| format!("failed to add clip: {err}"))?;
    let engine_gpu = gpu.share();
    let mut renderer = Renderer::with_gpu(gpu.share());
    let mut sink = GpuTextureSink::new(gpu);
    sink.preallocate(960, 540);
    renderer
        .render_frame_fit_into_with(
            &project,
            RationalTime::new(60, rate60),
            960,
            540,
            ResolveOverrides::default(),
            SeekPolicy::NearestSync,
            &mut sink,
        )
        .map_err(|err| format!("renderer warm-up failed: {err}"))?;

    let mut warm_renderer_samples = Vec::with_capacity(20);
    for step in 1..=20i64 {
        let tick = 60 + step * 3;
        let t0 = Instant::now();
        renderer
            .render_frame_fit_into_with(
                &project,
                RationalTime::new(tick, rate60),
                960,
                540,
                ResolveOverrides::default(),
                SeekPolicy::NearestSync,
                &mut sink,
            )
            .map_err(|err| format!("renderer scrub failed at {tick}: {err}"))?;
        warm_renderer_samples.push(t0.elapsed().as_secs_f64() * 1000.0);
    }

    // The Viewer's playback frames while the timeline's filmstrip fills. The
    // filmstrip thread takes the engine between two frames of the preview
    // worker's, so each frame, rendered as the worker renders it, follows one
    // cell, rendered as the filmstrip renders it, at the cell's tick: a strip's
    // cells walk the lane a cell step apart, away from the playhead. The same
    // frames with no cell between them follow. A cell is timed whole, and for
    // the part of it that needs the engine: taking its source.
    let mut engine_sink = GpuTextureSink::new(engine_gpu.share());
    engine_sink.preallocate(960, 540);
    let mut engine = Engine::with_renderer(
        EngineConfig::default(),
        project.clone(),
        Renderer::with_gpu(engine_gpu),
    );
    let mut play = |engine: &mut Engine, tick: i64| {
        let t0 = Instant::now();
        engine
            .get_frame_fit_into(
                RationalTime::new(tick, rate60),
                960,
                540,
                SeekPolicy::Exact,
                &mut engine_sink,
            )
            .map_err(|err| format!("playback frame at {tick} failed: {err}"))?;
        Ok::<f64, String>(t0.elapsed().as_secs_f64() * 1000.0)
    };
    for tick in 570..600i64 {
        play(&mut engine, tick)?;
    }
    let mut cells = CellRenderer::default();
    let mut after_cell_samples = Vec::with_capacity(30);
    let mut cell_samples = Vec::with_capacity(30);
    let mut cell_engine_samples = Vec::with_capacity(30);
    for (n, tick) in (600..630i64).enumerate() {
        let cell = RationalTime::new(1_800 + 64 * n as i64, rate60);
        let t0 = Instant::now();
        let source = engine.cell_source();
        cell_engine_samples.push(t0.elapsed().as_secs_f64() * 1000.0);
        std::hint::black_box(aria_ui::filmstrips::render_cell(
            &mut cells,
            &source,
            cell,
            SeekPolicy::Exact,
        ));
        drop(source);
        cell_samples.push(t0.elapsed().as_secs_f64() * 1000.0);
        after_cell_samples.push(play(&mut engine, tick)?);
    }
    let mut no_cell_samples = Vec::with_capacity(30);
    for tick in 630..660i64 {
        no_cell_samples.push(play(&mut engine, tick)?);
    }

    // 2. Cold seek: open a decoder at a distant target and decode the frame
    // there, as the renderer opens one for a clip it has no decoder for,
    // across 3 of the 4 media fixtures.
    let mut cold_seek_samples = Vec::new();
    for &tick in &[300i64, 1_200, 2_700] {
        let t0 = Instant::now();
        let target = RationalTime::new(tick, rate60);
        let mut dec = open_video_decoder_at(&h264.path, OutputMode::Cpu, target)
            .map_err(|err| format!("cold open {} failed: {err}", h264.path.display()))?;
        let f = dec
            .frame_at(target)
            .map_err(|err| format!("cold seek h264 at {tick} failed: {err}"))?;
        std::hint::black_box(f.map(|x| x.pts));
        cold_seek_samples.push(t0.elapsed().as_secs_f64() * 1000.0);
    }
    for &tick in &[300i64, 1_500] {
        let t0 = Instant::now();
        let target = RationalTime::new(tick, rate60);
        let mut dec = open_video_decoder_at(&hevc.path, OutputMode::Cpu, target)
            .map_err(|err| format!("cold open {} failed: {err}", hevc.path.display()))?;
        let f = dec
            .frame_at(target)
            .map_err(|err| format!("cold seek hevc at {tick} failed: {err}"))?;
        std::hint::black_box(f.map(|x| x.pts));
        cold_seek_samples.push(t0.elapsed().as_secs_f64() * 1000.0);
    }
    for &tick in &[150i64, 600] {
        let t0 = Instant::now();
        let target = RationalTime::new(tick, rate30);
        let mut dec = open_video_decoder_at(&prores.path, OutputMode::Cpu, target)
            .map_err(|err| format!("cold open {} failed: {err}", prores.path.display()))?;
        let f = dec
            .frame_at_nearest(target)
            .map_err(|err| format!("cold seek prores at {tick} failed: {err}"))?;
        std::hint::black_box(f.map(|x| x.pts));
        cold_seek_samples.push(t0.elapsed().as_secs_f64() * 1000.0);
    }

    // Also measure distant jumps in long-gop-scrub (10 s GOP = 300 frames).
    let mut long_gop_dec = open_video_decoder(&long_gop.path, OutputMode::Cpu)
        .map_err(|err| format!("open {} failed: {err}", long_gop.path.display()))?;
    let mut long_gop_samples = Vec::new();
    for &tick in &[915i64, 3_620, 9_030, 14_425] {
        let t0 = Instant::now();
        let f = long_gop_dec
            .frame_at(RationalTime::new(tick, rate30))
            .map_err(|err| format!("long-gop seek at {tick} failed: {err}"))?;
        std::hint::black_box(f.map(|x| x.pts));
        long_gop_samples.push(t0.elapsed().as_secs_f64() * 1000.0);
    }

    // 3. A paused scrub on the timeline ruler in the editor. Each sample is one
    // pointer move as the ruler's held press takes it: the playhead to the
    // tick under the pointer, the Viewer frame asked for, and the playhead
    // redrawn, `MOVE_GAP` after the move before. Over `h264-1080p60` each move
    // is six frames, within a GOP; over the long-GOP file each move is a
    // second, a seek the worker is still decoding when the next move comes.
    // Not part of a target.
    let ruler_scrub_samples = ruler_scrub(ctx, "ruler-h264", &h264.path, 10.0, 0.1)?;
    let ruler_long_gop_samples = ruler_scrub(ctx, "ruler-long-gop", &long_gop.path, 100.0, 1.0)?;

    // 4. The timeline's filmstrip filling after the view moves, over
    // `h264-1080p60` on V1, then over `hevc-2160p60`, whose cells each decode
    // a 4K frame: until no cell on screen is drawn empty, and until every
    // cell shows its exact frame. Not part of a target.
    let (cover_samples, refill_samples) = filmstrip_refill(ctx, "filmstrip-refill", &h264.path)?;
    let (cover_hevc_samples, refill_hevc_samples) =
        filmstrip_refill(ctx, "filmstrip-refill-hevc", &hevc.path)?;

    let warm_series = SampleSeries::from_values("warm_scrub_ms", "ms", warm_scrub_samples);
    let warm_render_series =
        SampleSeries::from_values("warm_renderer_scrub_ms", "ms", warm_renderer_samples);
    let cold_series = SampleSeries::from_values("cold_seek_ms", "ms", cold_seek_samples);
    let long_gop_series =
        SampleSeries::from_values("long_gop_exact_seek_ms", "ms", long_gop_samples);
    let ruler_series = SampleSeries::from_values("ruler_scrub_move_ms", "ms", ruler_scrub_samples);
    let ruler_long_gop_series =
        SampleSeries::from_values("ruler_scrub_long_gop_move_ms", "ms", ruler_long_gop_samples);
    let after_cell_series = SampleSeries::from_values(
        "playback_frame_after_filmstrip_cell_ms",
        "ms",
        after_cell_samples,
    );
    let no_cell_series =
        SampleSeries::from_values("playback_frame_no_cell_ms", "ms", no_cell_samples);
    let cell_series = SampleSeries::from_values("filmstrip_cell_ms", "ms", cell_samples);
    let cell_engine_series =
        SampleSeries::from_values("filmstrip_cell_engine_ms", "ms", cell_engine_samples);
    let cover_series = SampleSeries::from_values("filmstrip_cover_ms", "ms", cover_samples);
    let cover_hevc_series =
        SampleSeries::from_values("filmstrip_cover_hevc_ms", "ms", cover_hevc_samples);
    let refill_series = SampleSeries::from_values("filmstrip_refill_ms", "ms", refill_samples);
    let refill_hevc_series =
        SampleSeries::from_values("filmstrip_refill_hevc_ms", "ms", refill_hevc_samples);

    let targets = vec![
        TargetEvaluation::measured(
            "warm_scrub_ms",
            "Warm scrub (decoder open, frame in cache or one GOP away): next display frame (16.67 ms)",
            Comparison::AtMost,
            16.67,
            "ms",
            warm_series.p95,
        ),
        TargetEvaluation::measured(
            "cold_seek_ms",
            "Cold seek: 150 ms",
            Comparison::AtMost,
            150.0,
            "ms",
            cold_series.p50,
        ),
    ];

    let notes = vec![format!(
        "warm decoder scrub p50={:.2} ms (p95={:.2} ms), warm renderer scrub p50={:.2} ms, filmstrip cell p50={:.2} ms (p95={:.2} ms) of which the engine p50={:.2} ms, filmstrip after a view move covered p50={:.2} ms (max={:.2} ms) and refilled p50={:.2} ms (max={:.2} ms), over hevc-2160p60 covered p50={:.2} ms (max={:.2} ms) and refilled p50={:.2} ms (max={:.2} ms), playback frame after a filmstrip cell p50={:.2} ms (p95={:.2} ms), with no cell p50={:.2} ms, cold seek p50={:.2} ms (p95={:.2} ms), long-gop exact seek p50={:.2} ms, editor ruler scrub move p50={:.2} ms (p95={:.2} ms, max={:.2} ms), long-gop ruler scrub move p50={:.2} ms (p95={:.2} ms, max={:.2} ms)",
        warm_series.p50,
        warm_series.p95,
        warm_render_series.p50,
        cell_series.p50,
        cell_series.p95,
        cell_engine_series.p50,
        cover_series.p50,
        cover_series.max,
        refill_series.p50,
        refill_series.max,
        cover_hevc_series.p50,
        cover_hevc_series.max,
        refill_hevc_series.p50,
        refill_hevc_series.max,
        after_cell_series.p50,
        after_cell_series.p95,
        no_cell_series.p50,
        cold_series.p50,
        cold_series.p95,
        long_gop_series.p50,
        ruler_series.p50,
        ruler_series.p95,
        ruler_series.max,
        ruler_long_gop_series.p50,
        ruler_long_gop_series.p95,
        ruler_long_gop_series.max,
    )];

    Ok(ScenarioReport::new(
        ScenarioId::Scrub,
        ctx.env.clone(),
        targets,
        Vec::new(),
        vec![
            warm_series,
            warm_render_series,
            cell_series,
            cell_engine_series,
            cover_series,
            refill_series,
            cover_hevc_series,
            refill_hevc_series,
            after_cell_series,
            no_cell_series,
            cold_series,
            long_gop_series,
            ruler_series,
            ruler_long_gop_series,
        ],
        notes,
    ))
}

/// The milliseconds each move of a paused ruler scrub over `media` holds the
/// editor's window: the press at `from` seconds, then [`MOVES`] moves of
/// `step` seconds each. Fails when no frame the moves asked for landed, as
/// such a scrub measures no work.
fn ruler_scrub(
    ctx: &BenchContext,
    label: &str,
    media: &std::path::Path,
    from: f32,
    step: f32,
) -> Result<Vec<f64>, String> {
    let data_dir = ctx.ensure_scratch_subdir(label)?.join("data");
    std::fs::create_dir_all(&data_dir).map_err(|err| format!("mkdir failed: {err}"))?;
    let (mut app, state, mut window) = open_editor(&data_dir);
    let per_second = app.update(|cx| {
        state.update(cx, |s, cx| {
            let id = s
                .import_media(media.to_path_buf())
                .map_err(|err| format!("import {} failed: {err}", media.display()))?;
            s.drop_media_on_timeline(id, None, 0)
                .map_err(|err| format!("placing {} failed: {err}", media.display()))?;
            let per_second = s.store.sequence.ticks_per_second();
            s.set_playhead((from * per_second).round() as i64);
            cx.notify();
            Ok::<f32, String>(per_second)
        })
    })?;
    let landed = std::rc::Rc::new(std::cell::Cell::new(0usize));
    let _counting = app.update(|cx| {
        let landed = landed.clone();
        cx.subscribe(&state, move |_, _: &aria_ui::app::FrameReady, _| {
            landed.set(landed.get() + 1);
        })
    });
    // The frame at the press.
    let _ = timed_frame(&app, &mut window, |shell, _, cx| {
        aria_ui::app::spawn_preview(shell.state.clone(), cx);
    });
    let (start, stride) = (
        (from * per_second).round() as i64,
        (step * per_second).round() as i64,
    );
    let tick = |moves: u32| start + stride * i64::from(moves);
    let mut samples = Vec::with_capacity(MOVES as usize);
    for moves in 1..=MOVES {
        std::thread::sleep(MOVE_GAP);
        let to = tick(moves);
        samples.push(timed_frame(&app, &mut window, |shell, _, cx| {
            shell.state.update(cx, |s, _| s.scrub_playhead_on_ruler(to));
            aria_ui::app::spawn_preview(shell.state.clone(), cx);
            shell.redraw(Moved::Playhead, cx);
        }));
    }
    let (at, error) = app.update(|cx| {
        let state = state.read(cx);
        (state.playhead.value, state.last_error.clone())
    });
    let end = tick(MOVES);
    if at != end {
        return Err(format!(
            "the ruler scrub over {} left the playhead at {at}, not {end}: the moves did not reach it",
            media.display()
        ));
    }
    if landed.get() == 0 {
        return Err(format!(
            "no frame landed during the ruler scrub over {} ({error:?}): it measured no decode",
            media.display()
        ));
    }
    drop(state);
    drop(window);
    app.update(|cx| cx.shutdown());
    Ok(samples)
}

/// The milliseconds the timeline's filmstrip takes to cover and to fill after
/// the view changes, as [`refill`] measures them, over `media` in a new
/// project of an editor window whose profile is in the scratch directory
/// `scratch`: the first fill when `media` is placed on V1 at the default zoom,
/// then [`REFILL_ZOOMS`] zooms in by two about the middle of the lanes, each
/// of which leaves every other cell on screen to render, then
/// [`REFILL_SCROLLS`] scrolls a view along, each of which leaves every cell on
/// screen to render.
fn filmstrip_refill(
    ctx: &BenchContext,
    scratch: &str,
    media: &Path,
) -> Result<(Vec<f64>, Vec<f64>), String> {
    let data_dir = ctx.ensure_scratch_subdir(scratch)?.join("data");
    std::fs::create_dir_all(&data_dir).map_err(|err| format!("mkdir failed: {err}"))?;
    let (mut app, state, mut window) = open_editor(&data_dir);
    let id = app.update(|cx| {
        state.update(cx, |s, cx| {
            s.new_project()
                .map_err(|err| format!("a new project failed: {err}"))?;
            cx.notify();
            s.import_media(media.to_path_buf())
                .map_err(|err| format!("import {} failed: {err}", media.display()))
        })
    })?;
    let changes = (1 + REFILL_ZOOMS + REFILL_SCROLLS) as usize;
    let (mut covered, mut filled) = (Vec::with_capacity(changes), Vec::with_capacity(changes));
    let mut push = |(cover, fill): (f64, f64)| {
        covered.push(cover);
        filled.push(fill);
    };
    let mut placed = Ok(());
    let first = refill(&app, &mut window, &state, |shell, _, cx| {
        placed = shell.state.update(cx, |s, cx| {
            cx.notify();
            s.drop_media_on_timeline(id, None, 0)
        });
    });
    placed.map_err(|err| format!("placing {} failed: {err}", media.display()))?;
    push(first?);
    for _ in 0..REFILL_ZOOMS {
        push(refill(&app, &mut window, &state, |shell, _, cx| {
            let (left, width) = {
                let s = shell.state.read(cx);
                (
                    s.timeline_content_left().unwrap_or(0.0),
                    s.timeline_viewport_px(),
                )
            };
            shell.zoom_at(left + width / 2.0, 2.0, cx);
        })?);
    }
    for _ in 0..REFILL_SCROLLS {
        push(refill(&app, &mut window, &state, |shell, _, cx| {
            shell.state.update(cx, |s, _| {
                let left = s.timeline_content_left().unwrap_or(0.0);
                let ((x, w), _) = s.zoom_bar_thumb(s.scroll_x, s.zoom);
                let at = left + x + w / 2.0;
                s.press_zoom_bar(at);
                s.drag_zoom_bar(at + w);
                s.release_zoom_bar();
            });
            shell.redraw(Moved::View, cx);
        })?);
    }
    drop(state);
    drop(window);
    app.update(|cx| cx.shutdown());
    Ok((covered, filled))
}

/// Runs `change` on the editor window's shell, with every earlier cell landed
/// and the window drawn, and returns the milliseconds until no filmstrip cell
/// on screen is blank, and until a draw asks for no filmstrip cell: every
/// cell on screen shows its exact frame, or was found to show no frame. The
/// strip is covered at the first draw that draws no cell empty, or at the
/// first landing that leaves none of the cells the last draw drew empty: a
/// landing starts the next cell with no draw between, so one wait can cover
/// the strip and fill it. Each pass draws, which asks for every cell drawn
/// without its exact frame, and waits for the cells it asked for to land.
/// Fails when the change asks for no cell, so a strip that is not drawn is
/// not measured as one that fills at once, and when the strip still asks
/// after [`REFILL_DRAWS`] draws.
fn refill(
    app: &gpui::TestApp,
    window: &mut gpui::TestAppWindow<Shell>,
    state: &gpui::Entity<AppState>,
    change: impl FnOnce(&mut Shell, &mut gpui::Window, &mut gpui::Context<Shell>),
) -> Result<(f64, f64), String> {
    app.run_until_parked();
    window.draw();
    app.run_until_parked();
    let (cx, handle) = (app.to_async(), window.handle());
    let started = Instant::now();
    let since = move || started.elapsed().as_secs_f64() * 1000.0;
    let covered = std::rc::Rc::new(std::cell::Cell::new(None));
    let _landings = cx.update(|cx| {
        let covered = covered.clone();
        cx.subscribe(state, move |state, _: &FilmstripChanged, cx| {
            if covered.get().is_none() && state.read(cx).filmstrips.blanks() == 0 {
                covered.set(Some(since()));
            }
        })
    });
    cx.update(|cx| handle.update(cx, change))
        .map_err(|err| format!("the editor window closed: {err}"))?;
    for draw in 0..REFILL_DRAWS {
        window.draw();
        let (blanks, in_flight) = app.read(|cx| {
            let filmstrips = &state.read(cx).filmstrips;
            (filmstrips.blanks(), filmstrips.in_flight())
        });
        let elapsed = since();
        if covered.get().is_none() && blanks == 0 {
            covered.set(Some(elapsed));
        }
        if in_flight == 0 {
            if draw == 0 {
                return Err("the change asked for no filmstrip cell".to_owned());
            }
            return Ok((covered.get().unwrap_or(elapsed), elapsed));
        }
        app.run_until_parked();
    }
    Err(format!(
        "the filmstrip still asked for cells after {REFILL_DRAWS} draws"
    ))
}
