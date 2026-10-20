//! Shared chrome widgets used by every panel.

use crate::tooltip::WithTip as _;
use gpui::motion::presets;
use gpui::{
    CursorStyle, Div, InteractiveElement, IntoElement, ParentElement, SharedString, Stateful,
    StatefulInteractiveElement, Styled, div, px,
};

use crate::motion;
use crate::motion::glide;
use crate::theme::Theme;

pub mod drawn;
pub use drawn::{Picture, Wraps, picture};

/// A disclosure chevron, sized to sit beside 12px row text.
const CHEVRON_PX: f32 = 12.;

/// A quarter turn: shut points the way the writing goes, open points down.
const QUARTER: f32 = std::f32::consts::FRAC_PI_2;

/// A disclosure chevron that turns between shut and open.
///
/// One glyph and one rotation rather than two glyphs. A swap at 12px reads as a
/// flicker, and the turn is what tells the operator the click landed on the row
/// rather than beside it. The rotation eases from wherever the glyph currently
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
    let id = id.into();
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
        .child(label.into())
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
/// whole, so the operator reads the choices instead of opening them. The active
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

/// A key-routed text box: the value, a caret while it holds focus, and the
/// placeholder while it is empty.
///
/// Every text field in the app is drawn by this, so the accent border that means
/// "your keys go here" cannot be spelled differently in two panels. The caller
/// attaches the click that takes focus, because only it knows which field this
/// is.
pub fn text_box(
    id: impl Into<motion::ControlId>,
    value: impl Into<SharedString>,
    placeholder: impl Into<SharedString>,
    focused: bool,
    theme: &Theme,
) -> Stateful<Div> {
    let value = value.into();
    let empty = value.is_empty();
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
        .text_color(if empty { theme.text_muted } else { theme.text })
        .on_hover(motion::hover_listener(key))
        // A field narrower than its text ends it in an ellipsis inside the
        // border rather than drawing past it.
        .child(div().min_w_0().truncate().child(if empty && !focused {
            placeholder.into()
        } else if focused {
            SharedString::from(format!("{value}|"))
        } else {
            value
        }))
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
