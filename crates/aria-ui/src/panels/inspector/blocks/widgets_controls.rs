//! The rows of an inspector block whose value is not a number: a switch, a
//! choice among a set, a control standing where a number row's value stands,
//! and the controls and notes that wrap on a line of their own, starting where
//! the labels start.
//!
//! A control drawn straight into a block's column is stretched across the
//! block and starts at its edge, left of every label. Each control here stands
//! in a row's value column or on a line that starts at the label column, and is
//! as wide as its label. A ghost button or a chip draws no frame at rest, so
//! its label is what stands on the column: its frame hangs out by its side
//! padding, [`components::GHOST_INSET`] or [`ChipSize::pad`], into the row's
//! padding and the gutter left of the labels.

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
use super::{CHOICES, DrawnChoice, IN_INSPECTOR, label_selector};
use super::{Columns, VALUE_MIN, WATCH_SLOT, blanks, line, value_cell};

/// What stands on the label column at the start of a line of controls.
#[derive(Clone, Copy)]
pub(super) enum Starts {
    /// Each control's edge: a note, a swatch, a meter, a graph.
    Edge,
    /// Each control's label, its frame hung out to the left by the controls'
    /// side padding.
    Label(f32),
}

/// The controls `under` a line, `gap` apart, on a wrapping line of their own.
/// In the keyed rows' columns, `keyed`, what `starts` each line stands where
/// the label does.
fn under(
    keyed: bool,
    gap: f32,
    starts: Starts,
    controls: impl IntoIterator<Item = gpui::AnyElement>,
) -> gpui::Div {
    let hung = match starts {
        Starts::Edge => None,
        Starts::Label(inset) => Some(inset),
    };
    div()
        .flex()
        .flex_row()
        .wraps()
        .when(keyed, |under| under.pl(px(WATCH_SLOT + Theme::SPACE_XS)))
        .when_some(hung, |under, inset| under.ml(px(-inset)))
        .gap(px(gap))
        .children(controls)
}

/// A row of one `head` line with the controls `under` it, `gap` apart, on a
/// wrapping line of their own: a palette under a color's line, chips under a
/// choice's. In the keyed rows' columns, `keyed`, what `starts` each line of
/// controls stands where the label does.
pub(super) fn stacked(
    head: gpui::Div,
    keyed: bool,
    gap: f32,
    starts: Starts,
    controls: impl IntoIterator<Item = gpui::AnyElement>,
) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XXS))
        .px(px(Theme::SPACE_XS))
        .py(px(Theme::SPACE_XS))
        .child(head)
        .child(under(keyed, gap, starts, controls))
}

/// Controls on a wrapping line of their own whose edges start where the
/// labels of `columns` start: a meter, a graph, a note.
pub fn under_labels(
    columns: Columns,
    controls: impl IntoIterator<Item = gpui::AnyElement>,
) -> gpui::Div {
    lined(columns, Starts::Edge, controls)
}

/// Ghost buttons on a wrapping line of their own whose labels start where the
/// labels of `columns` start: a block's Reset, or a button that runs a job.
pub fn ghosts_under_labels(
    columns: Columns,
    ghosts: impl IntoIterator<Item = gpui::AnyElement>,
) -> gpui::Div {
    lined(columns, Starts::Label(components::GHOST_INSET.0), ghosts)
}

/// Chips of `size` on a wrapping line of their own whose labels start where
/// the labels of `columns` start: a mask's modes, or a list to add from.
pub fn chips_under_labels(
    columns: Columns,
    size: ChipSize,
    chips: impl IntoIterator<Item = gpui::AnyElement>,
) -> gpui::Div {
    lined(columns, Starts::Label(size.pad().0), chips)
}

/// Controls on a wrapping line of their own, what `starts` each line standing
/// where the labels of `columns` start.
fn lined(
    columns: Columns,
    starts: Starts,
    controls: impl IntoIterator<Item = gpui::AnyElement>,
) -> gpui::Div {
    div().px(px(Theme::SPACE_XS)).child(under(
        matches!(columns, Columns::Keyed),
        Theme::SPACE_XS,
        starts,
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

/// A row whose control, a ghost button such as a switch, stands where a number
/// row's value stands. The button's label ends where a number ends, and its
/// frame hangs out into the row's padding, so the line is as tall as a number
/// row's and a column of switches keeps the rows' rhythm. The caller sets the
/// line's id and tooltip.
pub fn control_line(
    theme: &Theme,
    key: motion::MotionKey,
    label: impl Into<SharedString>,
    columns: Columns,
    control: impl IntoElement,
) -> gpui::Div {
    let label = label.into();
    let column = columns.column(&label);
    let (across, down) = components::GHOST_INSET;
    let value = value_cell(column.as_ref())
        .child(div().flex().mr(px(-across)).my(px(-down)).child(control));
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

/// A ghost button that files one edit: a press runs `file` on the state, and
/// a refusal is reported in the status line.
pub fn pick_button(
    id: impl Into<motion::ControlId>,
    label: impl Into<SharedString>,
    theme: &Theme,
    cx: &gpui::Context<crate::shell::Shell>,
    file: impl Fn(&mut AppState) -> Result<(), EngineError> + 'static,
) -> gpui::Stateful<gpui::Div> {
    components::ghost_button(id, label, theme).on_mouse_down(
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

/// A switch of the selected clip in the keyed rows' columns: the `row`'s key
/// and label, and its button `control`, which reads On or Off for `on` and
/// files `file` on a press.
pub fn switch_line(
    theme: &Theme,
    (row, label): (&'static str, &'static str),
    control: impl Into<motion::ControlId>,
    on: bool,
    cx: &gpui::Context<crate::shell::Shell>,
    file: impl Fn(&mut AppState) -> Result<(), EngineError> + 'static,
) -> gpui::AnyElement {
    control_line(
        theme,
        motion::MotionKey::new("insp.row", row),
        label,
        Columns::Keyed,
        pick_button(control, if on { "On" } else { "Off" }, theme, cx, file),
    )
    .into_any_element()
}

/// A choice's label as its chip prints it: marked `• ` when it is `on`, the
/// choice in force.
pub fn picked(label: impl Into<SharedString>, on: bool) -> SharedString {
    let label = label.into();
    if on {
        SharedString::from(format!("• {label}"))
    } else {
        label
    }
}

/// One control under a [`chip_row`]'s line: a choice, marked when it is in
/// force, or anything else, such as Clear, a preset or a message.
pub struct Chip {
    element: gpui::AnyElement,
    /// The choice's label and whether it is in force.
    #[cfg(test)]
    pick: Option<(SharedString, bool)>,
}

impl Chip {
    /// The choice `label`, in force when `on`. `draw` builds the chip around
    /// the label as [`picked`] prints it.
    pub fn choice(
        label: impl Into<SharedString>,
        on: bool,
        draw: impl FnOnce(SharedString) -> gpui::AnyElement,
    ) -> Self {
        let label = label.into();
        Self {
            #[cfg(test)]
            pick: Some((label.clone(), on)),
            element: draw(picked(label, on)),
        }
    }

    /// A control that picks no choice.
    pub fn other(element: impl IntoElement) -> Self {
        Self {
            element: element.into_any_element(),
            #[cfg(test)]
            pick: None,
        }
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
        label_selector(column.as_ref(), &label),
        current.clone(),
        Vec::new(),
    );
    let value = value_cell(column.as_ref()).child(
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
    let drawn = stacked(
        head,
        keyed,
        Theme::SPACE_XS,
        Starts::Label(components::GHOST_INSET.0),
        controls,
    );
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
