//! Where each side's lanes stand in their viewport, measured on the drawn
//! shell.
//!
//! The class closed here is a lane side that leaves an empty band between its
//! lanes and the A/V divider, or that rests on any lane but V1 or A1 there.
//! Video lanes number upward from V1 on the divider and audio lanes downward
//! from A1 under it, so a side's spare height is above the top video lane and
//! below the last audio lane, and a video side its lanes overflow draws V1 on
//! the divider until a wheel lifts it toward the top lane. The video side is
//! drawn at every lane count from one to past the count its side holds, at
//! rest and under a scroll kept from a taller stack, so a lift that is wrong
//! for one count, that drops V1 below the divider once the lanes overflow, or
//! that a stale scroll moves off the divider, goes red. The wheel is turned
//! both ways over each overflowing side, so a scroll that runs the wrong way on
//! either side, or spends its first step on a stale offset, goes red. A drag
//! over the spare height above the top lane lands on the top lane.
//!
//! The Timeline is also drawn at heights 16 px apart from its frame's minimum
//! up, with one side empty, with lanes on both, and with the graph editor open.
//! At each, every lane is placed in the viewport its side is drawn at, and the
//! drop strips, the sides and the divider fill the stack between the header
//! and what is under it, with no band and nothing drawn past it. A stack
//! height reckoned from anything but the panel's drawn rows, or clamped to a
//! floor the panel does not have, lifts V1 out of a short side.
//!
//! Not caught: the band during a lane height or twirl glide, which the test
//! frame draws at its end with motion reduced; and the one frame after a
//! resize that is laid out for the panel height the frame before it recorded.

use std::collections::BTreeSet;

use aria_models::TrackKind;
use gpui::{
    Bounds, Modifiers, Pixels, ScrollDelta, ScrollWheelEvent, TouchPhase, VisualTestContext, point,
    px, size,
};

use crate::app::AppState;
use crate::dock::{Axis, DockTree, Frame, FrameId, Node, PANEL_PROJECT, PANEL_TIMELINE};
use crate::theme::Theme;

/// Layout is in floats; an edge two boxes share lands within this.
const EPS: f32 = 0.51;
/// More video lanes than the video side holds at the test window's size.
const MAX_VIDEO_LANES: usize = 16;
/// More audio lanes than the audio side holds beside the video lanes.
const MAX_AUDIO_LANES: usize = 16;
const VIDEO_SIDE: &str = "timeline-video-canvas";
const AUDIO_SIDE: &str = "timeline-audio-canvas";
const WINDOW_W: f32 = 1600.0;
const WINDOW_H: f32 = 1000.0;
/// The tallest Timeline frame drawn, and the step between heights.
const TALLEST: f32 = 640.0;
const STEP: f32 = 16.0;
/// A scroll far past any stack the tests draw, as one kept from a taller stack.
const STALE_SCROLL: f32 = 10_000.0;

fn bounds(visual: &mut VisualTestContext, selector: &str) -> Option<Bounds<Pixels>> {
    visual.debug_bounds(crate::interned::intern(selector))
}

fn redraw(
    visual: &mut VisualTestContext,
    state: &gpui::Entity<AppState>,
    edit: impl FnOnce(&mut AppState),
) {
    visual.update(|window, app| {
        state.update(app, |state, _| edit(state));
        window.refresh();
    });
    visual.run_until_parked();
}

fn near(a: Pixels, b: Pixels) -> bool {
    (f32::from(a) - f32::from(b)).abs() <= EPS
}

/// The video side's lane headers top to bottom, V<count> down to V1, each as
/// drawn or `None` where it is scrolled out of the side.
fn video_headers(visual: &mut VisualTestContext, count: usize) -> Vec<Option<Bounds<Pixels>>> {
    (1..=count)
        .rev()
        .map(|n| bounds(visual, &format!("track-header-V{n}")))
        .collect()
}

/// The drawn lanes in `headers` meet with no band between them.
fn assert_no_band(headers: &[Option<Bounds<Pixels>>], case: &str) {
    let drawn: Vec<Bounds<Pixels>> = headers.iter().flatten().copied().collect();
    for pair in drawn.windows(2) {
        assert!(
            near(pair[0].bottom(), pair[1].top()),
            "{case}: a band between {:?} and {:?}",
            pair[0],
            pair[1]
        );
    }
}

/// V1 is drawn and ends on the video side's bottom edge, the A/V divider.
fn assert_v1_on_the_divider(visual: &mut VisualTestContext, case: &str) {
    let canvas = bounds(visual, VIDEO_SIDE).expect("the video side is drawn");
    let v1 = bounds(visual, "track-header-V1").unwrap_or_else(|| panic!("{case}: no V1 drawn"));
    assert!(
        near(v1.bottom(), canvas.bottom()),
        "{case}: V1 ends at {:?}, the video side at {:?}",
        v1.bottom(),
        canvas.bottom()
    );
}

/// Turn the wheel `lines` over the side drawn as `side`: positive turns it up.
fn wheel_over(visual: &mut VisualTestContext, side: &str, lines: f32) {
    let canvas = bounds(visual, side).unwrap_or_else(|| panic!("{side} is not drawn"));
    let at = canvas.center();
    visual.simulate_mouse_move(at, None, Modifiers::none());
    visual.simulate_event(ScrollWheelEvent {
        position: at,
        delta: ScrollDelta::Lines(point(0.0, lines)),
        modifiers: Modifiers::none(),
        touch_phase: TouchPhase::Moved,
    });
    visual.run_until_parked();
}

#[gpui::test]
fn video_lanes_stand_on_the_divider_and_audio_lanes_hang_from_it(cx: &mut gpui::TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = crate::test_support::drawn_shell(cx, "lanes-anchor");
    visual.update(|_, app| app.set_reduce_motion(true));
    redraw(visual, &state, |state| {
        state
            .add_track(TrackKind::Audio, "Audio 1")
            .expect("add A1");
    });
    // A second draw: the first records the panel's frame, which sizes the sides.
    redraw(visual, &state, |_| {});

    let mut overflowed = false;
    for count in 1..=MAX_VIDEO_LANES {
        let video_count = visual.read(|app| state.read(app).visual_lane_count());
        assert_eq!(video_count, count, "the sequence holds {count} video lanes");
        let content: f32 = visual.read(|app| {
            let state = state.read(app);
            state.store.sequence.tracks[..video_count]
                .iter()
                .map(|track| track.height)
                .sum()
        });
        for scroll in [0.0, STALE_SCROLL] {
            let case = format!("{count} lanes scrolled {scroll} px");
            redraw(visual, &state, |state| {
                state.video_scroll_y = scroll;
                state.audio_scroll_y = scroll;
            });
            let canvas = bounds(visual, VIDEO_SIDE).expect("the video side is drawn");
            let fits = content <= f32::from(canvas.size.height) + EPS;
            overflowed |= !fits;
            let headers = video_headers(visual, video_count);
            assert_no_band(&headers, &case);
            if fits {
                // A stale scroll is past every lane that fits: nothing moves.
                assert!(
                    headers.iter().all(Option::is_some),
                    "{case}: a lane that fits its side is not drawn"
                );
                assert_v1_on_the_divider(visual, &case);
                let top = headers[0].expect("every lane is drawn");
                assert!(
                    top.top() + px(EPS) >= canvas.top(),
                    "{case}: V{count} starts above the video side"
                );
            } else if scroll == 0.0 {
                assert_v1_on_the_divider(visual, &case);
            } else {
                // The farthest scroll the lanes have: the top lane at the top.
                let top = headers[0].unwrap_or_else(|| panic!("{case}: V{count} is not drawn"));
                assert!(
                    near(top.top(), canvas.top()),
                    "{case}: V{count} starts at {:?}, the side at {:?}",
                    top.top(),
                    canvas.top()
                );
            }

            let audio = bounds(visual, AUDIO_SIDE).expect("the audio side is drawn");
            let a1 = bounds(visual, "track-header-A1")
                .unwrap_or_else(|| panic!("{case}: A1 is not drawn"));
            assert!(
                near(a1.top(), audio.top()),
                "{case}: A1 starts at {:?}, the audio side at {:?}",
                a1.top(),
                audio.top()
            );
        }
        redraw(visual, &state, |state| {
            state.video_scroll_y = 0.0;
            state.audio_scroll_y = 0.0;
            if count < MAX_VIDEO_LANES {
                state
                    .add_track(TrackKind::Video, format!("Video {}", count + 1))
                    .expect("add a video lane");
            }
        });
    }
    assert!(
        overflowed,
        "{MAX_VIDEO_LANES} video lanes fit the side; the overflow case was not drawn"
    );

    // The wheel over the overflowing side: up lifts it to the top lane, down
    // brings V1 back onto the divider.
    let top_lane = format!("track-header-V{MAX_VIDEO_LANES}");
    wheel_over(visual, VIDEO_SIDE, 1_000.0);
    let canvas = bounds(visual, VIDEO_SIDE).expect("the video side is drawn");
    let top = bounds(visual, &top_lane).expect("a wheel up draws the top lane");
    assert!(
        near(top.top(), canvas.top()),
        "a wheel up leaves V{MAX_VIDEO_LANES} at {:?}, the side's top at {:?}",
        top.top(),
        canvas.top()
    );
    wheel_over(visual, VIDEO_SIDE, -1_000.0);
    assert_v1_on_the_divider(visual, "a wheel down");

    // A scroll kept from a taller stack is spent before the first step, so one
    // line down moves the lanes.
    redraw(visual, &state, |state| state.video_scroll_y = STALE_SCROLL);
    wheel_over(visual, VIDEO_SIDE, -1.0);
    let canvas = bounds(visual, VIDEO_SIDE).expect("the video side is drawn");
    let top = bounds(visual, &top_lane).expect("one line down still draws the top lane");
    assert!(
        top.top() < canvas.top() - px(EPS),
        "one line down from a stale scroll leaves V{MAX_VIDEO_LANES} at {:?}, the side's top \
         at {:?}",
        top.top(),
        canvas.top()
    );

    // The audio side overflowed the same way: a wheel down shows its last lane
    // at the side's bottom, and a wheel up hangs A1 from the divider again.
    redraw(visual, &state, |state| {
        state.video_scroll_y = 0.0;
        for n in 2..=MAX_AUDIO_LANES {
            state
                .add_track(TrackKind::Audio, format!("Audio {n}"))
                .expect("add an audio lane");
        }
    });
    redraw(visual, &state, |_| {});
    let last_lane = format!("track-header-A{MAX_AUDIO_LANES}");
    let audio = bounds(visual, AUDIO_SIDE).expect("the audio side is drawn");
    assert!(
        bounds(visual, &last_lane).is_none_or(|last| last.bottom() > audio.bottom() + px(EPS)),
        "{MAX_AUDIO_LANES} audio lanes fit their side; the overflow case was not drawn"
    );
    let a1 = bounds(visual, "track-header-A1").expect("A1 is drawn at rest");
    assert!(
        near(a1.top(), audio.top()),
        "at rest A1 starts at {:?}, the audio side at {:?}",
        a1.top(),
        audio.top()
    );
    wheel_over(visual, AUDIO_SIDE, -1_000.0);
    let audio = bounds(visual, AUDIO_SIDE).expect("the audio side is drawn");
    let last = bounds(visual, &last_lane).expect("a wheel down draws the last lane");
    assert!(
        near(last.bottom(), audio.bottom()),
        "a wheel down leaves A{MAX_AUDIO_LANES} ending at {:?}, the side at {:?}",
        last.bottom(),
        audio.bottom()
    );
    wheel_over(visual, AUDIO_SIDE, 1_000.0);
    let audio = bounds(visual, AUDIO_SIDE).expect("the audio side is drawn");
    let a1 = bounds(visual, "track-header-A1").expect("a wheel up draws A1");
    assert!(
        near(a1.top(), audio.top()),
        "a wheel up leaves A1 at {:?}, the audio side at {:?}",
        a1.top(),
        audio.top()
    );
}

#[test]
fn a_drag_above_the_top_lane_lands_on_it_and_one_below_the_last_on_the_last() {
    let lanes = [(3, 100.0, 148.0), (2, 148.0, 196.0), (1, 196.0, 244.0)];
    let at = |y: f32| super::super::drop::lane_at_y(&lanes, y);
    assert_eq!(at(0.0), Some(3), "the spare height over the video lanes");
    assert_eq!(at(99.9), Some(3));
    assert_eq!(at(100.0), Some(3));
    assert_eq!(at(150.0), Some(2));
    assert_eq!(at(243.9), Some(1));
    assert_eq!(at(244.0), Some(1), "below the last lane");
    assert_eq!(at(400.0), Some(1));
    assert_eq!(super::super::drop::lane_at_y(&[], 10.0), None);
}

#[test]
fn the_first_lane_top_rests_each_side_on_the_divider() {
    // (audio, viewport, content, scroll, the first lane's top)
    let cases = [
        // A short video side stands on the divider, whatever the scroll.
        (false, 200.0, 48.0, 0.0, 152.0),
        (false, 200.0, 48.0, STALE_SCROLL, 152.0),
        (false, 200.0, 200.0, 0.0, 0.0),
        // An overflowing one draws V1 on the divider and lifts toward the top.
        (false, 200.0, 480.0, 0.0, -280.0),
        (false, 200.0, 480.0, 100.0, -180.0),
        (false, 200.0, 480.0, 280.0, 0.0),
        (false, 200.0, 480.0, STALE_SCROLL, 0.0),
        (false, 200.0, 480.0, -5.0, -280.0),
        // Audio hangs from the divider and lifts toward its last lane.
        (true, 200.0, 48.0, 0.0, 0.0),
        (true, 200.0, 48.0, STALE_SCROLL, 0.0),
        (true, 200.0, 480.0, 100.0, -100.0),
        (true, 200.0, 480.0, STALE_SCROLL, -280.0),
        (true, 200.0, 480.0, -5.0, 0.0),
    ];
    for (audio, viewport, content, scroll, top) in cases {
        assert_eq!(
            super::first_lane_top(audio, viewport, content, scroll),
            top,
            "audio {audio}, viewport {viewport}, content {content}, scroll {scroll}"
        );
    }
}

/// The Timeline docked alone in the lower of two stacked frames, `share` of
/// the window high.
fn timeline_under_a_filler(share: f32) -> DockTree {
    DockTree {
        version: DockTree::editing().version,
        root: Node::Split {
            axis: Axis::Column,
            children: vec![
                (
                    1.0 - share,
                    Node::Frame(Frame::new(FrameId::LEFT, vec![PANEL_PROJECT])),
                ),
                (
                    share,
                    Node::Frame(Frame::new(FrameId::RIGHT, vec![PANEL_TIMELINE])),
                ),
            ],
        },
        floating: Vec::new(),
        focused: FrameId::RIGHT,
        maximized: None,
    }
}

fn drawn(visual: &mut VisualTestContext, selector: &str, case: &str) -> Bounds<Pixels> {
    bounds(visual, selector).unwrap_or_else(|| panic!("{case}: {selector} is not drawn"))
}

#[gpui::test]
fn every_timeline_height_places_the_lanes_in_the_sides_it_draws(cx: &mut gpui::TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = crate::test_support::drawn_shell(cx, "lanes-heights");
    visual.update(|_, app| app.set_reduce_motion(true));
    visual.simulate_resize(size(px(WINDOW_W), px(WINDOW_H)));
    let min_h = visual.read(|app| state.read(app).dock_panel_min_size(PANEL_TIMELINE).1);
    // What V1 did across the heights: fit its side, or overflow one too short
    // for it. Either way it ends on the divider.
    let mut v1_seen = BTreeSet::new();
    for (audio, graph) in [(false, false), (true, false), (true, true)] {
        redraw(visual, &state, |state| {
            let has_audio = state.visual_lane_count() < state.store.sequence.tracks.len();
            if audio && !has_audio {
                state
                    .add_track(TrackKind::Audio, "Audio 1")
                    .expect("add A1");
            }
            if graph != state.timeline_graph_open() {
                state.toggle_timeline_graph_editor();
            }
        });
        let mut target = min_h;
        while target <= TALLEST {
            let case = format!("audio {audio}, graph {graph}, frame {target} px");
            redraw(visual, &state, |state| {
                state.dock_tree = timeline_under_a_filler(target / WINDOW_H);
            });
            // The first draw records the panel's height; the second is laid
            // out for it.
            super::built::take();
            redraw(visual, &state, |_| {});
            let (lanes, _) = super::built::take();
            let video_rows = visual.read(|app| state.read(app).visual_lane_count());
            let video = drawn(visual, VIDEO_SIDE, &case);
            let audio_side = drawn(visual, AUDIO_SIDE, &case);
            let edge_top = drawn(visual, "lane-edge-top", &case);
            let edge_bottom = drawn(visual, "lane-edge-bottom", &case);
            let under = drawn(
                visual,
                if graph {
                    "timeline-graph-editor"
                } else {
                    "sequence-overview"
                },
                &case,
            );

            assert!(!lanes.is_empty(), "{case}: no lane was built");
            for lane in &lanes {
                let side = if lane.row < video_rows {
                    video
                } else {
                    audio_side
                };
                assert!(
                    near(px(lane.viewport_h), side.size.height),
                    "{case}: row {} was placed in a viewport {} high, its side is drawn {:?} high",
                    lane.row,
                    lane.viewport_h,
                    side.size.height
                );
            }

            // The stack runs from the top strip's top to what is under it; the
            // side with no lanes keeps its strip.
            let fixed = 2.0 * Theme::SPACE_SM
                + super::super::AV_DIVIDER_H
                + if audio {
                    0.0
                } else {
                    super::super::EMPTY_SIDE_H
                };
            let room = f32::from(under.top() - edge_top.top()) - fixed;
            if room >= 0.0 {
                for (upper, lower, between) in [
                    (
                        edge_top.bottom(),
                        video.top(),
                        "the top strip and the video side",
                    ),
                    (
                        video.bottom() + px(super::super::AV_DIVIDER_H),
                        audio_side.top(),
                        "the divider and the audio side",
                    ),
                    (
                        audio_side.bottom(),
                        edge_bottom.top(),
                        "the audio side and the bottom strip",
                    ),
                    (
                        edge_bottom.bottom(),
                        under.top(),
                        "the bottom strip and what is under it",
                    ),
                ] {
                    assert!(
                        near(upper, lower),
                        "{case}: {between} do not meet: {upper:?} against {lower:?}"
                    );
                }
            } else {
                let with_lanes: &[Bounds<Pixels>] = if audio {
                    &[video, audio_side]
                } else {
                    &[video]
                };
                for side in with_lanes {
                    assert!(
                        f32::from(side.size.height) <= EPS,
                        "{case}: a stack {room} px short of its rows draws a side {:?} high",
                        side.size.height
                    );
                }
            }

            if let Some(v1) = bounds(visual, "track-header-V1") {
                v1_seen.insert(
                    if f32::from(v1.size.height) <= f32::from(video.size.height) + EPS {
                        "fits"
                    } else {
                        "overflows"
                    },
                );
                assert!(
                    near(v1.bottom(), video.bottom()),
                    "{case}: V1 ends at {:?}, the video side at {:?}",
                    v1.bottom(),
                    video.bottom()
                );
            }
            target += STEP;
        }
    }
    assert_eq!(
        v1_seen,
        BTreeSet::from(["fits", "overflows"]),
        "the heights drew V1 both in a side that holds it and in one too short"
    );
}
