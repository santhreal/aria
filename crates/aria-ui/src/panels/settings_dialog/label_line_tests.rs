//! A Settings row keeps its label on the first line of what follows it.
//!
//! The class closed here: a label centered against a value or a set of
//! controls that wraps. The AI section's Status sentence wrapped onto a second
//! line in the shipped dialog, and its label stood between the two lines,
//! level with neither, as did every long value of the AI, Storage, Caches,
//! Plugins, Configuration and Canvas sections, and the label of a plugin
//! choice whose chips wrapped.
//!
//! Draws, in a column narrow enough to wrap them, a one-line and a wrapped
//! [`kv`] row, a [`chip_row`] whose chips wrap, a [`chip_row`] holding a
//! switch, and a [`value_row`] whose value wraps beside a button, and reads
//! the drawn text back: each label's line shares its top with the first line
//! of a value, and its center with the first line of chips, with a switch,
//! and with the first line of a value and the button after it; the one-line
//! value row stays one `ROW_HEIGHT` row with its text on the row's center
//! line. Every read-only Settings row is the one `kv` widget, every labelled
//! control row, the Keymap row included, the one `chip_row`, and every value
//! with actions after it the one `value_row`, so the rows read here stand
//! for each.
//!
//! Not covered: a label too long for the label column, which wraps under its
//! own first line.

use gpui::{
    Context, InteractiveElement, IntoElement, ParentElement, Render, Styled, TestAppContext,
    Window, div, px,
};

use super::value_row::value_row;
use super::widgets::{chip_row, kv};
use crate::components::{self, ChipSize};
use crate::motion::MotionKey;
use crate::theme::Theme;

const EPS: f32 = 0.51;

/// Narrow enough that `WRAPPED` and `CHIPS` take more than one line beside the
/// label column.
const WIDTH: f32 = 320.0;

const ONE_LINE: (&str, &str) = ("Status", "Linked");
const WRAPPED: (&str, &str) = (
    "Can do",
    "Local AI needs a base URL and a supported model before it answers",
);
const CHIPS_LABEL: &str = "Rounding";
const CHIPS: [&str; 6] = [
    "Hundred",
    "Thousand",
    "Million",
    "Billion",
    "Trillion",
    "Quadrillion",
];
const SWITCH_LABEL: &str = "Log dropped frames";
const SWITCH: &str = "label-line-switch";
const READOUT: (&str, &str) = (
    "Whisper",
    "About 388 MB of RAM during inference · 141 MiB on disk · Not installed",
);
const READOUT_ACTION: &str = "Install";

struct Rows;

impl Render for Rows {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // A window root opens the frame its controls' motion reads.
        crate::motion::begin_frame(cx);
        let theme = Theme::of(cx);
        let chips = CHIPS.map(|name| {
            components::chip(
                MotionKey::new("label-line-chip", name),
                name,
                false,
                ChipSize::Regular,
                &theme,
            )
        });
        let switch =
            components::switch(SWITCH, Some(true), &theme).debug_selector(|| SWITCH.to_owned());
        let action = components::ghost_button("label-line-action", READOUT_ACTION, &theme);
        div()
            .w(px(WIDTH))
            .font_family(theme.font_sans.clone())
            .flex()
            .flex_col()
            .child(
                div()
                    .debug_selector(|| "label-line-one-line".to_owned())
                    .child(kv(&theme, ONE_LINE.0, ONE_LINE.1)),
            )
            .child(kv(&theme, WRAPPED.0, WRAPPED.1))
            .child(chip_row(&theme, CHIPS_LABEL, chips))
            .child(chip_row(&theme, SWITCH_LABEL, [switch]))
            .child(value_row(
                &theme,
                READOUT.0,
                READOUT.1,
                [action.into_any_element()],
            ))
    }
}

#[gpui::test]
fn a_label_stands_on_the_first_line_of_its_row(cx: &mut TestAppContext) {
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

    // The value rows: the label tops the value's first line.
    let line = drawn(ONE_LINE.0).size.height;
    let wrapped = drawn(WRAPPED.1);
    assert!(
        wrapped.size.height > line + px(EPS),
        "`{}` drew {:?} high in a {WIDTH} px column, one line {line:?}: the case reads no \
         wrapped value",
        WRAPPED.1,
        wrapped.size.height
    );
    for (label, value) in [ONE_LINE, WRAPPED] {
        let (label_at, value_at) = (drawn(label), drawn(value));
        assert!(
            (label_at.top() - value_at.top()).abs() < px(EPS),
            "`{label}` stands at {:?}, the first line of its value `{value}` at {:?}",
            label_at.top(),
            value_at.top()
        );
    }
    let row = visual
        .debug_bounds("label-line-one-line")
        .expect("the one-line row was drawn");
    let label_at = drawn(ONE_LINE.0);
    assert!(
        (row.size.height - px(Theme::ROW_HEIGHT)).abs() < px(EPS),
        "the one-line row is {:?} high, one row {:?}",
        row.size.height,
        Theme::ROW_HEIGHT
    );
    assert!(
        (label_at.center().y - row.center().y).abs() < px(EPS),
        "`{}` is centered at {:?}, its row at {:?}",
        ONE_LINE.0,
        label_at.center().y,
        row.center().y
    );

    // The chip row: the label shares the center line of the first chips.
    let (first, last) = (drawn(CHIPS[0]), drawn(CHIPS[CHIPS.len() - 1]));
    assert!(
        last.top() > first.bottom(),
        "`{}` drew on the line of `{}` in a {WIDTH} px column: the case reads no wrapped chips",
        CHIPS[CHIPS.len() - 1],
        CHIPS[0]
    );
    let label_at = drawn(CHIPS_LABEL);
    assert!(
        (label_at.center().y - first.center().y).abs() < px(EPS),
        "`{CHIPS_LABEL}` is centered at {:?}, the first line of chips at {:?}",
        label_at.center().y,
        first.center().y
    );

    // The switch row: the label shares the switch's center line.
    let switch = visual.debug_bounds(SWITCH).expect("the switch was drawn");
    let label_at = drawn(SWITCH_LABEL);
    assert!(
        (label_at.center().y - switch.center().y).abs() < px(EPS),
        "`{SWITCH_LABEL}` is centered at {:?}, its switch at {:?}",
        label_at.center().y,
        switch.center().y
    );

    // The value row: the value wraps beside its action, and the label and the
    // action share the center line of the value's first line.
    let value = drawn(READOUT.1);
    let first_line = px(Theme::ROW_HEIGHT - 2.0 * Theme::SPACE_XXS);
    assert!(
        value.size.height > first_line + px(EPS),
        "`{}` drew {:?} high in a {WIDTH} px column: the case reads no wrapped value",
        READOUT.1,
        value.size.height
    );
    let first_center = value.top() + first_line / 2.0;
    let action = drawn(READOUT_ACTION);
    assert!(
        action.left() > value.left(),
        "`{READOUT_ACTION}` starts at {:?}, left of its value at {:?}",
        action.left(),
        value.left()
    );
    for text in [READOUT.0, READOUT_ACTION] {
        let at = drawn(text).center().y;
        assert!(
            (at - first_center).abs() < px(EPS),
            "`{text}` is centered at {at:?}, the first line of `{}` at {first_center:?}",
            READOUT.1
        );
    }
}
