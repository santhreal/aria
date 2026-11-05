//! A snap guide's label draws on one line beside its guide.
//!
//! The class closed here: a label that takes the width of the element it stands
//! in. The label stands inside the guide, a line one pixel wide; its entrance
//! put it back in flow and nothing held its text on one line, so a drag that
//! snapped drew the magnet's name one letter per line down the ruler. Every
//! kind of magnet's label is drawn in a box as high as the ruler and read back:
//! one line high, right of its guide, and at its inset below the ruler's top.
//!
//! Not covered: the guide across the lane groups, which draws no label, and the
//! glyphs, which the test text system does not raster.

use std::time::Duration;

use gpui::{
    Context, InteractiveElement, IntoElement, ParentElement, Render, Styled, TestAppContext,
    Window, div, px,
};

use super::{RULER_H, SNAP_LABEL_INSET, snap_guide_line, ticks_to_px};
use crate::snap::{SnapCandidate, SnapKind};
use crate::theme::{Appearance, Theme};

const EPS: f32 = 0.51;
/// The frame rate and scale the guides are drawn at.
const FPS: f32 = 24.0;
const PPS: f32 = 100.0;
/// The frame every guide stands on.
const TICK: i64 = 24;
/// A line drawn at the label's size in a box wider than it, to read the height
/// of one line from.
const ONE_LINE: &str = "One line";

/// One ruler-high box per kind of magnet, each with its guide.
struct Guides;

impl Render for Guides {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // A window root opens the frame the label's entrance reads.
        crate::motion::begin_frame(cx);
        let theme = Theme::of(cx);
        div()
            .font_family(theme.font_sans.clone())
            .flex()
            .flex_col()
            .child(
                div()
                    .w(px(400.0))
                    .text_size(Theme::TEXT_SMALL)
                    .child(ONE_LINE),
            )
            .children(SnapKind::ALL.into_iter().enumerate().map(|(index, kind)| {
                div()
                    .debug_selector(move || format!("snap-ruler-{index}"))
                    .relative()
                    .w(px(400.0))
                    .h(px(RULER_H))
                    .child(snap_guide_line(
                        &theme,
                        SnapCandidate::new(TICK, kind),
                        PPS,
                        FPS,
                        0.0,
                        1.0,
                    ))
            }))
    }
}

#[gpui::test]
fn every_snap_label_draws_on_one_line_beside_its_guide(cx: &mut TestAppContext) {
    cx.update(|app| {
        crate::register_fonts(app);
        Theme::install(Appearance::Dark, Default::default(), None, app);
    });
    let (_, visual) = crate::test_support::window_view(cx, |_, _| Guides);
    // The label's entrance at rest.
    visual.executor().advance_clock(Duration::from_secs(2));
    visual.update(|window, _| window.refresh());
    visual.run_until_parked();
    let runs = visual.update(|window, _| window.rendered_text_runs().to_vec());
    let drawn = |text: &str| {
        runs.iter()
            .find(|run| run.text.as_ref() == text)
            .map(|run| run.bounds)
            .unwrap_or_else(|| panic!("`{text}` was not drawn"))
    };
    let one_line = drawn(ONE_LINE).size.height;
    let guide_x = px(ticks_to_px(TICK, FPS, PPS));
    for (index, kind) in SnapKind::ALL.into_iter().enumerate() {
        let text = kind.label();
        let ruler = visual
            .debug_bounds(crate::interned::intern(&format!("snap-ruler-{index}")))
            .unwrap_or_else(|| panic!("the {kind:?} ruler was not drawn"));
        let label = drawn(text);
        assert!(
            label.size.height <= one_line + px(EPS),
            "`{text}` drew {:?} high, one line is {one_line:?}",
            label.size.height
        );
        assert!(
            label.left() > ruler.left() + guide_x,
            "`{text}` starts at {:?}, left of its guide at {:?}",
            label.left(),
            ruler.left() + guide_x
        );
        assert!(
            (label.top() - ruler.top() - px(SNAP_LABEL_INSET)).abs() < px(EPS),
            "`{text}` stands {:?} below the ruler's top, its inset is {SNAP_LABEL_INSET} px",
            label.top() - ruler.top()
        );
    }
}
