//! The installed font families as one list, for every control that picks a
//! typeface: a title's font under Character and an effect's font slot.
//!
//! The list holds every family the host has and scrolls, drawing only the rows
//! in view ([`crate::edge_fade::scroll_list`]), so a host with a thousand
//! families draws it as fast as one with ten. It opens on the family in use,
//! which stands on the selected plate; a press on any other row files it.

use std::ops::Range;
use std::rc::Rc;

use aria_engine::EngineError;
use gpui::{InteractiveElement, ParentElement, StatefulInteractiveElement, Styled, div, px};

use crate::app::AppState;
use crate::edge_fade::ScrollFrame;
use crate::motion::MotionKey;
use crate::theme::Theme;

use super::widgets::filing;

/// Rows the list shows before it scrolls.
const ROWS_SHOWN: f32 = 7.0;

/// The installed `families` under an optional first row, `default`, that files
/// the empty family, each row a press that files `file` with its family.
///
/// `key` names the list and each row in it, so two lists keep their place and
/// their rows apart. `current` is the family in force, `""` for the default
/// row, or `None` while the selected clips differ. Its row stands on the
/// selected plate, files nothing, and is brought into view when the list opens
/// and whenever `current` changes.
pub fn font_list(
    theme: &Theme,
    key: MotionKey,
    families: &'static [String],
    default: Option<&'static str>,
    current: Option<&str>,
    cx: &gpui::Context<crate::shell::Shell>,
    file: impl Fn(&mut AppState, &str) -> Result<(), EngineError> + 'static,
) -> ScrollFrame {
    let lead = usize::from(default.is_some());
    // What row `row` files and reads: the default row the empty family, then
    // each installed family by its name.
    let entry = move |row: usize| match row.checked_sub(lead) {
        Some(at) => families
            .get(at)
            .map(|family| (family.as_str(), family.as_str())),
        None => default.map(|label| ("", label)),
    };
    let marked = current.and_then(|current| match current {
        "" => default.map(|_| 0),
        family => families
            .iter()
            .position(|name| name == family)
            .map(|at| at + lead),
    });
    let (plate, hover, text) = (theme.glass_selected_bg(), theme.element_hover, theme.text);
    let file = Rc::new(file);
    crate::edge_fade::scroll_list(
        key.element_id("font-list"),
        lead + families.len(),
        px(ROWS_SHOWN * Theme::ROW_HEIGHT),
        marked,
        cx.processor(move |_, rows: Range<usize>, _, cx| {
            rows.filter_map(|row| {
                let (family, label) = entry(row)?;
                let line = div()
                    .id(key.with(family).element_id("font"))
                    .debug_selector(move || format!("font-row-{row}"))
                    .flex()
                    .items_center()
                    .w_full()
                    .h(px(Theme::ROW_HEIGHT))
                    .px(px(Theme::SPACE_SM))
                    .rounded(px(Theme::CONTROL_RADIUS))
                    .text_size(Theme::TEXT_UI)
                    .text_color(text)
                    .child(div().min_w_0().truncate().child(label));
                if marked == Some(row) {
                    return Some(line.bg(plate));
                }
                let file = Rc::clone(&file);
                Some(filing(
                    line.cursor_pointer()
                        .hover(move |line| line.bg(hover))
                        .active(|line| line.opacity(0.92)),
                    cx,
                    move |state| file(state, family),
                ))
            })
            .collect::<Vec<_>>()
        }),
    )
}
