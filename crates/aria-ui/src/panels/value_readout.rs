//! The scrubbable readout: a number a press drags or opens as text.
//!
//! Drawn in the `value` colour, with a dotted underline while the pointer is
//! over it. A press hands the readout's [`ScrubTarget`] to the state, and the
//! shell routes the pointer from there: the drag is the state's, so it keeps
//! going when the pointer leaves the readout. Once open, the readout draws the
//! draft with a caret, selected while the next key would replace it.

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, CursorStyle, Entity, Hsla, InteractiveElement, IntoElement, MouseButton,
    MouseDownEvent, ParentElement, SharedString, StatefulInteractiveElement, Styled, div, px,
};

use crate::app::AppState;
use crate::app::value_scrub::ScrubTarget;
use crate::motion;
use crate::theme::Theme;

/// How a readout prints when its field is closed.
pub struct Look {
    pub size: gpui::Pixels,
    /// The colour the number is printed in.
    pub ink: Hsla,
}

/// One readout. `text` is what it prints closed.
pub fn readout(
    theme: &Theme,
    state: &Entity<AppState>,
    target: ScrubTarget,
    text: impl Into<SharedString>,
    look: Look,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> AnyElement {
    let key = target.key;
    let (draft, dragging, any_open) = {
        let scrubs = state.read(cx).value_scrubs();
        (
            scrubs
                .entry_for(key)
                .map(|entry| (entry.draft.clone(), entry.all)),
            scrubs.dragging(key),
            scrubs.entry_open(),
        )
    };
    // While a field is open each readout reports where it painted, which is
    // what Tab reads the next readout from. With none open it costs nothing.
    let painted = any_open.then(|| {
        let state = state.clone();
        let target = target.clone();
        gpui::canvas(
            move |_, _, cx| state.update(cx, |state, _| state.value_painted(&target)),
            |_, _, _, _| {},
        )
        .absolute()
        .size(px(0.))
    });

    if let Some((draft, all)) = draft {
        let body = div()
            .when(all, |el| el.bg(theme.accent.opacity(0.35)).rounded(px(2.)))
            .child(draft);
        return div()
            .id(key.element_id("value-field"))
            .relative()
            .flex()
            .flex_row()
            .items_center()
            .px(px(Theme::SPACE_XXS))
            .rounded(px(Theme::CONTROL_RADIUS))
            .bg(theme.input_bg)
            .border_1()
            .border_color(theme.accent)
            .text_size(look.size)
            .font_features(crate::theme::tabular_figures())
            .text_color(theme.text)
            .cursor(CursorStyle::IBeam)
            .child(body)
            .child("\u{258f}")
            .children(painted)
            // A press on the open field is not a new gesture on the row.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .into_any_element();
    }

    let hover = key.with("value");
    let lit = motion::hover_t(hover);
    let ink = if dragging { theme.accent } else { look.ink };
    div()
        .id(key.element_id("value"))
        .relative()
        .text_size(look.size)
        .font_features(crate::theme::tabular_figures())
        .text_color(ink)
        .cursor(CursorStyle::ResizeLeftRight)
        .border_b_1()
        .border_dashed()
        .border_color(ink.opacity(if dragging { 1.0 } else { lit }))
        .on_hover(motion::hover_listener(hover))
        .child(text.into())
        .children(painted)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                let target = target.clone();
                this.state.update(cx, |state, _| state.press_value(target));
                cx.notify();
            }),
        )
        .into_any_element()
}
