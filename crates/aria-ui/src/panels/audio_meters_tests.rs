//! The Audio Meters panel's labels stay inside the boxes they label, and its
//! bars fill the frame they are drawn in.
//!
//! WHY: in a 64 px frame, the panel's minimum width, the clip light, inset to the bars' column,
//! was 22 px wide and its `CLIP` label wider, so the label ran out of the light
//! over the scale's `0`. The class: a meter label drawn wider than its box.
//! And in a wide frame the bars kept a 22 px cap, so two thin bars stood at
//! the frame's left under a clip light that ran the whole width. The class: a
//! meter drawn for one frame width only.
//!
//! A third class: the marks at 0 and -60 dBFS are centered on the bars' ends,
//! and the half of each past them stood 1 px into the clip light above and
//! the channel names below.
//!
//! A fourth: the Audio workspace put the meters at the front of the upper
//! row's right frame, 28 % of the window, so its two bars drew as two 200 px
//! slabs away from the Timeline they meter, and the Animation, Color and
//! Effects workspaces drew no meters. Every shipped workspace stands them
//! right of the Timeline, in its row.
//!
//! Checked in the launch layout's frame, in a frame dragged to the panel's
//! minimum width, in the frame of every shipped workspace, read from the
//! workspace rows so a new workspace is checked with no case added here, and in the
//! Viewer's frame, where the meters are docked by hand: the clip light
//! inside the frame and its label across the light, every scale mark across
//! the scale column, which ends before the first bar, between the clip light
//! and the channel names, and centered on the level it names, and the bars
//! sharing the width beside the scale out to the panel's padding, with the
//! clip light over the scale and both.
//!
//! Not covered: real Inter advances, for which the test text system sets every
//! character 0.6 em wide; and the 48 px floor that a window too narrow for the
//! launch layout gives the frame.

use gpui::{Bounds, Pixels, TestAppContext, point, px, size};

use crate::app::meter::dbfs_fraction;
use crate::dock::{DockTree, FrameId, PANEL_AUDIO_METERS, PANEL_TIMELINE};
use crate::test_support::{bounds_inside, drawn_shell};
use crate::theme::Theme;

use super::MARKS;

/// Layout is in floats; an edge two boxes share lands within this.
const EPS: f32 = 0.51;

#[gpui::test]
fn the_meters_fill_their_frame_and_keep_every_label_inside_its_box(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let mut cases: Vec<(&str, Option<&str>, Option<FrameId>, bool)> = vec![
        ("launch", None, None, false),
        ("narrowest", None, None, true),
    ];
    cases.extend(
        crate::layouts::WORKSPACES
            .iter()
            .map(|ws| (ws.id, Some(ws.id), None, false)),
    );
    cases.push(("docked-wide", None, Some(FrameId::CENTER), false));
    for (layout, workspace, docked, narrowest) in cases {
        let tag = format!("meter-labels-{layout}");
        let (state, visual) = drawn_shell(cx, &tag);
        visual.update(|_, app| app.set_reduce_motion(true));
        if let Some(id) = workspace {
            visual.update(|_, app| {
                state.update(app, |state, cx| assert!(state.apply_workspace(id, cx)))
            });
        }
        if let Some(frame) = docked {
            visual.update(|_, app| {
                state.update(app, |state, _| {
                    assert!(state.dock_tree.add_tab(frame, PANEL_AUDIO_METERS));
                })
            });
        }
        for _ in 0..2 {
            visual.update(|_, app| state.update(app, |_, cx| cx.notify()));
            visual.run_until_parked();
        }
        if narrowest {
            // The lower row re-weighted so the frame stands at the panel's
            // minimum width, the narrowest a dragged splitter leaves it.
            let min_w =
                visual.read(|app| state.read(app).dock_panel_min_size(PANEL_AUDIO_METERS).0);
            let row: f32 = visual.read(|app| {
                let state = state.read(app);
                let rects = state.dock_frame_rects.lock();
                [FrameId::BOTTOM, FrameId::METERS]
                    .iter()
                    .map(|frame| rects.get(frame).map_or(0.0, |rect| rect[2]))
                    .sum()
            });
            visual.update(|_, app| {
                state.update(app, |state, cx| {
                    assert!(
                        state
                            .dock_tree
                            .set_weights(&[1], &[(row - min_w) / row, min_w / row]),
                        "{layout}: the lower row takes no weights"
                    );
                    cx.notify();
                })
            });
            for _ in 0..2 {
                visual.update(|_, app| state.update(app, |_, cx| cx.notify()));
                visual.run_until_parked();
            }
        }
        visual.update(|window, _| window.refresh());
        visual.run_until_parked();

        let [x, y, w, h] = visual
            .read(|app| {
                let state = state.read(app);
                let holder = state.dock_tree.find(PANEL_AUDIO_METERS)?;
                state.dock_frame_rects.lock().get(&holder).copied()
            })
            .unwrap_or_else(|| panic!("{layout} draws the Audio Meters frame"));
        let frame = Bounds::new(point(px(x), px(y)), size(px(w), px(h)));
        let timeline = visual.read(|app| {
            let state = state.read(app);
            let holder = state.dock_tree.find(PANEL_TIMELINE)?;
            state.dock_frame_rects.lock().get(&holder).copied()
        });
        if narrowest {
            let min_w =
                visual.read(|app| state.read(app).dock_panel_min_size(PANEL_AUDIO_METERS).0);
            assert!(
                w <= min_w + EPS + Theme::SPACE_SM,
                "{layout}: the frame is {w} px, not the panel's {min_w} px minimum"
            );
        } else if docked.is_some() {
            assert!(
                w > 4.0 * DockTree::EDITING_METERS_PX,
                "{layout}: the hand-docked frame is {w} px, no wider than the launch frame"
            );
        } else {
            // Beside the Timeline: its row, right of it.
            let [tx, ty, tw, th] =
                timeline.unwrap_or_else(|| panic!("{layout} draws the Timeline frame"));
            assert!(
                (y - ty).abs() <= EPS && (h - th).abs() <= EPS && x >= tx + tw - EPS,
                "{layout}: the Audio Meters frame {:?} does not stand right of the Timeline \
                 frame {:?} in its row",
                [x, y, w, h],
                [tx, ty, tw, th]
            );
        }
        let [body, light, scale, left, right] = [
            "audio-meters",
            "audio-meters-clip",
            "audio-meters-scale",
            "audio-meter-bar-0",
            "audio-meter-bar-1",
        ]
        .map(|selector| {
            visual
                .debug_bounds(selector)
                .unwrap_or_else(|| panic!("{layout}: `{selector}` is not drawn"))
        });
        let runs = visual.update(|window, _| window.rendered_text_runs().to_vec());
        // The run of `text` drawn in the frame. The marks at 0 and -60 dBFS are
        // centred on the scale's top and bottom edges, so the frame finds them.
        let drawn = |text: &str| {
            runs.iter()
                .find(|run| run.text.as_ref() == text && frame.contains(&run.bounds.center()))
                .map(|run| run.bounds)
        };
        // Across the box only: a line's box is its line height, taller than the
        // 14 px light, while its glyphs sit centred on it.
        let across = |inner: Bounds<Pixels>, outer: Bounds<Pixels>| {
            inner.left() >= outer.left() - px(0.5) && inner.right() <= outer.right() + px(0.5)
        };
        let at = |a: Pixels, b: Pixels| (f32::from(a) - f32::from(b)).abs() <= EPS;

        assert!(
            bounds_inside(light, frame),
            "{layout}: the clip light {light:?} leaves the {w} px frame {frame:?}"
        );
        let label = drawn("CLIP")
            .unwrap_or_else(|| panic!("{layout}: no `CLIP` drawn in the frame {frame:?}"));
        assert!(
            across(label, light) && light.contains(&label.center()),
            "{layout}: `CLIP` spans {:?}..{:?} centered at {:?}, the light {:?}..{:?}, in a \
             {w} px frame",
            label.left(),
            label.right(),
            label.center(),
            light.left(),
            light.right()
        );
        let names = drawn("L")
            .unwrap_or_else(|| panic!("{layout}: no channel name `L` drawn in the frame"));
        for (dbfs, text) in MARKS {
            let mark = drawn(text).unwrap_or_else(|| {
                panic!("{layout}: no mark `{text}` drawn in the frame {frame:?}")
            });
            assert!(
                across(mark, scale),
                "{layout}: the mark `{text}` spans {:?}..{:?}, the scale {:?}..{:?}",
                mark.left(),
                mark.right(),
                scale.left(),
                scale.right()
            );
            assert!(
                mark.top() >= light.bottom() - px(EPS) && mark.bottom() <= names.top() + px(EPS),
                "{layout}: the mark `{text}` runs {:?}..{:?} down, into the clip light ending \
                 at {:?} or the channel names starting at {:?}",
                mark.top(),
                mark.bottom(),
                light.bottom(),
                names.top()
            );
            // Each mark stands beside the level it names on the bars.
            let level = left.bottom() - left.size.height * dbfs_fraction(dbfs);
            assert!(
                at(mark.center().y, level),
                "{layout}: the mark `{text}` is centered at {:?}, its level on the bars at \
                 {level:?}",
                mark.center().y
            );
        }

        let gap = px(Theme::SPACE_XS);
        assert!(
            at(left.left(), scale.right() + gap) && at(right.left(), left.right() + gap),
            "{layout}: the scale ends at {:?} and the bars run {:?}..{:?} and {:?}..{:?}, \
             not one {gap:?} gap apart",
            scale.right(),
            left.left(),
            left.right(),
            right.left(),
            right.right()
        );
        assert!(
            at(right.right(), body.right() - gap)
                && at(left.size.width, right.size.width)
                && at(light.left(), scale.left())
                && at(light.right(), right.right()),
            "{layout}: in a {w} px frame the bars are {:?} and {:?} wide and end at {:?}, the \
             panel's padding at {:?}; the clip light runs {:?}..{:?} over {:?}..{:?}",
            left.size.width,
            right.size.width,
            right.right(),
            body.right() - gap,
            light.left(),
            light.right(),
            scale.left(),
            right.right()
        );
    }
}
