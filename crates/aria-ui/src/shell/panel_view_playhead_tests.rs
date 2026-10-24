//! The timeline's playhead layer draws the playhead the timeline body draws.
//!
//! The class closed here is a playhead painted by the shell's layer
//! (`crate::timeline_playhead`) somewhere the body would not paint it, and a
//! timeline body replayed under a layer that cannot stand in for it. Over a
//! still on the timeline, a scrub must replay the timeline body and draw the
//! window the body draws when it paints the playhead itself, with the playhead
//! before the view, on its faded edges, inside it and past it, at rest and
//! with the timecode open for typing. The layer's head and lines must stand
//! at the playhead's pixel of the ruler and of each lane group, and its fade
//! must be the ruler's on each edge the head reaches into. A body built while
//! it drew the playhead itself, and a scrub that changes what the timecode
//! field lays out (a figure gained or lost, the field opened or closed), must
//! build the timeline body. Every state in `INLINE_WHEN` is entered in turn
//! over a fresh project, first by the scrub itself and then before one: the
//! scrub must build the timeline body, the body must paint the playhead, and
//! the window must equal a full redraw. A state added to `INLINE_WHEN` that
//! `enter` cannot enter fails the test. A vsync whose playhead pages the view
//! must build the timeline body.
//!
//! A frame is compared as `panel_view_tests` compares it. Not caught: a part
//! of the body painted over the playhead's line that no state in
//! `INLINE_WHEN` names, and a timecode wider than its field, which the body
//! ends in an ellipsis and the layer clips.

use aria_models::TimeDisplay;
use gpui::{Bounds, EdgeFade, Edges, Pixels, TestAppContext, VisualTestContext, px};

use crate::app::AppState;
use crate::app::clock::ClockSite;
use crate::app::playback_present_tests::{settle, vsync};
use crate::dock::{PANEL_TIMELINE, PanelId};
use crate::edge_fade;
use crate::interned::intern;
use crate::shell::{Moved, Shell};
use crate::timeline_playhead;

use super::builds;
use super::tests::{Drawing, drawing, first_clip, opened, shell_of};

/// Moves the playhead to `tick` the way a ruler scrub does, after `before`
/// changes the state with no notification, and returns the panel bodies the
/// redraw built.
fn scrub(
    visual: &mut VisualTestContext,
    shell: &gpui::Entity<Shell>,
    tick: i64,
    before: impl FnOnce(&mut AppState),
) -> Vec<PanelId> {
    builds::take();
    visual.update(|_, app| {
        shell.update(app, |shell, cx| {
            shell.state.update(cx, |state, _| {
                before(state);
                state.set_playhead(tick);
            });
            shell.redraw(Moved::Playhead, cx);
        })
    });
    visual.run_until_parked();
    builds::take()
}

/// `items` with each repeat after the first dropped. A scrub draws a frame for
/// the playhead and another when the still's frame lands, and the layer paints
/// the same parts in both.
fn distinct<T: PartialEq>(items: Vec<T>) -> Vec<T> {
    let mut kept = Vec::with_capacity(items.len());
    for item in items {
        if !kept.contains(&item) {
            kept.push(item);
        }
    }
    kept
}

/// The window drawn anew with every body built, the timeline body painting
/// the playhead itself while `inline`.
fn redrawn(visual: &mut VisualTestContext, inline: bool) -> Drawing {
    timeline_playhead::FORCE_INLINE.set(inline);
    visual.update(|window, _| window.refresh());
    visual.run_until_parked();
    timeline_playhead::FORCE_INLINE.set(false);
    drawing(visual)
}

/// Changes the state and redraws the window in full.
fn change(
    state: &gpui::Entity<AppState>,
    visual: &mut VisualTestContext,
    f: impl FnOnce(&mut AppState),
) {
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            f(state);
            cx.notify();
        })
    });
    visual.run_until_parked();
}

/// The timeline's scale and scroll as drawn: px per second, ticks per second,
/// the scroll offset, and the time area's width.
fn view(state: &gpui::Entity<AppState>, visual: &mut VisualTestContext) -> (f32, f32, f32, f32) {
    visual.read(|app| {
        let state = state.read(app);
        (
            crate::transport::TIMELINE_BASE_PPS * crate::transport::clamp_zoom(state.zoom),
            state.store.sequence.ticks_per_second(),
            state.scroll_x,
            state.timeline_viewport_px(),
        )
    })
}

/// Scales the timeline so the still spans `views` widths of the time area.
fn spread(state: &gpui::Entity<AppState>, visual: &mut VisualTestContext, views: f32) {
    let (pps, fps, _, width) = view(state, visual);
    let seconds = visual.read(|app| state.read(app).store.sequence.duration_ticks) as f32 / fps;
    let zoom = visual.read(|app| state.read(app).zoom);
    change(state, visual, |state| {
        state.set_zoom(zoom * views * width / (seconds * pps), false)
    });
    let (pps, _, _, width) = view(state, visual);
    assert!(
        seconds * pps >= views * width * 0.99,
        "the zoom ceiling holds the still to {} px of a {width} px view",
        seconds * pps
    );
}

#[gpui::test]
fn a_scrub_replays_the_timeline_body_under_the_playhead_it_would_draw(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = opened(cx, "playhead-layer");
    let shell = shell_of(visual);
    spread(&state, visual, 3.0);
    let (_, _, _, width) = view(&state, visual);
    change(&state, visual, |state| {
        state.scroll_timeline_to(width, false)
    });
    let ruler: Bounds<Pixels> = visual
        .debug_bounds(intern("timeline-ruler"))
        .expect("the timeline draws its ruler");
    let groups: Vec<Bounds<Pixels>> = ["timeline-video-canvas", "timeline-audio-canvas"]
        .into_iter()
        .filter_map(|group| visual.debug_bounds(group))
        .collect();
    assert!(!groups.is_empty(), "the timeline draws a lane group");
    let (pps, fps, scroll, width) = view(&state, visual);
    assert!(scroll > 0.0, "the view scrolled into the still");

    let mut last = drawing(visual);
    for open in [false, true] {
        if open {
            change(&state, visual, |state| {
                state.clock_site = ClockSite::Timeline;
                state.focus_clock(true);
            });
            last = drawing(visual);
        }
        // Before the view, on the left fade, inside, on the right fade, and
        // past the view, with the edges the head reaches into.
        for (x, left, right) in [
            (-30.0, true, false),
            (3.0, true, false),
            (width / 2.0, false, false),
            (width - 6.0, false, true),
            (width + 30.0, false, true),
        ] {
            // A tick on the near side of `x`, so the head reaches as far into
            // the faded edge as `x` does.
            let near = (scroll + x) / pps * fps;
            let tick = if left {
                near.floor()
            } else if right {
                near.ceil()
            } else {
                near.round()
            } as i64;
            edge_fade::take_painted();
            timeline_playhead::take_lane_lines();
            let built = scrub(visual, &shell, tick, |_| {});
            assert!(
                !built.contains(&PANEL_TIMELINE),
                "a scrub to {x} px with the timecode open: {open} built the timeline body"
            );
            let at = tick as f32 / fps * pps - scroll;
            let (head, line) =
                timeline_playhead::painted_mark().expect("the layer paints the head");
            assert!(
                (f32::from(head.left()) - (f32::from(ruler.left()) + at - 5.0)).abs() < 0.01
                    && (f32::from(line.left()) - (f32::from(ruler.left()) + at)).abs() < 0.01
                    && line.top() == ruler.top()
                    && line.size.height == ruler.size.height,
                "the layer painted the head at {head:?} and the line at {line:?} for a playhead \
                 at {at} px of a ruler at {ruler:?}"
            );
            let lines = distinct(timeline_playhead::take_lane_lines());
            for group in &groups {
                let across: Vec<&Bounds<Pixels>> = lines
                    .iter()
                    .filter(|line| line.top() == group.top())
                    .collect();
                let group_w = f32::from(group.size.width);
                if (0.0..=group_w).contains(&at) {
                    assert!(
                        across.len() == 1
                            && (f32::from(across[0].left()) - (f32::from(group.left()) + at)).abs()
                                < 0.01
                            && across[0].size.height == group.size.height,
                        "the layer painted {across:?} across the lane group at {group:?} for a \
                         playhead at {at} px"
                    );
                } else if at < -5.0 || at > group_w + 5.0 {
                    assert!(
                        across.is_empty(),
                        "the layer painted {across:?} across the lane group at {group:?} for a \
                         playhead {at} px off it"
                    );
                }
            }
            assert!(
                lines
                    .iter()
                    .all(|line| groups.iter().any(|group| group.top() == line.top())),
                "the layer painted lane lines {lines:?} off every lane group {groups:?}"
            );
            let faded: Vec<Edges<Pixels>> = distinct(
                edge_fade::take_painted()
                    .into_iter()
                    .filter(|fade| fade.bounds == ruler)
                    .map(|fade| fade.bands)
                    .collect(),
            );
            let cached = drawing(visual);
            assert!(
                cached != last,
                "the scrub to {x} px drew the window it drew before"
            );
            let inline = redrawn(visual, true);
            let own: EdgeFade = edge_fade::take_painted()
                .into_iter()
                .find(|fade| fade.bounds == ruler)
                .expect("the ruler fades the edges its marks continue past");
            let wanted: Vec<Edges<Pixels>> = (left || right)
                .then(|| Edges {
                    top: px(0.0),
                    bottom: px(0.0),
                    left: if left { own.bands.left } else { px(0.0) },
                    right: if right { own.bands.right } else { px(0.0) },
                })
                .into_iter()
                .collect();
            assert_eq!(
                faded, wanted,
                "the layer faded the head at {x} px other than the ruler fades its edges, {:?}",
                own.bands
            );
            let full = redrawn(visual, false);
            assert!(
                cached == full,
                "the scrub to {x} px drew a window that differs from a full redraw\n{}",
                cached.differences(&full)
            );
            assert!(
                full == inline,
                "the layer at {x} px drew a window that differs from the body's own playhead\n{}",
                full.differences(&inline)
            );
            last = full;
        }
    }

    // A body built while it drew the playhead itself holds no lines for the
    // layer to stand in for.
    redrawn(visual, true);
    let tick = ((scroll + width / 3.0) / pps * fps).round() as i64;
    let built = scrub(visual, &shell, tick, |_| {});
    assert!(
        built.contains(&PANEL_TIMELINE),
        "a scrub over a body that drew the playhead itself left it replaying"
    );
    let cached = drawing(visual);
    let full = redrawn(visual, false);
    assert!(
        cached == full,
        "a scrub over a body that drew the playhead itself drew a window that differs from a \
         full redraw\n{}",
        cached.differences(&full)
    );
}

#[gpui::test]
fn a_scrub_that_changes_what_the_timecode_field_lays_out_builds_the_timeline_body(
    cx: &mut TestAppContext,
) {
    fn rest(_: &mut AppState) {}
    fn open(state: &mut AppState) {
        state.clock_site = ClockSite::Timeline;
        state.focus_clock(true);
    }
    fn close(state: &mut AppState) {
        state.focus_clock(false);
    }

    cx.executor().allow_parking();
    let (state, visual) = opened(cx, "playhead-timecode");
    let shell = shell_of(visual);
    change(&state, visual, |state| {
        state
            .set_time_display(TimeDisplay::Frames)
            .expect("count the sequence in frames")
    });
    // The first tick whose count has a figure more than the tick before.
    let wider = visual
        .read(|app| {
            let state = state.read(app);
            (1..state.store.sequence.duration_ticks).find(|&tick| {
                state.sequence_time(tick).len() != state.sequence_time(tick - 1).len()
            })
        })
        .expect("a count inside the still that gains a figure");
    let cases: [(&str, i64, fn(&mut AppState), i64, fn(&mut AppState)); 4] = [
        ("gains a figure", wider - 1, rest, wider, rest),
        ("loses a figure", wider, rest, wider - 1, rest),
        ("opens the field", 2, rest, 3, open),
        ("closes the field", 2, open, 3, close),
    ];
    for (what, from, set, to, before) in cases {
        change(&state, visual, |state| {
            set(state);
            state.set_playhead(from);
        });
        let built = scrub(visual, &shell, to, before);
        assert!(
            built.contains(&PANEL_TIMELINE),
            "a scrub that {what} left the timeline body replaying"
        );
        let cached = drawing(visual);
        let full = redrawn(visual, false);
        assert!(
            cached == full,
            "a scrub that {what} drew a window that differs from a full redraw\n{}",
            cached.differences(&full)
        );
    }
}

/// Enters the state `INLINE_WHEN` lists as `name`.
fn enter(name: &str, state: &mut AppState) {
    let clip = first_clip(state);
    match name {
        "property rows" => state.toggle_expanded(clip.raw()),
        "graph editor" => state.toggle_timeline_graph_editor(),
        "trim" => {
            state.trim_feedback = Some(crate::types::TrimFeedback {
                tool_label: "Ripple".into(),
                clip_name: "texture".into(),
                delta_ticks: 4,
                delta_label: "+4".into(),
                detail_label: "".into(),
            })
        }
        "snap guide" => {
            state.snap_guide = Some(crate::snap::SnapCandidate::new(
                12,
                crate::snap::SnapKind::Marker,
            ))
        }
        "empty timeline" => {
            state.select_all_unlocked();
            state
                .ripple_delete_selected()
                .expect("delete the still from the timeline");
        }
        other => panic!("INLINE_WHEN lists {other:?}, which `enter` cannot enter: add it there"),
    }
}

#[gpui::test]
fn a_scrub_in_every_inline_state_builds_the_timeline_body(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    for (index, (name, holds)) in timeline_playhead::INLINE_WHEN.iter().enumerate() {
        let (state, visual) = opened(cx, &format!("playhead-inline-{index}"));
        let shell = shell_of(visual);
        assert!(
            visual.read(|app| timeline_playhead::layered(state.read(app))),
            "the layer paints the playhead over a still before {name} is entered"
        );
        // Entered by the scrub itself: the body's record still has the layer
        // painting the lines.
        let built = scrub(visual, &shell, 9, |state| enter(name, state));
        visual.read(|app| {
            let state = state.read(app);
            assert!(holds(state), "`enter` did not enter {name}");
            assert!(
                !timeline_playhead::layered(state),
                "the layer paints the playhead over {name}"
            );
        });
        assert!(
            built.contains(&PANEL_TIMELINE),
            "the scrub that entered {name} left the timeline body replaying"
        );
        redrawn(visual, false);
        // Entered before the scrub: the body painted the playhead itself.
        let built = scrub(visual, &shell, 17, |_| {});
        assert!(
            built.contains(&PANEL_TIMELINE),
            "a scrub over {name} left the timeline body replaying"
        );
        let cached = drawing(visual);
        let full = redrawn(visual, false);
        assert!(
            cached == full,
            "a scrub over {name} drew a window that differs from a full redraw\n{}",
            cached.differences(&full)
        );
        visual.update(|window, _| window.remove_window());
    }
}

#[gpui::test]
async fn a_vsync_that_pages_the_view_builds_the_timeline_body(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = opened(cx, "playhead-follow");
    spread(&state, visual, 3.0);
    let (pps, fps, scroll, width) = view(&state, visual);
    // The first tick the view pages for, and the run starts on the one
    // before it.
    let (follow, duration) = visual.read(|app| {
        let state = state.read(app);
        (
            state.ui_settings.timeline_display.follow,
            state.store.sequence.duration_ticks,
        )
    });
    let pages = (1..duration)
        .find(|&tick| {
            follow
                .target(tick as f32 / fps * pps, scroll, width)
                .is_some()
        })
        .expect("a tick of the still the view pages for");
    change(&state, visual, |state| state.set_playhead(pages - 1));
    for frame in 0.. {
        assert!(frame < 8, "the open never came to rest");
        if visual.update(|window, app| window.simulate_next_frame(app)) == 0 {
            break;
        }
        visual.run_until_parked();
    }
    visual.update(|window, app| {
        state.update(app, |state, _| state.toggle_playback());
        crate::app::start_playback(state.clone(), window, app);
        state.update(app, |_, cx| cx.notify());
    });
    settle(&state, visual).await;
    visual.run_until_parked();
    builds::take();
    assert_eq!(vsync(&state, visual, 1), 1, "the run ended");
    visual.run_until_parked();
    let built = builds::take();
    let (tick, paged) = visual.read(|app| {
        let state = state.read(app);
        (state.playhead.value, state.scroll_x)
    });
    assert!(
        tick >= pages,
        "the vsync moved the playhead to {tick}, short of {pages}"
    );
    assert_ne!(paged, scroll, "the vsync to tick {tick} left the view");
    assert!(
        built.contains(&PANEL_TIMELINE),
        "the vsync that paged the view left the timeline body replaying"
    );
    let cached = drawing(visual);
    let full = redrawn(visual, false);
    assert!(
        cached == full,
        "the vsync that paged the view drew a window that differs from a full redraw\n{}",
        cached.differences(&full)
    );
    visual.update(|_, app| state.update(app, |state, _| state.toggle_playback()));
}
