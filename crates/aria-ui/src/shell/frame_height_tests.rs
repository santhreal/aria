//! Every panel at its frame's minimum height keeps its text inside the frame,
//! or scrolls it there.
//!
//! The class closed here: a panel laid out for a tall frame whose lower rows
//! are drawn past a short frame's bottom edge with nothing that scrolls them
//! back, so the frame clips them out of reach. The panels are read from the
//! registry when the test runs, so a panel added to the app is drawn here
//! without an edit. Each is docked alone into the lower of two frames stacked
//! in a 1920 x 1080 window, at its own minimum height (80 px for most), with a
//! clip selected so the panels that show a selection have one. The wheel is
//! turned down at points across the frame on an 8 px grid, so every list under
//! the pointer is at its end, and then every line of text that starts inside
//! the frame must end inside it. The status bar under the frames is set aside
//! as the strip of [`Theme::STATUS_STRIP_HEIGHT`] at the window's bottom.
//!
//! Not covered: text hidden under a panel's own fixed footer, which stays
//! inside the frame; a line drawn wholly inside the status bar's strip, which
//! is read as the status bar's; a scroller narrower than the 8 px grid; a panel
//! that reads the wheel as something other than a scroll and draws its last
//! line past the bottom anyway; and the glyphs, which the test text system does
//! not raster.

use gpui::{
    Bounds, Modifiers, Pixels, ScrollDelta, ScrollWheelEvent, TestAppContext, TextRunLayout,
    TouchPhase, VisualTestContext, point, px, size,
};

use super::tab_overflow_tests::{frame_bounds, redraw};
use crate::dock::{Axis, DockTree, Frame, FrameId, Node};
use crate::test_support::drawn_shell;
use crate::theme::Theme;

/// Layout is in floats; an edge two boxes share lands within this.
const EPS: f32 = 0.51;
const WINDOW_W: f32 = 1920.0;
const WINDOW_H: f32 = 1080.0;
/// Spacing of the points the wheel is turned at.
const PITCH: f32 = 8.0;
/// One turn of the wheel, in lines, past the end of any panel's list.
const TURN: f32 = -400.0;

/// Whether `run` lies wholly in the status bar's strip under the frames.
fn in_status_strip(run: &TextRunLayout) -> bool {
    run.bounds.top() >= px(WINDOW_H - Theme::STATUS_STRIP_HEIGHT - EPS)
        && run.bounds.bottom() <= px(WINDOW_H + EPS)
}

/// Turn the wheel down at every grid point inside `frame`.
fn scroll_to_end(visual: &mut VisualTestContext, frame: Bounds<Pixels>) {
    let w = f32::from(frame.size.width);
    let xs = [0.25, 0.5, 0.75].map(|at| frame.left() + px(w * at));
    let mut y = frame.top() + px(PITCH / 2.0);
    while y < frame.bottom() {
        for x in xs {
            let position = point(x, y);
            visual.simulate_mouse_move(position, None, Modifiers::none());
            visual.simulate_event(ScrollWheelEvent {
                position,
                delta: ScrollDelta::Lines(point(0.0, TURN)),
                modifiers: Modifiers::none(),
                touch_phase: TouchPhase::Moved,
            });
        }
        visual.run_until_parked();
        y = y + px(PITCH);
    }
    // Off the frame, so no hover draws over it.
    visual.simulate_mouse_move(point(px(2.0), px(2.0)), None, Modifiers::none());
    visual.run_until_parked();
}

#[gpui::test]
fn every_panel_keeps_its_text_inside_a_frame_at_its_minimum_height(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, "frame-height");
    visual.update(|_, app| app.set_reduce_motion(true));
    visual.simulate_resize(size(px(WINDOW_W), px(WINDOW_H)));
    let still = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/texture.png");
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            let media = state.import_media(still).expect("import the still");
            state
                .drop_media_on_timeline(media, None, 0)
                .expect("place the still");
            let clip = state.store.sequence.tracks[0].clips[0].id;
            state.select_clip(aria_models::ClipId::from_raw(clip));
            state.dock_tree = DockTree::editing_for_width(WINDOW_W);
            cx.notify();
        });
    });
    // The boot cover takes the wheel until it is gone.
    crate::test_support::wait_out_splash(visual);
    redraw(&state, visual);

    let panels = visual.read(|app| state.read(app).registered_panel_ids());
    assert!(panels.len() > 1, "the ui plugin registers {panels:?}");
    let version = DockTree::editing().version;
    let mut misses = Vec::new();
    for &panel in &panels {
        let filler = *panels
            .iter()
            .find(|&&other| other != panel)
            .expect("a second panel");
        let min_h = visual.read(|app| state.read(app).dock_panel_min_size(panel).1);
        let stacked = |share: f32| DockTree {
            version,
            root: Node::Split {
                axis: Axis::Column,
                children: vec![
                    (
                        1.0 - share,
                        Node::Frame(Frame::new(FrameId::LEFT, vec![filler])),
                    ),
                    (share, Node::Frame(Frame::new(FrameId::RIGHT, vec![panel]))),
                ],
            },
            floating: Vec::new(),
            focused: FrameId::RIGHT,
            maximized: None,
        };
        // The column's splitter and borders take a few pixels, so the share
        // is corrected by what the frame drew.
        let mut share = min_h / WINDOW_H;
        let mut frame = Bounds::default();
        for _ in 0..3 {
            visual.update(|_, app| {
                state.update(app, |state, cx| {
                    state.dock_tree = stacked(share);
                    cx.notify();
                })
            });
            redraw(&state, visual);
            frame = frame_bounds(&state, visual, FrameId::RIGHT);
            let drawn_h = f32::from(frame.size.height);
            if (drawn_h - min_h).abs() <= EPS {
                break;
            }
            share *= min_h / drawn_h;
        }
        assert!(
            (f32::from(frame.size.height) - min_h).abs() <= EPS,
            "{panel}: the frame drew {:?} high, not its minimum {min_h}",
            frame.size.height
        );
        scroll_to_end(visual, frame);
        redraw(&state, visual);
        let runs: Vec<TextRunLayout> =
            visual.update(|window, _| window.rendered_text_runs().to_vec());
        for run in &runs {
            let x = run.bounds.center().x;
            let inside_across = x >= frame.left() && x <= frame.right();
            let starts_inside = run.bounds.top() >= frame.top() - px(EPS);
            let past_bottom = run.bounds.bottom() > frame.bottom() + px(EPS);
            if inside_across && starts_inside && past_bottom && !in_status_strip(run) {
                misses.push(format!(
                    "{panel} at {min_h} px: `{}` at {:?} ends past the frame's bottom {:?}",
                    run.text,
                    run.bounds,
                    frame.bottom()
                ));
            }
        }
    }
    assert!(misses.is_empty(), "{}", misses.join("\n"));
}
