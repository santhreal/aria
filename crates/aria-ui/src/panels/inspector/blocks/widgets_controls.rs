//! The rows of an inspector block whose value is not a number: a switch, a
//! choice among a set, a fact of the clip, a control standing where a number
//! row's value stands, and the controls and notes that wrap on a line of their
//! own, starting where the labels start.
//!
//! A control drawn straight into a block's column is stretched across the
//! block and starts at its edge, left of every label. Each control here stands
//! in a row's value column or on a line that starts at the label column, and is
//! as wide as its label. A control's outer edge is what stands on a column: a
//! ghost button's border, a chip's plate, a switch's track.

use aria_engine::EngineError;
use gpui::prelude::FluentBuilder;
use gpui::{
    InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement, SharedString,
    Styled, div, px,
};

use crate::app::AppState;
use crate::components::{self, ChipSize, Wraps as _};
use crate::motion;
use crate::theme::Theme;
use crate::tooltip::WithTip as _;

#[cfg(test)]
use super::{CHOICES, DrawnChoice, IN_INSPECTOR, cell_selector};
use super::{Columns, VALUE_MIN, WATCH_SLOT, blanks, line, value_cell};

/// The controls `under` a line, `gap` apart, on a wrapping line of their own.
/// In the keyed rows' columns, `keyed`, each line starts where the label does.
fn under(keyed: bool, gap: f32, controls: impl IntoIterator<Item = gpui::AnyElement>) -> gpui::Div {
    div()
        .flex()
        .flex_row()
        .wraps()
        .when(keyed, |under| under.pl(px(WATCH_SLOT + Theme::SPACE_XS)))
        .gap(px(gap))
        .children(controls)
}

/// A row of one `head` line with the controls `under` it, `gap` apart, on a
/// wrapping line of their own: a palette under a color's line, chips under a
/// choice's. In the keyed rows' columns, `keyed`, each line of controls starts
/// where the label does.
pub(super) fn stacked(
    head: gpui::Div,
    keyed: bool,
    gap: f32,
    controls: impl IntoIterator<Item = gpui::AnyElement>,
) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XXS))
        .px(px(Theme::SPACE_XS))
        .py(px(Theme::SPACE_XS))
        .child(head)
        .child(under(keyed, gap, controls))
}

/// Controls on a wrapping line of their own whose edges start where the
/// labels of `columns` start: a block's Reset, a mask's modes, a button that
/// runs a job, a meter, a graph, a note.
pub fn under_labels(
    columns: Columns,
    controls: impl IntoIterator<Item = gpui::AnyElement>,
) -> gpui::Div {
    div().px(px(Theme::SPACE_XS)).child(under(
        matches!(columns, Columns::Keyed),
        Theme::SPACE_XS,
        controls,
    ))
}

/// A line of text in `ink` that starts where the labels of `columns` start
/// and wraps at the block's edge: a warning, or what a block waits for.
pub fn label_note(columns: Columns, ink: gpui::Hsla, text: impl Into<SharedString>) -> gpui::Div {
    under_labels(
        columns,
        [div()
            .w_full()
            .text_size(Theme::TEXT_UI)
            .text_color(ink)
            .child(text.into())
            .into_any_element()],
    )
}

/// A line whose `value` is a control standing where a number row's value
/// stands, its right edge where the numbers end.
fn value_line(
    theme: &Theme,
    key: motion::MotionKey,
    label: SharedString,
    columns: Columns,
    value: impl IntoElement,
) -> gpui::Div {
    let column = columns.column(&label);
    let value = value_cell(column.as_ref(), &label).child(value);
    line(
        key,
        label,
        column.as_ref(),
        blanks(columns),
        theme.text_muted,
        value,
    )
    .px(px(Theme::SPACE_XS))
    .py(px(Theme::SPACE_XS))
}

/// A row whose control, a ghost button, stands where a number row's value
/// stands, its border ending where the numbers end. The button is one control
/// tall, past a number's line, so it reaches into the row's padding above and
/// below by [`components::GHOST_INSET`]'s height, and the line is as tall as a
/// number row's. The caller sets the line's id and tooltip.
pub fn control_line(
    theme: &Theme,
    key: motion::MotionKey,
    label: impl Into<SharedString>,
    columns: Columns,
    control: impl IntoElement,
) -> gpui::Div {
    let down = components::GHOST_INSET.1;
    value_line(
        theme,
        key,
        label.into(),
        columns,
        div().flex().my(px(-down)).child(control),
    )
}

/// A row that prints a fact of the clip: the label, and `value` where a number
/// row's value stands, in tabular figures. In the free columns, a value too
/// wide for its label's line moves under the label and wraps at the row's
/// width.
pub fn fact_row(
    theme: &Theme,
    label: impl Into<SharedString>,
    value: impl Into<SharedString>,
    columns: Columns,
) -> gpui::Div {
    let label = label.into();
    let column = columns.column(&label);
    let value = value_cell(column.as_ref(), &label).max_w_full().child(
        div()
            .text_size(Theme::TEXT_UI)
            .text_color(theme.text)
            .font_features(crate::theme::tabular_figures())
            .child(value.into()),
    );
    line(
        motion::MotionKey::new("insp.fact", &label),
        label,
        column.as_ref(),
        blanks(columns),
        theme.text_muted,
        value,
    )
    .px(px(Theme::SPACE_XS))
}

/// `control` with a press that runs `file` on the state, reporting a refusal
/// in the status line: a [`pick_button`], a [`choice_chip`], a switch, or an
/// icon such as a heading's reset.
pub fn filing(
    control: gpui::Stateful<gpui::Div>,
    cx: &gpui::Context<crate::shell::Shell>,
    file: impl Fn(&mut AppState) -> Result<(), EngineError> + 'static,
) -> gpui::Stateful<gpui::Div> {
    control.on_mouse_down(
        MouseButton::Left,
        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
            this.state.update(cx, |state, _| {
                if let Err(err) = file(state) {
                    state.report_error(&err);
                }
            });
            cx.notify();
        }),
    )
}

/// A ghost button that files one edit on a press.
pub fn pick_button(
    id: impl Into<motion::ControlId>,
    label: impl Into<SharedString>,
    theme: &Theme,
    cx: &gpui::Context<crate::shell::Shell>,
    file: impl Fn(&mut AppState) -> Result<(), EngineError> + 'static,
) -> gpui::Stateful<gpui::Div> {
    filing(components::ghost_button(id, label, theme), cx, file)
}

/// The choice `label` of a [`chip_row`]: a regular chip, lit while the choice
/// is `on`, that files one edit on a press.
pub fn choice_chip(
    id: impl Into<motion::ControlId>,
    label: impl Into<SharedString>,
    on: bool,
    theme: &Theme,
    cx: &gpui::Context<crate::shell::Shell>,
    file: impl Fn(&mut AppState) -> Result<(), EngineError> + 'static,
) -> Chip {
    let label = label.into();
    Chip {
        #[cfg(test)]
        pick: Some((label.clone(), on)),
        element: filing(
            components::chip(id, label, on, ChipSize::Regular, theme),
            cx,
            file,
        )
        .into_any_element(),
    }
}

/// A switch of the selection in `columns`: the line `key` with its `label`,
/// and the [`components::switch`] `control` where a number row's value
/// stands, on for `Some(true)`, off for `Some(false)`, and between for
/// `None`, where the selected clips differ. A press files `file`. The caller
/// sets the line's id and tooltip.
#[allow(clippy::too_many_arguments)]
pub fn switch_line(
    theme: &Theme,
    key: motion::MotionKey,
    label: impl Into<SharedString>,
    columns: Columns,
    control: impl Into<motion::ControlId>,
    on: Option<bool>,
    cx: &gpui::Context<crate::shell::Shell>,
    file: impl Fn(&mut AppState) -> Result<(), EngineError> + 'static,
) -> gpui::Div {
    value_line(
        theme,
        key,
        label.into(),
        columns,
        filing(components::switch(control, on, theme), cx, file),
    )
}

/// A list entry's label as its row prints it: marked `• ` when it is `on`,
/// the entry in force.
pub fn picked(label: impl Into<SharedString>, on: bool) -> SharedString {
    let label = label.into();
    if on {
        SharedString::from(format!("• {label}"))
    } else {
        label
    }
}

/// One control under a [`chip_row`]'s line: a [`choice_chip`], lit when its
/// choice is in force, or anything else, such as Clear, a preset or a
/// message.
pub struct Chip {
    element: gpui::AnyElement,
    /// The choice's label and whether it is in force.
    #[cfg(test)]
    pick: Option<(SharedString, bool)>,
}

impl Chip {
    /// A control that picks no choice.
    pub fn other(element: impl IntoElement) -> Self {
        Self {
            element: element.into_any_element(),
            #[cfg(test)]
            pick: None,
        }
    }

    /// The control on a line of its own rather than under a [`chip_row`].
    pub fn into_any_element(self) -> gpui::AnyElement {
        self.element
    }
}

/// A row whose value is one of a set: the label and the `current` choice on
/// one line, the choice where a number row's value stands, and the `chips` that
/// pick another on the line under it, as a color row's palette is. The choice
/// is cut to the value column's width, with the whole of it in a tooltip. The
/// caller sets the row's id and tooltip.
pub fn chip_row(
    theme: &Theme,
    key: motion::MotionKey,
    label: impl Into<SharedString>,
    columns: Columns,
    current: impl Into<SharedString>,
    chips: impl IntoIterator<Item = Chip>,
) -> gpui::Div {
    let label = label.into();
    let current = current.into();
    let column = columns.column(&label);
    #[cfg(test)]
    let (row, shown, mut marks) = (
        cell_selector(column.as_ref(), &label, "label"),
        current.clone(),
        Vec::new(),
    );
    let value = value_cell(column.as_ref(), &label).child(
        div()
            .id(key.element_id("insp-current"))
            .max_w(px(VALUE_MIN))
            .truncate()
            .text_size(Theme::TEXT_UI)
            .text_color(theme.text_faint)
            .with_tip(crate::tooltip::text_builder(current.clone()))
            .child(current),
    );
    let keyed = column.is_some();
    let head = line(
        key,
        label,
        column.as_ref(),
        blanks(columns),
        theme.text_muted,
        value,
    );
    let controls = chips.into_iter().map(|chip| {
        #[cfg(test)]
        marks.extend(chip.pick);
        chip.element
    });
    let drawn = stacked(head, keyed, Theme::SPACE_XS, controls);
    #[cfg(test)]
    if IN_INSPECTOR.get() {
        CHOICES.with_borrow_mut(|choices| {
            if let Some(choices) = choices {
                choices.push(DrawnChoice {
                    row,
                    current: shown,
                    chips: marks,
                });
            }
        });
    }
    drawn
}
