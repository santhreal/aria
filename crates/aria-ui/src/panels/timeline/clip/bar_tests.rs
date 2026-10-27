//! Pressing the parts of a clip bar where the canvas painted them, and what
//! the canvas paints for the state a bar draws.
//!
//! Closes the class "a press lands somewhere other than what was drawn under
//! it": the line hit-tested at a height its paint did not use, a keyframe dot a
//! press passes through to the line, a Ctrl+click on the line that keys
//! nothing. Also pins the fills the spec names: the fx badge grey, yellow, or
//! purple with the purple underline when both, an offline bar red, and the
//! bracket on a held edge red while its drag is stopped at the limit. Not
//! covered: the pointer's cursor shape, which the test platform does not
//! report back, and glyphs, which the test text system does not draw.

use std::path::PathBuf;

use aria_commands::{Command, EditCommand};
use aria_models::{ClipId, ClipParam, MarkerColor, Param, ParamValue};
use gpui::{
    Background, Bounds, Entity, Modifiers, MouseButton, MouseDownEvent, Pixels, Point,
    TestAppContext, VisualTestContext, point, px, size,
};

use super::{BarRows, DOT, FX_H, FX_W};
use crate::app::AppState;
use crate::surfaces::timeline_gesture::{Drag, Gesture, Span};
use crate::theme::Theme;
use crate::types::{ContextTarget, TrimSide};

fn texture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/texture.png")
}

/// One quad the last frame painted, in window pixels.
#[derive(Debug)]
struct Painted {
    bounds: Bounds<Pixels>,
    background: Background,
    rounded: bool,
}

/// Every quad the last frame painted with its centre inside `within`.
fn painted(visual: &mut VisualTestContext, within: Bounds<Pixels>) -> Vec<Painted> {
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
                within.contains(&bounds.center()).then_some(Painted {
                    bounds,
                    background: quad.background,
                    rounded: quad.corner_radii.top_left.0 > 0.0,
                })
            })
            .collect()
    })
}

/// A drawn shell with `asset` placed at the start of the timeline.
fn placed<'a>(
    cx: &'a mut TestAppContext,
    tag: &str,
    asset: PathBuf,
) -> (Entity<AppState>, &'a mut VisualTestContext, ClipId) {
    let (state, visual) = crate::test_support::drawn_shell(cx, tag);
    let clip = visual.update(|window, app| {
        app.set_reduce_motion(true);
        let clip = state.update(app, |s, _| {
            let media = s.import_media(asset).expect("import the fixture");
            s.drop_media_on_timeline(media, None, 0)
                .expect("place the fixture");
            let id = s
                .store
                .sequence
                .tracks
                .iter()
                .flat_map(|lane| lane.clips.iter())
                .next()
                .expect("one clip")
                .id;
            ClipId::from_raw(id)
        });
        window.refresh();
        clip
    });
    visual.run_until_parked();
    (state, visual, clip)
}

/// The V1 lane on the canvas: the canvas's width at the height of V1's header.
fn v1_lane(visual: &mut VisualTestContext) -> Bounds<Pixels> {
    let canvas = visual
        .debug_bounds("timeline-video-canvas")
        .expect("the video canvas is drawn");
    let header = visual
        .debug_bounds("track-header-V1")
        .expect("the V1 header is drawn");
    Bounds::new(
        point(canvas.left(), header.top()),
        size(canvas.size.width, header.size.height),
    )
}

fn edit(visual: &mut VisualTestContext, state: &Entity<AppState>, command: EditCommand) {
    visual.update(|window, app| {
        state.update(app, |s, _| {
            s.apply_command(Command::Edit(command))
                .expect("the edit is accepted");
        });
        window.refresh();
    });
    visual.run_until_parked();
}

fn opacity(visual: &mut VisualTestContext, state: &Entity<AppState>, clip: ClipId) -> Param<f32> {
    visual.read(|app| {
        let engine = state.read(app).engine().lock();
        engine
            .project()
            .clip(clip)
            .expect("the clip is in the project")
            .transform
            .opacity
            .clone()
    })
}

fn drag(visual: &mut VisualTestContext, from: Point<Pixels>, to: Point<Pixels>) {
    visual.simulate_mouse_move(from, None, Modifiers::none());
    visual.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
    visual.simulate_mouse_move(to, Some(MouseButton::Left), Modifiers::none());
    visual.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
    visual.run_until_parked();
}

fn near(a: Pixels, b: Pixels) -> bool {
    (f32::from(a) - f32::from(b)).abs() <= 0.5
}

/// The one line the bar on `lane` paints: a hairline narrower than the lane,
/// whose own rules span all of it.
fn line_of(visual: &mut VisualTestContext, lane: Bounds<Pixels>) -> Bounds<Pixels> {
    let lines: Vec<Painted> = painted(visual, lane)
        .into_iter()
        .filter(|quad| {
            quad.bounds.size.height <= px(1.01)
                && quad.bounds.size.width >= px(20.0)
                && quad.bounds.size.width < lane.size.width - px(1.0)
        })
        .collect();
    assert_eq!(lines.len(), 1, "one line on one bar: {lines:?}");
    lines[0].bounds
}

/// The timeline tick `clip` starts on.
fn start_of(visual: &mut VisualTestContext, state: &Entity<AppState>, clip: ClipId) -> i64 {
    visual.read(|app| {
        let engine = state.read(app).engine().lock();
        engine
            .project()
            .clip(clip)
            .expect("the clip is in the project")
            .timeline
            .start
            .value
    })
}

#[gpui::test]
fn a_press_on_the_painted_line_drags_it_and_a_ctrl_click_keys_it(cx: &mut TestAppContext) {
    let (state, visual, clip) = placed(cx, "bar-line", texture());
    let lane = v1_lane(visual);
    let rows = BarRows::of(f32::from(lane.size.height));
    assert!(
        f32::from(lane.size.height) >= super::DOTS_FROM,
        "a new lane is tall enough for keyframe dots"
    );

    // An unanimated line is one hairline across the bar, at the top of the
    // scale for Opacity at 100 %.
    let line = line_of(visual, lane);
    assert!(near(line.center().y, lane.top() + px(rows.line_top)));

    // Half the band down halves it.
    let at = point(line.left() + px(30.0), line.center().y);
    drag(visual, at, point(at.x, at.y + px(rows.band() / 2.0)));
    let after = opacity(visual, &state, clip);
    let value = after
        .constant()
        .expect("a drag on a flat line keeps it flat");
    assert!(
        (value - 0.5).abs() < 1e-3,
        "opacity after the drag: {value}"
    );
    assert!(
        visual.read(|app| !state.read(app).clip_line_held()),
        "the release lets the line go"
    );

    // The line repaints where the drag left it, and a Ctrl+click there keys
    // the value it draws.
    let line = line_of(visual, lane);
    assert!(near(line.center().y, lane.top() + px(rows.line_y(0.5))));
    let key_at = point(line.left() + px(40.0), line.center().y);
    visual.simulate_mouse_move(key_at, None, Modifiers::none());
    visual.simulate_click(key_at, Modifiers::secondary_key());
    visual.run_until_parked();
    let keys = opacity(visual, &state, clip).keyframes().to_vec();
    assert_eq!(keys.len(), 1, "a Ctrl+click on the line adds one keyframe");
    assert!((keys[0].value - 0.5).abs() < 1e-3);

    // The key is drawn as a dot on the line, and a press on the dot moves it.
    let dots: Vec<Painted> = painted(visual, lane)
        .into_iter()
        .filter(|quad| {
            quad.rounded
                && near(quad.bounds.size.width, px(DOT))
                && near(quad.bounds.size.height, px(DOT))
        })
        .collect();
    assert_eq!(dots.len(), 1, "one dot per keyframe: {dots:?}");
    let dot = dots[0].bounds.center();
    assert!(near(dot.y, line.center().y));
    drag(visual, dot, point(dot.x, dot.y - px(rows.band() / 4.0)));
    let keys = opacity(visual, &state, clip).keyframes().to_vec();
    assert_eq!(keys.len(), 1, "the key moved rather than gaining a sibling");
    assert!(
        (keys[0].value - 0.75).abs() < 1e-3,
        "key after the drag: {}",
        keys[0].value
    );

    // A press on the dot holds the key whichever way the pointer goes: a drag
    // along the lane from where the dot now sits leaves the clip in place.
    let dot = point(dot.x, lane.top() + px(rows.line_y(0.75)));
    drag(visual, dot, point(dot.x + px(120.0), dot.y));
    assert_eq!(
        start_of(visual, &state, clip),
        0,
        "a drag from a key dot moved the clip"
    );
    let keys = opacity(visual, &state, clip).keyframes().to_vec();
    assert_eq!(keys.len(), 1);
    assert!((keys[0].value - 0.75).abs() < 1e-3);
}

/// The line runs through the middle of a clip's body, where a hand grabs a clip
/// to move it. A press there reads the first travel past the click slop: along
/// the lane it moves the clip, across the lane it moves the line, and under the
/// slop it moves nothing; a double-click there opens the clip as one anywhere
/// on its body does. Before, the press held the line whichever way the pointer
/// went, so a clip dragged by its middle stayed put and its line moved by the
/// hand's jitter instead. Not covered: a press on a keyframe dot, which holds
/// the key whichever way it goes and is pinned by the test above, and a line
/// the press can no longer hold, whose clip or value went away mid-press.
#[gpui::test]
fn a_press_on_the_line_moves_the_clip_along_the_lane_and_the_line_across_it(
    cx: &mut TestAppContext,
) {
    let (state, visual, clip) = placed(cx, "bar-line-axis", texture());
    let lane = v1_lane(visual);
    let rows = BarRows::of(f32::from(lane.size.height));
    let slop = crate::app::value_scrub::CLICK_SLOP_PX;
    let full = |visual: &mut VisualTestContext| {
        opacity(visual, &state, clip)
            .constant()
            .expect("the line stays flat")
    };

    // Travel inside the slop, either way, is a click: it selects the clip and
    // moves nothing.
    visual.update(|window, app| {
        state.update(app, |state, _| state.clear_selection());
        window.refresh();
    });
    visual.run_until_parked();
    let selected = |visual: &mut VisualTestContext| {
        visual.read(|app| state.read(app).selection.contains(&clip))
    };
    assert!(!selected(visual), "the clip starts deselected");
    let line = line_of(visual, lane);
    let at = point(line.left() + px(30.0), line.center().y);
    for (dx, dy) in [
        (slop - 1.0, 0.0),
        (0.0, slop - 1.0),
        (1.0 - slop, 1.0 - slop),
    ] {
        drag(visual, at, point(at.x + px(dx), at.y + px(dy)));
        assert_eq!(
            start_of(visual, &state, clip),
            0,
            "({dx}, {dy}) moved the clip"
        );
        assert_eq!(full(visual), 1.0, "({dx}, {dy}) moved the line");
    }
    assert!(selected(visual), "a click on the line selects its clip");

    // Off its dots the line is the clip's body: a double-click there opens
    // the clip's take in a Source tab linked to it, and moves nothing.
    visual.simulate_event(MouseDownEvent {
        position: at,
        modifiers: Modifiers::none(),
        button: MouseButton::Left,
        click_count: 2,
        first_mouse: false,
    });
    visual.simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
    visual.run_until_parked();
    let linked = visual.read(|app| state.read(app).sources.focused().and_then(|view| view.clip));
    assert_eq!(
        linked,
        Some(clip.raw()),
        "a double-click on the line opens the clip"
    );
    assert_eq!(full(visual), 1.0, "the double-click moved the line");

    // Across the lane with a hand's sideways jitter: the line, not the clip.
    drag(
        visual,
        at,
        point(at.x + px(slop), at.y + px(rows.band() / 2.0)),
    );
    assert!(
        (full(visual) - 0.5).abs() < 1e-3,
        "opacity after the drag down"
    );
    assert_eq!(
        start_of(visual, &state, clip),
        0,
        "a drag across the lane moved the clip"
    );

    // Along the lane with a downward jitter, from the line where it now is:
    // the clip, and the line rides along unchanged.
    let line = line_of(visual, lane);
    let at = point(line.left() + px(30.0), line.center().y);
    let (pps, fps) = visual.read(|app| {
        let s = state.read(app);
        (
            crate::transport::TIMELINE_BASE_PPS * s.zoom,
            s.store.sequence.ticks_per_second(),
        )
    });
    let along = 120.0;
    drag(visual, at, point(at.x + px(along), at.y + px(slop)));
    assert_eq!(
        start_of(visual, &state, clip),
        (along / pps * fps).round() as i64,
        "a drag along the lane from the line moves the clip by the pointer's travel"
    );
    assert!(
        (full(visual) - 0.5).abs() < 1e-3,
        "the move left the line where it was"
    );
    assert!(
        visual.read(|app| !state.read(app).clip_line_held()),
        "a move holds no line"
    );
}

#[gpui::test]
fn the_fx_badge_is_grey_yellow_purple_or_yellow_over_purple(cx: &mut TestAppContext) {
    let (state, visual, clip) = placed(cx, "bar-badge", texture());
    let lane = v1_lane(visual);
    let theme = visual.read(Theme::of);
    let yellow = theme.marker(MarkerColor::Yellow);
    let purple = theme.marker(MarkerColor::Purple);
    let badge = |visual: &mut VisualTestContext| {
        let quads = painted(visual, lane);
        let badges: Vec<&Painted> = quads
            .iter()
            .filter(|quad| {
                quad.rounded
                    && near(quad.bounds.size.width, px(FX_W))
                    && near(quad.bounds.size.height, px(FX_H))
            })
            .collect();
        assert_eq!(badges.len(), 1, "one badge on one bar: {badges:?}");
        let fx = badges[0].bounds;
        let underlined = quads.iter().any(|quad| {
            near(quad.bounds.size.width, px(FX_W))
                && near(quad.bounds.size.height, px(2.0))
                && near(quad.bounds.left(), fx.left())
                && quad.background == Background::from(purple)
        });
        (badges[0].background, underlined)
    };

    assert_eq!(badge(visual), (Background::from(theme.text_muted), false));

    edit(
        visual,
        &state,
        EditCommand::SetParamConstant {
            clip,
            param: ClipParam::Opacity,
            value: ParamValue::Scalar(0.5),
        },
    );
    assert_eq!(badge(visual), (Background::from(purple), false));

    edit(
        visual,
        &state,
        EditCommand::AddEffect {
            clip,
            effect_id: "gaussian_blur".to_owned(),
        },
    );
    assert_eq!(badge(visual), (Background::from(yellow), true));

    edit(
        visual,
        &state,
        EditCommand::SetParamConstant {
            clip,
            param: ClipParam::Opacity,
            value: ParamValue::Scalar(1.0),
        },
    );
    assert_eq!(badge(visual), (Background::from(yellow), false));
}

#[gpui::test]
fn a_bar_whose_file_is_gone_is_painted_red_with_its_file_name(cx: &mut TestAppContext) {
    let dir = crate::scratch_pool::scratch("bar-offline-files");
    let gone = dir.join("gone.png");
    std::fs::copy(texture(), &gone).expect("stage a copy of the fixture");
    let (state, visual, clip) = placed(cx, "bar-offline", gone.clone());
    let lane = v1_lane(visual);
    let danger = visual.read(|app| Theme::of(app).danger);
    let red = |visual: &mut VisualTestContext| {
        painted(visual, lane)
            .iter()
            .any(|quad| quad.background == Background::from(danger))
    };
    assert!(!red(visual), "a bar whose file is there takes its label");

    std::fs::remove_file(&gone).expect("lose the file");
    crate::projection::invalidate_media_statuses();
    let other = dir.join("other.png");
    std::fs::copy(texture(), &other).expect("stage a second copy");
    visual.update(|window, app| {
        state.update(app, |s, _| {
            s.import_media(other)
                .expect("an import re-projects the pool");
            s.sync_from_engine();
        });
        window.refresh();
    });
    visual.run_until_parked();

    let offline = visual.read(|app| {
        state
            .read(app)
            .ui_clip_ref(clip)
            .and_then(|ui| ui.offline.clone())
    });
    assert_eq!(offline.as_ref().map(|name| name.as_ref()), Some("gone.png"));
    assert!(red(visual), "an offline bar is red whatever its label");
}

#[gpui::test]
fn a_held_edge_stopped_at_its_limit_is_painted_red(cx: &mut TestAppContext) {
    let (state, visual, clip) = placed(cx, "bar-held-edge", texture());
    let lane = v1_lane(visual);
    let (warning, danger) = visual.read(|app| {
        let theme = Theme::of(app);
        (
            Background::from(theme.warning),
            Background::from(theme.danger),
        )
    });
    // The clip starts at the sequence start, so a head trim to the left is
    // stopped there and one to the right follows the pointer.
    for (delta, fill) in [(2, warning), (-(1 << 20), danger), (2, warning)] {
        visual.update(|window, app| {
            state.update(app, |s, _| {
                let span = {
                    let ui = s.ui_clip_ref(clip).expect("projected");
                    Span {
                        start: ui.timeline_start_ticks,
                        end: ui.timeline_end_ticks,
                        threshold: 0,
                        magnet: false,
                    }
                };
                s.select_edge(clip, TrimSide::In);
                s.hold_drag(
                    Gesture::Edge {
                        drag: None,
                        left: true,
                        span,
                    },
                    Drag { clip, delta },
                );
            });
            window.refresh();
        });
        visual.run_until_parked();
        let brackets: Vec<Background> = painted(visual, lane)
            .into_iter()
            .filter(|quad| {
                near(quad.bounds.size.width, px(3.0))
                    && (quad.background == warning || quad.background == danger)
            })
            .map(|quad| quad.background)
            .collect();
        assert_eq!(brackets, [fill], "a head trim held at {delta}");
    }
}

#[gpui::test]
fn a_press_on_the_fx_badge_opens_its_menu_and_nothing_else(cx: &mut TestAppContext) {
    let (state, visual, clip) = placed(cx, "bar-badge-press", texture());
    let lane = v1_lane(visual);
    let fx = painted(visual, lane)
        .into_iter()
        .find(|quad| {
            quad.rounded
                && near(quad.bounds.size.width, px(FX_W))
                && near(quad.bounds.size.height, px(FX_H))
        })
        .expect("the bar draws its badge")
        .bounds;
    let start = |visual: &mut VisualTestContext| {
        visual.read(|app| {
            state
                .read(app)
                .ui_clip_ref(clip)
                .expect("projected")
                .timeline_start_ticks
        })
    };
    let before = start(visual);
    let track = visual.read(|app| {
        state
            .read(app)
            .store
            .sequence
            .tracks
            .iter()
            .find(|lane| lane.clips.iter().any(|c| c.id == clip.raw()))
            .expect("the clip has a lane")
            .id
    });
    let opened = ContextTarget::ClipBadge {
        clip: clip.raw(),
        track,
    };

    for button in [MouseButton::Left, MouseButton::Right] {
        visual.update(|window, app| {
            state.update(app, |s, _| s.close_context());
            window.refresh();
        });
        visual.run_until_parked();
        let at = fx.center();
        visual.simulate_mouse_move(at, None, Modifiers::none());
        visual.simulate_mouse_down(at, button, Modifiers::none());
        visual.simulate_mouse_move(
            at + point(px(30.0), px(0.0)),
            Some(button),
            Modifiers::none(),
        );
        visual.simulate_mouse_up(at + point(px(30.0), px(0.0)), button, Modifiers::none());
        visual.run_until_parked();
        assert_eq!(
            visual.read(|app| state.read(app).context_menu.clone()),
            Some(opened.clone()),
            "{button:?}"
        );
        assert_eq!(
            start(visual),
            before,
            "{button:?}: the press moved the clip"
        );
    }
}
