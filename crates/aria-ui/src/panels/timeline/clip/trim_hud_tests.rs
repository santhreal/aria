//! The trim readout's pill holds its text with 8 px on each side.
//!
//! The class closed here: a box sized from a guess at its text rather than
//! from the text as shaped, and a readout that prints its empty parts. The pill
//! took 6.8 px per byte of the readout and at least 140 px, so a `·`, two
//! bytes, and every short or long readout drew empty pill past the text; an
//! empty part printed as a separator with nothing after it. Readouts of each
//! shape a trim draws, short and long, one with an empty part, are drawn on
//! the timeline and read back: the text is the parts that hold text joined by
//! ` · `, and the pill's edges stand 8 px outside it.
//!
//! Not covered: a readout wider than the timeline, which the pill clips at the
//! view, and the pill's height.

use std::path::PathBuf;

use gpui::{Background, Bounds, Pixels, TestAppContext, point, px, size};

use crate::theme::Theme;
use crate::types::TrimFeedback;

/// Tool, clip, move and detail, and the readout they print.
const CASES: [([&str; 4], &str); 5] = [
    (
        ["Ripple Trim", "texture", "+4", "Out 00:00:01:04"],
        "Ripple Trim · texture · +4 · Out 00:00:01:04",
    ),
    (
        ["Trim", "x", "-1", "In 00:00:00:23"],
        "Trim · x · -1 · In 00:00:00:23",
    ),
    (
        [
            "Slip",
            "a take with a much longer name than the others",
            "-00:00:00:12",
            "In 00:00:01:12 · Out 00:00:07:12",
        ],
        "Slip · a take with a much longer name than the others · -00:00:00:12 · \
         In 00:00:01:12 · Out 00:00:07:12",
    ),
    (
        ["Rate Stretch", "bridge", "+1", "150.0% · Dur 00:00:02:00"],
        "Rate Stretch · bridge · +1 · 150.0% · Dur 00:00:02:00",
    ),
    (
        ["Opacity", "texture", "", "Esc puts it back"],
        "Opacity · texture · Esc puts it back",
    ),
];

#[gpui::test]
fn the_trim_readout_pill_stands_eight_pixels_outside_its_text(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = crate::test_support::drawn_shell(cx, "trim-hud");
    // An entrance fades the panels in, and a fade scales every fill it paints.
    visual.update(|_, app| app.set_reduce_motion(true));
    let take = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/fixture-640x360-24fps-2s-av.mp4");
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            let media = state.import_media(take).expect("import the take");
            state
                .drop_media_on_timeline(media, None, 0)
                .expect("place the take");
            cx.notify();
        })
    });
    visual.run_until_parked();
    let fill = Background::from(visual.read(|app| Theme::of(app).surface_raised));
    for ([tool, clip, delta, detail], readout) in CASES {
        visual.update(|_, app| {
            state.update(app, |state, cx| {
                state.trim_feedback = Some(TrimFeedback {
                    tool_label: tool.into(),
                    clip_name: clip.into(),
                    delta_ticks: 0,
                    delta_label: delta.into(),
                    detail_label: detail.into(),
                });
                cx.notify();
            })
        });
        visual.update(|window, _| window.refresh());
        visual.run_until_parked();
        let (text, quads) = visual.update(|window, _| {
            let text = window
                .rendered_text_runs()
                .iter()
                .find(|run| run.text.starts_with(tool) && run.text.contains(" · "))
                .map(|run| (run.text.to_string(), run.bounds));
            let scale = window.scale_factor();
            let quads: Vec<(Bounds<Pixels>, Background)> = window
                .painted_quads()
                .iter()
                .map(|quad| {
                    let b = quad.bounds;
                    let logical = Bounds::new(
                        point(px(b.origin.x.0 / scale), px(b.origin.y.0 / scale)),
                        size(px(b.size.width.0 / scale), px(b.size.height.0 / scale)),
                    );
                    (logical, quad.background)
                })
                .collect();
            (text, quads)
        });
        let (drawn, text) = text.unwrap_or_else(|| panic!("the timeline drew no {tool} readout"));
        assert_eq!(drawn, readout, "the {tool} readout");
        let under: Vec<_> = quads
            .into_iter()
            .filter(|(pill, _)| pill.contains(&text.center()))
            .collect();
        let pill = under
            .iter()
            .filter(|(_, background)| *background == fill)
            .map(|(pill, _)| *pill)
            .min_by(|a, b| f32::from(a.size.width).total_cmp(&f32::from(b.size.width)))
            .unwrap_or_else(|| {
                panic!("no pill in {fill:?} under {readout:?}; the quads under it: {under:?}")
            });
        let left = f32::from(text.left() - pill.left());
        let right = f32::from(pill.right() - text.right());
        assert!(
            (left - 8.0).abs() <= 0.5 && (right - 8.0).abs() <= 0.5,
            "{readout:?}: the pill stands {left} px left and {right} px right of its text"
        );
    }
}
