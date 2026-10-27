//! Shared chrome widgets used by every panel.

use crate::tooltip::WithTip as _;
use gpui::motion::presets;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    CursorStyle, Div, InteractiveElement, IntoElement, ParentElement, Pixels, SharedString,
    Stateful, StatefulInteractiveElement, Styled, div, px,
};

use crate::motion;
use crate::motion::glide;
use crate::theme::Theme;

pub mod drawn;
pub use drawn::{Picture, Wraps, picture};

/// A disclosure chevron, sized to sit beside 12px row text.
const CHEVRON_PX: f32 = 12.;

/// How wide a caret's bar is drawn.
const CARET_WIDTH: f32 = 1.5;

/// How far a caret reaches past the size of its text: the room above the
/// capitals and below the baseline.
const CARET_OVERHANG: f32 = 4.0;

/// A quarter turn: shut points the way the writing goes, open points down.
const QUARTER: f32 = std::f32::consts::FRAC_PI_2;

/// A disclosure chevron that turns between shut and open.
///
/// One glyph and one rotation rather than two glyphs. A swap at 12px reads as a
/// flicker, and the turn is what shows the click landed on the row rather than
/// beside it. The rotation eases from wherever the glyph currently
/// points, so a row toggled twice quickly turns back from part way instead of
/// restarting.
pub fn chevron(
    key: motion::MotionKey,
    open: bool,
    color: gpui::Hsla,
    reduced: bool,
) -> impl IntoElement {
    let turn = glide::glide(
        key,
        if open { 0.0 } else { -QUARTER },
        presets::CHEVRON,
        reduced,
    );
    crate::icons::icon(crate::icons::CHEVRON_DOWN)
        .size(px(CHEVRON_PX))
        .text_color(color)
        .with_transformation(gpui::Transformation::rotate(gpui::radians(turn)))
}

/// A solid button: an action, such as Done or Export. One control tall, so it
/// stands level with a [`text_box`] beside it. Under the pointer the plate
/// dims a step and the label keeps its color, so the button reads as the same
/// control while it lights.
pub fn button(
    id: impl Into<motion::ControlId>,
    label: impl Into<SharedString>,
    theme: &Theme,
) -> Stateful<Div> {
    let id = id.into();
    let key = id.motion("button");
    let lit = motion::hover_t(key);
    let pressed = theme.solid.opacity(0.76);
    div()
        .id(id.element("button"))
        // Press reads as a settle: the fill deepens and the control dims.
        .active(move |style| style.bg(pressed).opacity(0.92))
        .h(px(Theme::CONTROL_HEIGHT))
        .flex()
        .items_center()
        .px(px(Theme::SPACE_SM))
        .rounded(px(Theme::CONTROL_RADIUS))
        .bg(motion::mix(theme.solid, theme.solid.opacity(0.88), lit))
        .text_color(theme.on_solid)
        .text_size(Theme::TEXT_UI)
        .cursor(CursorStyle::PointingHand)
        .on_hover(motion::hover_listener(key))
        .child(label.into())
}

/// A ghost button: a secondary action, such as Cancel or Measure. A border on
/// no fill at rest, so it reads as a button beside a solid one and apart from a
/// chip, which marks a choice with a fill. One control tall, its label
/// [`GHOST_INSET`] inside its outer edge. Under the pointer the border and the
/// label brighten and a wash fills it.
pub fn ghost_button(
    id: impl Into<motion::ControlId>,
    label: impl Into<SharedString>,
    theme: &Theme,
) -> Stateful<Div> {
    let (id, label) = (id.into(), label.into());
    let lit = motion::hover_t(id.motion("ghost"));
    let button = ghost_frame(id, theme)
        .border_1()
        .border_color(motion::mix(theme.border, theme.border_strong, lit))
        .px(px(GHOST_INSET.0 - GHOST_BORDER))
        .py(px(GHOST_INSET.1 - GHOST_BORDER));
    #[cfg(test)]
    let button = match noted(
        "ghost",
        id,
        Drawn::Labelled {
            label: label.clone(),
            inset: GHOST_INSET.0,
            size: Theme::TEXT_UI,
        },
    ) {
        Some(selector) => button.debug_selector(move || selector),
        None => button,
    };
    button.child(label)
}

#[cfg(test)]
thread_local! {
    /// The controls built since [`record_controls`] while a
    /// [`NotingControls`] stands.
    static CONTROLS: std::cell::RefCell<Option<ControlRecord>> =
        const { std::cell::RefCell::new(None) };
    /// Whether a [`NotingControls`] stands.
    static NOTING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// The kind and id of each control built since the standing
    /// [`NotingControls`] began.
    static BUILT: std::cell::RefCell<std::collections::HashSet<(&'static str, motion::ControlId)>> =
        std::cell::RefCell::new(std::collections::HashSet::new());
}

/// What [`record_controls`] has recorded.
#[cfg(test)]
#[derive(Default)]
struct ControlRecord {
    /// Each control built.
    built: Vec<DrawnControl>,
    /// The debug selector of each control built with the kind and id of an
    /// earlier control under the same [`NotingControls`].
    shared: Vec<String>,
}

/// What a recorded control draws in its frame.
#[cfg(test)]
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Drawn {
    /// A ghost button or a chip: `label` at text size `size`, `inset` inside
    /// each side of the frame.
    Labelled {
        label: SharedString,
        inset: f32,
        size: Pixels,
    },
    /// A [`switch`], [`SWITCH_WIDTH`] wide.
    Switch,
}

/// A control built while a [`NotingControls`] stands: the debug selector the
/// control draws under, and what it draws.
#[cfg(test)]
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DrawnControl {
    pub selector: String,
    pub drawn: Drawn,
}

/// The ghost buttons, chips and switches built until this drops are the ones
/// [`drawn_controls`] returns, and no other surface's. Two of one kind built
/// with one id share one hover, so each lights whenever the pointer is on
/// either.
#[cfg(test)]
pub(crate) struct NotingControls(());

#[cfg(test)]
impl NotingControls {
    pub(crate) fn start() -> Self {
        NOTING.set(true);
        BUILT.with_borrow_mut(std::collections::HashSet::clear);
        Self(())
    }
}

#[cfg(test)]
impl Drop for NotingControls {
    fn drop(&mut self) {
        NOTING.set(false);
    }
}

/// Start a fresh record of the controls built while a [`NotingControls`]
/// stands.
#[cfg(test)]
pub(crate) fn record_controls() {
    CONTROLS.set(Some(ControlRecord::default()));
}

/// Stop recording and return every control built since [`record_controls`],
/// once each, then the debug selector of each one built with the kind and id
/// of another under the same [`NotingControls`].
#[cfg(test)]
pub(crate) fn drawn_controls() -> (Vec<DrawnControl>, Vec<String>) {
    let ControlRecord {
        mut built,
        mut shared,
    } = CONTROLS.with_borrow_mut(|record| record.take().unwrap_or_default());
    built.sort_by(|a, b| a.selector.cmp(&b.selector));
    built.dedup_by(|a, b| a.selector == b.selector);
    shared.sort();
    shared.dedup();
    (built, shared)
}

/// The debug selector of the `kind` of control `id` drawing `drawn` while a
/// [`NotingControls`] stands, after recording it while a record stands. A
/// control keeps its selector after [`drawn_controls`] stops the record, so a
/// later frame still draws it where a test can find it.
#[cfg(test)]
fn noted(kind: &'static str, id: motion::ControlId, drawn: Drawn) -> Option<String> {
    if !NOTING.get() {
        return None;
    }
    let selector = control_selector(kind, id, &drawn);
    CONTROLS.with_borrow_mut(|record| {
        if let Some(record) = record.as_mut() {
            if !BUILT.with_borrow_mut(|built| built.insert((kind, id))) {
                record.shared.push(selector.clone());
            }
            record.built.push(DrawnControl {
                selector: selector.clone(),
                drawn,
            });
        }
    });
    Some(selector)
}

/// The debug selector a `kind` of control `id` drawing `drawn` draws under
/// while a [`NotingControls`] stands: `{kind}-{id:?}`, then `-{label}` for a
/// labelled one.
#[cfg(test)]
pub(crate) fn control_selector(kind: &str, id: motion::ControlId, drawn: &Drawn) -> String {
    match drawn {
        Drawn::Labelled { label, .. } => format!("{kind}-{id:?}-{label}"),
        Drawn::Switch => format!("{kind}-{id:?}"),
    }
}

/// A [`ghost_button`] that opens and shuts the rows under it: the disclosure
/// [`chevron`], turned for `open`, then the label.
pub fn disclosure_button(
    id: impl Into<motion::ControlId>,
    label: impl Into<SharedString>,
    open: bool,
    theme: &Theme,
    reduced: bool,
) -> Stateful<Div> {
    let id = id.into();
    let ink = motion::mix(
        theme.text_muted,
        theme.text,
        motion::hover_t(id.motion("ghost")),
    );
    ghost_frame(id, theme)
        .flex()
        .items_center()
        .gap(px(Theme::SPACE_XS))
        .child(chevron(id.motion("chevron"), open, ink, reduced))
        .child(label.into())
}

/// How far a [`ghost_button`]'s label stands inside its outer edge: across,
/// then down. A [`disclosure_button`] draws no frame at rest, so a layout that
/// lines its label up with the text above it hangs the frame out by this much.
pub const GHOST_INSET: (f32, f32) = (Theme::SPACE_SM, Theme::SPACE_XS);

/// The width of a [`ghost_button`]'s border, inside [`GHOST_INSET`].
const GHOST_BORDER: f32 = 1.0;

/// The frame of a [`ghost_button`] and a [`disclosure_button`]: its padding,
/// its wash and text color, which fade in under the pointer, and its press
/// dim, with nothing in it. The label's line fills the frame to
/// [`Theme::CONTROL_HEIGHT`], so a ghost stands level with a solid button, a
/// chip and a text box beside it.
fn ghost_frame(id: motion::ControlId, theme: &Theme) -> Stateful<Div> {
    let key = id.motion("ghost");
    let lit = motion::hover_t(key);
    let pressed = theme.element_active;
    div()
        .id(id.element("ghost"))
        .active(move |style| style.bg(pressed).opacity(0.92))
        .px(px(GHOST_INSET.0))
        .py(px(GHOST_INSET.1))
        .line_height(px(Theme::CONTROL_HEIGHT - 2.0 * GHOST_INSET.1))
        .rounded(px(Theme::CONTROL_RADIUS))
        .bg(motion::mix(theme.wash(0.0), theme.element_hover, lit))
        .text_color(motion::mix(theme.text_muted, theme.text, lit))
        .text_size(Theme::TEXT_UI)
        .cursor(CursorStyle::PointingHand)
        .on_hover(motion::hover_listener(key))
}

/// How much room a chip row is given.
///
/// Two sizes, because a chip row is one of two things: the panel's own filter,
/// which is the first thing read in it, or a secondary row beside a title or a
/// count. A size sets the padding, the text and the height, so the two rows
/// cannot drift apart in separate files. Both rest at muted text: faint text
/// marks a disabled control, and an inactive chip is a choice that is
/// available.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChipSize {
    /// The panel's primary filter row.
    Regular,
    /// A row that sits beside a title or a count.
    Compact,
}

impl ChipSize {
    /// Every size, so a row of specimens or a size-aware caller enumerates them
    /// instead of naming the two it knows about.
    pub const ALL: [Self; 2] = [Self::Regular, Self::Compact];

    /// The side and the top padding around a chip's label.
    pub fn pad(self) -> (f32, f32) {
        match self {
            Self::Regular => (Theme::SPACE_SM, Theme::SPACE_XS),
            Self::Compact => (Theme::SPACE_XS, 2.),
        }
    }

    /// A chip's height: a regular chip is one control row, level with a text
    /// box and a button beside it; a compact one is an icon control's row.
    pub fn height(self) -> f32 {
        match self {
            Self::Regular => Theme::CONTROL_HEIGHT,
            Self::Compact => Theme::ICON_HIT_ROW,
        }
    }

    fn text_size(self) -> gpui::Pixels {
        match self {
            Self::Regular => Theme::TEXT_UI,
            Self::Compact => Theme::TEXT_SMALL,
        }
    }
}

/// A chip: the smallest control that says which one of a set is in force.
///
/// A row of these replaces a dropdown wherever the set is short enough to show
/// whole, so the choices are visible without opening them. The active
/// chip carries a faint wash and full-strength text; the others sit at their
/// size's color on nothing, and light towards the active look on hover so the
/// row answers the pointer before the click.
///
/// Drawing only. The caller attaches the click, because only it knows which
/// verb the choice runs.
pub fn chip(
    id: impl Into<motion::ControlId>,
    label: impl Into<SharedString>,
    active: bool,
    size: ChipSize,
    theme: &Theme,
) -> Stateful<Div> {
    let label: SharedString = label.into();
    chip_frame(id, &label, active, size, theme).child(div().min_w_0().truncate().child(label))
}

/// A [`chip`] without its label: the box, the wash, the text style, and the
/// hover. `label` is the name the chip is recorded under in tests.
fn chip_frame(
    id: impl Into<motion::ControlId>,
    #[cfg_attr(not(test), expect(unused_variables))] label: &SharedString,
    active: bool,
    size: ChipSize,
    theme: &Theme,
) -> Stateful<Div> {
    let id = id.into();
    let key = id.motion("chip");
    let lit = motion::hover_t(key);
    let (pad_x, pad_y) = size.pad();
    let chip = div()
        .id(id.element("chip"))
        .flex()
        .items_center()
        .px(px(pad_x))
        .py(px(pad_y))
        .line_height(px(size.height() - 2.0 * pad_y))
        .rounded(px(Theme::CONTROL_RADIUS))
        .bg(motion::mix(
            theme.wash(if active { 0.11 } else { 0.0 }),
            theme.element_hover,
            lit,
        ))
        .text_size(size.text_size())
        .text_color(motion::mix(
            if active { theme.text } else { theme.text_muted },
            theme.text,
            lit,
        ))
        .cursor(CursorStyle::PointingHand)
        // Press feedback: dim while held, since GPUI Div styles expose no scale.
        .active(move |style| style.opacity(0.92))
        .on_hover(motion::hover_listener(key));
    #[cfg(test)]
    let chip = match noted(
        "chip",
        id,
        Drawn::Labelled {
            label: label.clone(),
            inset: pad_x,
            size: size.text_size(),
        },
    ) {
        Some(selector) => chip.debug_selector(move || selector),
        None => chip,
    };
    chip
}

/// A [`chip`] that opens a list of choices: the chosen one's name, then the
/// disclosure glyph pointing down. The list's rows are the caller's.
pub fn list_chip(
    id: impl Into<motion::ControlId>,
    label: impl Into<SharedString>,
    open: bool,
    theme: &Theme,
) -> Stateful<Div> {
    chip(id, label, open, ChipSize::Compact, theme)
        .gap(px(Theme::SPACE_XXS))
        .child(chevron_down())
}

/// A [`list_chip`] held at the width of `widest`, the longest choice it can
/// state, so the controls beside it stay put as its choice changes. Figures
/// are tabular, so one run of figures is as wide as any other of its length.
pub fn sized_list_chip(
    id: impl Into<motion::ControlId>,
    label: impl Into<SharedString>,
    widest: &'static str,
    open: bool,
    theme: &Theme,
) -> Stateful<Div> {
    let label: SharedString = label.into();
    chip_frame(id, &label, open, ChipSize::Compact, theme)
        .font_features(crate::theme::tabular_figures())
        .gap(px(Theme::SPACE_XXS))
        .child(
            // The hidden second line is laid out and never drawn: it sets the
            // column's width and adds no height.
            div()
                .flex()
                .flex_col()
                .child(label)
                .child(div().h_0().invisible().child(widest)),
        )
        .child(chevron_down())
}

/// The disclosure glyph a [`list_chip`] ends with.
fn chevron_down() -> crate::icons::Icon {
    crate::icons::icon(crate::icons::CHEVRON_DOWN)
        .flex_none()
        .size(px(CHEVRON_PX))
}

/// How wide a [`switch`]'s track is.
pub const SWITCH_WIDTH: f32 = 28.0;

/// How tall a [`switch`]'s track is. The knob is a circle this tall less
/// [`SWITCH_KNOB_INSET`] on each side.
pub const SWITCH_HEIGHT: f32 = 16.0;

/// How far a [`switch`]'s knob stands inside its track.
pub const SWITCH_KNOB_INSET: f32 = 2.0;

/// A switch: a setting that is on or off. A track with a knob, the knob at
/// the track's right end on the accent while `on` is `Some(true)`, at its left
/// end while `Some(false)`, and in its middle while `None`, when the selection
/// holds both. The knob travels between ends at the state beat, and the track
/// brightens under the pointer. Drawing only: the caller attaches the click.
pub fn switch(id: impl Into<motion::ControlId>, on: Option<bool>, theme: &Theme) -> Stateful<Div> {
    let id = id.into();
    let key = id.motion("switch");
    let lit = motion::hover_t(key);
    let at = match on {
        Some(true) => 1.0,
        Some(false) => 0.0,
        None => 0.5,
    };
    let travel = glide::glide(id.motion("switch-knob"), at, glide::STATE, false);
    let fill = glide::glide(
        id.motion("switch-fill"),
        if on == Some(true) { 1.0 } else { 0.0 },
        glide::STATE,
        false,
    );
    let rest = motion::mix(theme.element_active, theme.border_strong, lit);
    let knob = SWITCH_HEIGHT - 2.0 * SWITCH_KNOB_INSET;
    let track = div()
        .id(id.element("switch"))
        .relative()
        .flex_none()
        .w(px(SWITCH_WIDTH))
        .h(px(SWITCH_HEIGHT))
        .rounded_full()
        .bg(motion::mix(rest, theme.accent, fill))
        .cursor(CursorStyle::PointingHand)
        .active(|style| style.opacity(0.92))
        .on_hover(motion::hover_listener(key))
        .child(
            div()
                .absolute()
                .top(px(SWITCH_KNOB_INSET))
                .left(px(
                    SWITCH_KNOB_INSET + (SWITCH_WIDTH - SWITCH_HEIGHT) * travel
                ))
                .size(px(knob))
                .rounded_full()
                .bg(motion::mix(theme.text_muted, theme.on_accent, fill)),
        );
    #[cfg(test)]
    let track = match noted("switch", id, Drawn::Switch) {
        Some(selector) => track.debug_selector(move || selector),
        None => track,
    };
    track
}

/// One tab of a tab row: muted text at rest, and full-strength text over the
/// accent underline while it is the open one, as a dock tab is. The underline is
/// drawn over the tab's bottom edge and takes no layout, so an open label
/// stands on the same center line as the others. A tab with no room for its
/// label ends it in an ellipsis.
///
/// The tab fills the height of its row, so a row of these is as tall as the
/// row the caller sets. Drawing only: the caller attaches the click.
pub fn underline_tab(
    id: impl Into<motion::ControlId>,
    label: impl Into<SharedString>,
    active: bool,
    theme: &Theme,
) -> Stateful<Div> {
    let id = id.into();
    let key = id.motion("tab");
    let lit = motion::hover_t(key);
    div()
        .id(id.element("tab"))
        .relative()
        .min_w_0()
        .h_full()
        .flex()
        .items_center()
        .gap(px(Theme::SPACE_XS))
        .px(px(Theme::SPACE_SM))
        .bg(motion::mix(theme.wash(0.0), theme.element_hover, lit))
        .text_size(Theme::TEXT_UI)
        .text_color(motion::mix(
            if active { theme.text } else { theme.text_muted },
            theme.text,
            lit,
        ))
        .cursor(CursorStyle::PointingHand)
        .active(|style| style.opacity(0.92))
        .on_hover(motion::hover_listener(key))
        .child(div().min_w_0().truncate().child(label.into()))
        .when(active, |el| {
            el.child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .h(px(Theme::TAB_UNDERLINE))
                    .bg(theme.accent),
            )
        })
}

/// A tab row for static surface catalogs and galleries: one [`underline_tab`]
/// per label, the open ones underlined.
///
/// The label and its position form the identity, so duplicate labels in one
/// row animate independently without allocating an id on each draw.
pub fn tab_bar(theme: &Theme, tabs: &[(&'static str, bool)]) -> Div {
    div()
        .h(px(Theme::TAB_ROW_HEIGHT))
        .flex()
        .gap(px(Theme::SPACE_XXS))
        .children(tabs.iter().enumerate().map(|(index, (label, active))| {
            let key = motion::MotionKey::new("tab", format_args!("{label}-{index}"));
            underline_tab(key, *label, *active, theme).flex_none()
        }))
}

/// The banding fill for row `index` of a long list.
///
/// Every other row sits on a faint recess, which is what lets the eye carry a
/// timecode on the left across to a control on the right. Zero for even rows, so
/// a short list looks like no banding at all.
pub fn band_fill(theme: &Theme, index: usize) -> gpui::Hsla {
    if index % 2 == 1 {
        theme.band
    } else {
        theme.wash(0.0)
    }
}

/// The lead column of a menu row: the check mark while `checked` holds,
/// otherwise the icon at the asset path `icon`, otherwise nothing.
///
/// Drawn on every row of a menu bar drop and of a context menu, so the
/// captions line up whether or not a row is a toggle or has an icon.
pub fn menu_lead(checked: bool, icon: Option<&'static str>, theme: &Theme) -> Div {
    div()
        .w(px(Theme::ICON_SIZE))
        .flex_none()
        .flex()
        .justify_center()
        .text_color(theme.text_muted)
        .children(match (checked, icon) {
            (true, _) => Some("✓".into_any_element()),
            (false, Some(path)) => Some(crate::icons::icon(path).into_any_element()),
            (false, None) => None,
        })
}

/// The label a form row starts with: `label` in a column `width` wide that
/// keeps its width, so the controls after the labels of a form start on one
/// edge.
pub fn row_label(theme: &Theme, label: impl Into<SharedString>, width: f32) -> Div {
    div()
        .flex_none()
        .w(px(width))
        .text_size(Theme::TEXT_UI)
        .text_color(theme.text_muted)
        .child(label.into())
}

/// How wide the label column of a panel form is: the widest label a Project
/// panel form draws, with room to spare.
pub const FORM_LABEL_WIDTH: f32 = 64.0;

/// One row of a panel form: `label` in the label column, then `controls` in
/// the rest of the row, `SPACE_XS` apart and wrapping onto further lines in a
/// narrow panel. The label stands on the center line of the first line of
/// controls. Drawn as `form-{label}`, with the controls as
/// `form-{label}-controls`.
pub fn form_row(
    theme: &Theme,
    label: &'static str,
    controls: impl IntoIterator<Item = gpui::AnyElement>,
) -> Div {
    div()
        .debug_selector(move || format!("form-{label}"))
        .flex()
        .items_start()
        .gap(px(Theme::SPACE_SM))
        .child(
            row_label(theme, label, FORM_LABEL_WIDTH)
                .h(px(Theme::CONTROL_HEIGHT))
                .flex()
                .items_center(),
        )
        .child(
            div()
                .debug_selector(move || format!("form-{label}-controls"))
                .flex_1()
                .min_w_0()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(Theme::SPACE_XS))
                .children(controls),
        )
}

/// A key-routed text box: the value, or the placeholder while it is empty, and
/// the caret at the insertion point while it holds focus.
///
/// Every text box in the app is drawn by this, so the accent border that means
/// "your keys go here" and the caret cannot be spelled differently in two
/// panels. The caller attaches the click that takes focus, since the field a
/// click focuses is the caller's. An empty box keeps its placeholder while it
/// holds focus, with the caret before it, so a clicked box still states what
/// it takes.
pub fn text_box(
    id: impl Into<motion::ControlId>,
    value: impl Into<SharedString>,
    placeholder: impl Into<SharedString>,
    focused: bool,
    theme: &Theme,
) -> Stateful<Div> {
    let value = value.into();
    let empty = value.is_empty();
    let caret = || {
        focused
            .then(|| caret_mark(Theme::TEXT_UI, 1.0, theme).debug_selector(|| "text-caret".into()))
    };
    field_frame(id, focused, theme)
        .text_color(if empty { theme.text_muted } else { theme.text })
        .children(if empty { caret() } else { None })
        .child(if empty {
            typed_line(placeholder, false)
        } else {
            typed_line(value, focused)
        })
        .children(if empty { None } else { caret() })
}

/// The text of a box that takes typed text, on one line that ends in an
/// ellipsis inside the box when it is wider than the box.
///
/// `typing` is true while the next typed character joins the end of `text`.
/// The ellipsis then leads instead, so that end stays in view beside the
/// caret, as a field scrolled to its caret shows it.
pub fn typed_line(text: impl Into<SharedString>, typing: bool) -> Div {
    let line = div().min_w_0().overflow_hidden().whitespace_nowrap();
    let line = if typing {
        line.text_ellipsis_start()
    } else {
        line.text_ellipsis()
    };
    line.child(text.into())
}

/// The caret of a box that takes typed text: a bar in the caret color at
/// `opacity`, as tall as a line of `size` text, standing on a mark of no
/// width so the glyphs on either side of it stay where the font sets them.
///
/// The text boxes, the rename bar, a value readout opened for typing and a
/// driver's expression draw this one mark. The rename bar passes the opacity of its
/// blink; the others hold it solid.
pub fn caret_mark(size: Pixels, opacity: f32, theme: &Theme) -> Div {
    div()
        .relative()
        .flex_none()
        .h(size + px(CARET_OVERHANG))
        .child(
            div()
                .absolute()
                .top_0()
                .left(px(-CARET_WIDTH / 2.0))
                .w(px(CARET_WIDTH))
                .h_full()
                .bg(theme.caret.opacity(opacity)),
        )
}

/// The frame of a [`text_box`]: its border, fill, padding and hover fade,
/// with no text. A field whose text is painted elsewhere, such as the
/// timeline timecode at rest, is this frame around a box of the text's size.
pub fn field_frame(
    id: impl Into<motion::ControlId>,
    focused: bool,
    theme: &Theme,
) -> Stateful<Div> {
    let id = id.into();
    let key = id.motion("field");
    let lit = motion::hover_t(key);
    // Focus outranks the pointer: an accent border means "your keys go here",
    // and fading it under the mouse would spell that two ways.
    let border = if focused {
        theme.accent
    } else {
        motion::mix(theme.border, theme.border_strong, lit)
    };
    div()
        .id(id.element("field"))
        .px(px(Theme::SPACE_SM))
        .min_h(px(Theme::CONTROL_HEIGHT))
        .min_w_0()
        .flex()
        .items_center()
        .rounded(px(Theme::CONTROL_RADIUS))
        .bg(motion::mix(
            theme.surface_raised,
            theme.surface_raised_hover,
            lit,
        ))
        .border_1()
        .border_color(border)
        .cursor(CursorStyle::IBeam)
        .text_size(Theme::TEXT_UI)
        .on_hover(motion::hover_listener(key))
}

/// How an icon control identifies itself: the element id it takes this frame,
/// and the name the tooltip table knows it by.
///
/// A control that exists once is a plain `"timeline-snap"`, which is both. A control drawn
/// once per row is `("inspector-fx-remove", index)`: each row needs its own
/// element id, and its own hover fade with it, while one tip names them all. A
/// control drawn once per model entity is `("nudge-multicam-back", key)`, where
/// the key already folds the entity ids the row is drawn for.
pub trait IconId {
    fn element_id(&self) -> gpui::ElementId;
    fn tip(&self) -> &'static str;
}

impl IconId for &'static str {
    fn element_id(&self) -> gpui::ElementId {
        gpui::ElementId::Name(SharedString::from(*self))
    }

    fn tip(&self) -> &'static str {
        self
    }
}

impl IconId for (&'static str, usize) {
    fn element_id(&self) -> gpui::ElementId {
        gpui::ElementId::named_usize(self.0, self.1)
    }

    fn tip(&self) -> &'static str {
        self.0
    }
}

impl IconId for (&'static str, motion::MotionKey) {
    fn element_id(&self) -> gpui::ElementId {
        self.1.element_id(self.0)
    }

    fn tip(&self) -> &'static str {
        self.0
    }
}

/// Square icon control: a 16 px glyph in a 24 px hit box (look.md Density).
/// A control inside a 22 px row sets `.size(px(Theme::ICON_HIT_ROW))`; the tool
/// bar sets `ICON_HIT_LARGE`.
///
/// Names itself on hover when [`crate::tooltip`] knows the id. A glyph is not a
/// name, so the tip is attached here rather than at each call site: every icon
/// control gets one from the same table. Background and glyph fade between rest
/// and hover, keyed by the element id, so one row of many lights on its own.
pub fn icon_button(id: impl IconId, path: &'static str, theme: &Theme) -> Stateful<Div> {
    let tip = id.tip();
    let id = id.element_id();
    let key = motion::MotionKey::new("icon", &id);
    let lit = motion::hover_t(key);
    let pressed = theme.element_active;
    div()
        .id(id)
        .active(move |style| style.bg(pressed).opacity(0.92))
        .flex_none()
        .size(px(Theme::ICON_HIT))
        .rounded(px(Theme::SPACE_XS))
        .flex()
        .items_center()
        .justify_center()
        .bg(motion::mix(theme.wash(0.0), theme.element_hover, lit))
        .text_color(motion::mix(theme.text_muted, theme.text, lit))
        .cursor(CursorStyle::PointingHand)
        .with_tip(crate::tooltip::deferred_builder(tip))
        .on_hover(motion::hover_listener(key))
        .child(crate::icons::icon(path))
}

/// An [`icon_button`] that reads lit while `on`: a toggle, or a mode such as a
/// pick whip that waits for the next click. The caller adds the press.
pub fn lit_icon(id: impl IconId, path: &'static str, on: bool, theme: &Theme) -> Stateful<Div> {
    let element = id.element_id();
    // The key `icon_button` fades on. It holds the element's hover listener,
    // and an element takes one, so the lit look reads that fade rather than
    // starting another.
    let key = motion::MotionKey::new("icon", &element);
    let (lit, pressed, text) = (theme.glass_selected_bg(), theme.element_active, theme.text);
    icon_button(id, path, theme)
        .debug_selector(move || element.to_string())
        .when(on, |el| {
            el.bg(motion::hover_blend(key, lit, pressed))
                .text_color(text)
        })
}

/// The room of an [`icon_button`] with nothing in it, for a row of icon
/// controls where one does not apply: the controls after it stand in the
/// columns the rows above and below draw them in.
pub fn icon_space() -> Div {
    div().flex_none().size(px(Theme::ICON_HIT))
}

/// A danger button: an action that discards or stops, such as Stop. A ghost's
/// geometry, one control tall, in the danger color.
pub fn danger_button(
    id: impl Into<motion::ControlId>,
    label: impl Into<SharedString>,
    theme: &Theme,
) -> Stateful<Div> {
    let id = id.into();
    let key = id.motion("danger");
    let lit = motion::hover_t(key);
    let pressed = theme.danger.opacity(0.24);
    div()
        .id(id.element("danger-button"))
        .active(move |style| style.bg(pressed).opacity(0.92))
        .px(px(GHOST_INSET.0 - GHOST_BORDER))
        .py(px(GHOST_INSET.1 - GHOST_BORDER))
        .line_height(px(Theme::CONTROL_HEIGHT - 2.0 * GHOST_INSET.1))
        .rounded(px(Theme::CONTROL_RADIUS))
        .border_1()
        .border_color(motion::mix(theme.danger, theme.danger_strong, lit))
        .bg(motion::mix(
            theme.wash(0.0),
            theme.danger.opacity(0.16),
            lit,
        ))
        .text_color(motion::mix(theme.danger, theme.danger_strong, lit))
        .text_size(Theme::TEXT_UI)
        .cursor(CursorStyle::PointingHand)
        .on_hover(motion::hover_listener(key))
        .child(label.into())
}

/// A label and its value in one row: the label in `text_muted`, the value in
/// `text` with tabular figures.
///
/// The value stands at the right edge beside its label while both fit, moves
/// to the next line at the right edge once they do not, and wraps at the row's
/// width when it is wider than the row, as a label wider than the row does.
pub fn readout(
    theme: &Theme,
    label: impl Into<SharedString>,
    value: impl Into<SharedString>,
) -> Div {
    readout_tinted(theme, label, value, theme.text)
}

/// [`readout`] with its value in `tint`, for a value whose color is its
/// verdict: `warning` for a fact to look at, `text_faint` for an empty answer.
pub fn readout_tinted(
    theme: &Theme,
    label: impl Into<SharedString>,
    value: impl Into<SharedString>,
    tint: gpui::Hsla,
) -> Div {
    div()
        .flex()
        .flex_row()
        .flex_wrap()
        .gap_x(px(Theme::SPACE_SM))
        .text_size(Theme::TEXT_UI)
        .child(
            div()
                .max_w_full()
                .text_color(theme.text_muted)
                .child(label.into()),
        )
        .child(
            div()
                .ml_auto()
                .max_w_full()
                .text_color(tint)
                .font_features(crate::theme::tabular_figures())
                .child(value.into()),
        )
}

/// The heading over a group of rows, such as a group of [`readout`] rows: full
/// text in medium weight, so it reads as a heading over the rows' labels and
/// not as a disabled control, which faint text marks.
pub fn section_label(theme: &Theme, title: impl Into<SharedString>) -> Div {
    div()
        .pt(px(Theme::SPACE_SM))
        .text_size(Theme::TEXT_UI)
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(theme.text)
        .child(title.into())
}

/// A [`section_label`] whose line ends in one `action`, such as the group's
/// reset, over the edge the rows' value areas end on.
pub fn section_heading(
    theme: &Theme,
    title: impl Into<SharedString>,
    action: impl IntoElement,
) -> Div {
    div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .pt(px(Theme::SPACE_SM))
        .pr(px(Theme::SPACE_XS))
        .child(section_label(theme, title).pt_0())
        .child(action)
}

/// The line a panel draws in place of content it does not have, centered on
/// both axes in the space that content fills. An action the line offers is
/// added below it with `.child`.
///
/// The caller's frame lays it out as a flex item: it grows into the height a
/// column frame has left, and into the width a row frame has left. Drawn as
/// `empty-state-{id}`, with its line as `empty-state-{id}-line`.
pub fn empty_state(id: &'static str, text: impl Into<SharedString>, theme: &Theme) -> Div {
    div()
        .debug_selector(move || format!("empty-state-{id}"))
        .flex_1()
        .w_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(Theme::SPACE_SM))
        .p(px(Theme::SPACE_MD))
        .child(
            div()
                .debug_selector(move || format!("empty-state-{id}-line"))
                .max_w_full()
                .text_center()
                .text_size(Theme::TEXT_UI)
                .text_color(theme.text_faint)
                .child(text.into()),
        )
}

/// The line a list under a form draws in place of rows it does not have,
/// where its first row would stand: on the column's edge, under the form, as
/// a status line under a panel's controls is. An action the line offers is
/// added below it with `.child`. Drawn as `empty-state-{id}`, with its line as
/// `empty-state-{id}-line`, as an [`empty_state`] is.
pub fn list_empty(id: &'static str, text: impl Into<SharedString>, theme: &Theme) -> Div {
    div()
        .debug_selector(move || format!("empty-state-{id}"))
        .flex()
        .flex_col()
        .items_start()
        .gap(px(Theme::SPACE_SM))
        .child(
            div()
                .debug_selector(move || format!("empty-state-{id}-line"))
                .max_w_full()
                .text_size(Theme::TEXT_UI)
                .text_color(theme.text_muted)
                .child(text.into()),
        )
}

pub fn slider_row(
    theme: &Theme,
    label: impl Into<SharedString>,
    value: f32,
    display: impl Into<SharedString>,
) -> impl IntoElement {
    let label = label.into();
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(Theme::SPACE_SM))
        .child(
            div()
                .w(px(72.))
                .text_size(Theme::TEXT_UI)
                .text_color(theme.text_muted)
                .child(label),
        )
        .child(
            div()
                .flex_1()
                .h(px(4.))
                .rounded_full()
                .bg(theme.border)
                .child(
                    div()
                        .h_full()
                        .w(px((value.clamp(0.0, 1.0) * 160.0).max(2.0)))
                        .rounded_full()
                        .bg(theme.text_muted),
                ),
        )
        .child(
            div()
                .w(px(48.))
                .text_size(Theme::TEXT_UI)
                .text_color(theme.text)
                .child(display.into()),
        )
}

#[cfg(test)]
#[path = "components_readout_tests.rs"]
mod readout_tests;

#[cfg(test)]
#[path = "components_field_tests.rs"]
mod field_tests;

#[cfg(test)]
#[path = "components_control_tests.rs"]
mod control_tests;
