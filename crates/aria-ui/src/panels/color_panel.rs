//! The Color dock panel: the selected clip's grade, laid out by section.
//!
//! The rows are the catalog's `grade` group, the same rows the inspector's Look
//! tab draws, sorted into sections by `SECTIONS`. A row the table does not
//! place is drawn under "Other", so a grade property the model gains is never
//! dropped from the panel, and `every_grade_row_has_a_section` turns red until
//! the row is placed.
//!
//! Each section's header opens and closes it. Basic Correction starts open
//! and the rest closed; a closed section's rows are not built.

use gpui::{
    AnyElement, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    StatefulInteractiveElement, Styled, div, px,
};

use crate::app::AppState;
use crate::components;
use crate::components::section_label;
use crate::motion::MotionKey;
use crate::panels::inspector::blocks::wheels::wheels_block;
use crate::panels::inspector::blocks::widgets::param_rows_into;
use crate::surfaces::inspector_pick::{self, Pick};
use crate::theme::Theme;

/// The surface name of the panel.
const SURFACE: &str = "color-panel";

/// The section the colour wheels head. Its rows are the wheels' numeric
/// channels, drawn under the wheels.
const WHEELS: &str = "Color Wheels";

/// Every section in panel order, with the grade rows it draws, by catalog id.
pub(crate) const SECTIONS: &[(&str, &[&str])] = &[
    (
        "Basic Correction",
        &[
            "adjust-temperature",
            "adjust-tint",
            "adjust-exposure",
            "adjust-contrast",
            "adjust-highlights",
            "adjust-shadows",
            "adjust-whites",
            "adjust-blacks",
            "adjust-brightness",
            "adjust-saturation",
        ],
    ),
    (
        "Creative",
        &[
            "adjust-faded-film",
            "adjust-vibrance",
            "adjust-sharpness",
            "adjust-hue",
        ],
    ),
    (
        "RGB Curves",
        &[
            "adjust-curve-master-shadows",
            "adjust-curve-master-midtones",
            "adjust-curve-master-highlights",
            "adjust-curve-red-shadows",
            "adjust-curve-red-midtones",
            "adjust-curve-red-highlights",
            "adjust-curve-green-shadows",
            "adjust-curve-green-midtones",
            "adjust-curve-green-highlights",
            "adjust-curve-blue-shadows",
            "adjust-curve-blue-midtones",
            "adjust-curve-blue-highlights",
        ],
    ),
    (
        "Hue Curves",
        &[
            "adjust-hue-sat-red",
            "adjust-hue-sat-yellow",
            "adjust-hue-sat-green",
            "adjust-hue-sat-cyan",
            "adjust-hue-sat-blue",
            "adjust-hue-sat-magenta",
            "adjust-hue-hue-red",
            "adjust-hue-hue-yellow",
            "adjust-hue-hue-green",
            "adjust-hue-hue-cyan",
            "adjust-hue-hue-blue",
            "adjust-hue-hue-magenta",
            "adjust-hue-luma-red",
            "adjust-hue-luma-yellow",
            "adjust-hue-luma-green",
            "adjust-hue-luma-cyan",
            "adjust-hue-luma-blue",
            "adjust-hue-luma-magenta",
        ],
    ),
    (
        WHEELS,
        &[
            "adjust-lift-master",
            "adjust-lift-r",
            "adjust-lift-g",
            "adjust-lift-b",
            "adjust-gamma-master",
            "adjust-gamma-r",
            "adjust-gamma-g",
            "adjust-gamma-b",
            "adjust-gain-master",
            "adjust-gain-r",
            "adjust-gain-g",
            "adjust-gain-b",
        ],
    ),
    (
        "HSL Secondary",
        &[
            "adjust-hsl-key-hue",
            "adjust-hsl-key-width",
            "adjust-hsl-hue",
            "adjust-hsl-saturation",
            "adjust-hsl-luma",
        ],
    ),
    ("Vignette", &["adjust-vignette"]),
];

// A section past the open-state capacity could never open.
const _: () = assert!(
    SECTIONS.len() <= crate::app::grade_controls::COLOR_SECTIONS,
    "more Color panel sections than open-state bits"
);

/// Sections plus the "Other" slot for a row no section names.
const SLOTS: usize = SECTIONS.len() + 1;

/// The section index of grade row `id`, `None` for a row no section names.
pub(crate) fn section_of(id: &str) -> Option<usize> {
    SECTIONS.iter().position(|(_, rows)| rows.contains(&id))
}

/// The slot grade row `id` is drawn in: its section, or "Other" last.
fn slot_of(id: &str) -> usize {
    section_of(id).unwrap_or(SECTIONS.len())
}

/// Window > Color.
pub fn render(
    state: &gpui::Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> AnyElement {
    let theme = Theme::of(cx);
    let root = div()
        .id(SURFACE)
        .debug_selector(|| SURFACE.into())
        .size_full()
        .overflow_y_scroll()
        .p(px(Theme::SPACE_SM))
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS));
    if state.read(cx).selected_clip().is_none() {
        return root
            .child(hint(&theme, "Select a clip to grade."))
            .into_any_element();
    }

    let open: [bool; SECTIONS.len()] = {
        let read = state.read(cx);
        std::array::from_fn(|index| read.color_section_open(index))
    };
    let mut present = [false; SLOTS];
    let mut slots: [Vec<AnyElement>; SLOTS] = std::array::from_fn(|_| Vec::new());
    // A closed section's rows are counted, so its header still draws, and are
    // not built.
    param_rows_into(
        &theme,
        "color.param",
        "grade",
        state,
        cx,
        |id| {
            let slot = slot_of(id);
            present[slot] = true;
            open.get(slot).copied().unwrap_or(true)
        },
        |id, element| slots[slot_of(id)].push(element),
    );
    if !present.contains(&true) {
        return root
            .child(hint(&theme, "This clip has no colour to grade."))
            .into_any_element();
    }

    // Reset heads the panel, where it is reached without scrolling past every
    // section.
    let reset = components::ghost_button("color-grade-clear", "Reset grade", &theme)
        .debug_selector(|| "color-grade-clear".into())
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                this.state.update(cx, |state, _| {
                    if let Err(err) = inspector_pick::apply(state, Pick::ClearGrade) {
                        state.report_error(&err);
                    }
                });
                cx.notify();
            }),
        );
    let mut body: Vec<AnyElement> = Vec::with_capacity(SLOTS * 2 + 2);
    body.push(div().flex().justify_end().child(reset).into_any_element());
    for (index, rows) in slots.into_iter().enumerate() {
        if !present[index] {
            continue;
        }
        let Some(&(title, _)) = SECTIONS.get(index) else {
            // "Other" has no header to close it.
            body.push(section_label(&theme, "Other").into_any_element());
            body.extend(rows);
            continue;
        };
        body.push(section_header(&theme, index, title, open[index], cx));
        if !open[index] {
            continue;
        }
        if title == WHEELS {
            body.push(wheels_block(&theme, "color", state, cx).into_any_element());
        }
        body.extend(rows);
    }
    root.children(body).into_any_element()
}

/// A section's title, as a row a click opens or closes the section from.
fn section_header(
    theme: &Theme,
    index: usize,
    title: &'static str,
    open: bool,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> AnyElement {
    components::disclosure_button(
        MotionKey::new("color-section", index),
        title,
        open,
        theme,
        cx.reduce_motion(),
    )
    .debug_selector(move || format!("color-section-{index}"))
    .mt(px(Theme::SPACE_XS))
    .font_weight(gpui::FontWeight::MEDIUM)
    .on_mouse_down(
        MouseButton::Left,
        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
            this.state
                .update(cx, |state, _| state.toggle_color_section(index));
            cx.notify();
        }),
    )
    .into_any_element()
}

/// A line in place of the rows.
fn hint(theme: &Theme, text: &'static str) -> AnyElement {
    div()
        .text_size(Theme::TEXT_UI)
        .text_color(theme.text_faint)
        .child(text)
        .into_any_element()
}

#[cfg(test)]
#[path = "color_panel_tests.rs"]
mod tests;
