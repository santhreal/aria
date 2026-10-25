//! Shape tab: the geometry and the fill of a shape clip.
//!
//! Every row comes from the catalog's `shape` group: a rectangle's corner
//! radius, a star's inner ratio and the stroke width as steppers, the fill and
//! the stroke colour as swatches.
//!
//! What is still missing is stated rather than faked: the shape *kind* (which
//! outline the clip is) is a structural choice, and there is no command that
//! changes one, so this tab does not offer a picker that could not bind.

use gpui::{IntoElement, ParentElement, Styled, div, px};

use crate::theme::Theme;

use crate::panels::inspector::blocks::widgets::{color_rows, param_rows};

use crate::components::section_label;

pub fn shape_tab(
    theme: &Theme,
    state: &gpui::Entity<crate::app::AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let rows = param_rows(theme, "shape", state, cx);
    let colors = color_rows(theme, "shape", state, cx);
    let empty = rows.is_empty() && colors.is_empty();
    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .child(section_label(theme, "Shape"))
        .children(rows)
        .children(colors)
        .child(
            div()
                .text_size(Theme::TEXT_UI)
                .text_color(theme.text_faint)
                .child(if empty {
                    "This clip carries no shape properties"
                } else {
                    "Which outline a shape draws is set when it is placed"
                }),
        )
        .child(crate::panels::inspector::blocks::keyframes::keyframes_block(theme, state, cx))
}
