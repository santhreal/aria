//! The Audio Meters panel's labels stay inside the boxes they label.
//!
//! WHY: in the 64 px launch frame the clip light, inset to the bars' column,
//! was 22 px wide and its `CLIP` label wider, so the label ran out of the light
//! over the scale's `0`. The class: a meter label drawn wider than its box.
//! Checked in the launch layout's frame, which is also the panel's minimum
//! width: the clip light inside the frame and its label across the light, and
//! every scale mark across the scale column, which ends before the first bar.
//!
//! Not covered: real Inter advances, for which the test text system sets every
//! character 0.6 em wide; and the 48 px floor that a window too narrow for the
//! launch layout gives the frame.

use gpui::{Bounds, Pixels, TestAppContext, point, px, size};

use crate::dock::FrameId;
use crate::test_support::{bounds_inside, drawn_shell};

use super::MARKS;

#[gpui::test]
fn the_clip_label_and_the_scale_marks_stay_inside_their_boxes(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, "meter-labels");
    visual.run_until_parked();

    let [x, y, w, h] = visual
        .read(|app| {
            state
                .read(app)
                .dock_frame_rects
                .lock()
                .get(&FrameId::METERS)
                .copied()
        })
        .unwrap_or_else(|| panic!("the launch layout draws the Audio Meters frame"));
    let frame = Bounds::new(point(px(x), px(y)), size(px(w), px(h)));
    let light = visual
        .debug_bounds("audio-meters-clip")
        .unwrap_or_else(|| panic!("the clip light is drawn"));
    let scale = visual
        .debug_bounds("audio-meters-scale")
        .unwrap_or_else(|| panic!("the scale is drawn"));
    let bar = visual
        .debug_bounds("audio-meter-bar-0")
        .unwrap_or_else(|| panic!("the first bar is drawn"));
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

    assert!(
        bounds_inside(light, frame),
        "the clip light {light:?} leaves the {w} px frame {frame:?}"
    );
    let label = drawn("CLIP").unwrap_or_else(|| panic!("no `CLIP` drawn in the frame {frame:?}"));
    assert!(
        across(label, light) && light.contains(&label.center()),
        "`CLIP` spans {:?}..{:?} centred at {:?}, the light {:?}..{:?}, in a {w} px frame",
        label.left(),
        label.right(),
        label.center(),
        light.left(),
        light.right()
    );

    assert!(
        scale.right() <= bar.left(),
        "the scale {scale:?} runs into the first bar {bar:?}"
    );
    for (_, text) in MARKS {
        let mark =
            drawn(text).unwrap_or_else(|| panic!("no mark `{text}` drawn in the frame {frame:?}"));
        assert!(
            across(mark, scale),
            "the mark `{text}` spans {:?}..{:?}, the scale {:?}..{:?}",
            mark.left(),
            mark.right(),
            scale.left(),
            scale.right()
        );
    }
}
