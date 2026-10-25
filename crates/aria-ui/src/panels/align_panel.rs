//! The Align dock panel: line the selected clips up on an edge of the frame,
//! the selection or the key clip, and spread them evenly.
//!
//! Every button hands a pick to [`align_pick`].
//! The Align To row is a view setting, so it writes the state directly.

use crate::components::Wraps as _;
use gpui::{
    AnyElement, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    Styled, div, px,
};

use crate::app::AppState;
use crate::app::align::{AlignAxis, AlignEdge, AlignTo, DISTRIBUTE_MIN};
use crate::components::section_label;
use crate::components::{self, ChipSize};
use crate::motion::MotionKey;
use crate::surfaces::align_pick::{self, Pick};
use crate::theme::Theme;

/// The surface name of the panel's controls.
const SURFACE: &str = "align-panel";

/// Window > Align.
pub fn render(
    state: &gpui::Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> AnyElement {
    let theme = Theme::of(cx);
    let (to, selected) = {
        let read = state.read(cx);
        (read.align_to(), read.selection.len())
    };
    let targets: Vec<AnyElement> = AlignTo::ALL
        .iter()
        .map(|&choice| {
            components::chip(
                MotionKey::new("align-to", choice.label()).with(SURFACE),
                choice.label(),
                to == choice,
                ChipSize::Compact,
                &theme,
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    this.state.update(cx, |state, _| state.set_align_to(choice));
                    cx.notify();
                }),
            )
            .into_any_element()
        })
        .collect();
    let mut edges: Vec<AnyElement> = AlignEdge::ALL
        .iter()
        .map(|&edge| {
            align_button(
                &theme,
                MotionKey::new("align-edge", edge.label()).with(SURFACE),
                edge.label(),
                selected > 0,
                Pick::Align(edge),
                cx,
            )
        })
        .collect();
    let down = edges.split_off(3);
    let spread: Vec<AnyElement> = AlignAxis::ALL
        .iter()
        .map(|&axis| {
            align_button(
                &theme,
                MotionKey::new("align-spread", axis.label()).with(SURFACE),
                axis.label(),
                selected >= DISTRIBUTE_MIN,
                Pick::Distribute(axis),
                cx,
            )
        })
        .collect();
    let hint = match selected {
        0 => "Select clips to align. A lone clip aligns to the frame.",
        1 | 2 => "Distribute needs three selected clips.",
        _ => "Distribute keeps the two outermost clips and spaces the centers between them.",
    };
    crate::edge_fade::scroll_y(
        div()
            .id(SURFACE)
            .debug_selector(|| SURFACE.into())
            .size_full()
            .p(px(Theme::SPACE_SM))
            .flex()
            .flex_col()
            .gap(px(Theme::SPACE_XS))
            .child(section_label(&theme, "Align to"))
            .child(row(targets))
            .child(section_label(&theme, "Align"))
            .child(row(edges))
            .child(row(down))
            .child(section_label(&theme, "Distribute"))
            .child(row(spread))
            .child(
                div()
                    .text_size(Theme::TEXT_SMALL)
                    .text_color(theme.text_faint)
                    .child(hint),
            ),
    )
    .into_any_element()
}

/// A wrapping row of controls.
fn row(controls: Vec<AnyElement>) -> AnyElement {
    div()
        .flex()
        .flex_row()
        .wraps()
        .gap(px(Theme::SPACE_XXS))
        .children(controls)
        .into_any_element()
}

/// One button that hands `pick` to the registry when pressed. Drawn faint when
/// the selection is too small for it; a press then says why on the status line.
fn align_button(
    theme: &Theme,
    key: MotionKey,
    label: &'static str,
    ready: bool,
    pick: Pick,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> AnyElement {
    components::ghost_button(key, label, theme)
        .opacity(if ready { 1.0 } else { 0.45 })
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                this.state.update(cx, |state, _| {
                    if let Err(err) = align_pick::apply(state, pick) {
                        state.report_error(&err);
                    }
                });
                cx.notify();
            }),
        )
        .into_any_element()
}
