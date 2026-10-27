//! A drag begun on a timeline element follows the pointer past that element,
//! and the release ends it wherever the button comes up.
//!
//! An element's own move and release handlers run only while the pointer is
//! over it. A drag carried by them stops at the element's edge, and a release
//! beside the element reaches none of them, so the edit is lost and the drag
//! stays held. The class closed here: each timeline drag that was carried by
//! the handlers of the element it began on. The ruler's scrub, the divider
//! between the video and audio sides, a clip moved on its lane group and a key
//! dragged on the graph are each pressed, carried off the element, and let go
//! there. Each follows the pointer the whole way, the release writes it, and a
//! move after the release changes nothing. Escape puts the divider back; the
//! other drags' Escape is in `escape_drag_tests`.
//!
//! The zoom scroll bar's case is in `zoom_bar_tests`, the work area bar's in
//! `work_area_tests` and the outline border's in `outline_column_tests`.
//!
//! Not covered: a trim, a fade, a band and a clip's line on the lane groups
//! carried off them, which the same hold carries and no test here drives, and
//! the drags of the Inspector. The Viewer's, the Mixer's and the track
//! headers' are in `held_drag_panel_tests`.

use aria_commands::{Command, EditCommand};
use aria_models::{ClipId, ClipParam, Easing, ParamValue, RationalTime, TrackKind};
use gpui::{Bounds, Entity, Modifiers, MouseButton, Pixels, Point, VisualTestContext, point, px};

use crate::app::AppState;
use crate::app::property_rows::GRAPH_EDITOR_H;
use crate::app::viewer_row_tests::{change, shell_with_take};
use crate::interned::intern;
use crate::transport::TIMELINE_BASE_PPS;
use crate::types::UiTrackKind;

/// The bounds the last frame drew under `selector`.
#[track_caller]
pub(super) fn bounds(visual: &mut VisualTestContext, selector: &str) -> Bounds<Pixels> {
    visual.update(|window, _| window.refresh());
    visual.run_until_parked();
    visual
        .debug_bounds(intern(selector))
        .unwrap_or_else(|| panic!("the shell draws no {selector}"))
}

pub(super) fn press(visual: &mut VisualTestContext, at: Point<Pixels>) {
    visual.simulate_mouse_move(at, None, Modifiers::none());
    visual.simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
    visual.run_until_parked();
}

/// A move with the left button held.
pub(super) fn carry(visual: &mut VisualTestContext, to: Point<Pixels>) {
    visual.simulate_mouse_move(to, MouseButton::Left, Modifiers::none());
    visual.run_until_parked();
}

pub(super) fn release(visual: &mut VisualTestContext, at: Point<Pixels>) {
    visual.simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
    visual.run_until_parked();
}

/// Pixels per tick of the drawn timeline.
pub(super) fn tick_px(state: &Entity<AppState>, visual: &mut VisualTestContext) -> f32 {
    visual.read(|app| {
        let state = state.read(app);
        TIMELINE_BASE_PPS * state.zoom / state.store.sequence.ticks_per_second()
    })
}

/// The take's picture clip: its id, start and end.
pub(super) fn picture_clip(
    state: &Entity<AppState>,
    visual: &mut VisualTestContext,
) -> (u64, i64, i64) {
    visual.read(|app| {
        let clip = state
            .read(app)
            .store
            .sequence
            .tracks
            .iter()
            .filter(|lane| lane.kind == UiTrackKind::Video)
            .flat_map(|lane| lane.clips.iter())
            .next()
            .expect("the take has a picture clip");
        (clip.id, clip.timeline_start_ticks, clip.timeline_end_ticks)
    })
}

#[gpui::test]
fn a_ruler_scrub_follows_the_pointer_off_the_ruler_and_ends_at_the_release(
    cx: &mut gpui::TestAppContext,
) {
    let (state, visual, _) = shell_with_take(cx, "held-ruler-scrub");
    let ruler = bounds(visual, "timeline-ruler");
    let tick_px = tick_px(&state, visual);
    let playhead =
        |visual: &mut VisualTestContext| visual.read(|app| state.read(app).playhead.value);
    let duration = visual.read(|app| state.read(app).store.sequence.duration_ticks);
    let (quarter, half, three_quarters) = (duration / 4, duration / 2, duration * 3 / 4);
    assert!(
        quarter > 2,
        "the take is {duration} ticks, too short to scrub in"
    );
    // A quarter of a tick past the tick, so a press reads as that tick whether
    // the ruler rounds or floors.
    let x = |tick: i64| ruler.left() + px((tick as f32 + 0.25) * tick_px);
    let below = ruler.bottom() + px(60.0);

    press(visual, point(x(quarter), ruler.center().y));
    assert_eq!(playhead(visual), quarter, "the press");
    carry(visual, point(x(three_quarters), below));
    assert_eq!(playhead(visual), three_quarters, "a move below the ruler");
    carry(visual, point(px(-20.0), below));
    assert_eq!(playhead(visual), 0, "a move left of the window");
    carry(visual, point(x(half), below));
    release(visual, point(x(half), below));
    assert_eq!(playhead(visual), half, "the release below the ruler");

    carry(visual, point(x(quarter), ruler.center().y));
    visual.simulate_mouse_move(
        point(x(three_quarters), ruler.center().y),
        None,
        Modifiers::none(),
    );
    visual.run_until_parked();
    assert_eq!(playhead(visual), half, "a move after the release");
}

#[gpui::test]
fn the_av_divider_follows_the_pointer_out_of_the_timeline_and_escape_puts_it_back(
    cx: &mut gpui::TestAppContext,
) {
    let (state, visual, _) = shell_with_take(cx, "held-av-divider");
    // The take's sound rides in its picture; an audio lane gives the divider
    // two sides to split.
    change(visual, &state, |state| {
        state
            .add_track(TrackKind::Audio, "A1")
            .expect("an audio lane");
    });
    let split =
        |visual: &mut VisualTestContext| visual.read(|app| state.read(app).timeline_av_split);
    let start = split(visual);
    let at = bounds(visual, "timeline-av-divider").center();

    press(visual, at);
    carry(visual, point(at.x, at.y - px(30.0)));
    let moved = bounds(visual, "timeline-av-divider").center().y - at.y;
    assert!(
        (f32::from(moved) + 30.0).abs() <= 0.5,
        "a move of -30 px moved the divider {moved:?}"
    );
    // Above the window, where nothing the shell drew is under the pointer: the
    // video side keeps its least share.
    carry(visual, point(at.x, px(-20.0)));
    assert_eq!(split(visual), 0.15, "a move above the window");

    visual.simulate_keystrokes("escape");
    visual.run_until_parked();
    assert_eq!(split(visual), start, "Escape");
    let back = bounds(visual, "timeline-av-divider").center().y;
    assert!(
        (f32::from(back - at.y)).abs() <= 0.5,
        "after Escape the divider is drawn {:?} from where it was pressed",
        back - at.y
    );
    carry(visual, point(at.x, at.y - px(50.0)));
    assert_eq!(split(visual), start, "a move after Escape");
    release(visual, point(at.x, at.y - px(50.0)));

    // Let go above the window: the split stays where the drag left it.
    press(visual, at);
    carry(visual, point(at.x, at.y - px(30.0)));
    let dragged = split(visual);
    assert!(dragged < start, "the drag moved the split up");
    release(visual, point(at.x, px(-20.0)));
    carry(visual, point(at.x, at.y + px(40.0)));
    assert_eq!(split(visual), dragged, "a move after the release");
}

#[gpui::test]
fn a_clip_moved_off_its_lane_group_lands_where_the_button_comes_up(cx: &mut gpui::TestAppContext) {
    let (state, visual, _) = shell_with_take(cx, "held-clip-move");
    let (clip, start, end) = picture_clip(&state, visual);
    let canvas = bounds(visual, "timeline-video-canvas");
    let lane = bounds(visual, "track-header-V1");
    let ruler = bounds(visual, "timeline-ruler");
    let tick_px = tick_px(&state, visual);
    let placed = |visual: &mut VisualTestContext| {
        visual.read(|app| {
            let clip = state
                .read(app)
                .store
                .sequence
                .tracks
                .iter()
                .flat_map(|lane| lane.clips.iter())
                .find(|c| c.id == clip)
                .expect("the clip is on the sequence");
            (clip.track_id, clip.timeline_start_ticks)
        })
    };
    let (track, _) = placed(visual);

    // Mid-bar on the name strip: the bar's body, clear of its edges, its fade
    // grips and its line.
    let from = point(
        canvas.left() + px((start + end) as f32 * 0.5 * tick_px),
        lane.top() + px(10.0),
    );
    let by = 24;
    press(visual, from);
    carry(visual, point(from.x + px(12.0 * tick_px), from.y));
    // Over the ruler, off the lane group the press began on.
    let over_ruler = point(from.x + px(by as f32 * tick_px), ruler.center().y);
    carry(visual, over_ruler);
    let held = visual.read(|app| {
        state
            .read(app)
            .trim_feedback
            .as_ref()
            .map(|feedback| feedback.delta_ticks)
    });
    assert_eq!(held, Some(by), "the clip held over the ruler");
    release(visual, over_ruler);
    assert_eq!(
        placed(visual),
        (track, start + by),
        "the clip let go over the ruler"
    );

    carry(visual, point(from.x, from.y));
    assert_eq!(
        placed(visual),
        (track, start + by),
        "a move after the release"
    );
}

/// The ticks and values of the opacity keys on `clip`.
pub(super) fn opacity_keys(
    state: &Entity<AppState>,
    visual: &mut VisualTestContext,
    clip: ClipId,
) -> Vec<(i64, f32)> {
    visual.read(|app| {
        let engine = state.read(app).engine().lock();
        engine
            .project()
            .clip(clip)
            .expect("the clip is in the project")
            .transform
            .opacity
            .keyframes()
            .iter()
            .map(|key| (key.tick, key.value))
            .collect()
    })
}

/// Two opacity keys on the take's picture clip, a quarter and three quarters
/// in, drawn on the graph: the clip, where the first key is drawn, and the
/// top of the graph's value range.
pub(super) fn graph_key(
    state: &Entity<AppState>,
    visual: &mut VisualTestContext,
) -> (ClipId, Point<Pixels>, f32) {
    let (clip, start, end) = picture_clip(state, visual);
    let clip = ClipId::from_raw(clip);
    let (first, second) = ((end - start) / 4, (end - start) * 3 / 4);
    change(visual, state, |state| {
        let rate = state.store.sequence.frame_rate;
        for (tick, value) in [(first, 0.3), (second, 0.9)] {
            state
                .apply_command(Command::Edit(EditCommand::SetParamKeyframe {
                    clip,
                    param: ClipParam::Opacity,
                    at: RationalTime::new(start + tick, rate),
                    value: ParamValue::Scalar(value),
                    easing: Easing::Linear,
                    tangents: None,
                }))
                .expect("the key is set");
        }
        // The graph draws a property of the selected clip.
        state.select_clip_linked(clip, false);
        state.toggle_timeline_graph_editor();
        state.select_timeline_property(clip, ClipParam::Opacity, false);
    });
    let canvas = bounds(visual, "timeline-graph-canvas");
    let (key, top) = visual.read(|app| {
        let state = state.read(app);
        let model = state.build_timeline_graph_model(
            state.store.sequence.ticks_per_second(),
            TIMELINE_BASE_PPS * state.zoom,
            state.scroll_x,
            GRAPH_EDITOR_H,
        );
        let key = model
            .curves
            .iter()
            .flat_map(|curve| curve.keys.iter())
            .find(|key| key.tick == start + first)
            .expect("the graph draws the first key");
        (
            point(canvas.left() + px(key.x), canvas.top() + px(key.y)),
            model.max_y,
        )
    });
    (clip, key, top)
}

#[gpui::test]
fn a_graph_key_dragged_off_the_graph_is_written_where_the_button_comes_up(
    cx: &mut gpui::TestAppContext,
) {
    let (state, visual, _) = shell_with_take(cx, "held-graph-key");
    let (clip, key, top) = graph_key(&state, visual);
    let before = opacity_keys(&state, visual, clip);
    assert_eq!(before.len(), 2, "two keys: {before:?}");
    let canvas = bounds(visual, "timeline-graph-canvas");

    press(visual, key);
    carry(visual, point(key.x, key.y - px(10.0)));
    // Above the graph, off the canvas the press began on. A press on a key
    // takes the key, not the collapsed influence handles drawn under it.
    let above = point(key.x, canvas.top() - px(40.0));
    carry(visual, above);
    release(visual, above);
    let after = opacity_keys(&state, visual, clip);
    assert_eq!(after.len(), 2, "two keys after the drag: {after:?}");
    assert_eq!(after[0].0, before[0].0, "a straight drag up keeps the tick");
    assert!(
        (after[0].1 - top).abs() < 1e-3,
        "the key let go above the graph is at {}, not at the top of the graph, {top}",
        after[0].1,
    );
    assert_eq!(after[1], before[1], "the other key");

    carry(visual, point(key.x, canvas.bottom()));
    assert_eq!(
        opacity_keys(&state, visual, clip),
        after,
        "a move after the release"
    );
}
