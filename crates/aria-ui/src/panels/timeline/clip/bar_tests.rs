//! Pressing the parts of a clip bar where the canvas painted them, and what
//! the canvas paints for the state a bar draws.
//!
//! Closes the class "a press lands somewhere other than what was drawn under
//! it": the line hit-tested at a height its paint did not use, a keyframe dot a
//! press passes through to the line, a Ctrl+click on the line that keys
//! nothing. Also pins the fills the spec names: the fx badge grey, yellow, or
//! purple with the purple underline when both, and an offline bar red. Not
//! covered: the pointer's cursor shape, which the test platform does not report
//! back, and glyphs, which the test text system does not draw.

use std::path::PathBuf;

use aria_commands::{Command, EditCommand};
use aria_models::{ClipId, ClipParam, MarkerColor, Param, ParamValue};
use gpui::{
    Background, Bounds, Entity, Modifiers, MouseButton, Pixels, Point, TestAppContext,
    VisualTestContext, point, px, size,
};

use super::{BarRows, DOT, FX_H, FX_W};
use crate::app::AppState;
use crate::theme::Theme;
use crate::types::ContextTarget;

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
                within.contains(&bounds.center()).then(|| Painted {
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
    // The bar's line is a hairline narrower than the lane; the lane's own
    // rules span all of it.
    let line_of = |visual: &mut VisualTestContext| {
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
    };
    let line = line_of(visual);
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
    let line = line_of(visual);
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
}

#[gpui::test]
fn the_fx_badge_is_grey_yellow_purple_or_yellow_over_purple(cx: &mut TestAppContext) {
    let (state, visual, clip) = placed(cx, "bar-badge", texture());
    let lane = v1_lane(visual);
    let theme = visual.read(|app| Theme::of(app));
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
