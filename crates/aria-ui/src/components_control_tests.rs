//! Worded controls stand one control tall, a choice at rest reads muted, and
//! a text box's placeholder reads as a hint.
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
//!
//! The placeholder case: an empty box drew its placeholder in `text_muted`,
//! the color of a row label, so "Search" in a field read as a typed value.
//! It checks the box's text color in both looks, empty with and without
//! focus and filled. Not covered: a caller that sets the color after.
//!
//! The ghost edge case: a ghost button's border rested at `border`, the
//! splitter token, which draws darker than every surface in the dark look, so
//! a Cancel or an "Add at playhead" read as a groove cut in the panel rather
//! than a button on it. It reads the border of a drawn ghost at rest and checks
//! it, laid over every surface a ghost sits on, steps away from that surface
//! the way a control outline does, and that hover steps it further. Not
//! covered: a caller that sets the border color after.
//!
//! The blocked case: disabled controls drew three ways (menu rows at 40 %
//! opacity, Align buttons at 45 %, the graph tools in faint text over the
//! splitter border) where the look spec states one. It passes a button, a
//! ghost and a chip through `blocked::blocked` and checks each draws at the
//! spec's opacity with the arrow cursor, and that an unblocked one keeps its
//! own. Not covered: a caller that draws a disabled control without it.

use gpui::{
    Context, InteractiveElement, IntoElement, ParentElement, Render, Styled, TestAppContext,
    Window, div, px,
};

use super::{ChipSize, button, chip, ghost_button, text_box, underline_tab};
use crate::theme::{Appearance, Theme, contrast_ratio, flatten, relative_luminance};

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
                text_box("empty-box", "", "Search", None, &theme)
                    .w_full()
                    .debug_selector(|| CONTROLS[0].into()),
            )
            .child(
                text_box("filled-box", "a query", "Search", Some((7, 7)), &theme)
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

#[gpui::test]
fn a_placeholder_reads_as_a_hint_and_a_value_as_text(cx: &mut TestAppContext) {
    for appearance in [Appearance::Dark, Appearance::Light] {
        install(cx, appearance);
        cx.update(|app| {
            crate::motion::begin_frame(app);
            let theme = Theme::of(app);
            assert_ne!(
                theme.text_faint, theme.text_muted,
                "the theme draws hints and labels alike in {appearance:?}"
            );
            for (value, editing, color, case) in [
                ("", None, theme.text_faint, "an empty box"),
                ("", Some((0, 0)), theme.text_faint, "an empty focused box"),
                ("a query", None, theme.text, "a filled box"),
                ("a query", Some((7, 7)), theme.text, "a filled focused box"),
            ] {
                let mut field = text_box("box", value, "Search", editing, &theme);
                assert_eq!(
                    field.style().text.color,
                    Some(color),
                    "{case} draws its text at another color in {appearance:?}"
                );
            }
        });
    }
}

#[gpui::test]
fn a_ghost_edge_rests_as_a_control_outline_on_every_surface(cx: &mut TestAppContext) {
    for appearance in [Appearance::Dark, Appearance::Light] {
        install(cx, appearance);
        cx.update(|app| {
            crate::motion::begin_frame(app);
            let theme = Theme::of(app);
            let mut ghost = ghost_button("rest-ghost", "Cancel", &theme);
            let rest = ghost
                .style()
                .border_color
                .expect("a ghost button draws a border");
            let lit = super::ghost_edge(1.0, &theme);
            for (name, ground) in [
                ("bg", theme.bg),
                ("surface", theme.surface),
                ("surface_header", theme.surface_header),
                ("surface_card", theme.surface_card),
                ("surface_dialog", theme.surface_dialog),
                ("surface_overlay", theme.surface_overlay),
                ("surface_raised", theme.surface_raised),
            ] {
                let outline = flatten(theme.border_strong, ground);
                let edge = flatten(rest, ground);
                let lifts = |c| relative_luminance(c) > relative_luminance(ground);
                assert_eq!(
                    lifts(edge),
                    lifts(outline),
                    "{appearance:?}: a resting ghost edge on {name} steps the other way from a control outline"
                );
                let at_rest = contrast_ratio(edge, ground);
                assert!(
                    at_rest >= 1.15,
                    "{appearance:?}: a resting ghost edge on {name} stands {at_rest:.2}:1 off it"
                );
                let under_pointer = contrast_ratio(flatten(lit, ground), ground);
                assert!(
                    under_pointer > at_rest,
                    "{appearance:?}: a ghost edge on {name} does not brighten under the pointer"
                );
            }
        });
    }
}

#[gpui::test]
fn a_blocked_control_draws_at_the_spec_opacity_with_the_arrow(cx: &mut TestAppContext) {
    use super::blocked::{BLOCKED_OPACITY, blocked};
    use gpui::CursorStyle;

    install(cx, Appearance::Dark);
    cx.update(|app| {
        crate::motion::begin_frame(app);
        let theme = Theme::of(app);
        assert_eq!(
            BLOCKED_OPACITY, 0.4,
            "docs/spec/look.md draws a disabled control at 40 %"
        );
        let controls = || {
            [
                ("button", button("b-button", "Send", &theme)),
                ("ghost", ghost_button("b-ghost", "Hold", &theme)),
                (
                    "chip",
                    chip("b-chip", "Value", false, ChipSize::Compact, &theme),
                ),
            ]
        };
        for (name, control) in controls() {
            let mut drawn = blocked(control, true);
            let style = drawn.style();
            assert_eq!(style.opacity, Some(BLOCKED_OPACITY), "a blocked {name}");
            assert_eq!(
                style.mouse_cursor,
                Some(CursorStyle::Arrow),
                "a blocked {name}"
            );
        }
        for (name, control) in controls() {
            let mut drawn = blocked(control, false);
            let style = drawn.style();
            assert_eq!(style.opacity, None, "a {name} that can act");
            assert_eq!(
                style.mouse_cursor,
                Some(CursorStyle::PointingHand),
                "a {name} that can act"
            );
        }
    });
}
