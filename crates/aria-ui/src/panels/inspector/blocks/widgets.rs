//! Shared inspector rows and labels, used by more than one tab.
//!
//! Every numeric control in the inspector is the same row — a label and a
//! scrubbable value (`panels::value_readout`) — so it is drawn once, here. That is
//! what makes the motion a property of "a row" rather than of the fifteen
//! places that draw one: the catalog's properties, the transform's six, and
//! every effect knob all animate because [`stepper_row`] does.
//!
//! A row drawn from the property catalog gets the whole treatment, because the
//! catalog supplies the two things the rest cannot: a range, so there is a
//! track for the value to travel along, and a [`ClipParam`], so there is
//! something for a keyframe to be written against. A row whose caller has
//! neither still fades under the pointer and reads the same.

use crate::app::AppState;
use crate::app::clip_params::{ColorRow, ParamRow};
use crate::app::drivers::DriverDraft;
use crate::app::key_strip::KeyStrip;
use crate::app::value_scrub::{ScrubTarget, ValueKind};
use crate::components::Wraps as _;
use crate::motion::{self, glide};
use crate::panels::value_readout::{self, Look};
use crate::theme::Theme;
use crate::tooltip::WithTip as _;
use crate::types::UiClipKind;
use aria_engine::EngineError;
use aria_models::ClipParam;
use gpui::motion::presets;
use gpui::prelude::FluentBuilder;
use gpui::{
    CursorStyle, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    SharedString, StatefulInteractiveElement, Styled, div, px, relative,
};

#[path = "widgets_catalog.rs"]
mod catalog;
#[path = "widgets_controls.rs"]
mod controls;
#[path = "widgets_keys.rs"]
mod keys;

pub use catalog::{color_rows, param_rows, param_rows_into, swatch_row, unkeyed_row};
pub use controls::{
    Chip, chip_row, chips_under_labels, control_line, fact_row, ghosts_under_labels, label_note,
    pick_button, picked, switch_line, under_labels,
};
use keys::{Affordances, WATCH_SLOT, affordances, blanks};

/// Thickness of a row's value track.
const TRACK_H: f32 = 3.0;

/// Diameter of the thumb riding the track.
const THUMB: f32 = 7.0;

/// Diameter of the stopwatch's ring.
const DOT: f32 = 8.0;

/// The room a label holds on a row with a time area before it shrinks, in
/// pixels: the width of Anchor point X at `TEXT_UI` in Inter (77.4 px) plus a
/// little air. The label grows into any room the row has past it. In a frame
/// too narrow for every control, the strip narrows first and is left blank
/// below its minimum, and the label ends in an ellipsis only once the strip has
/// no width left.
pub const LABEL_RESERVE: f32 = 80.0;

/// A keyed row label's flex shrink factor. Shrink is shared in proportion to
/// factor times width, so of two neighbors whose factors are a thousand apart
/// the larger gives up its width first: the strip's factor is a thousand times
/// this, and the value column's a thousandth of it.
pub(super) const LABEL_YIELD: f32 = 1000.0;

/// The width of the value column on a row in the keyed rows' columns, in
/// pixels. A timecode at `TEXT_UI` is the widest value a built-in row prints:
/// 66.6 px in Inter's tabular figures, and 72.6 px in the test text system,
/// which sets every glyph 0.6 em wide. A value narrower than the column stands
/// at its right edge, so the controls after it start at one x down the panel
/// whatever each row prints, and the strips narrow together in a narrow frame.
pub const VALUE_MIN: f32 = 76.0;

/// The value column's flex shrink factor, a thousandth of `LABEL_YIELD`, so
/// while a label narrows to nothing its value column loses under a tenth of a
/// pixel. In a frame too narrow for every control, the column narrows once
/// the strip and the label have no width left, and never below the value it
/// prints. It is no less than 1: flex layout shrinks items whose factors sum
/// to less than 1 by that fraction of the overflow only.
const VALUE_YIELD: f32 = LABEL_YIELD / 1000.0;

/// What a row's value is doing, beyond the number it prints.
#[derive(Clone, Copy)]
pub struct Travel {
    /// The value itself. The row eases toward this, so it can tell a value
    /// that is still arriving from one that has settled.
    pub value: f32,
    /// One nudge. How far the row still has to go is measured in these, so the
    /// property's own scale decides what "nearly there" means — a constant
    /// would be wrong for degrees and right for fractions.
    pub step: f32,
    /// The bounds the model enforces, when it bounds both sides. The track is
    /// drawn against these and the fill follows the same eased value the
    /// readout does, so the bar and the number can never disagree.
    pub range: Option<(f32, f32)>,
}

/// What a row prints where the selected clips read differently.
pub const MIXED: &str = "—";

/// A row's number: what it prints, its value in the units it prints, what one
/// unit of drag moves it by, and how it is typed.
pub struct Readout {
    pub text: String,
    pub value: f32,
    pub unit: f32,
    pub kind: ValueKind,
}

impl Readout {
    /// A bare number printed with `decimals` digits.
    pub fn number(value: f32, decimals: u8, unit: f32) -> Self {
        let kind = ValueKind::Number { decimals };
        Self {
            text: kind.format(value),
            value,
            unit,
            kind,
        }
    }

    /// A number the caller prints with its own unit or suffix, typed back as a
    /// number with `decimals` digits.
    pub fn printed(text: String, value: f32, decimals: u8, unit: f32) -> Self {
        Self {
            text,
            value,
            unit,
            kind: ValueKind::Number { decimals },
        }
    }

    /// This readout with a dash printed instead of its number when `mixed`:
    /// the selected clips read differently there. A drag still moves the value
    /// it holds, the first clip's, and the others take where it lands.
    pub fn dashed(mut self, mixed: bool) -> Self {
        if mixed {
            self.text = MIXED.to_owned();
        }
        self
    }

    /// A frame count, printed and typed as timecode at `num / den`.
    pub fn frames(frames: i64, num: i32, den: i32) -> Self {
        let kind = ValueKind::Frames { num, den };
        let value = frames as f32;
        Self {
            text: kind.format(value),
            value,
            unit: 1.0,
            kind,
        }
    }
}

/// The keyframe affordances of a row, and what the property's curve looks like
/// now: the stopwatch left of the label, and the navigator, the reset button
/// and the time area right of the value.
#[derive(Clone)]
pub struct Stopwatch {
    pub param: ClipParam,
    /// Whether the property carries any keyframes at all, which is what the
    /// stopwatch turns on and off.
    pub animated: bool,
    /// Whether one of them sits under the playhead, which is what the
    /// navigator's center diamond adds or removes.
    pub at_playhead: bool,
    /// Whether a keyframe sits before the playhead, and whether one sits after
    /// it: where the navigator's arrows go.
    pub keys_around: (bool, bool),
    /// The property holds something other than its reset value at the
    /// playhead, on the drawn clip or on another selected clip that shares it,
    /// so the reset button has something to do.
    pub off_default: bool,
    /// The property's driver, shown under the row.
    pub driver: Option<DriverLine>,
    /// A pick whip is waiting: a click on this row inserts a reference to
    /// the property instead of changing it.
    pub pick_target: bool,
    /// The property's time area, on a surface that draws one.
    pub strip: Option<KeyStrip>,
}

/// What a surface reads off the state once a frame to draw the stopwatches of
/// its catalog rows: the driver draft open on the selected clip, and whether
/// its pick whip waits.
pub struct Watching {
    draft: Option<DriverDraft>,
    picking: bool,
}

impl Watching {
    pub fn read(state: &AppState) -> Self {
        let selected = state.selected_clip();
        Self {
            draft: state
                .driver_draft()
                .filter(|draft| Some(draft.clip) == selected)
                .cloned(),
            picking: state.driver_picking(),
        }
    }

    /// The stopwatch `row` draws, with `strip` as its time area.
    pub fn stopwatch(&self, row: &ParamRow, strip: Option<KeyStrip>) -> Stopwatch {
        let param = row.param;
        // A pair shows its driver once, under its first component.
        let driver = if row.component.unwrap_or(0) != 0 {
            None
        } else if let Some(draft) = self.draft.as_ref().filter(|draft| draft.param == param) {
            Some(DriverLine {
                source: draft.text.clone().into(),
                fault: row.fault.clone(),
                editing: true,
                picking: draft.picking,
            })
        } else {
            row.driver.clone().map(|source| DriverLine {
                source,
                fault: row.fault.clone(),
                editing: false,
                picking: false,
            })
        };
        Stopwatch {
            param,
            animated: row.animated,
            at_playhead: row.at_playhead,
            keys_around: row.keys_around,
            off_default: row.off_default,
            driver,
            pick_target: self.picking,
            strip,
        }
    }
}

/// The stopwatch a color row draws. A color has no driver.
pub fn color_stopwatch(row: &ColorRow, strip: Option<KeyStrip>) -> Stopwatch {
    Stopwatch {
        param: row.param,
        animated: row.animated,
        at_playhead: row.at_playhead,
        keys_around: row.keys_around,
        off_default: row.off_default,
        driver: None,
        pick_target: false,
        strip,
    }
}

/// A property's driver as its row shows it.
#[derive(Clone)]
pub struct DriverLine {
    /// The driver's source, or the draft while it is edited.
    pub source: SharedString,
    /// Why the driver failed at the playhead.
    pub fault: Option<SharedString>,
    /// The source is open in the `aria.field.driver` box.
    pub editing: bool,
    /// The pick whip is waiting for a property.
    pub picking: bool,
}

/// The columns a row that cannot be keyed is drawn in.
#[derive(Clone, Copy)]
pub enum Columns {
    /// The keyed rows' columns, on an Effect Controls tab: a blank where their
    /// stopwatch stands, the label and the value in theirs, and a blank where
    /// their keyframe navigator, reset button and time area stand.
    Keyed,
    /// The label at the row's left edge and the value at its right, on a
    /// surface that draws no time area.
    Free,
}

impl Columns {
    /// The column identity of a row labelled `label` drawn in these columns,
    /// or `None` for a free row.
    fn column(self, label: &SharedString) -> Option<Column> {
        match self {
            Self::Keyed => Some(Column::Named(label.clone())),
            Self::Free => None,
        }
    }
}

/// A row's keyframe affordances, or the columns of a row that has none.
pub enum Keys {
    Watch(Stopwatch),
    Unkeyed(Columns),
}

impl Keys {
    /// The column identity of a row drawn in the keyed rows' columns, or
    /// `None` for a row that is not. A keyed row stands in them beside its
    /// time area.
    fn column(&self, label: &SharedString) -> Option<Column> {
        match self {
            Self::Watch(watch) => watch.strip.as_ref().map(|_| Column::Param(watch.param)),
            Self::Unkeyed(columns) => columns.column(label),
        }
    }
}

/// A row in the keyed rows' columns: the property a keyed row edits, or the
/// label of a row that cannot be keyed.
enum Column {
    Param(ClipParam),
    Named(SharedString),
}

impl Column {
    /// The debug selector of the row's `cell`: `insp-label-Opacity`,
    /// `insp-value-Fade in`.
    fn selector(&self, cell: &str) -> String {
        match self {
            Self::Param(param) => format!("insp-{cell}-{param:?}"),
            Self::Named(label) => format!("insp-{cell}-{label}"),
        }
    }
}

/// The debug selector of a line's `cell`, `label` or `value`: its column's,
/// or `insp-{cell}-free-{label}` on a line in no column.
fn cell_selector(column: Option<&Column>, label: &SharedString, cell: &str) -> String {
    column.map_or_else(
        || format!("insp-{cell}-free-{label}"),
        |column| column.selector(cell),
    )
}

#[cfg(test)]
thread_local! {
    /// The lines drawn into the Inspector since [`record_lines`].
    static LINES: std::cell::RefCell<Option<Vec<(String, bool)>>> =
        const { std::cell::RefCell::new(None) };
    /// The choice rows drawn into the Inspector since [`record_lines`].
    static CHOICES: std::cell::RefCell<Option<Vec<DrawnChoice>>> =
        const { std::cell::RefCell::new(None) };
    /// Whether the Inspector's body is being built.
    static IN_INSPECTOR: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// A [`chip_row`] drawn into the Inspector: its label's debug selector, the
/// choice its value column shows, and each choice under it with whether it is
/// marked in force.
#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct DrawnChoice {
    pub row: String,
    pub current: SharedString,
    pub chips: Vec<(SharedString, bool)>,
}

/// The Inspector's body being built, until this drops: the lines
/// [`record_lines`] records are the ones built meanwhile, and no other
/// panel's, and so are the ghost buttons it notes.
#[cfg(test)]
pub(crate) struct BuildingInspector {
    _ghosts: crate::components::NotingGhosts,
}

#[cfg(test)]
impl BuildingInspector {
    pub(crate) fn start() -> Self {
        IN_INSPECTOR.set(true);
        Self {
            _ghosts: crate::components::NotingGhosts::start(),
        }
    }
}

#[cfg(test)]
impl Drop for BuildingInspector {
    fn drop(&mut self) {
        IN_INSPECTOR.set(false);
    }
}

/// Start a fresh record of the lines, the choice rows and the ghost buttons
/// drawn into the Inspector.
#[cfg(test)]
pub(crate) fn record_lines() {
    LINES.set(Some(Vec::new()));
    CHOICES.set(Some(Vec::new()));
    crate::components::record_ghosts();
}

/// Stop recording and return every line drawn into the Inspector since
/// [`record_lines`], once each: its label's debug selector and whether it
/// stands in the keyed rows' columns.
#[cfg(test)]
pub(crate) fn drawn_lines() -> Vec<(String, bool)> {
    let mut lines = LINES.with_borrow_mut(|lines| lines.take().unwrap_or_default());
    lines.sort();
    lines.dedup();
    lines
}

/// Stop recording and return every choice row drawn into the Inspector since
/// [`record_lines`], once each.
#[cfg(test)]
pub(crate) fn drawn_choices() -> Vec<DrawnChoice> {
    let mut choices = CHOICES.with_borrow_mut(|choices| choices.take().unwrap_or_default());
    choices.sort();
    choices.dedup();
    choices
}

/// One numeric row.
///
/// The value is a scrubbable readout: a drag moves it, a click opens it as
/// text. `write` moves the value by a delta in the units `readout` prints, from
/// the state the gesture started on. `keys` draws the row's stopwatch and time
/// area, or the columns of a row that has none. The row reports its own state:
/// it fades under the pointer, its fill travels to a new value instead of
/// appearing at it, and its readout leads while the value is still moving.
#[allow(clippy::too_many_arguments)]
pub fn stepper_row(
    theme: &Theme,
    state: &gpui::Entity<AppState>,
    key: motion::MotionKey,
    label: impl Into<SharedString>,
    readout: Readout,
    travel: Option<Travel>,
    keys: Keys,
    write: impl Fn(&mut AppState, f32) -> Result<(), EngineError> + 'static,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let label = label.into();
    let hovered = motion::hover_t(key);
    let watch = match &keys {
        Keys::Watch(watch) => Some(watch),
        Keys::Unkeyed(_) => None,
    };
    let fault = watch
        .and_then(|watch| watch.driver.as_ref())
        .and_then(|line| line.fault.clone());
    let driver_line = watch.and_then(|watch| watch.driver.clone().map(|line| (line, watch.param)));
    let pick = watch
        .filter(|watch| watch.pick_target)
        .map(|watch| watch.param);

    // One eased value feeds both the fill and the readout, so a bar that has
    // arrived cannot be drawn beside a number that says it has not.
    let (fill, lead) = match travel {
        Some(travel) => {
            let eased = glide::glide(key, travel.value, presets::RESIZE, false);
            let remaining = if travel.step > 0.0 {
                ((eased - travel.value).abs() / travel.step).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let fill = travel.range.and_then(|(min, max)| {
                let span = max - min;
                (span > 0.0).then(|| ((eased - min) / span).clamp(0.0, 1.0))
            });
            (fill, remaining)
        }
        None => (None, 0.0),
    };

    // A row with a time area stays on one line, so its strip starts where the
    // strips of the rows above and below it start and their keyframes line up
    // in time. Every such row sizes its label from `LABEL_RESERVE` rather than
    // from its text, so the strips share one column whatever each label reads.
    // In a tight frame the strip narrows first, is left blank below its
    // minimum, and the label ends in an ellipsis, with the full name in a
    // tooltip, only once the strip has no width left. A row that cannot be
    // keyed among rows that can stands in their columns, with blanks where
    // their stopwatch, navigator and time area stand. A row with no time area
    // too narrow for both, a mixer strip's, moves the readout under the label
    // rather than breaking the label mid-word.
    let column = keys.column(&label);
    let value = value_cell(column.as_ref(), &label)
        .id(key.element_id("insp-value"))
        .when_some(fault.clone(), |el, fault| {
            el.with_tip(crate::tooltip::text_builder(fault))
        })
        .child(value_readout::readout(
            theme,
            state,
            ScrubTarget {
                key,
                value: readout.value,
                unit: readout.unit,
                kind: readout.kind,
                write: std::rc::Rc::new(write),
            },
            readout.text,
            Look {
                size: Theme::TEXT_UI,
                // Leading while the value is still traveling is what makes a
                // change visible without restarting an animation: a value that
                // keeps changing stays lit until it stops, rather than
                // strobing once per frame. A failed driver is red instead.
                ink: if fault.is_some() {
                    theme.danger
                } else {
                    motion::mix(theme.value, theme.accent, lead)
                },
            },
            cx,
        ));
    let head = line(
        key,
        label,
        column.as_ref(),
        affordances(theme, key, keys, cx),
        motion::mix(theme.text_muted, theme.text, hovered),
        value,
    );

    let mut row = div()
        .id(key.element_id("insp-row"))
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XXS))
        .px(px(Theme::SPACE_XS))
        .py(px(Theme::SPACE_XS))
        .rounded(px(Theme::CONTROL_RADIUS))
        .bg(motion::hover_blend(
            key,
            theme.wash(0.0),
            theme.glass_hover(),
        ))
        .on_hover(motion::hover_listener(key))
        .when(pick.is_some(), |el| {
            el.cursor(CursorStyle::PointingHand)
                .border_1()
                .border_color(theme.accent)
        })
        .child(head);
    if let Some(fill) = fill {
        row = row.child(track(theme, fill, hovered));
    }
    if let Some((line, param)) = driver_line {
        row = row.child(driver_source(theme, key, param, line, cx));
    }
    // A waiting pick whip takes the press on the row as a reference to the
    // property rather than an edit of it.
    row.on_mouse_down(
        MouseButton::Left,
        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
            let Some(param) = pick else {
                return;
            };
            if !this.state.read(cx).driver_picking() {
                return;
            }
            this.state.update(cx, |state, _| {
                if let Err(err) = state.pick_driver_input(param) {
                    state.report_error(&err);
                }
            });
            cx.notify();
        }),
    )
}

/// One line of a row: the stopwatch, the label, `value` in the cell
/// [`value_cell`] drew for `column`, then the keyframe navigator, the reset
/// button and the time area, or the blanks drawn in their place. A line in the
/// keyed rows' columns stays on one row; a free line moves its value under the
/// label rather than breaking the label mid-word.
fn line(
    key: motion::MotionKey,
    label: SharedString,
    column: Option<&Column>,
    Affordances {
        dot,
        controls,
        strip,
    }: Affordances,
    ink: gpui::Hsla,
    value: impl IntoElement,
) -> gpui::Div {
    #[cfg(test)]
    if IN_INSPECTOR.get() {
        LINES.with_borrow_mut(|lines| {
            if let Some(lines) = lines {
                lines.push((cell_selector(column, &label, "label"), column.is_some()));
            }
        });
    }
    div()
        .flex()
        .flex_row()
        .when(column.is_none(), |line| line.wraps())
        .items_center()
        .gap(px(Theme::SPACE_XS))
        .children(dot)
        .child(row_label(key, label, column, ink))
        .child(value)
        .children(controls)
        .children(strip)
}

/// The driver's source under a driven row: a click opens it for editing, and
/// while it is open the pick whip beside it waits for a property row.
fn driver_source(
    theme: &Theme,
    key: motion::MotionKey,
    param: ClipParam,
    line: DriverLine,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let ink = if line.fault.is_some() {
        theme.danger
    } else {
        theme.text_muted
    };
    let source = div()
        .id(key.with("driver").element_id("insp-driver"))
        .debug_selector(move || format!("insp-driver-{param:?}"))
        .flex_auto()
        .min_w_0()
        .px(px(Theme::SPACE_XS))
        .py(px(Theme::SPACE_XXS))
        .rounded(px(Theme::CONTROL_RADIUS))
        .font_family(theme.font_mono.clone())
        .text_size(Theme::TEXT_TITLE)
        .text_color(ink)
        .cursor(CursorStyle::IBeam)
        .when(line.editing, |el| {
            el.bg(theme.input_bg).border_1().border_color(theme.accent)
        })
        .when_some(line.fault.clone(), |el, fault| {
            el.with_tip(crate::tooltip::text_builder(fault))
        })
        .when_else(
            line.editing,
            |el| {
                el.flex()
                    .items_center()
                    .child(crate::components::typed_line(line.source.clone(), true))
                    .child(crate::components::caret_mark(Theme::TEXT_TITLE, 1.0, theme))
            },
            |el| el.child(line.source.clone()),
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                // The row underneath nudges on the same button.
                cx.stop_propagation();
                this.state
                    .update(cx, |state, _| state.open_driver_draft(param));
                cx.notify();
            }),
        );
    let whip = line.editing.then(|| {
        crate::components::ghost_button(("param-whip", key.with("whip")), "@", theme)
            .when(line.picking, |el| {
                el.bg(theme.accent).text_color(theme.text)
            })
            .when_some(
                crate::tooltip::builder("inspector-driver-whip"),
                |el, tip| el.with_tip(tip),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    this.state.update(cx, |state, _| state.toggle_driver_pick());
                    cx.notify();
                }),
            )
    });
    div()
        .flex()
        .flex_row()
        .items_start()
        .gap(px(Theme::SPACE_XS))
        .child(source)
        .children(whip)
}

/// Where the value sits in its range: a rail, the part of it the value covers,
/// and the thumb where the two meet.
///
/// `hovered` brightens all three together rather than only the thumb, because
/// a lit thumb on a dim rail reads as a different control.
fn track(theme: &Theme, fill: f32, hovered: f32) -> impl IntoElement {
    let rail = motion::mix(theme.border, theme.border_strong, hovered);
    let lit = motion::mix(theme.accent, theme.accent_strong, hovered);
    let mid = (THUMB - TRACK_H) / 2.0;
    div()
        .relative()
        .w_full()
        .h(px(THUMB))
        .child(
            div()
                .absolute()
                .top(px(mid))
                .w_full()
                .h(px(TRACK_H))
                .rounded(px(TRACK_H / 2.0))
                .bg(rail),
        )
        .child(
            div()
                .absolute()
                .top(px(mid))
                .w(relative(fill))
                .h(px(TRACK_H))
                .rounded(px(TRACK_H / 2.0))
                .bg(lit),
        )
        .child(
            div()
                .absolute()
                .top(px(0.))
                .left(relative(fill))
                // Centered on the fill's edge rather than starting at it, so
                // the thumb still reads as "here" at both ends of the range.
                .ml(px(-THUMB / 2.0))
                .size(px(THUMB))
                .rounded(px(THUMB / 2.0))
                .bg(lit),
        )
}

/// A row's label. On a row in the keyed rows' columns, `column` is set: the
/// label's basis is `LABEL_RESERVE` and it takes any room the row has past
/// that, and it shrinks at `LABEL_YIELD`, so the strip has no width left
/// before the label ends in an ellipsis, with the full name in a tooltip.
/// Elsewhere its basis is its unwrapped width, so the line breaks before the
/// label is squeezed, and at zero minimum a label wider than the row wraps at
/// the width it is given.
fn row_label(
    key: motion::MotionKey,
    label: SharedString,
    column: Option<&Column>,
    ink: gpui::Hsla,
) -> impl IntoElement {
    div()
        .id(key.element_id("insp-label"))
        .debug_selector(|| cell_selector(column, &label, "label"))
        .map(|el| match column {
            Some(_) => el
                .flex_basis(px(LABEL_RESERVE))
                .flex_grow(1.0)
                .flex_shrink(LABEL_YIELD)
                .min_w_0()
                .truncate()
                .with_tip(crate::tooltip::text_builder(label.clone())),
            None => el.flex_auto().min_w_0(),
        })
        .text_size(Theme::TEXT_UI)
        .text_color(ink)
        .child(label)
}

/// The cell a row's value is drawn in, right of the `label`. On a row in the
/// keyed rows' columns, `column` is set and the cell is the value column:
/// `VALUE_MIN` wide, narrowing at `VALUE_YIELD`, with the value at its right
/// edge.
fn value_cell(column: Option<&Column>, label: &SharedString) -> gpui::Div {
    let cell = div()
        .ml_auto()
        .debug_selector(|| cell_selector(column, label, "value"));
    match column {
        Some(_) => cell
            .flex()
            .justify_end()
            .flex_grow_0()
            .flex_shrink(VALUE_YIELD)
            .flex_basis(px(VALUE_MIN)),
        None => cell.flex_none(),
    }
}

/// A color's bytes as a paint color.
pub fn swatch(rgba: [u8; 4]) -> gpui::Hsla {
    gpui::Hsla::from(gpui::Rgba {
        r: rgba[0] as f32 / 255.0,
        g: rgba[1] as f32 / 255.0,
        b: rgba[2] as f32 / 255.0,
        a: rgba[3] as f32 / 255.0,
    })
}

/// The palette name of a color, or its hex when it is not one of the presets,
/// so a value written by the agent or an older project still reads as
/// something. An opaque color prints six digits, `#ff0000`, and fits the value
/// column; a translucent one adds its alpha, `#ff000080`.
pub fn color_name(rgba: [u8; 4]) -> SharedString {
    crate::app::clip_effects::PARAM_COLORS
        .iter()
        .find(|(_, _, preset)| *preset == rgba)
        .map(|(_, label, _)| SharedString::new_static(label))
        .unwrap_or_else(|| {
            let [r, g, b, a] = rgba;
            SharedString::from(if a == u8::MAX {
                format!("#{r:02x}{g:02x}{b:02x}")
            } else {
                format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
            })
        })
}

pub fn kind_label(kind: UiClipKind) -> &'static str {
    match kind {
        UiClipKind::Media => "Media",
        UiClipKind::Audio => "Audio",
        UiClipKind::Text => "Text",
        UiClipKind::Shape => "Shape",
        UiClipKind::Generator => "Generator",
    }
}
