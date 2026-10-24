//! A labelled value keeps its label and its value inside the row at any width.
//!
//! The class closed here: a row that sets a label and a value apart and draws
//! either past the panel's edge once the two do not fit side by side. The Info
//! panel at its minimum width drew the reading of `Scales with size` past its
//! frame.
//!
//! Each case draws one [`readout`] in a column as wide as the Info panel's
//! content at its minimum width, and reads the drawn text back: a pair that
//! fits shares one line with the value at the right edge, a long label puts the
//! value on the next line at the right edge, a label wider than the row wraps
//! inside it with the value below, and a value wider than the row wraps inside
//! it below the label.
//!
//! Not covered: the colour of a tinted value, which the test renderer does not
//! read, and the right edge of a run that wraps: a drawn run records the width
//! of its text laid out on one line, so only its left edge and its line count
//! are read.

use gpui::{
    Context, InteractiveElement, IntoElement, ParentElement, Render, Styled, TestAppContext,
    Window, div, px,
};

use super::readout;
use crate::theme::Theme;

const EPS: f32 = 0.51;

/// How far inside its box a run may end: a text element rounds its width up
/// to a whole pixel, so a right-aligned value ends up to a pixel short of the
/// row's edge.
const EDGE: f32 = 1.0;

/// The Info panel's content width at its minimum width: the 160 px frame less
/// the panel's padding on both sides.
const WIDTH: f32 = 160.0 - 2.0 * Theme::SPACE_SM;

/// One row and where its value lands.
struct Case {
    label: &'static str,
    value: &'static str,
    /// The value shares the label's line.
    beside: bool,
    label_wraps: bool,
    value_wraps: bool,
}

const CASES: [Case; 4] = [
    // Fits side by side.
    Case {
        label: "Decode",
        value: "0.01 ms",
        beside: true,
        label_wraps: false,
        value_wraps: false,
    },
    // Leaves no room beside the label for the value.
    Case {
        label: "Scales with size",
        value: "2469.12 ms",
        beside: false,
        label_wraps: false,
        value_wraps: false,
    },
    // A label wider than the row.
    Case {
        label: "Effect pass bind groups per frame",
        value: "1234",
        beside: false,
        label_wraps: true,
        value_wraps: false,
    },
    // A value wider than the row.
    Case {
        label: "Late",
        value: "1234 of 1234 frames, worst 375.0 ms",
        beside: false,
        label_wraps: false,
        value_wraps: true,
    },
];

struct Rows;

impl Render for Rows {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        div()
            .w(px(WIDTH))
            .font_family(theme.font_sans.clone())
            .flex()
            .flex_col()
            .gap(px(Theme::SPACE_XS))
            .children(CASES.iter().enumerate().map(|(index, case)| {
                div()
                    .debug_selector(move || format!("readout-{index}"))
                    .child(readout(&theme, case.label, case.value))
            }))
    }
}

#[gpui::test]
fn a_value_stays_inside_its_row_beside_or_below_its_label(cx: &mut TestAppContext) {
    cx.update(|app| {
        crate::register_fonts(app);
        Theme::install(
            crate::theme::Appearance::Dark,
            Default::default(),
            None,
            app,
        );
    });
    let (_, visual) = crate::test_support::window_view(cx, |_, _| Rows);
    visual.run_until_parked();
    let runs = visual.update(|window, _| window.rendered_text_runs().to_vec());
    let drawn = |text: &str| {
        runs.iter()
            .find(|run| run.text.as_ref() == text)
            .map(|run| run.bounds)
            .unwrap_or_else(|| panic!("`{text}` was not drawn"))
    };
    let one_line = drawn(CASES[0].label).size.height;
    for (index, case) in CASES.iter().enumerate() {
        let Case { label, value, .. } = *case;
        let row = visual
            .debug_bounds(crate::interned::intern(&format!("readout-{index}")))
            .unwrap_or_else(|| panic!("the `{label}` row was not drawn"));
        let (label_at, value_at) = (drawn(label), drawn(value));
        for (text, at, wraps) in [
            (label, label_at, case.label_wraps),
            (value, value_at, case.value_wraps),
        ] {
            assert_eq!(
                at.size.height > one_line + px(EPS),
                wraps,
                "`{text}` drew {:?} high, one line {one_line:?}; it should wrap: {wraps}",
                at.size.height
            );
            assert!(
                at.left() >= row.left() - px(EPS) && (wraps || at.right() <= row.right() + px(EPS)),
                "`{text}` spans {:?}..{:?}, its row {:?}..{:?}",
                at.left(),
                at.right(),
                row.left(),
                row.right()
            );
        }
        assert!(
            (label_at.left() - row.left()).abs() < px(EPS),
            "`{label}` starts at {:?}, its row at {:?}",
            label_at.left(),
            row.left()
        );
        if case.beside {
            assert!(
                (value_at.top() - label_at.top()).abs() < px(EPS),
                "`{value}` left the line of `{label}` though both fit on it"
            );
        } else {
            assert!(
                value_at.top() >= label_at.bottom() - px(EPS),
                "`{value}` stayed beside `{label}` with no room for it"
            );
        }
        if !case.value_wraps {
            assert!(
                row.right() - value_at.right() < px(EDGE),
                "`{value}` ends at {:?}, short of the row's right edge {:?}",
                value_at.right(),
                row.right()
            );
        }
    }
}
