//! Where each side's lanes stand in their viewport, measured on the drawn
//! shell.
//!
//! The class closed here is a lane side that leaves an empty band between its
//! lanes and the A/V divider. Video lanes number upward from V1 on the divider
//! and audio lanes downward from A1 under it, so a side's spare height is above
//! the top video lane and below the last audio lane. The video side is drawn
//! at every lane count from one to past the count its side holds, so a lift
//! that is wrong for one count, or that stays on once the lanes overflow the
//! side, goes red. A drag over the spare height above the top lane lands on
//! the top lane.
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
use gpui::{Bounds, Pixels, VisualTestContext, px, size};

use crate::app::AppState;
use crate::dock::{Axis, DockTree, Frame, FrameId, Node, PANEL_PROJECT, PANEL_TIMELINE};
use crate::theme::Theme;

/// Layout is in floats; an edge two boxes share lands within this.
const EPS: f32 = 0.51;
/// More video lanes than the video side holds at the test window's size.
const MAX_VIDEO_LANES: usize = 16;
const WINDOW_W: f32 = 1600.0;
const WINDOW_H: f32 = 1000.0;
/// The tallest Timeline frame drawn, and the step between heights.
const TALLEST: f32 = 640.0;
const STEP: f32 = 16.0;

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
        let canvas = bounds(visual, "timeline-video-canvas").expect("the video side is drawn");
        let content: f32 = visual.read(|app| {
            let state = state.read(app);
            state.store.sequence.tracks[..video_count]
                .iter()
                .map(|track| track.height)
                .sum()
        });
        // Top to bottom: V<count> down to V1.
        let headers: Vec<Option<Bounds<Pixels>>> = (1..=video_count)
            .rev()
            .map(|n| bounds(visual, &format!("track-header-V{n}")))
            .collect();
        if content <= f32::from(canvas.size.height) + EPS {
            let drawn: Vec<Bounds<Pixels>> = headers
                .iter()
                .enumerate()
                .map(|(i, header)| {
                    header.unwrap_or_else(|| {
                        panic!("{count} lanes: V{} is not drawn", video_count - i)
                    })
                })
                .collect();
            let v1 = drawn[drawn.len() - 1];
            assert!(
                near(v1.bottom(), canvas.bottom()),
                "{count} lanes: V1 ends at {:?}, the video side at {:?}",
                v1.bottom(),
                canvas.bottom()
            );
            for pair in drawn.windows(2) {
                assert!(
                    near(pair[0].bottom(), pair[1].top()),
                    "{count} lanes: a band between {:?} and {:?}",
                    pair[0],
                    pair[1]
                );
            }
            assert!(
                drawn[0].top() + gpui::px(EPS) >= canvas.top(),
                "{count} lanes: V{count} starts above the video side"
            );
        } else {
            overflowed = true;
            let top = headers[0].unwrap_or_else(|| panic!("{count} lanes: V{count} is not drawn"));
            assert!(
                near(top.top(), canvas.top()),
                "{count} lanes overflow the side: V{count} starts at {:?}, the side at {:?}",
                top.top(),
                canvas.top()
            );
            break;
        }

        let audio = bounds(visual, "timeline-audio-canvas").expect("the audio side is drawn");
        let a1 = bounds(visual, "track-header-A1").expect("A1 is drawn");
        assert!(
            near(a1.top(), audio.top()),
            "{count} video lanes: A1 starts at {:?}, the audio side at {:?}",
            a1.top(),
            audio.top()
        );

        redraw(visual, &state, |state| {
            state
                .add_track(TrackKind::Video, format!("Video {}", count + 1))
                .expect("add a video lane");
        });
    }
    assert!(
        overflowed,
        "{MAX_VIDEO_LANES} video lanes fit the side; the overflow case was not drawn"
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
fn the_first_lane_top_lifts_only_short_video_content() {
    assert_eq!(super::first_lane_top(false, 200.0, 48.0), 152.0);
    assert_eq!(super::first_lane_top(false, 200.0, 200.0), 0.0);
    assert_eq!(super::first_lane_top(false, 200.0, 480.0), 0.0);
    assert_eq!(super::first_lane_top(true, 200.0, 48.0), 0.0);
    assert_eq!(super::first_lane_top(true, 200.0, 480.0), 0.0);
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
    // What V1 did across the heights: stood on the divider, or hung from the
    // top of a side too short for it.
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
            let video = drawn(visual, "timeline-video-canvas", &case);
            let audio_side = drawn(visual, "timeline-audio-canvas", &case);
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
                if f32::from(v1.size.height) <= f32::from(video.size.height) + EPS {
                    v1_seen.insert("stands");
                    assert!(
                        near(v1.bottom(), video.bottom()),
                        "{case}: V1 ends at {:?}, the video side at {:?}",
                        v1.bottom(),
                        video.bottom()
                    );
                } else {
                    v1_seen.insert("hangs");
                    assert!(
                        near(v1.top(), video.top()),
                        "{case}: V1 is taller than its side and starts at {:?}, the side at {:?}",
                        v1.top(),
                        video.top()
                    );
                }
            }
            target += STEP;
        }
    }
    assert_eq!(
        v1_seen,
        BTreeSet::from(["hangs", "stands"]),
        "the heights drew V1 both in a side that holds it and in one too short"
    );
}
