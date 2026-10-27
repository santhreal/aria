//! Worded controls stand one control tall, and a choice at rest reads muted.
//!
//! WHY: a text box was 20 px tall and a solid button 26 px, so a box with its
//! action beside it stood on two heights; a ghost button and a regular chip
//! took the height of their text's line, 1.8 px over the box beside them, so
//! a form label stood off their center line; and a compact chip rested at the
//! color the look spec reserves for a disabled control, so every secondary
//! chip row read as switched off. The height cases draw each worded control in
//! a window and read its box back, with one chip per [`ChipSize::ALL`] at run
//! time, so a new size without a height of its own fails. The color cases
//! enumerate the same sizes and check the tab of a tab row beside them.
//!
//! Not covered: a caller that overrides the height or the color after the
//! call.

use gpui::{
    Context, InteractiveElement, IntoElement, ParentElement, Render, Styled, TestAppContext,
    Window, div, px,
};

use super::{ChipSize, button, chip, ghost_button, text_box, underline_tab};
use crate::theme::{Appearance, Theme};

const EPS: f32 = 0.51;

/// The worded controls one control row tall, each under the selector it is
/// drawn as.
const CONTROLS: [&str; 4] = ["empty-box", "filled-box", "button", "ghost"];

struct Controls;

impl Render for Controls {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // A window root opens the frame its controls' motion reads.
        crate::motion::begin_frame(cx);
        let theme = Theme::of(cx);
        div()
            .w(px(240.0))
            .font_family(theme.font_sans.clone())
            .flex()
            .flex_col()
            .items_start()
            .child(
                text_box("empty-box", "", "Search", false, &theme)
                    .w_full()
                    .debug_selector(|| CONTROLS[0].into()),
            )
            .child(
                text_box("filled-box", "a query", "Search", true, &theme)
                    .w_full()
                    .debug_selector(|| CONTROLS[1].into()),
            )
            .child(button("button", "Search", &theme).debug_selector(|| CONTROLS[2].into()))
            .child(ghost_button("ghost", "Search", &theme).debug_selector(|| CONTROLS[3].into()))
            .children(ChipSize::ALL.into_iter().enumerate().map(|(index, size)| {
                chip(("chip", index), "Choice", false, size, &theme)
                    .debug_selector(move || format!("chip-{index}"))
            }))
    }
}

fn install(cx: &mut TestAppContext, appearance: Appearance) {
    cx.update(|app| {
        crate::register_fonts(app);
        Theme::install(appearance, Default::default(), None, app);
    });
}

#[gpui::test]
fn every_worded_control_is_one_control_tall(cx: &mut TestAppContext) {
    install(cx, Appearance::Dark);
    let (_, visual) = crate::test_support::window_view(cx, |_, _| Controls);
    visual.run_until_parked();
    for selector in CONTROLS {
        let at = visual
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("no {selector} was drawn"));
        assert!(
            (at.size.height - px(Theme::CONTROL_HEIGHT)).abs() < px(EPS),
            "the {selector} is {:?} tall, one control is {} px",
            at.size.height,
            Theme::CONTROL_HEIGHT
        );
    }
    // A regular chip stands in a row of boxes and buttons; a compact one in
    // a row of icon controls.
    for (index, size) in ChipSize::ALL.into_iter().enumerate() {
        let at = visual
            .debug_bounds(crate::interned::intern(&format!("chip-{index}")))
            .unwrap_or_else(|| panic!("no {size:?} chip was drawn"));
        let row = match size {
            ChipSize::Regular => Theme::CONTROL_HEIGHT,
            ChipSize::Compact => Theme::ICON_HIT_ROW,
        };
        assert!(
            (at.size.height - px(row)).abs() < px(EPS),
            "a {size:?} chip is {:?} tall, its row is {row} px",
            at.size.height
        );
    }
}

#[gpui::test]
fn an_inactive_chip_or_tab_rests_at_muted_text_in_every_size(cx: &mut TestAppContext) {
    for appearance in [Appearance::Dark, Appearance::Light] {
        install(cx, appearance);
        cx.update(|app| {
            crate::motion::begin_frame(app);
            let theme = Theme::of(app);
            assert_ne!(
                theme.text_muted, theme.text_faint,
                "the theme draws muted and disabled text alike"
            );
            for (index, size) in ChipSize::ALL.into_iter().enumerate() {
                let mut rest = chip(("rest-chip", index), "Choice", false, size, &theme);
                assert_eq!(
                    rest.style().text.color,
                    Some(theme.text_muted),
                    "an inactive {size:?} chip rests at another color in {appearance:?}"
                );
            }
            let key = crate::motion::MotionKey::new("rest-tab", 0usize);
            let mut tab = underline_tab(key, "Tab", false, &theme);
            assert_eq!(
                tab.style().text.color,
                Some(theme.text_muted),
                "a tab that is not open rests at another color in {appearance:?}"
            );
        });
    }
}
