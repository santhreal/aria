//! A chip that opens a frosted list of choices above or below itself.
//!
//! The Viewer's zoom and resolution pickers, the Project panel's sort picker,
//! and each mixer strip's automation mode picker share this module: the chip's
//! toggle press, the outside press that closes the open list, the deferred
//! frosted panel, its section headings, and its rows.

use std::cell::RefCell;

use crate::tooltip::WithTip as _;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, CursorStyle, Div, Entity, Hitbox, HitboxBehavior, InteractiveElement, IntoElement,
    MouseButton, MouseDownEvent, ParentElement, SharedString, Stateful, StatefulInteractiveElement,
    Styled, Window, WindowId, canvas, deferred, div, px,
};

use crate::app::AppState;
use crate::app::preview_view::ViewerMenu;
use crate::motion::{ControlId, MotionKey};
use crate::theme::Theme;

/// Which list picker is open. At most one list is open in the window at a
/// time: opening one closes any other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListId {
    /// A list on the Viewer's control row.
    Viewer(ViewerMenu),
    /// The Project panel's sort order list.
    PoolSort,
    /// The automation mode list on the mixer strip keyed by its raw lane or
    /// bus id (`0` for the master strip).
    MixerAuto(u64),
}

/// Where a [`picker`]'s list opens relative to its chip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opens {
    /// Above the chip, left edges aligned.
    AboveLeft,
    /// Above the chip, right edges aligned.
    AboveRight,
    /// Below the chip, left edges aligned.
    BelowLeft,
}

thread_local! {
    /// The open list panel drawn last frame, with the window it was drawn in.
    /// A press on the panel belongs to its row, not to the outside handler on
    /// the chip wrapper.
    static PANEL: RefCell<Option<(ListId, WindowId, Hitbox)>> = const { RefCell::new(None) };
}

fn pointer_on_panel(id: ListId, window: &Window) -> bool {
    let here = window.window_handle().window_id();
    PANEL.with_borrow(|panel| {
        panel.as_ref().is_some_and(|(drawn, drawn_in, hitbox)| {
            *drawn == id && *drawn_in == here && hitbox.is_hovered(window)
        })
    })
}

fn panel_mark(id: ListId) -> impl IntoElement {
    canvas(
        move |bounds, window, _| {
            let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
            let drawn_in = window.window_handle().window_id();
            PANEL.with_borrow_mut(|panel| *panel = Some((id, drawn_in, hitbox)));
        },
        |_, _, _, _| {},
    )
    .absolute()
    .inset_0()
}

/// Whether `id` is the open list.
pub fn is_open(state: &Entity<AppState>, id: ListId, cx: &gpui::App) -> bool {
    state.read(cx).open_list == Some(id)
}

/// The chip named `name`, and `rows` in a frosted list beside it while `id` is
/// open. A press on the chip opens or closes the list; a press outside the
/// chip and list closes it, and so does a press on a row.
pub fn picker(
    name: &'static str,
    id: ListId,
    chip: Stateful<Div>,
    chip_h: f32,
    opens: Opens,
    rows: Option<Vec<AnyElement>>,
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let open = rows.is_some();
    div()
        .relative()
        .debug_selector(move || match id {
            ListId::MixerAuto(raw) => format!("{name}-{raw}"),
            ListId::Viewer(_) | ListId::PoolSort => name.to_owned(),
        })
        .when(open, |el| {
            el.on_mouse_down_out(cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                if pointer_on_panel(id, window) {
                    return;
                }
                this.state.update(cx, |state, _| {
                    if state.open_list == Some(id) {
                        state.open_list = None;
                    }
                });
                cx.notify();
            }))
        })
        .child(
            chip.h(px(chip_h))
                .when_some(crate::tooltip::builder(name), |el, tip| el.with_tip(tip))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        this.state.update(cx, |state, _| state.toggle_list(id));
                        cx.notify();
                    }),
                ),
        )
        .when_some(rows, |el, rows| {
            el.child(list(name, id, chip_h, opens, rows, theme))
        })
}

/// The open list, drawn over the surrounding panel above or below the chip.
fn list(
    name: &'static str,
    id: ListId,
    chip_h: f32,
    opens: Opens,
    rows: Vec<AnyElement>,
    theme: &Theme,
) -> impl IntoElement + use<> {
    let key = MotionKey::new("menu", name);
    let panel = div()
        .id(key.element_id("list"))
        .min_w(px(120.))
        .rounded(px(Theme::PANEL_RADIUS))
        .bg(theme.surface_overlay)
        .border_1()
        .border_color(theme.border)
        .p(px(Theme::SPACE_XS))
        .flex()
        .flex_col()
        // A press on the list stops at the list rather than reaching the panel
        // drawn under it.
        .occlude()
        .children(rows)
        .child(panel_mark(id));
    let offset = px(chip_h + Theme::SPACE_XS);
    let placed = match opens {
        Opens::AboveLeft => div().absolute().bottom(offset).left(px(0.)),
        Opens::AboveRight => div().absolute().bottom(offset).right(px(0.)),
        Opens::BelowLeft => div().absolute().top(offset).left(px(0.)),
    };
    // Deferred so the list paints over the controls around the chip.
    deferred(placed.child(crate::frost::frost(crate::motion::menu_in(key, panel))))
}

/// A section label inside a list.
pub fn heading(text: &'static str, theme: &Theme) -> AnyElement {
    div()
        .px(px(Theme::SPACE_SM))
        .pt(px(Theme::SPACE_XS))
        .text_size(Theme::TEXT_UI)
        .text_color(theme.text_faint)
        .child(text)
        .into_any_element()
}

/// The debug selector a [`row`] draws under.
pub(crate) fn row_selector(id: impl Into<ControlId>, label: &str) -> String {
    format!("row-{:?}-{label}", id.into())
}

/// One choice in a [`picker`]'s list. The selected row holds the selected
/// plate under the pointer; the others rest in muted text and fade to the
/// hover plate. A press closes the list and runs `choose`.
pub fn row(
    id: impl Into<ControlId>,
    label: impl Into<SharedString>,
    selected: bool,
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
    choose: impl Fn(&mut AppState, &mut gpui::Context<AppState>) + 'static,
) -> AnyElement {
    let id = id.into();
    let label: SharedString = label.into();
    let selector = row_selector(id, label.as_ref());
    let key = match id {
        ControlId::Fixed(name) => MotionKey::new("viewer-menu-row", name),
        ControlId::Key(key) => key,
    };
    div()
        .id(id.element("viewer-menu-row"))
        .debug_selector(move || selector)
        .px(px(Theme::SPACE_SM))
        .py(px(Theme::SPACE_XXS))
        .rounded(px(Theme::CONTROL_RADIUS))
        .text_size(Theme::TEXT_TITLE)
        .text_color(if selected {
            theme.text
        } else {
            theme.text_muted
        })
        .bg(crate::motion::row_fill(key, selected, theme))
        .cursor(CursorStyle::PointingHand)
        .active(|style| style.opacity(0.92))
        .on_hover(crate::motion::hover_listener(key))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                this.state.update(cx, |state, cx| {
                    state.open_list = None;
                    choose(state, cx);
                });
                cx.notify();
            }),
        )
        .child(label)
        .into_any_element()
}

#[cfg(test)]
#[path = "pick_list_tests.rs"]
mod tests;
