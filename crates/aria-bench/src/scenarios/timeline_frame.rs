//! `timeline-frame` scenario: `project-10k` scroll, zoom, and drag UI frame build latency,
//! with the window chrome, playback vsyncs, an Inspector value scrub, and a clip drag
//! measured beside it.

use std::rc::Rc;

use aria_models::{ClipId, ClipParam};
use aria_ui::app::AppState;
use aria_ui::app::value_scrub::{CLICK_SLOP_PX, ScrubTarget, ValueKind};
use aria_ui::motion::MotionKey;
use aria_ui::shell::Moved;
use aria_ui::transport::TIMELINE_BASE_PPS;
use aria_ui::types::UiTrackKind;
use gpui::{Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, point, px};

use crate::fixtures::{FixtureId, ensure_fixture};
use crate::scenarios::{BenchContext, open_editor, open_marked_project, timed_frame, timed_input};
use crate::{Comparison, SampleSeries, ScenarioId, ScenarioReport, TargetEvaluation};

/// The time between two playback vsyncs: longer than one tick of a 24 fps
/// sequence played at 2x (20.8 ms), so each vsync moves the playhead.
const PLAYBACK_GAP: std::time::Duration = std::time::Duration::from_millis(25);

/// The width V1's first card on project-10k is zoomed to before the clip drag:
/// a press three quarters along it is clear of its edge handles.
const CLIP_MOVE_CARD_PX: f32 = 160.0;

pub(crate) fn run(ctx: &BenchContext) -> Result<ScenarioReport, String> {
    let fixture = ensure_fixture(&ctx.fixtures_dir, FixtureId::Project10k)?;
    let scratch = ctx.ensure_scratch_subdir("timeline-frame")?;

    let data_dir = scratch.join("data");
    std::fs::create_dir_all(&data_dir).map_err(|err| format!("mkdir failed: {err}"))?;
    let (mut app, state, mut window) = open_editor(&data_dir);
    open_marked_project(&mut app, &state, fixture.path);

    // Each step below changes what the matching pointer listener changes and
    // redraws the way that listener does: `Shell::redraw` with what moved.

    // 1. Scroll 30 frames across horizontal and vertical lanes on project-10k:
    // the zoom scroll bar's thumb, held at its middle, dragged 2.5 % of the
    // strip each frame.
    let mut scroll_samples = Vec::with_capacity(30);
    let mut grab = None;
    for step in 0..30usize {
        scroll_samples.push(timed_frame(&app, &mut window, |shell, _, cx| {
            shell.state.update(cx, |s, _| {
                let left = s.timeline_content_left().unwrap_or(0.0);
                let at = *grab.get_or_insert_with(|| {
                    let ((x, w), _) = s.zoom_bar_thumb(s.scroll_x, s.zoom);
                    let at = left + x + w / 2.0;
                    s.press_zoom_bar(at);
                    at
                });
                let travel = (step + 1) as f32 * 0.025 * s.timeline_viewport_px();
                s.drag_zoom_bar(at + travel);
                s.video_scroll_y = ((step % 15) as f32) * 24.0;
                s.audio_scroll_y = ((step % 15) as f32) * 24.0;
            });
            shell.redraw(Moved::View, cx);
        }));
    }
    let _ = app.update(|cx| state.update(cx, |s, _| s.release_zoom_bar()));

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

    // 7. A clip drag on project-10k: the view zoomed out to the floor, which
    // draws the sequence from its first frame, then in about that frame until
    // V1's first card is `CLIP_MOVE_CARD_PX` wide; the card pressed three
    // quarters along, and each sample one move of the held button 4 px right,
    // taken by the timeline canvas's own pointer listener as a real move is.
    // Not part of the target.
    let first_card = |s: &AppState| {
        let (_, v1) = s
            .store
            .sequence
            .tracks
            .iter()
            .enumerate()
            .find(|&(row, _)| s.lane_badge_at(row) == "V1")?;
        let clip = v1.clips.first()?;
        let ticks_px = TIMELINE_BASE_PPS * s.zoom / s.store.sequence.ticks_per_second();
        let start = clip.timeline_start_ticks as f32 * ticks_px - s.scroll_x;
        let width = (clip.timeline_end_ticks - clip.timeline_start_ticks) as f32 * ticks_px;
        Some((start, width, v1.height))
    };
    window.update(|shell, _, cx| {
        let left = shell.state.read(cx).timeline_content_left().unwrap_or(0.0);
        // A factor of zero lands on the floor.
        shell.zoom_at(left, 0.0, cx);
        if let Some((_, width, _)) = first_card(shell.state.read(cx)) {
            shell.zoom_at(left, CLIP_MOVE_CARD_PX / width, cx);
        }
        shell.state.update(cx, |s, _| s.video_scroll_y = 0.0);
        shell.redraw(Moved::View, cx);
    });
    app.run_until_parked();
    window.draw();
    let grab = app.update(|cx| {
        let s = state.read(cx);
        let [x, y, width, height] = s.timeline_lane_area(false)?;
        let (start, card_w, lane_h) = first_card(s)?;
        let grab_x = x + start + 0.75 * card_w;
        // V1 stands on the divider, at the foot of the video lanes; a drag
        // carries the pointer 132 px right of the press.
        (card_w >= CLIP_MOVE_CARD_PX - 1.0 && grab_x > x && grab_x + 140.0 < x + width)
            .then(|| point(px(grab_x), px(y + height - lane_h / 2.0)))
    });
    let Some(grab) = grab else {
        return Err(format!(
            "project-10k shows no V1 card {CLIP_MOVE_CARD_PX} px wide at the sequence start to press"
        ));
    };
    window.simulate_event(MouseMoveEvent {
        position: grab,
        delta: point(px(0.0), px(0.0)),
        modifiers: Modifiers::default(),
        pressed_button: None,
    });
    window.simulate_event(MouseDownEvent {
        button: MouseButton::Left,
        position: grab,
        modifiers: Modifiers::default(),
        click_count: 1,
        first_mouse: false,
    });
    let carried = |to: f32, by: f32| MouseMoveEvent {
        position: point(grab.x + px(to), grab.y),
        delta: point(px(by), px(0.0)),
        modifiers: Modifiers::default(),
        pressed_button: Some(MouseButton::Left),
    };
    // The first move passes the click slop and opens the drag.
    timed_input(&app, &mut window, carried(12.0, 12.0));
    if app.update(|cx| state.read(cx).held_move.is_none()) {
        return Err(format!(
            "a press at {grab:?} and a 12 px move held no clip move: the press missed V1's first card"
        ));
    }
    let mut clip_move_samples = Vec::with_capacity(30);
    let mut to = 12.0;
    for _ in 0..30 {
        to += 4.0;
        clip_move_samples.push(timed_input(&app, &mut window, carried(to, 4.0)));
    }
    // Escape drops the held move, so the release edits nothing.
    window.simulate_keystroke("escape");
    window.simulate_event(MouseUpEvent {
        position: carried(to, 0.0).position,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
        click_count: 1,
    });

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
    let clip_move_series = SampleSeries::from_values("clip_move_frame_ms", "ms", clip_move_samples);

    let targets = vec![TargetEvaluation::measured(
        "ui_frame_build_ms",
        "project-10k: scroll, zoom, and drag at display refresh, UI frame build at most 4 ms",
        Comparison::AtMost,
        4.0,
        "ms",
        all_series.p95,
    )];

    let notes = vec![format!(
        "project-10k GPUI Shell frame, one update and the draw it causes: scroll p50={:.2} ms, zoom p50={:.2} ms, drag p50={:.2} ms, overall p95={:.2} ms, chrome alone p50={:.2} ms, playback vsync p50={:.2} ms (p95={:.2} ms; {} presented, {} dropped), Inspector value scrub p50={:.2} ms (p95={:.2} ms), clip drag p50={:.2} ms (p95={:.2} ms); TimelinePaintStats: total_lanes={}, visible_lanes={}, total_clips={}, visible_clips={}, canvas_elements={}, paint_calls={}",
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
        clip_move_series.p50,
        clip_move_series.p95,
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
            clip_move_series,
        ],
        notes,
    ))
}
