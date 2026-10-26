//! `timeline-frame` scenario: `project-10k` scroll, zoom, and drag UI frame build latency,
//! with the window chrome, playback vsyncs, and an Inspector value scrub measured beside it.

use std::rc::Rc;

use aria_models::{ClipId, ClipParam};
use aria_ui::app::AppState;
use aria_ui::app::value_scrub::{CLICK_SLOP_PX, ScrubTarget, ValueKind};
use aria_ui::motion::MotionKey;
use aria_ui::shell::Moved;
use aria_ui::types::UiTrackKind;
use gpui::{Modifiers, MouseButton, MouseMoveEvent, MouseUpEvent, point, px};

use crate::fixtures::{FixtureId, ensure_fixture};
use crate::scenarios::{BenchContext, open_editor, open_marked_project, timed_frame, timed_input};
use crate::{Comparison, SampleSeries, ScenarioId, ScenarioReport, TargetEvaluation};

/// The time between two playback vsyncs: longer than one tick of a 24 fps
/// sequence played at 2x (20.8 ms), so each vsync moves the playhead.
const PLAYBACK_GAP: std::time::Duration = std::time::Duration::from_millis(25);

pub(crate) fn run(ctx: &BenchContext) -> Result<ScenarioReport, String> {
    let fixture = ensure_fixture(&ctx.fixtures_dir, FixtureId::Project10k)?;
    let scratch = ctx.ensure_scratch_subdir("timeline-frame")?;

    let data_dir = scratch.join("data");
    std::fs::create_dir_all(&data_dir).map_err(|err| format!("mkdir failed: {err}"))?;
    let (mut app, state, mut window) = open_editor(&data_dir);
    open_marked_project(&mut app, &state, fixture.path);

    // Each step below changes what the matching pointer listener changes and
    // redraws the way that listener does: `Shell::redraw` with what moved.

    // 1. Scroll 30 frames across horizontal and vertical lanes on project-10k.
    let mut scroll_samples = Vec::with_capacity(30);
    for step in 0..30usize {
        scroll_samples.push(timed_frame(&app, &mut window, |shell, _, cx| {
            shell.state.update(cx, |s, _| {
                s.zoom_bar_pan(0.025);
                s.video_scroll_y = ((step % 15) as f32) * 24.0;
                s.audio_scroll_y = ((step % 15) as f32) * 24.0;
            });
            shell.redraw(Moved::View, cx);
        }));
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
        zoom_samples.push(timed_frame(&app, &mut window, |shell, _, cx| {
            let (left, width) = {
                let s = shell.state.read(cx);
                (
                    s.timeline_content_left().unwrap_or(0.0),
                    s.timeline_viewport_px(),
                )
            };
            shell.zoom_at(left + width / 3.0, factor, cx);
        }));
    }

    // 3. Drag / scrub with snap index lookup across 30 frames on project-10k.
    let threshold = aria_ui::snap::threshold_ticks(80.0, 24.0);
    let mut drag_samples = Vec::with_capacity(30);
    for step in 0..30i64 {
        let desired = (step * 73) % 4_800;
        drag_samples.push(timed_frame(&app, &mut window, |shell, _, cx| {
            shell.state.update(cx, |s, _| {
                let (snapped, _) = s.snap_at(desired, Some(1), threshold, false);
                s.set_playhead(snapped);
            });
            shell.redraw(Moved::Playhead, cx);
        }));
    }

    // 4. The floor under every frame above: the window chrome redrawn with
    // every panel body replaying its last drawing. Not part of the target.
    let mut chrome_samples = Vec::with_capacity(30);
    for _ in 0..30 {
        chrome_samples.push(timed_frame(&app, &mut window, |shell, _, cx| {
            shell.redraw_chrome(cx)
        }));
    }

    // 5. Playback at 2x: each sample is one vsync, which moves the playhead to
    // the playback clock and shows the newest frame rendered ahead or counts
    // the slot due as dropped. The vsyncs are at least `PLAYBACK_GAP` apart, so
    // each one moves the playhead. Not part of the target.
    app.update(|cx| {
        state.update(cx, |s, cx| {
            s.set_shuttle(1);
            s.set_shuttle(1);
            cx.notify();
        })
    });
    if !app.update(|cx| state.read(cx).playing) {
        return Err("project-10k did not start playing at 2x".to_string());
    }
    window.update(|_, window, cx| aria_ui::app::start_playback(state.clone(), window, cx));
    let mut playback_samples = Vec::with_capacity(30);
    for _ in 0..30 {
        std::thread::sleep(PLAYBACK_GAP);
        playback_samples.push(timed_frame(&app, &mut window, |_, window, cx| {
            window.simulate_next_frame(cx);
        }));
    }
    let playback = app.update(|cx| state.read(cx).playback_report());
    app.update(|cx| {
        state.update(cx, |s, cx| {
            s.toggle_playback();
            cx.notify();
        })
    });

    // 6. A value scrub in the Inspector: the video clip at frame 0 selected,
    // its Opacity readout pressed as the Inspector's row presses it, and each
    // sample one move of the held button, taken by the window's own pointer
    // listener as a real move is. Each move travels 2 px, one step of 0.01 at
    // the readout's rate, so every sample writes the value. Not part of the
    // target.
    let drawn = app.update(|cx| {
        state.update(cx, |s, cx| {
            s.set_playhead(0);
            let clip = s
                .store
                .sequence
                .tracks
                .iter()
                .filter(|track| track.kind == UiTrackKind::Video)
                .find_map(|track| {
                    track
                        .clips
                        .iter()
                        .find(|clip| clip.timeline_start_ticks == 0)
                })
                .map(|clip| ClipId::from_raw(clip.id))?;
            s.select_clip(clip);
            cx.notify();
            s.param_scalar(ClipParam::Opacity, None)
        })
    });
    let Some(drawn) = drawn else {
        return Err("project-10k has no video clip with an Opacity at frame 0".to_string());
    };
    app.update(|cx| {
        state.update(cx, |s, _| {
            s.press_value(ScrubTarget {
                key: MotionKey::new("bench-scrub", "opacity"),
                value: drawn,
                unit: 0.01,
                kind: ValueKind::Number { decimals: 2 },
                write: Rc::new(move |s: &mut AppState, delta| {
                    s.set_param_scalar(ClipParam::Opacity, None, drawn + delta)
                }),
            });
        })
    });
    let at = point(px(640.0), px(360.0));
    let held = |dx: f32| MouseMoveEvent {
        position: at,
        delta: point(px(dx), px(0.0)),
        modifiers: Modifiers::default(),
        pressed_button: Some(MouseButton::Left),
    };
    // The first move passes the click slop and opens the edit group.
    timed_input(&app, &mut window, held(-2.0 - CLICK_SLOP_PX));
    let mut scrub_samples = Vec::with_capacity(30);
    for _ in 0..30 {
        scrub_samples.push(timed_input(&app, &mut window, held(-2.0)));
    }
    window.simulate_event(MouseUpEvent {
        position: at,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
        click_count: 1,
    });
    let scrubbed = app.update(|cx| state.read(cx).param_scalar(ClipParam::Opacity, None));
    if scrubbed.is_none_or(|value| value >= drawn) {
        return Err(format!(
            "the Inspector scrub left Opacity at {scrubbed:?} from {drawn}: the moves did not reach the held readout"
        ));
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
    let playback_series = SampleSeries::from_values("playback_vsync_ms", "ms", playback_samples);
    let scrub_series = SampleSeries::from_values("inspector_scrub_frame_ms", "ms", scrub_samples);

    let targets = vec![TargetEvaluation::measured(
        "ui_frame_build_ms",
        "project-10k: scroll, zoom, and drag at display refresh, UI frame build at most 4 ms",
        Comparison::AtMost,
        4.0,
        "ms",
        all_series.p95,
    )];

    let notes = vec![format!(
        "project-10k GPUI Shell frame, one update and the draw it causes: scroll p50={:.2} ms, zoom p50={:.2} ms, drag p50={:.2} ms, overall p95={:.2} ms, chrome alone p50={:.2} ms, playback vsync p50={:.2} ms (p95={:.2} ms; {} presented, {} dropped), Inspector value scrub p50={:.2} ms (p95={:.2} ms); TimelinePaintStats: total_lanes={}, visible_lanes={}, total_clips={}, visible_clips={}, canvas_elements={}, paint_calls={}",
        scroll_series.p50,
        zoom_series.p50,
        drag_series.p50,
        all_series.p95,
        chrome_series.p50,
        playback_series.p50,
        playback_series.p95,
        playback.presented,
        playback.dropped.total(),
        scrub_series.p50,
        scrub_series.p95,
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
            playback_series,
            scrub_series,
        ],
        notes,
    ))
}
