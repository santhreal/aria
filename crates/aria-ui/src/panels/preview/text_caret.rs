//! The Type tool's caret and selection over a title in the Viewer.
//!
//! Both come from [`AppState::drawn_text_caret`] in window pixels, so the caret
//! sits on the boundary the renderer laid the glyphs out at. The press that
//! places the caret is the picture's own (`AppState::select_layer_at`); the
//! shell carries the drag that selects and ends it at the release
//! (`Shell::carry_gesture`), wherever the pointer goes.

use gpui::{
    ContentMask, Entity, IntoElement, ParentElement, PathBuilder, Styled, canvas, div, point, px,
};

use crate::app::AppState;
use crate::app::clip_text::text_caret::DrawnTextCaret;
use crate::theme::Theme;

/// The caret's stroke, in pixels.
const CARET: f32 = 1.5;

fn at([x, y]: [f32; 2]) -> gpui::Point<gpui::Pixels> {
    point(px(x), px(y))
}

/// Paint the selection under the caret, then the caret.
fn paint(
    drawn: &DrawnTextCaret,
    selection: gpui::Hsla,
    caret: gpui::Hsla,
    window: &mut gpui::Window,
) {
    if !drawn.selection.is_empty() {
        let mut cells = PathBuilder::fill();
        for [a, b, c, d] in &drawn.selection {
            cells.move_to(at(*a));
            cells.line_to(at(*b));
            cells.line_to(at(*c));
            cells.line_to(at(*d));
            cells.close();
        }
        if let Ok(cells) = cells.build() {
            window.paint_path(cells, selection);
        }
    }
    if let Some([top, bottom]) = drawn.caret {
        let mut line = PathBuilder::stroke(px(CARET));
        line.move_to(at(top));
        line.line_to(at(bottom));
        if let Ok(line) = line.build() {
            window.paint_path(line, caret);
        }
    }
}

/// The caret and selection of the title being typed into, or nothing when no
/// title is.
pub(super) fn overlay(
    state: &Entity<AppState>,
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Option<gpui::AnyElement> {
    let read = state.read(cx);
    let drawn = read.drawn_text_caret()?;
    let (selection, caret) = (theme.selection, theme.caret);
    let root = div().absolute().size_full().child(
        canvas(
            |_, _, _| (),
            move |bounds, (), window, _| {
                window.with_content_mask(Some(ContentMask::from(bounds)), |window| {
                    paint(&drawn, selection, caret, window);
                });
            },
        )
        .absolute()
        .size_full(),
    );
    Some(root.into_any_element())
}
