//! Shared chrome widgets used by every panel.

use crate::tooltip::WithTip as _;
use gpui::motion::presets;
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

pub fn button(
    id: impl Into<motion::ControlId>,
    label: impl Into<SharedString>,
    theme: &Theme,
) -> Stateful<Div> {
    let id = id.into();
    let key = id.motion("button");
    let lit = motion::hover_t(key);
    let pressed = theme.element_active;
    div()
        .id(id.element("button"))
        // Press reads as a settle: the fill deepens and the control dims.
        .active(move |style| style.bg(pressed).opacity(0.92))
        .px(px(Theme::SPACE_SM))
        .py(px(Theme::SPACE_XS))
        .rounded(px(Theme::CONTROL_RADIUS))
        .bg(motion::mix(theme.solid, theme.surface_raised_hover, lit))
        .text_color(motion::mix(theme.on_solid, theme.text, lit))
        .text_size(Theme::TEXT_UI)
        .cursor(CursorStyle::PointingHand)
        .on_hover(motion::hover_listener(key))
        .child(label.into())
}

pub fn ghost_button(
    id: impl Into<motion::ControlId>,
    label: impl Into<SharedString>,
    theme: &Theme,
) -> Stateful<Div> {
    ghost_frame(id.into(), theme).child(label.into())
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

/// The frame of a [`ghost_button`]: its padding, its wash and text colour,
/// which fade in under the pointer, and its press dim, with nothing in it.
fn ghost_frame(id: motion::ControlId, theme: &Theme) -> Stateful<Div> {
    let key = id.motion("ghost");
    let lit = motion::hover_t(key);
    let pressed = theme.element_active;
    div()
        .id(id.element("ghost"))
        .active(move |style| style.bg(pressed).opacity(0.92))
        .px(px(Theme::SPACE_SM))
        .py(px(Theme::SPACE_XS))
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
/// count. A size answers for the padding, the text, and how far the inactive
/// chips recede, so the two rows cannot drift apart in three separate files.
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

    fn pad(self) -> (f32, f32) {
        match self {
            Self::Regular => (Theme::SPACE_SM, Theme::SPACE_XS),
            Self::Compact => (Theme::SPACE_XS, 2.),
        }
    }

    fn text_size(self) -> gpui::Pixels {
        match self {
            Self::Regular => Theme::TEXT_UI,
            Self::Compact => Theme::TEXT_SMALL,
        }
    }

    /// The colour an inactive chip rests at. A secondary row recedes further,
    /// so the primary row stays the one the eye lands on.
    fn rest(self, theme: &Theme) -> gpui::Hsla {
        match self {
            Self::Regular => theme.text_muted,
            Self::Compact => theme.text_faint,
        }
    }
}

/// A chip: the smallest control that says which one of a set is in force.
///
/// A row of these replaces a dropdown wherever the set is short enough to show
/// whole, so the choices are visible without opening them. The active
/// chip carries a faint wash and full-strength text; the others sit at their
/// size's colour on nothing, and light towards the active look on hover so the
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
    let id = id.into();
    let label: SharedString = label.into();
    let key = id.motion("chip");
    let lit = motion::hover_t(key);
    let (pad_x, pad_y) = size.pad();
    div()
        .id(id.element("chip"))
        .flex()
        .items_center()
        .px(px(pad_x))
        .py(px(pad_y))
        .rounded(px(Theme::CONTROL_RADIUS))
        .bg(motion::mix(
            theme.wash(if active { 0.11 } else { 0.0 }),
            theme.element_hover,
            lit,
        ))
        .text_size(size.text_size())
        .text_color(motion::mix(
            if active { theme.text } else { size.rest(theme) },
            theme.text,
            lit,
        ))
        .cursor(CursorStyle::PointingHand)
        // Press feedback: dim while held, since GPUI Div styles expose no scale.
        .active(move |style| style.opacity(0.92))
        .on_hover(motion::hover_listener(key))
        .child(div().min_w_0().truncate().child(label))
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
        .child(
            crate::icons::icon(crate::icons::CHEVRON_DOWN)
                .flex_none()
                .size(px(CHEVRON_PX)),
        )
}

/// A compact tab strip for static surface catalogs and galleries.
///
/// The label and its position form the identity, so duplicate labels in one
/// strip animate independently without allocating an id on each draw.
pub fn tab_bar(theme: &Theme, tabs: &[(&'static str, bool)]) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(Theme::SPACE_XXS))
        .children(tabs.iter().enumerate().map(|(index, (label, active))| {
            let key = motion::MotionKey::new("tab", format_args!("{label}-{index}"));
            let lit = motion::hover_t(key);
            div()
                .id(key.element_id("tab"))
                .px(px(Theme::SPACE_SM))
                .py(px(Theme::SPACE_XS))
                .border_b_2()
                .border_color(if *active {
                    theme.accent
                } else {
                    theme.wash(0.0)
                })
                .bg(motion::mix(
                    theme.wash(if *active { 0.08 } else { 0.0 }),
                    theme.element_hover,
                    lit,
                ))
                .text_size(Theme::TEXT_UI)
                .text_color(motion::mix(
                    if *active {
                        theme.text
                    } else {
                        theme.text_muted
                    },
                    theme.text,
                    lit,
                ))
                .cursor(CursorStyle::PointingHand)
                .active(|style| style.opacity(0.92))
                .on_hover(motion::hover_listener(key))
                .child(*label)
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

/// The caret of a box that takes typed text: a bar in the caret colour at
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
        .min_h(px(Theme::INPUT_HEIGHT))
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

/// The room of an [`icon_button`] with nothing in it, for a row of icon
/// controls where one does not apply: the controls after it stand in the
/// columns the rows above and below draw them in.
pub fn icon_space() -> Div {
    div().flex_none().size(px(Theme::ICON_HIT))
}

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
        .px(px(Theme::SPACE_SM))
        .py(px(Theme::SPACE_XS))
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

/// [`readout`] with its value in `tint`, for a value whose colour is its
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

/// The heading over a group of rows, such as a group of [`readout`] rows.
pub fn section_label(theme: &Theme, title: impl Into<SharedString>) -> Div {
    div()
        .pt(px(Theme::SPACE_SM))
        .text_size(Theme::TEXT_UI)
        .text_color(theme.text_faint)
        .child(title.into())
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
