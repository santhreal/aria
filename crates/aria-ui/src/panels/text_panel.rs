//! The Character and Paragraph dock panels: a title's type, drawn beside the
//! viewer instead of inside the inspector.
//!
//! Both draw the controls the inspector's Text tab draws, from
//! [`text`](crate::panels::inspector::tabs::text), so a title has one set of
//! writes whichever surface edits it. A selection that is not a title shows a
//! line saying what the panel edits.

use gpui::{AnyElement, InteractiveElement, IntoElement, ParentElement, Styled, div, px};

use crate::app::AppState;
use crate::panels::inspector::tabs::text::{character_controls, paragraph_controls};
use crate::theme::Theme;
use gpui::prelude::FluentBuilder;

/// The surface name of the Character panel's controls.
const CHARACTER: &str = "character-panel";

/// The surface name of the Paragraph panel's controls.
const PARAGRAPH: &str = "paragraph-panel";

/// Window > Character: font, style, size, tracking, leading, kerning, colours
/// and treatments of the selected title.
pub fn render_character(
    state: &gpui::Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> AnyElement {
    let theme = Theme::of(cx);
    let controls = character_controls(&theme, CHARACTER, state, cx);
    panel(&theme, CHARACTER, controls)
}

/// Window > Paragraph: alignment, justification and wrapping of the selected
/// title.
pub fn render_paragraph(
    state: &gpui::Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> AnyElement {
    let theme = Theme::of(cx);
    let controls = paragraph_controls(&theme, PARAGRAPH, state, cx);
    panel(&theme, PARAGRAPH, controls)
}

/// The scrolling body both panels share: the controls, or the line a panel
/// shows when the selection is not a title.
fn panel(theme: &Theme, surface: &'static str, controls: Vec<AnyElement>) -> AnyElement {
    let empty = controls.is_empty();
    crate::edge_fade::scroll_y(
        div()
            .id(surface)
            .debug_selector(move || surface.into())
            .size_full()
            .p(px(Theme::SPACE_SM))
            .flex()
            .flex_col()
            .gap(px(Theme::SPACE_XS))
            .children(controls)
            .when(empty, |el| {
                el.child(
                    div()
                        .text_size(Theme::TEXT_UI)
                        .text_color(theme.text_faint)
                        .child("Select a title to edit its type."),
                )
            }),
    )
    .into_any_element()
}
