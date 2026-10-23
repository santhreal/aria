//! The Audio Meters panel's labels stay inside the boxes they label, and its
//! bars fill the frame they are drawn in.
//!
//! WHY: in the 64 px launch frame the clip light, inset to the bars' column,
//! was 22 px wide and its `CLIP` label wider, so the label ran out of the light
//! over the scale's `0`. The class: a meter label drawn wider than its box.
//! And in the Audio workspace's wide frame the bars kept a 22 px cap, so two
//! thin bars stood at the frame's left under a clip light that ran the whole
//! width. The class: a meter drawn for one frame width only.
//!
//! Checked in the launch layout's frame, which is also the panel's minimum
//! width, and in the Audio workspace's: the clip light inside the frame and
//! its label across the light, every scale mark across the scale column, which
//! ends before the first bar, and the bars sharing the width beside the scale
//! out to the panel's padding, with the clip light over the scale and both.
//!
//! Not covered: real Inter advances, for which the test text system sets every
//! character 0.6 em wide; and the 48 px floor that a window too narrow for the
//! launch layout gives the frame.

use gpui::{Bounds, Pixels, TestAppContext, point, px, size};

use crate::dock::PANEL_AUDIO_METERS;
use crate::test_support::{bounds_inside, drawn_shell};
use crate::theme::Theme;

use super::MARKS;

/// Layout is in floats; an edge two boxes share lands within this.
const EPS: f32 = 0.51;

#[gpui::test]
fn the_meters_fill_their_frame_and_keep_every_label_inside_its_box(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    for workspace in [None, Some("win-workspace-audio")] {
        let layout = workspace.unwrap_or("the launch layout");
        let tag = format!("meter-labels-{}", workspace.unwrap_or("launch"));
        let (state, visual) = drawn_shell(cx, &tag);
        visual.update(|_, app| app.set_reduce_motion(true));
        if let Some(id) = workspace {
            visual.update(|_, app| {
                state.update(app, |state, cx| assert!(state.apply_workspace(id, cx)))
            });
        }
        for _ in 0..2 {
            visual.update(|_, app| state.update(app, |_, cx| cx.notify()));
            visual.run_until_parked();
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
            "{layout}: `CLIP` spans {:?}..{:?} centred at {:?}, the light {:?}..{:?}, in a \
             {w} px frame",
            label.left(),
            label.right(),
            label.center(),
            light.left(),
            light.right()
        );
        for (_, text) in MARKS {
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
