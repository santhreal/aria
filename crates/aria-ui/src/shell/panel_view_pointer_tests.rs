//! The pointer over the window: what a hover or a held drag's move builds,
//! and the cursor the window shows.
//!
//! The class closed here is a pointer move that redraws more than the panels
//! that draw what it moved, or leaves the window showing a cursor other than
//! the one the target under it calls for. The lanes paint their cursor into
//! the cached Timeline body, so a hover onto a target that calls for another
//! cursor redraws the Timeline alone, and a held gesture keeps the cursor of
//! the gesture it holds while its moves build what a drag moves. A sweep of
//! the whole window, with the Info panel reading the pixel under the pointer,
//! with the Pen drawing a mask path, with a lane dragged by its header and
//! across the pool tile of a take open in a Source tab, and each drag held on
//! the Viewer carried across what it moves, finds any move that rebuilds
//! every part of the window.
//!
//! Not caught: the cursor over the audio lanes, which hold no clip in this
//! project, a platform backend that shows another cursor than the one it is
//! sent, and a target smaller than the sweep's 4 px step along its lines and
//! its 40 px between them.

use gpui::{
    Bounds, CursorStyle, Entity, Modifiers, MouseButton, Pixels, Point, TestAppContext,
    VisualTestContext, point, px,
};

use crate::app::AppState;
use crate::dock::{FrameId, PANEL_INFO, PANEL_TIMELINE, PanelId};
use crate::shell::Moved;
use crate::types::EditorTool;

use super::builds;
use super::tests::{
    carry_by, dock_right, drawing, first_clip, hold_still, hover_still, left_alone, opened,
    shell_of, still_card,
};

/// The cursor the window's last frame sets for the pointer.
fn cursor(visual: &mut VisualTestContext) -> Option<CursorStyle> {
    visual.update(|window, _| window.rendered_cursor_style())
}

/// The docked bodies a change that moved only `moved` builds, sorted.
fn bodies_for(
    state: &Entity<AppState>,
    visual: &mut VisualTestContext,
    moved: Moved,
) -> Vec<PanelId> {
    let docked = visual.read(|app| {
        state
            .read(app)
            .dock_tree
            .frames()
            .into_iter()
            .map(|frame| frame.active_panel())
            .collect::<Vec<_>>()
    });
    let mut bodies: Vec<_> = docked
        .into_iter()
        .filter(|&panel| !left_alone(state, visual, panel, moved))
        .collect();
    bodies.sort_unstable();
    bodies.dedup();
    bodies
}

/// The bodies and chrome parts built since the last call, bodies sorted.
fn built() -> (Vec<PanelId>, Vec<builds::Part>) {
    let mut bodies = builds::take();
    bodies.sort_unstable();
    bodies.dedup();
    (bodies, builds::take_parts())
}

/// The pointer moved over the still's card and the lane past it sets the
/// cursor of what a press there does, and each move onto a target that calls
/// for another cursor builds the Timeline body alone. A move that keeps the
/// cursor builds nothing. Fails when a hover redraws the whole window, redraws
/// the Timeline on every move, or leaves the cursor of the last target on
/// screen.
#[gpui::test]
fn a_hover_over_the_lanes_sets_its_targets_cursor_and_builds_the_timeline_alone(
    cx: &mut TestAppContext,
) {
    cx.executor().allow_parking();
    let (state, visual) = opened(cx, "pointer-hover");
    let shell = shell_of(visual);
    let timeline = bodies_for(&state, visual, Moved::Pointer);
    assert_eq!(
        timeline,
        vec![PANEL_TIMELINE],
        "a pointer change moves the Timeline"
    );

    hover_still(visual, &shell, 2);
    assert_eq!(
        cursor(visual),
        Some(CursorStyle::PointingHand),
        "over the card"
    );
    for (step, want) in [
        (1, CursorStyle::ResizeLeftRight),
        (2, CursorStyle::PointingHand),
        (1, CursorStyle::ResizeLeftRight),
    ] {
        built();
        hover_still(visual, &shell, step);
        let at = visual.update(|window, _| window.mouse_position());
        assert_eq!(cursor(visual), Some(want), "the cursor at {at:?}");
        assert_eq!(
            built(),
            (timeline.clone(), Vec::new()),
            "built by the hover to {at:?}"
        );
    }

    hover_still(visual, &shell, 2);
    built();
    let at = visual.update(|window, _| window.mouse_position());
    visual.simulate_mouse_move(point(at.x + px(5.0), at.y), None, Modifiers::none());
    visual.run_until_parked();
    assert_eq!(
        cursor(visual),
        Some(CursorStyle::PointingHand),
        "still over the card"
    );
    assert_eq!(
        built(),
        (Vec::new(), Vec::new()),
        "built by a move that keeps the cursor"
    );

    let (canvas, (_, right)) = still_card(visual, &shell);
    let past = right + 24.0;
    assert!(
        past < f32::from(canvas.right()),
        "the still's right edge at {right} and the lane past it are inside the lanes at \
         {canvas:?}"
    );
    visual.simulate_mouse_move(point(px(past), at.y), None, Modifiers::none());
    visual.run_until_parked();
    assert_eq!(
        cursor(visual),
        Some(CursorStyle::Arrow),
        "over the empty lane"
    );
    assert_eq!(
        built(),
        (timeline, Vec::new()),
        "built by the hover onto the empty lane"
    );
}

/// Every move of a held clip drag keeps the cursor of its press and builds the
/// bodies a drag moves and no chrome part, while the pointer runs onto the
/// edge handle of the card it carries, with no modifier and with Ctrl, which
/// over an edge turns the Pointer tool into the Ripple tool. The still stands at
/// the sequence start, so a drag to the left holds its card there and the
/// pointer travels over the card onto its 8 px left handle. The canvas's
/// hover listener ran under a held drag too: it switched the tool, redrew the
/// whole window each time the cursor its target calls for changed, and the
/// lanes painted that cursor. Fails on any of the three, and when the release
/// leaves the press's cursor on screen.
#[gpui::test]
fn a_held_drag_keeps_its_press_cursor_and_builds_what_a_drag_moves(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    for (tag, modifiers) in [("plain", Modifiers::none()), ("ctrl", Modifiers::control())] {
        let (state, visual) = opened(cx, &format!("pointer-drag-{tag}"));
        let shell = shell_of(visual);
        let drag = bodies_for(&state, visual, Moved::Drag);
        let tool = |visual: &mut VisualTestContext| visual.read(|app| state.read(app).tool);
        assert_eq!(tool(visual), EditorTool::Pointer, "{tag}: the launch tool");
        // The press redraws in full; the sweep starts after it.
        hold_still(visual, &shell);
        assert_eq!(
            cursor(visual),
            Some(CursorStyle::PointingHand),
            "{tag}: a held move of the card"
        );
        let (canvas, (left, _)) = still_card(visual, &shell);
        assert!(
            left >= f32::from(canvas.left()),
            "{tag}: the still's left edge at {left} is inside the lanes at {canvas:?}"
        );
        // 7 px steps end inside the handle's first 7 px, short of the lanes'
        // edge.
        let mut steps = 0;
        while f32::from(visual.update(|window, _| window.mouse_position()).x) > left + 7.0 {
            steps += 1;
            assert!(
                steps <= 200,
                "{tag}: the sweep reaches the handle within 200 moves"
            );
            built();
            carry_by(visual, -7.0, modifiers);
            let at = visual.update(|window, _| window.mouse_position());
            assert_eq!(
                built(),
                (drag.clone(), Vec::new()),
                "{tag}: built by a held drag's move to {at:?}"
            );
            assert_eq!(
                cursor(visual),
                Some(CursorStyle::PointingHand),
                "{tag}: the cursor of a held move at {at:?}"
            );
            assert_eq!(
                tool(visual),
                EditorTool::Pointer,
                "{tag}: the tool at {at:?}"
            );
        }
        if modifiers == Modifiers::none() {
            let at = visual.update(|window, _| window.mouse_position());
            visual.simulate_mouse_up(at, MouseButton::Left, modifiers);
            visual.run_until_parked();
            assert_eq!(
                cursor(visual),
                Some(CursorStyle::ResizeLeftRight),
                "released over the card's left handle at {at:?}"
            );
        }
        visual.update(|window, _| window.remove_window());
    }
}

/// A press on the still's opacity line, carried sideways past the click slop,
/// moves the clip, and the window shows the cursor of a move from there on
/// instead of the up-down cursor of the line it pressed. Fails when the cursor
/// of the press outlives the gesture the press turned into.
#[gpui::test]
fn a_press_on_a_clip_line_carried_sideways_shows_the_move_cursor(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = opened(cx, "pointer-line");
    let shell = shell_of(visual);
    let (canvas, card) = still_card(visual, &shell);
    let lane = visual
        .debug_bounds("track-header-V1")
        .expect("the timeline draws V1's header");
    let x = (card.0.max(f32::from(canvas.left())) + card.1.min(f32::from(canvas.right()))) / 2.0;
    let (top, bottom) = (f32::from(lane.top()), f32::from(lane.bottom()));
    let mut y = top;
    let line = loop {
        assert!(
            y < bottom,
            "no point of V1 between {top} and {bottom} at x {x} shows the line's up-down cursor"
        );
        visual.simulate_mouse_move(point(px(x), px(y)), None, Modifiers::none());
        visual.run_until_parked();
        if cursor(visual) == Some(CursorStyle::ResizeUpDown) {
            break point(px(x), px(y));
        }
        y += 1.0;
    };
    visual.simulate_mouse_down(line, MouseButton::Left, Modifiers::none());
    visual.run_until_parked();
    for _ in 0..3 {
        carry_by(visual, 7.0, Modifiers::none());
        let at = visual.update(|window, _| window.mouse_position());
        assert!(
            visual.read(|app| state.read(app).held_move.is_some()),
            "a press on the line at {line:?} carried sideways to {at:?} moves the clip"
        );
        assert_eq!(
            cursor(visual),
            Some(CursorStyle::PointingHand),
            "the cursor of the move at {at:?}"
        );
    }
}

/// The chrome parts a redraw of the whole window builds, sorted.
fn every_part(state: &Entity<AppState>, visual: &mut VisualTestContext) -> Vec<builds::Part> {
    built();
    visual.update(|_, app| state.update(app, |_, cx| cx.notify()));
    visual.run_until_parked();
    let mut parts = built().1;
    parts.sort_unstable();
    parts.dedup();
    assert!(
        parts.len() >= 3,
        "a redraw of the whole window builds the editor bar and a tab row per frame, not \
         {parts:?}"
    );
    parts
}

/// The points of a sweep of `area`: lines 40 px apart, across and then down,
/// each walked 4 px at a time.
fn sweep(area: Bounds<Pixels>) -> Vec<Point<Pixels>> {
    const LINE: f32 = 40.0;
    const STEP: f32 = 4.0;
    let (left, top) = (f32::from(area.left()), f32::from(area.top()));
    let (right, bottom) = (f32::from(area.right()), f32::from(area.bottom()));
    let walk = |from: f32, to: f32, by: f32| {
        std::iter::successors(Some(from), move |at| Some(at + by)).take_while(move |at| *at < to)
    };
    let across = walk(top + LINE / 2.0, bottom, LINE)
        .flat_map(|y| walk(left + 1.0, right, STEP).map(move |x| point(px(x), px(y))));
    let down = walk(left + LINE / 2.0, right, LINE)
        .flat_map(|x| walk(top + 1.0, bottom, STEP).map(move |y| point(px(x), px(y))));
    across.chain(down).collect()
}

/// Moves the pointer through `points` with `button` held, and returns each
/// point whose move built every part in `every`.
fn whole_redraws(
    visual: &mut VisualTestContext,
    points: &[Point<Pixels>],
    button: Option<MouseButton>,
    every: &[builds::Part],
) -> Vec<Point<Pixels>> {
    built();
    let mut whole = Vec::new();
    for &at in points {
        visual.simulate_mouse_move(at, button, Modifiers::none());
        visual.run_until_parked();
        let mut parts = built().1;
        parts.sort_unstable();
        parts.dedup();
        if parts == every {
            whole.push(at);
        }
    }
    whole
}

/// The rect of dock frame `frame` in the window.
fn frame_rect(
    state: &Entity<AppState>,
    visual: &mut VisualTestContext,
    frame: FrameId,
) -> Bounds<Pixels> {
    let [x, y, w, h] = visual
        .read(|app| state.read(app).dock_frame_rects.lock().get(&frame).copied())
        .unwrap_or_else(|| panic!("the window draws frame {frame:?}"));
    Bounds::new(point(px(x), px(y)), gpui::size(px(w), px(h)))
}

/// Asserts the window as drawn equals the window drawn anew.
fn draws_as_a_whole_redraw(visual: &mut VisualTestContext, after: &str) {
    let cached = drawing(visual);
    visual.update(|window, _| window.refresh());
    visual.run_until_parked();
    let full = drawing(visual);
    assert!(
        cached == full,
        "after {after}, the window drawn differs from a whole redraw\n{}",
        cached.differences(&full)
    );
}

/// No pointer move rebuilds every part of the window: not across the whole
/// window with no button held while the Info panel reads the pixel under the
/// pointer over the Viewer, not over the Viewer while the Pen draws a mask
/// path, not down the track headers while a lane is dragged by its header,
/// and not across the pool tile of a take open in a Source tab, which moves
/// the take. Each of those once notified the whole shell on every pixel or
/// row it moved, and each frame of the take that landed did too. What the
/// moves changed is drawn: the Info panel reads the pixel under the pointer,
/// the rubber band ends at it, a row is marked for the lane's drop, the take
/// stands at its tile's frame, and the window equals one drawn anew. Fails on
/// any move that rebuilds the whole window, from a listener, from a frame it
/// asked for, or from a hover style drawn by the shell itself.
#[gpui::test]
fn no_pointer_move_rebuilds_the_whole_window(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = opened(cx, "pointer-sweep");
    dock_right(&state, visual, PANEL_INFO).expect("the Info panel docks in the right frame");
    let window =
        visual.update(|window, _| Bounds::new(point(px(0.0), px(0.0)), window.viewport_size()));
    let every = every_part(&state, visual);
    let whole = whole_redraws(visual, &sweep(window), None, &every);
    assert!(
        whole.is_empty(),
        "{} moves with no button held rebuilt the whole window, first at {:?}",
        whole.len(),
        whole.first()
    );
    let viewer = frame_rect(&state, visual, FrameId::CENTER);
    visual.simulate_mouse_move(viewer.center(), None, Modifiers::none());
    visual.run_until_parked();
    assert!(
        visual.read(|app| state.read(app).info_readout().pointer.is_some()),
        "the Info panel reads the pixel under the pointer at the Viewer's center {:?}",
        viewer.center()
    );
    draws_as_a_whole_redraw(visual, "a sweep of the window");

    // The Pen draws a mask path from a click on the still.
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            let clip = first_clip(state);
            state.select_clip(clip);
            state.tool = EditorTool::Pen;
            cx.notify();
        })
    });
    for _ in 0..2 {
        visual.update(|window, _| window.refresh());
        visual.run_until_parked();
    }
    let layer = visual
        .read(|app| state.read(app).overlay_frame.layer)
        .expect("the selected still draws a layer in the Viewer");
    let [x, y] = layer.at([0.25, 0.25]);
    visual.simulate_click(point(px(x), px(y)), Modifiers::none());
    visual.run_until_parked();
    assert!(
        visual.read(|app| state.read(app).mask_path_drawing()),
        "a Pen click at ({x}, {y}) on the still starts a mask path"
    );
    let every = every_part(&state, visual);
    let points = sweep(viewer);
    let whole = whole_redraws(visual, &points, None, &every);
    assert!(
        whole.is_empty(),
        "{} moves over the Viewer while the Pen draws rebuilt the whole window, first at {:?}",
        whole.len(),
        whole.first()
    );
    let inside = points
        .iter()
        .rev()
        .copied()
        .find(|at| layer_rect_contains(&state, visual, *at))
        .expect("the sweep crosses the still");
    visual.simulate_mouse_move(inside, None, Modifiers::none());
    visual.run_until_parked();
    let rubber = visual.read(|app| {
        state
            .read(app)
            .drawn_mask_path()
            .and_then(|path| path.rubber)
    });
    assert_eq!(
        rubber.map(|[_, end]| end),
        Some([f32::from(inside.x), f32::from(inside.y)]),
        "the rubber band ends at the pointer"
    );
    draws_as_a_whole_redraw(visual, "a sweep of the Viewer while the Pen draws");
    // A second video lane, and V1 dragged by its header down the header
    // column and back.
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.tool = EditorTool::Pointer;
            state
                .add_track(aria_models::TrackKind::Video, "Video 2")
                .expect("add a video lane");
            cx.notify();
        })
    });
    visual.run_until_parked();

    let header = visual
        .debug_bounds("track-header-V1")
        .expect("the timeline draws V1's header");
    let timeline = frame_rect(&state, visual, FrameId::BOTTOM);
    visual.simulate_mouse_down(header.center(), MouseButton::Left, Modifiers::none());
    visual.run_until_parked();
    assert!(
        visual.read(|app| state.read(app).track_dragging()),
        "a press on V1's header at {:?} drags the lane",
        header.center()
    );
    let every = every_part(&state, visual);
    let column: Vec<_> =
        std::iter::successors(Some(f32::from(timeline.top()) + 1.0), |y| Some(y + 4.0))
            .take_while(|y| *y < f32::from(timeline.bottom()))
            .map(|y| point(header.center().x, px(y)))
            .collect();
    let whole = whole_redraws(visual, &column, Some(MouseButton::Left), &every);
    assert!(
        whole.is_empty(),
        "{} moves of a lane drag rebuilt the whole window, first at {:?}",
        whole.len(),
        whole.first()
    );
    let (row, badge, other) = visual
        .read(|app| {
            let state = state.read(app);
            (0..state.store.sequence.tracks.len())
                .map(|row| (row, state.lane_badge_at(row)))
                .collect::<Vec<_>>()
        })
        .into_iter()
        .filter(|(_, badge)| badge != "V1")
        .find_map(|(row, badge)| {
            let header = format!("track-header-{badge}");
            let bounds = visual.debug_bounds(crate::interned::intern(&header))?;
            Some((row, badge, bounds))
        })
        .expect("the timeline draws a header besides V1's");
    visual.simulate_mouse_move(other.center(), MouseButton::Left, Modifiers::none());
    visual.run_until_parked();
    let (dragging, drop_row) = visual.read(|app| {
        let state = state.read(app);
        (state.track_dragging(), state.track_drop_row)
    });
    assert_eq!(
        (dragging, drop_row),
        (true, Some(row)),
        "the header of {badge} at {other:?} under the drag marks its row"
    );
    draws_as_a_whole_redraw(visual, "a lane drag down the headers");
    visual.simulate_mouse_move(header.center(), MouseButton::Left, Modifiers::none());
    visual.simulate_mouse_up(header.center(), MouseButton::Left, Modifiers::none());
    visual.run_until_parked();

    // The still open in a Source tab, and its pool tile scrubbed with no
    // button held.
    let media = visual
        .read(|app| state.read(app).store.media.first().map(|item| item.id))
        .expect("the pool holds the still");
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            assert!(state.open_source(media), "the still's Source tab opens");
            cx.notify();
        })
    });
    visual.run_until_parked();
    let picture = visual
        .debug_bounds("pool-picture-0")
        .expect("the pool draws the still's picture");
    let every = every_part(&state, visual);
    let whole = whole_redraws(visual, &along(picture), None, &every);
    assert!(
        whole.is_empty(),
        "{} moves across the pool tile of a take open in a Source tab rebuilt the whole \
         window, first at {:?}",
        whole.len(),
        whole.first()
    );
    let (take, tile) = visual.read(|app| {
        let state = state.read(app);
        (
            state.sources.focused().map(|view| view.tick),
            state.pool_tile_tick(media),
        )
    });
    assert!(
        take.is_some_and(|tick| tick > 0) && take == Some(tile),
        "the scrub across the tile moved the take to {take:?} and the tile to {tile}"
    );
    draws_as_a_whole_redraw(
        visual,
        "a scrub across the pool tile of a Source tab's take",
    );
}

/// The points 4 px apart along the middle of `strip`, from its left edge to
/// its right.
fn along(strip: Bounds<Pixels>) -> Vec<Point<Pixels>> {
    let y = strip.center().y;
    std::iter::successors(Some(f32::from(strip.left()) + 0.5), |x| Some(x + 4.0))
        .take_while(|x| *x < f32::from(strip.right()))
        .map(|x| point(px(x), y))
        .collect()
}

/// Presses `button` at the first of `points`, carries the press through the
/// rest, and returns each point whose move built every part of the window.
/// The button stays down.
fn held_through(
    state: &Entity<AppState>,
    visual: &mut VisualTestContext,
    points: &[Point<Pixels>],
    button: MouseButton,
) -> Vec<Point<Pixels>> {
    visual.simulate_mouse_down(points[0], button, Modifiers::none());
    visual.run_until_parked();
    let every = every_part(state, visual);
    whole_redraws(visual, &points[1..], Some(button), &every)
}

/// No move of a drag held on the Viewer rebuilds every part of the window:
/// the mini ruler scrubbing the playhead over a timeline zoomed in so far
/// that the playhead pages it, a zoomed frame panned with the middle button,
/// a guide pulled from the top ruler, and the mini ruler of a Source tab
/// moving its take. Each once notified the whole shell on every move, and
/// each frame of the take that landed did too. What each moved is drawn: the
/// timeline paged to the playhead, the frame and the selected still's handles
/// panned, the guide under the pointer, the take and its pool tile at the
/// pointer's frame, and the window equals one drawn anew, its images
/// included. Fails on any of those moves rebuilding the whole window, and on
/// a move that leaves a panel drawing what was there before it, such as a
/// page of the timeline drawn at the scroll it left or a picture drawn where
/// it stood before the pan.
#[gpui::test]
fn no_held_viewer_drag_rebuilds_the_whole_window(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = opened(cx, "viewer-drag");
    let change = |visual: &mut VisualTestContext, edit: &dyn Fn(&mut AppState)| {
        visual.update(|_, app| {
            state.update(app, |state, cx| {
                edit(state);
                cx.notify();
            })
        });
        visual.run_until_parked();
    };
    change(visual, &|state| {
        state.set_zoom(crate::transport::ZOOM_MAX, false)
    });

    let ruler = visual
        .debug_bounds("viewer-ruler")
        .expect("the Viewer draws its mini ruler");
    let points = along(ruler);
    let scroll = visual.read(|app| state.read(app).scroll_x);
    let whole = held_through(&state, visual, &points, MouseButton::Left);
    assert!(
        whole.is_empty(),
        "{} moves of a scrub on the mini ruler rebuilt the whole window, first at {:?}",
        whole.len(),
        whole.first()
    );
    let (playhead, paged) = visual.read(|app| {
        let state = state.read(app);
        (state.playhead.value, state.scroll_x)
    });
    assert!(
        paged > scroll,
        "the scrub to tick {playhead} paged the timeline from {scroll} px"
    );
    draws_as_a_whole_redraw(visual, "a scrub across the mini ruler");
    let end = points[points.len() - 1];
    visual.simulate_mouse_up(end, MouseButton::Left, Modifiers::none());
    visual.run_until_parked();

    // The playhead back on the still, the still selected and shrunk to a
    // fifth, so at 400% its handles stand inside the Viewer on the picture the
    // pan moves.
    change(visual, &|state| {
        state.set_playhead(0);
        let clip = first_clip(state);
        state.select_clip(clip);
        let mut transform = state
            .selected_ui_clip()
            .expect("the still is selected")
            .transform;
        transform.scale_x = 0.2;
        transform.scale_y = 0.2;
        state
            .set_selected_transform(transform)
            .expect("shrink the still");
        state.set_preview_zoom(Some(4.0));
    });
    assert!(
        visual.read(|app| state.read(app).preview_pannable()),
        "the frame at 400% is larger than the Viewer"
    );
    let viewer = frame_rect(&state, visual, FrameId::CENTER);
    let layer = visual
        .read(|app| state.read(app).overlay_frame.layer)
        .expect("the selected still draws a layer in the Viewer");
    assert!(
        viewer.contains(&point(px(layer.x), px(layer.y)))
            && viewer.contains(&point(px(layer.x + layer.w), px(layer.y + layer.h))),
        "the shrunk still's handles stand inside the Viewer {viewer:?}, at {layer:?}"
    );
    let center = viewer.center();
    let pan = visual.read(|app| state.read(app).preview_view.pan);
    let points: Vec<_> = (0..30)
        .map(|step| center + point(px(step as f32 * 4.0), px(step as f32 * 2.0)))
        .collect();
    let whole = held_through(&state, visual, &points, MouseButton::Middle);
    assert!(
        whole.is_empty(),
        "{} moves of a pan rebuilt the whole window, first at {:?}",
        whole.len(),
        whole.first()
    );
    assert_ne!(
        visual.read(|app| state.read(app).preview_view.pan),
        pan,
        "the middle button's drag panned the frame"
    );
    draws_as_a_whole_redraw(visual, "a pan of the zoomed frame");
    let end = points[points.len() - 1];
    visual.simulate_mouse_up(end, MouseButton::Middle, Modifiers::none());
    visual.run_until_parked();

    change(visual, &|state| {
        state.set_preview_zoom(None);
        state.viewer_guides.rulers = true;
    });
    change(visual, &|_| {});
    let top = visual
        .debug_bounds("viewer-ruler-top")
        .expect("the Viewer draws its top ruler");
    let points: Vec<_> = (0..40)
        .map(|step| top.center() + point(px(0.0), px(step as f32 * 4.0)))
        .collect();
    let whole = held_through(&state, visual, &points, MouseButton::Left);
    assert!(
        whole.is_empty(),
        "{} moves of a guide rebuilt the whole window, first at {:?}",
        whole.len(),
        whole.first()
    );
    let guide = visual.read(|app| {
        let guides = &state.read(app).viewer_guides;
        guides
            .dragging
            .and_then(|index| guides.guides.get(index).copied())
    });
    let below = visual.read(|app| {
        let canvas = state.read(app).overlay_frame.canvas?;
        crate::app::viewer_guides::canvas_at(
            crate::app::viewer_guides::GuideAxis::Horizontal,
            f32::from(points[points.len() - 1].y),
            canvas,
            state.read(app).canvas_size(),
        )
    });
    assert_eq!(
        guide.map(|guide| guide.at),
        below,
        "the pulled guide is under the pointer"
    );
    draws_as_a_whole_redraw(visual, "a guide pulled from the top ruler");
    let end = points[points.len() - 1];
    visual.simulate_mouse_up(end, MouseButton::Left, Modifiers::none());
    visual.run_until_parked();

    let media = visual
        .read(|app| state.read(app).store.media.first().map(|item| item.id))
        .expect("the pool holds the still");
    change(visual, &|state| {
        assert!(state.open_source(media), "the still's Source tab opens");
    });
    let ruler = visual
        .debug_bounds("viewer-ruler")
        .expect("the Source tab draws its mini ruler");
    let points = along(ruler);
    let whole = held_through(&state, visual, &points, MouseButton::Left);
    assert!(
        whole.is_empty(),
        "{} moves of a scrub on a Source tab's mini ruler rebuilt the whole window, first at \
         {:?}",
        whole.len(),
        whole.first()
    );
    let (take, tile) = visual.read(|app| {
        let state = state.read(app);
        (
            state.sources.focused().map(|view| view.tick),
            state.pool_tile_tick(media),
        )
    });
    assert!(
        take.is_some_and(|tick| tick > 0) && take == Some(tile),
        "the scrub moved the take to {take:?} and its tile to {tile}"
    );
    draws_as_a_whole_redraw(visual, "a scrub across a Source tab's mini ruler");
    let end = points[points.len() - 1];
    visual.simulate_mouse_up(end, MouseButton::Left, Modifiers::none());
    visual.run_until_parked();
}

/// Whether window point `at` lies on the selected clip's layer in the Viewer.
fn layer_rect_contains(
    state: &Entity<AppState>,
    visual: &mut VisualTestContext,
    at: Point<Pixels>,
) -> bool {
    visual.read(|app| {
        state.read(app).overlay_frame.layer.is_some_and(|layer| {
            let (x, y) = (f32::from(at.x), f32::from(at.y));
            x > layer.x && y > layer.y && x < layer.x + layer.w && y < layer.y + layer.h
        })
    })
}
