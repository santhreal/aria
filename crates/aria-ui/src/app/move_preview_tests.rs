//! A clip drag draws its clips where the release puts them while the button
//! is still down.
//!
//! # Why
//!
//! The timeline drew a dragged clip where it started until the button came up,
//! so a drag showed nothing of where it was going: not the magnet's pull, not
//! another lane, not the clip it would cover. The class closed here is a
//! preview that disagrees with its release. Each case carries the take, reads
//! the cards the held drag drew for the picture and for its linked sound, and
//! the move the readout states, lets go, and reads them again: they are the
//! same. A Shift drag, which inserts, draws the insertion line at the tick its
//! release opens the lane at. A card drawn moved is the clip as the projection
//! draws it at its landing, field for field, so the marks and beats drawn on a
//! card travel with it. An edge drag of each tool, on each edge, inward and far
//! outward, draws the take and its linked sound as its release leaves them.
//!
//! Not covered: a landing the release refuses, such as one on a lane of
//! another kind, the transition at the moved clip's right edge, and the
//! property rows of a twirled-open clip, which stay where the clip was.

use aria_models::{ClipId, Marker, MarkerColor, RationalTime, TrackKind};
use gpui::{
    Background, Bounds, Entity, Hsla, Modifiers, MouseButton, Pixels, Point, VisualTestContext,
    point, px, size,
};

use super::held_drag_tests::{bounds, carry, picture_clip, press, release, tick_px};
use crate::app::AppState;
use crate::app::viewer_row_tests::{change, shell_with_take};
use crate::theme::Theme;
use crate::types::{EditorTool, UiTrackKind};

/// Every quad the last frame painted with its center inside `within`, as
/// `(bounds, fill)` in window pixels.
fn painted(
    visual: &mut VisualTestContext,
    within: Bounds<Pixels>,
) -> Vec<(Bounds<Pixels>, Background)> {
    visual.update(|window, _| window.refresh());
    visual.run_until_parked();
    visual.update(|window, _| {
        let scale = window.scale_factor();
        window
            .painted_quads()
            .iter()
            .filter_map(|quad| {
                let b = quad.bounds;
                let bounds = Bounds::new(
                    point(px(b.origin.x.0 / scale), px(b.origin.y.0 / scale)),
                    size(px(b.size.width.0 / scale), px(b.size.height.0 / scale)),
                );
                within
                    .contains(&bounds.center())
                    .then_some((bounds, quad.background))
            })
            .collect()
    })
}

/// The fill `clip`'s card is drawn in, as the store has the clip now.
fn fill_of(state: &Entity<AppState>, visual: &mut VisualTestContext, clip: u64) -> Hsla {
    visual.read(|app| {
        let state = state.read(app);
        let ui = state
            .ui_clip_ref(ClipId::from_raw(clip))
            .expect("the clip is on the sequence");
        Theme::of(app).label(ui.drawn_label(), ui.selected)
    })
}

/// The card the last frame drew for `clip` on the canvas: the largest quad in
/// the clip's fill. Exactly one card-sized quad is drawn in it, so a drag that
/// drew the clip twice, where it was and where it lands, fails here.
#[track_caller]
fn card(
    state: &Entity<AppState>,
    visual: &mut VisualTestContext,
    clip: u64,
    canvas: Bounds<Pixels>,
) -> Bounds<Pixels> {
    let fill = Background::from(fill_of(state, visual, clip));
    let quads: Vec<Bounds<Pixels>> = painted(visual, canvas)
        .into_iter()
        .filter(|(_, background)| *background == fill)
        .map(|(bounds, _)| bounds)
        .collect();
    let tallest = quads
        .iter()
        .map(|bounds| bounds.size.height)
        .fold(px(0.0), Pixels::max);
    let cards: Vec<Bounds<Pixels>> = quads
        .into_iter()
        .filter(|bounds| bounds.size.height >= tallest * 0.8)
        .collect();
    assert_eq!(cards.len(), 1, "one card drawn for the clip: {cards:?}");
    cards[0]
}

/// How many edits are applied. The History list keeps an undone row until the
/// next edit, so its length does not count them.
fn history(state: &Entity<AppState>, visual: &mut VisualTestContext) -> usize {
    visual.read(|app| state.read(app).history_depth())
}

fn undo(visual: &mut VisualTestContext, state: &Entity<AppState>) {
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            cx.notify();
            assert!(state.undo(), "undo the move");
        })
    });
    visual.run_until_parked();
}

/// A move with the left button and `modifiers` held.
fn carry_with(visual: &mut VisualTestContext, to: Point<Pixels>, modifiers: Modifiers) {
    visual.simulate_mouse_move(to, MouseButton::Left, modifiers);
    visual.run_until_parked();
}

fn release_with(visual: &mut VisualTestContext, at: Point<Pixels>, modifiers: Modifiers) {
    visual.simulate_mouse_up(at, MouseButton::Left, modifiers);
    visual.run_until_parked();
}

/// A drawn shell with the take on V1 and an empty V2 above it. Motion is
/// reduced, so a card is drawn in its own fill from the first frame.
fn shell<'a>(
    cx: &'a mut gpui::TestAppContext,
    tag: &str,
) -> (Entity<AppState>, &'a mut VisualTestContext) {
    let (state, visual, _) = shell_with_take(cx, tag);
    visual.update(|_, app| app.set_reduce_motion(true));
    change(visual, &state, |state| {
        state
            .add_track(TrackKind::Video, "V2")
            .expect("a second video lane");
    });
    (state, visual)
}

/// The take's own sound, which its drop placed on A1 linked to it, moved to
/// the sequence start so it starts before the take, and linked again, so a
/// move of the take moves it too. Its id.
fn link_sound(state: &Entity<AppState>, visual: &mut VisualTestContext, take: u64) -> u64 {
    let (sound, lane) = visual.read(|app| {
        state
            .read(app)
            .store
            .sequence
            .tracks
            .iter()
            .filter(|lane| lane.kind == UiTrackKind::Audio)
            .flat_map(|lane| lane.clips.iter())
            .map(|clip| (clip.id, clip.track_id))
            .next()
            .expect("the take's sound is placed")
    });
    change(visual, state, |state| {
        let pair = vec![ClipId::from_raw(take), ClipId::from_raw(sound)];
        state.selection = pair.clone();
        state
            .unlink_selected()
            .expect("unlink the sound from the take");
        state
            .move_clip(
                ClipId::from_raw(sound),
                aria_models::TrackId::from_raw(lane),
                0,
            )
            .expect("start the sound at the sequence start");
        state.selection = pair;
        state.sync_from_engine();
        state.link_selected().expect("link the sound to the take");
        state.sync_from_engine();
    });
    sound
}

#[gpui::test]
fn a_carried_clip_is_drawn_where_its_release_lands(cx: &mut gpui::TestAppContext) {
    let (state, visual) = shell(cx, "move-preview-land");
    let (take, ..) = picture_clip(&state, visual);
    // The take starts after its sound, so a move that stops the take at the
    // sequence start has stopped the sound there first.
    change(visual, &state, |state| {
        let id = ClipId::from_raw(take);
        let lane = state.ui_clip_ref(id).expect("the take").track_id;
        state
            .move_clip(id, aria_models::TrackId::from_raw(lane), 12)
            .expect("start the take after its sound");
    });
    let sound = link_sound(&state, visual, take);
    let (_, start, end) = picture_clip(&state, visual);
    let canvas = bounds(visual, "timeline-video-canvas");
    let audio = bounds(visual, "timeline-audio-canvas");
    let (v1, v2) = (
        bounds(visual, "track-header-V1"),
        bounds(visual, "track-header-V2"),
    );
    let tick_px = tick_px(&state, visual);
    let rest = card(&state, visual, take, canvas);
    let heard = card(&state, visual, sound, audio);
    let grab = point(
        canvas.left() + px((start + end) as f32 * 0.5 * tick_px),
        v1.top() + px(10.0),
    );

    // Along the lane, onto the other lane straight up and with travel, back
    // past the sequence start, where the move stops, and a hair after the
    // sound's start, where the magnet pulls the take onto it.
    for (lane, name, travel) in [
        (v1, "V1", 10.0),
        (v1, "V1", 37.0),
        (v2, "V2", 0.4),
        (v2, "V2", 23.0),
        (v1, "V1", -(start as f32) - 5.0),
        (v1, "V1", -(start as f32) + 5.0 / tick_px),
    ] {
        let to = point(grab.x + px(travel * tick_px), lane.top() + px(10.0));
        let steps = history(&state, visual);
        press(visual, grab);
        carry(visual, point(grab.x, to.y));
        carry(visual, to);
        let held = card(&state, visual, take, canvas);
        let held_sound = card(&state, visual, sound, audio);
        let stated = visual.read(|app| {
            state
                .read(app)
                .trim_feedback
                .as_ref()
                .map(|readout| readout.delta_ticks)
                .unwrap_or_else(|| {
                    panic!("{name} by {travel} ticks: the held drag states its move")
                })
        });
        release(visual, to);
        let landed = card(&state, visual, take, canvas);
        assert_eq!(
            held, landed,
            "{name} by {travel} ticks: the card the held drag drew against the card the release left"
        );
        assert_eq!(
            held_sound,
            card(&state, visual, sound, audio),
            "{name} by {travel} ticks: the linked sound's card"
        );
        assert!(
            lane.top() <= landed.center().y && landed.center().y <= lane.bottom(),
            "{name} by {travel} ticks: the card is drawn on {name}: {landed:?}"
        );
        let (_, now, _) = picture_clip(&state, visual);
        assert_eq!(
            stated,
            now - start,
            "{name} by {travel} ticks: the move the readout stated against the one the release made"
        );
        if history(&state, visual) > steps {
            undo(visual, &state);
        }
        assert_eq!(
            (
                card(&state, visual, take, canvas),
                card(&state, visual, sound, audio)
            ),
            (rest, heard),
            "{name} by {travel} ticks: the cards after undo"
        );
    }
}

#[gpui::test]
fn a_carried_card_takes_its_marks_with_it(cx: &mut gpui::TestAppContext) {
    // A mark is drawn where it sits on the clip, so a card drawn somewhere
    // else draws it at the same place on itself.
    let (state, visual) = shell(cx, "move-preview-marks");
    let (take, start, end) = picture_clip(&state, visual);
    change(visual, &state, |state| {
        {
            let mut engine = state.engine.lock();
            let mut project = (*engine.project()).clone();
            let clip = project.clip_mut(ClipId::from_raw(take)).expect("the take");
            let window = clip.clip_time_range();
            let at = RationalTime::new(
                window.start.value + window.duration.value / 3,
                window.start.rate,
            );
            clip.add_marker(Marker::new(at, "mark", MarkerColor::Pink))
                .expect("a mark on the take");
            engine.reset_project(project);
        }
        state.sync_from_engine();
    });
    let canvas = bounds(visual, "timeline-video-canvas");
    let (v1, v2) = (
        bounds(visual, "track-header-V1"),
        bounds(visual, "track-header-V2"),
    );
    let tick_px = tick_px(&state, visual);
    let swatch = Background::from(visual.read(|app| Theme::of(app).marker(MarkerColor::Pink)));
    let mark = |visual: &mut VisualTestContext, card: Bounds<Pixels>| {
        let marks: Vec<Bounds<Pixels>> = painted(visual, card)
            .into_iter()
            .filter(|(_, background)| *background == swatch)
            .map(|(bounds, _)| bounds)
            .collect();
        assert_eq!(marks.len(), 1, "one mark drawn on the card: {marks:?}");
        marks[0].origin - card.origin
    };
    let rest = card(&state, visual, take, canvas);
    let on_rest = mark(visual, rest);

    let grab = point(
        canvas.left() + px((start + end) as f32 * 0.5 * tick_px),
        v1.top() + px(10.0),
    );
    let to = point(grab.x + px(23.0 * tick_px), v2.top() + px(10.0));
    press(visual, grab);
    carry(visual, point(grab.x, to.y));
    carry(visual, to);
    let held = card(&state, visual, take, canvas);
    assert_ne!(held, rest, "the held drag drew the card where it lands");
    assert_eq!(mark(visual, held), on_rest, "the mark on the carried card");
    release(visual, to);
}

#[gpui::test]
fn a_card_drawn_moved_is_the_clip_projected_where_it_lands(cx: &mut gpui::TestAppContext) {
    // The held frame draws a moved card from the clip as it was drawn, so
    // every field the projection reads in timeline ticks or seconds has to
    // move with it. Compared field for field with the projection after the
    // move, so a field added later in timeline time fails here until the
    // card moves it too.
    let (state, visual) = shell(cx, "move-preview-projection");
    let (take, start, _) = picture_clip(&state, visual);
    let id = ClipId::from_raw(take);
    change(visual, &state, |state| {
        {
            let mut engine = state.engine.lock();
            let mut project = (*engine.project()).clone();
            let clip = project.clip_mut(id).expect("the take");
            let window = clip.clip_time_range();
            let at = |part: i64| {
                RationalTime::new(
                    window.start.value + window.duration.value * part / 8,
                    window.start.rate,
                )
            };
            clip.add_marker(Marker::new(at(2), "point", MarkerColor::Teal))
                .expect("a point mark on the take");
            clip.add_marker(
                Marker::new(at(4), "range", MarkerColor::Teal).with_duration(RationalTime::new(
                    window.duration.value / 8,
                    window.start.rate,
                )),
            )
            .expect("a range mark on the take");
            let source = clip.source_range().expect("the take places media");
            clip.beats = (1..4)
                .map(|beat| source.start.value + source.duration.value * beat / 4)
                .collect();
            engine.reset_project(project);
        }
        state.sync_from_engine();
    });
    let (before, v2, rate) = visual.read(|app| {
        let state = state.read(app);
        let v2 = state
            .store
            .sequence
            .tracks
            .iter()
            .find(|lane| lane.name.as_ref() == "V2")
            .expect("V2")
            .id;
        (
            state.ui_clip(id).expect("the take"),
            v2,
            state.store.sequence.frame_rate,
        )
    });
    assert!(
        before.markers.len() == 2 && before.beat_ticks.len() == 3,
        "the take draws two marks and three beats: {:?} {:?}",
        before.markers,
        before.beat_ticks
    );
    let delta = 30;
    change(visual, &state, |state| {
        state
            .move_clip(id, aria_models::TrackId::from_raw(v2), start + delta)
            .expect("move the take onto V2");
    });
    let after = visual.read(|app| state.read(app).ui_clip(id).expect("the moved take"));
    assert_eq!(before.moved(v2, delta, rate), after);
}

#[gpui::test]
fn a_shift_drag_draws_where_it_inserts(cx: &mut gpui::TestAppContext) {
    // A ripple move opens the lane where the clip's head is let go and closes
    // the hole the clip left, so the card does not follow the pointer: the
    // insertion line does, at the tick the release opens the lane at.
    let (state, visual) = shell(cx, "move-preview-insert");
    let (take, start, end) = picture_clip(&state, visual);
    let canvas = bounds(visual, "timeline-video-canvas");
    let (v1, v2) = (
        bounds(visual, "track-header-V1"),
        bounds(visual, "track-header-V2"),
    );
    let tick_px = tick_px(&state, visual);
    let rest = card(&state, visual, take, canvas);
    let line = Background::from(visual.read(|app| Theme::of(app).cursor));
    let grab = point(
        canvas.left() + px((start + end) as f32 * 0.5 * tick_px),
        v1.top() + px(10.0),
    );
    let travel = (end - start) / 2;
    let to = point(grab.x + px(travel as f32 * tick_px), v2.top() + px(10.0));
    let shift = Modifiers::shift();
    press(visual, grab);
    carry_with(visual, point(grab.x, to.y), shift);
    carry_with(visual, to, shift);

    assert_eq!(
        card(&state, visual, take, canvas),
        rest,
        "a held ripple move leaves the card where it is"
    );
    let lines: Vec<Bounds<Pixels>> = painted(visual, v2_band(canvas, v2))
        .into_iter()
        .filter(|(_, background)| *background == line)
        .map(|(bounds, _)| bounds)
        .collect();
    assert_eq!(lines.len(), 1, "one insertion line on V2: {lines:?}");
    release_with(visual, to, shift);
    let landed = visual.read(|app| {
        let state = state.read(app);
        let ui = state
            .ui_clip_ref(ClipId::from_raw(take))
            .expect("the take after the drop");
        ui.timeline_start_ticks
    });
    let scroll = visual.read(|app| state.read(app).scroll_x);
    let expected = canvas.left() + px(landed as f32 * tick_px - scroll);
    assert!(
        (lines[0].left() - expected).abs() < px(0.5),
        "the insertion line at {:?}, the take inserted at tick {landed} ({expected:?})",
        lines[0].left()
    );
}

#[gpui::test]
fn a_held_edge_draws_its_clips_where_the_release_leaves_them(cx: &mut gpui::TestAppContext) {
    let (state, visual) = shell(cx, "edge-preview-land");
    let (take, ..) = picture_clip(&state, visual);
    change(visual, &state, |state| {
        let id = ClipId::from_raw(take);
        let lane = state.ui_clip_ref(id).expect("the take").track_id;
        state
            .move_clip(id, aria_models::TrackId::from_raw(lane), 12)
            .expect("start the take after its sound");
    });
    let sound = link_sound(&state, visual, take);
    let (_, start, end) = picture_clip(&state, visual);
    let canvas = bounds(visual, "timeline-video-canvas");
    let audio = bounds(visual, "timeline-audio-canvas");
    let v1 = bounds(visual, "track-header-V1");
    let tick_px = tick_px(&state, visual);
    let rest = (
        card(&state, visual, take, canvas),
        card(&state, visual, sound, audio),
    );
    // One tool per edge drag: two tools that drag an edge the same way draw
    // the same drag.
    let mut tools: Vec<EditorTool> = Vec::new();
    for tool in EditorTool::all() {
        if tools
            .iter()
            .all(|seen| seen.edge_drag() != tool.edge_drag())
        {
            tools.push(tool);
        }
    }
    let y = v1.top() + px(10.0);
    let head = canvas.left() + px(start as f32 * tick_px + 2.0);
    let tail = canvas.left() + px(end as f32 * tick_px - 2.0);
    for tool in tools {
        change(visual, &state, |state| state.tool = tool);
        // Each edge inward by five ticks and outward past every limit.
        for (edge, grab, travel) in [
            ("head", head, 5.0 * tick_px),
            ("head", head, -400.0),
            ("tail", tail, -5.0 * tick_px),
            ("tail", tail, 400.0),
        ] {
            let name = format!("{tool:?} on the {edge} by {travel}px");
            let steps = history(&state, visual);
            let to = point(grab + px(travel), y);
            press(visual, point(grab, y));
            carry(visual, to);
            let held = (
                card(&state, visual, take, canvas),
                card(&state, visual, sound, audio),
            );
            release(visual, to);
            assert_eq!(
                held,
                (
                    card(&state, visual, take, canvas),
                    card(&state, visual, sound, audio)
                ),
                "{name}: the cards the held drag drew against the cards the release left"
            );
            if history(&state, visual) > steps {
                undo(visual, &state);
            }
            assert_eq!(
                (
                    card(&state, visual, take, canvas),
                    card(&state, visual, sound, audio)
                ),
                rest,
                "{name}: the cards after undo"
            );
        }
    }
}

/// The canvas across the height of V2's header.
fn v2_band(canvas: Bounds<Pixels>, v2: Bounds<Pixels>) -> Bounds<Pixels> {
    Bounds::new(
        point(canvas.left(), v2.top()),
        size(canvas.size.width, v2.size.height),
    )
}
