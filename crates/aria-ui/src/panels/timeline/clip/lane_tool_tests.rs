//! A Razor or Track Select press acts on the lane under the pointer, wherever
//! on the lane it lands, and a press off every lane lands on no bar.
//!
//! Closes the class "a press reads its lane from the part of a bar it hit": a
//! press on a clip's line, a fade handle, a transition bar or a property row
//! resolved to the top lane of the group, so the Razor cut there and Track
//! Select selected there; a press in the room above the video lanes landed on
//! the bottom lane, on whichever of its bars spanned the pointer's time; and a
//! marquee carried above the lanes took the bottom lane as its corner. V1 holds
//! two clips cut together under a transition, under V2, which holds a clip over
//! the first. Every column a part of V1's bars is drawn on is pressed at every
//! fourth row of V1 and on its line, with each tool, then down V1's property
//! rows. A Razor click inside the transition's reach is refused on V1, with
//! the refusal on the error line, and cuts nothing on V2. Also pins the
//! modifiers: a Razor click cuts its own lane and a Shift+click every lane, a
//! Track Select click selects on every lane and a Shift+click on its own. Not
//! covered: a mask's Path rows and the audio lanes, which resolve their lane
//! through the same functions.

use aria_models::{ClipId, MediaId, RationalTime, TrackId, TrackKind, TransitionAlignment};
use gpui::{
    Bounds, Entity, Modifiers, MouseButton, Pixels, Point, TestAppContext, VisualTestContext,
    point, px,
};

use super::super::{FADE_GRIP, TRIM};
use super::grip_tests::card;
use super::tests::{a_cut_under_a_transition, align, drag, lane_of, v1_lane};
use super::{BarRows, transition_span};
use crate::app::AppState;
use crate::app::property_rows::{PROPERTY_ROW_H, TimelineOutlineRow};
use crate::types::{EditMode, EditorTool};

/// The two video lanes the presses land on.
struct Lanes {
    state: Entity<AppState>,
    /// V1, and its two clips cut together under a transition.
    low: u64,
    outgoing: ClipId,
    incoming: ClipId,
    /// V2, above V1, and its clip over the outgoing one.
    high: u64,
    over: ClipId,
}

/// The clips on `lane`, in its order.
fn clips_on(state: &AppState, lane: u64) -> Vec<ClipId> {
    state
        .store
        .sequence
        .tracks
        .iter()
        .filter(|track| track.id == lane)
        .flat_map(|track| track.clips.iter())
        .map(|clip| ClipId::from_raw(clip.id))
        .collect()
}

/// The lane `clip` is on.
fn lane_with(state: &AppState, clip: ClipId) -> u64 {
    state
        .store
        .sequence
        .tracks
        .iter()
        .find(|track| track.clips.iter().any(|c| c.id == clip.raw()))
        .map(|track| track.id)
        .expect("the clip is on a lane")
}

fn two_lanes<'a>(cx: &'a mut TestAppContext, tag: &str) -> (Lanes, &'a mut VisualTestContext) {
    let (state, visual, outgoing, _) = a_cut_under_a_transition(cx, tag);
    let lanes = visual.update(|window, app| {
        let lanes = state.update(app, |s, _| {
            let bar = s.ui_clip_ref(outgoing).expect("the outgoing clip");
            let media = MediaId::from_raw(bar.media_id.expect("a clip of the fixture"));
            let cut = bar.timeline_end_ticks;
            let bar_start = bar.timeline_start_ticks;
            let low = lane_with(s, outgoing);
            let incoming = clips_on(s, low)
                .into_iter()
                .find(|&clip| s.ui_clip_ref(clip).unwrap().timeline_start_ticks == cut)
                .expect("the incoming clip starts at the cut");
            let before: Vec<u64> = s.store.sequence.tracks.iter().map(|t| t.id).collect();
            s.add_track(TrackKind::Video, "V2")
                .expect("a second video lane");
            let high = s
                .store
                .sequence
                .tracks
                .iter()
                .map(|t| t.id)
                .find(|id| !before.contains(id))
                .expect("the new lane");
            // Overwrite, so the drop opens no room on V1 and the clip lands
            // over the outgoing one.
            let mode = std::mem::replace(&mut s.edit_mode, EditMode::Overwrite);
            s.drop_media_on_timeline(media, Some(TrackId::from_raw(high)), bar_start)
                .expect("a clip on V2 over the outgoing one");
            s.edit_mode = mode;
            let over = clips_on(s, high)[0];
            let span = |clip: ClipId| {
                let bar = s.ui_clip_ref(clip).expect("a projected clip");
                (bar.timeline_start_ticks, bar.timeline_end_ticks)
            };
            assert_eq!(
                span(over),
                span(outgoing),
                "V2's clip is over the outgoing one"
            );
            (low, incoming, high, over)
        });
        window.refresh();
        lanes
    });
    visual.run_until_parked();
    align(visual, &state, outgoing, TransitionAlignment::Center);
    let (low, incoming, high, over) = lanes;
    let lanes = Lanes {
        state,
        low,
        outgoing,
        incoming,
        high,
        over,
    };
    (lanes, visual)
}

/// A part of V1's bars a press can land on.
struct Part {
    name: &'static str,
    /// Its window x.
    x: f32,
    /// The V1 clip under it.
    clip: ClipId,
    /// Whether a Razor click there cuts. Inside the transition's window a cut
    /// leaves a half shorter than its side of the transition, so the cut is
    /// refused and the error line reports it.
    cuts: bool,
}

/// Each part of V1's bars a press can land on. Each is inside its clip and
/// off the cut, so a cut there has a clip to cut.
fn parts(visual: &mut VisualTestContext, lanes: &Lanes) -> Vec<Part> {
    let (a0, _, tick_px) = card(visual, &lanes.state, lanes.outgoing);
    let (_, b1, _) = card(visual, &lanes.state, lanes.incoming);
    let (start, low, high) = visual.read(|app| {
        let state = lanes.state.read(app);
        let bar = state
            .ui_clip_ref(lanes.outgoing)
            .expect("the outgoing clip is projected");
        let transition = bar
            .transition
            .as_ref()
            .expect("the outgoing clip holds the transition");
        let (low, high) = transition_span(bar, transition);
        (bar.timeline_start_ticks, low, high)
    });
    let x = |tick: i64| a0 + (tick - start) as f32 * tick_px;
    let (t0, t1) = (x(low), x(high));
    assert!(t1 - t0 >= 16.0, "the transition bar is {} px wide", t1 - t0);
    let part = |name, x, clip, cuts| Part {
        name,
        x,
        clip,
        cuts,
    };
    let parts = vec![
        part(
            "the outgoing fade handle",
            a0 + TRIM + FADE_GRIP / 2.0,
            lanes.outgoing,
            true,
        ),
        part("the outgoing body", (a0 + t0) / 2.0, lanes.outgoing, true),
        part(
            "the transition's left edge",
            t0 + 3.0,
            lanes.outgoing,
            false,
        ),
        part(
            "the transition",
            t0 + (t1 - t0) / 4.0,
            lanes.outgoing,
            false,
        ),
        part(
            "the transition's right edge",
            t1 - 3.0,
            lanes.incoming,
            false,
        ),
        part("the incoming body", (t1 + b1) / 2.0, lanes.incoming, true),
        part(
            "the incoming fade handle",
            b1 - TRIM - FADE_GRIP / 2.0,
            lanes.incoming,
            true,
        ),
    ];
    visual.read(|app| {
        let state = lanes.state.read(app);
        for part in &parts {
            let tick = start + ((part.x - a0) / tick_px).floor() as i64;
            let bar = state.ui_clip_ref(part.clip).expect("a clip on V1");
            assert!(
                tick > bar.timeline_start_ticks && tick < bar.timeline_end_ticks,
                "{} at tick {tick} is outside {}..{}",
                part.name,
                bar.timeline_start_ticks,
                bar.timeline_end_ticks
            );
            let at = RationalTime::new(tick, state.playhead.rate);
            let keeps = state
                .engine()
                .lock()
                .project()
                .check_split_keeps_transitions(part.clip, at)
                .is_ok();
            assert_eq!(
                part.cuts, keeps,
                "whether a cut on {} at tick {tick} keeps the transition",
                part.name
            );
        }
    });
    parts
}

/// The x of the part named `name`.
fn x_of(parts: &[Part], name: &str) -> f32 {
    parts
        .iter()
        .find(|part| part.name == name)
        .map(|part| part.x)
        .expect("a named part")
}

fn use_tool(visual: &mut VisualTestContext, state: &Entity<AppState>, tool: EditorTool) {
    visual.update(|window, app| {
        state.update(app, |s, _| s.tool = tool);
        window.refresh();
    });
    visual.run_until_parked();
}

/// Press and release the left button at `at` with `modifiers` held.
fn click(visual: &mut VisualTestContext, at: Point<Pixels>, modifiers: Modifiers) {
    visual.simulate_mouse_move(at, None, modifiers);
    visual.simulate_mouse_down(at, MouseButton::Left, modifiers);
    visual.simulate_mouse_up(at, MouseButton::Left, modifiers);
    visual.run_until_parked();
}

fn undo(visual: &mut VisualTestContext, state: &Entity<AppState>) {
    visual.update(|window, app| {
        state.update(app, |s, _| assert!(s.undo(), "the cut is undoable"));
        window.refresh();
    });
    visual.run_until_parked();
}

/// How many clips V1 and V2 hold.
fn counts(visual: &mut VisualTestContext, lanes: &Lanes) -> (usize, usize) {
    visual.read(|app| {
        let state = lanes.state.read(app);
        (
            clips_on(state, lanes.low).len(),
            clips_on(state, lanes.high).len(),
        )
    })
}

fn selection(visual: &mut VisualTestContext, lanes: &Lanes) -> Vec<ClipId> {
    visual.read(|app| lanes.state.read(app).selection.clone())
}

/// A Razor click at `at` with `modifiers` adds `cuts` clips to V1 and V2;
/// undone after.
fn razor(
    visual: &mut VisualTestContext,
    lanes: &Lanes,
    at: Point<Pixels>,
    modifiers: Modifiers,
    cuts: (usize, usize),
    what: &str,
) {
    let before = counts(visual, lanes);
    click(visual, at, modifiers);
    let after = counts(visual, lanes);
    assert_eq!(
        (after.0 - before.0, after.1 - before.1),
        cuts,
        "clips cut on V1 and V2 by a Razor click {what}"
    );
    if cuts != (0, 0) {
        undo(visual, &lanes.state);
        assert_eq!(counts(visual, lanes), before, "undo after the cut {what}");
    }
}

/// A Razor click at `at` cuts nothing and posts the refusal on the error line:
/// the press reached V1's clip, whose transition the cut would cut short.
fn refused(visual: &mut VisualTestContext, lanes: &Lanes, at: Point<Pixels>, what: &str) {
    visual.update(|_, app| lanes.state.update(app, |s, _| s.last_error = None));
    let before = counts(visual, lanes);
    click(visual, at, Modifiers::none());
    assert_eq!(
        counts(visual, lanes),
        before,
        "a Razor click {what} cut a clip"
    );
    let error = visual.read(|app| lanes.state.read(app).last_error.clone());
    assert!(
        error.is_some(),
        "a Razor click {what} cut nothing and reported nothing"
    );
}

/// A Track Select Shift+click at `at` selects `under` on V1 and nothing on V2.
fn selects_v1(
    visual: &mut VisualTestContext,
    lanes: &Lanes,
    at: Point<Pixels>,
    under: ClipId,
    what: &str,
) {
    click(visual, at, Modifiers::shift());
    let selected = selection(visual, lanes);
    assert!(
        selected.contains(&under) && !selected.contains(&lanes.over),
        "a Track Select Shift+click {what} selected {selected:?}, not {under:?} on V1 alone"
    );
}

/// The room above the top video lane, at the height of its middle.
fn room_above(canvas: Bounds<Pixels>, v2: Bounds<Pixels>) -> Pixels {
    assert!(
        v2.top() - canvas.top() >= px(4.0),
        "the video lanes leave room above V2: {v2:?} in {canvas:?}"
    );
    canvas.top() + (v2.top() - canvas.top()) / 2.0
}

#[gpui::test]
fn a_razor_or_track_select_press_acts_on_the_lane_under_it(cx: &mut TestAppContext) {
    let (lanes, visual) = two_lanes(cx, "lane-tools");
    let v1 = v1_lane(visual);
    let v2 = lane_of(visual, "timeline-video-canvas", "track-header-V2");
    let canvas = visual
        .debug_bounds("timeline-video-canvas")
        .expect("the timeline draws its video lanes");
    let parts = parts(visual, &lanes);
    let line = BarRows::of(f32::from(v1.size.height)).line_top;
    let mut rows: Vec<f32> = (0..)
        .map(|k| 1.5 + 4.0 * k as f32)
        .take_while(|dy| *dy < f32::from(v1.size.height) - 1.0)
        .collect();
    rows.push(line);

    for (tool, name) in [
        (EditorTool::Razor, "Razor"),
        (EditorTool::TrackSelectForward, "Track Select Forward"),
        (EditorTool::TrackSelectBackward, "Track Select Backward"),
    ] {
        use_tool(visual, &lanes.state, tool);
        for part in &parts {
            for &dy in &rows {
                let at = point(px(part.x), v1.top() + px(dy));
                let what = format!("of {name} on {}, {dy} px into V1", part.name);
                match tool {
                    EditorTool::Razor if part.cuts => {
                        razor(visual, &lanes, at, Modifiers::none(), (1, 0), &what);
                    }
                    EditorTool::Razor => refused(visual, &lanes, at, &what),
                    _ => selects_v1(visual, &lanes, at, part.clip, &what),
                }
            }
        }
    }

    // A Razor click cuts its own lane, so off every lane it cuts nothing, and a
    // Shift+click cuts every lane from anywhere.
    let body = x_of(&parts, "the outgoing body");
    let on_line = point(px(body), v1.top() + px(line));
    let above = point(px(body), room_above(canvas, v2));
    use_tool(visual, &lanes.state, EditorTool::Razor);
    for (at, modifiers, cuts, what) in [
        (
            on_line,
            Modifiers::shift(),
            (1, 1),
            "on V1's line with Shift",
        ),
        (above, Modifiers::none(), (0, 0), "above the lanes"),
        (
            above,
            Modifiers::shift(),
            (1, 1),
            "above the lanes with Shift",
        ),
    ] {
        razor(visual, &lanes, at, modifiers, cuts, what);
    }

    // A Track Select click selects on every lane, from a bar or from the room
    // above the lanes; a Shift+click there has no lane to select on.
    use_tool(visual, &lanes.state, EditorTool::TrackSelectForward);
    for (what, at) in [("on V1's line", on_line), ("above the lanes", above)] {
        click(visual, at, Modifiers::none());
        let selected = selection(visual, &lanes);
        for clip in [lanes.outgoing, lanes.incoming, lanes.over] {
            assert!(
                selected.contains(&clip),
                "a Track Select click {what} selected {selected:?}, without {clip:?}"
            );
        }
    }
    click(visual, above, Modifiers::shift());
    assert_eq!(
        selection(visual, &lanes).len(),
        3,
        "a Track Select Shift+click above the lanes changed the selection"
    );

    // V1's property rows stand on the divider under its bars.
    visual.update(|window, app| {
        lanes
            .state
            .update(app, |s, _| s.toggle_expanded(lanes.outgoing.raw()));
        window.refresh();
    });
    visual.run_until_parked();
    let outline = visual.read(|app| lanes.state.read(app).track_outline_rows(lanes.low));
    let rows: Vec<(Pixels, bool)> = outline
        .iter()
        .rev()
        .enumerate()
        .map(|(k, row)| {
            let y = canvas.bottom() - px((k as f32 + 0.5) * PROPERTY_ROW_H);
            (y, matches!(row, TimelineOutlineRow::Property(_)))
        })
        .take_while(|(y, _)| *y > canvas.top())
        .collect();
    assert!(
        rows.iter().any(|(_, property)| *property),
        "a property row of V1 is on screen: {outline:?}"
    );
    for (tool, name) in [
        (EditorTool::Razor, "Razor"),
        (EditorTool::TrackSelectForward, "Track Select Forward"),
    ] {
        use_tool(visual, &lanes.state, tool);
        for (k, &(y, _)) in rows.iter().enumerate() {
            let at = point(px(body), y);
            let what = format!("of {name} on V1's outline row {k} up from the divider");
            if tool == EditorTool::Razor {
                razor(visual, &lanes, at, Modifiers::none(), (1, 0), &what);
            } else {
                selects_v1(visual, &lanes, at, lanes.outgoing, &what);
            }
        }
    }
}

#[gpui::test]
fn a_marquee_corner_off_the_lanes_takes_the_nearest_lane(cx: &mut TestAppContext) {
    let (lanes, visual) = two_lanes(cx, "lane-marquee");
    let v1 = v1_lane(visual);
    let v2 = lane_of(visual, "timeline-video-canvas", "track-header-V2");
    let canvas = visual
        .debug_bounds("timeline-video-canvas")
        .expect("the timeline draws its video lanes");
    let parts = parts(visual, &lanes);
    let outgoing = x_of(&parts, "the outgoing body");
    let incoming = x_of(&parts, "the incoming body");
    let starts = |visual: &mut VisualTestContext| {
        visual.read(|app| {
            let state = lanes.state.read(app);
            [lanes.outgoing, lanes.incoming, lanes.over]
                .map(|clip| state.ui_clip_ref(clip).unwrap().timeline_start_ticks)
        })
    };
    let rest = starts(visual);

    // From V2's empty space up past the canvas: the band stays on V2.
    drag(
        visual,
        point(px(incoming), v2.center().y),
        point(px(outgoing), canvas.top() - px(12.0)),
    );
    assert_eq!(
        selection(visual, &lanes),
        vec![lanes.over],
        "a band carried above the lanes from V2"
    );

    // From the room above the lanes, over V2's clip, down onto V1: the band
    // spans both lanes and moves nothing.
    drag(
        visual,
        point(px(outgoing), room_above(canvas, v2)),
        point(px(incoming), v1.center().y),
    );
    let selected = selection(visual, &lanes);
    for clip in [lanes.outgoing, lanes.incoming, lanes.over] {
        assert!(
            selected.contains(&clip),
            "a band from above the lanes onto V1 selected {selected:?}, without {clip:?}"
        );
    }
    assert_eq!(
        starts(visual),
        rest,
        "a band from above the lanes moved a clip"
    );
}
