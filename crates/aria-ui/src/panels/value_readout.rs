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
#[derive(Debug)]
pub struct Look {
    pub size: gpui::Pixels,
    /// The colour the number is printed in.
    pub ink: Hsla,
}

#[cfg(test)]
thread_local! {
    /// The readouts drawn since [`record_readouts`].
    static READOUTS: std::cell::RefCell<Option<Vec<DrawnReadout>>> =
        const { std::cell::RefCell::new(None) };
}

/// A readout as it was drawn: the target a press hands the state, and what it
/// printed closed.
#[cfg(test)]
#[derive(Debug, Clone)]
pub(crate) struct DrawnReadout {
    pub target: ScrubTarget,
    pub text: SharedString,
}

/// Start a fresh record of the readouts drawn, on every surface.
#[cfg(test)]
pub(crate) fn record_readouts() {
    READOUTS.set(Some(Vec::new()));
}

/// Stop recording and return every readout drawn since [`record_readouts`],
/// in the order they were drawn.
#[cfg(test)]
pub(crate) fn drawn_readouts() -> Vec<DrawnReadout> {
    READOUTS.with_borrow_mut(|drawn| drawn.take().unwrap_or_default())
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
    let text: SharedString = text.into();
    #[cfg(test)]
    READOUTS.with_borrow_mut(|drawn| {
        if let Some(drawn) = drawn {
            drawn.push(DrawnReadout {
                target: target.clone(),
                text: text.clone(),
            });
        }
    });
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
            .when(all, |el| {
                el.bg(theme.accent.opacity(0.35))
                    .rounded(px(Theme::CONTROL_RADIUS))
            })
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
            .child(crate::components::caret_mark(look.size, 1.0, theme))
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
        .on_hover(motion::hover_listener(hover))
        .child(text)
        // The underline hangs below the text, outside the readout's box, so the
        // box is the text's line and the number centres on its row like the
        // label and the keyframe navigator beside it.
        .child(
            div()
                .absolute()
                .left_0()
                .right_0()
                .bottom(px(-1.0))
                .h(px(1.0))
                .border_b_1()
                .border_dashed()
                .border_color(ink.opacity(if dragging { 1.0 } else { lit })),
        )
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
