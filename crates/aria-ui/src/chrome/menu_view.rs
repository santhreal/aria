//! The menu drop: a title in the bar, and the panel it opens.
//!
//! [`super::menu_host`] builds the menu contents; this module draws the popup,
//! animates it in and out, and blends title and row hover states.

use crate::tooltip::WithTip as _;
use std::cell::RefCell;

use gpui::motion::{MotionPolicy, presets};
use gpui::{
    AnyElement, Context, CursorStyle, Edges, Entity, Hitbox, HitboxBehavior, InteractiveElement,
    IntoElement, MouseButton, MouseDownEvent, ParentElement, StatefulInteractiveElement, Styled,
    Window, WindowId, anchored, canvas, deferred, div, px,
};

use super::MENU_H;
use super::menu_items::MenuItem;
use crate::app::AppState;
use crate::motion;
use crate::popover::Popup;
use crate::shell::Shell;
use crate::theme::Theme;
use crate::types::AppMenu;
use gpui::prelude::FluentBuilder;

/// What the bar draws for one menu this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Phase {
    /// Open: drawn, and it takes clicks.
    Open,
    /// On its way out, `0..1` through the exit: drawn, and inert.
    Closing(f32),
}

thread_local! {
    /// The drop as the bar draws it, which is not quite what the state holds.
    ///
    /// `AppState::open_menu` goes to `None` the instant a row is chosen, which
    /// leaves an exit animation nothing to animate. This keeps the menu that was
    /// open alive for exactly one [`presets::MENU_OUT`], so the drop fades out
    /// instead of vanishing. Thread-local because it is frame state, like the
    /// hover fades: no part of the model wants to know.
    static DROP: RefCell<Popup<AppMenu>> = RefCell::new(Popup::default());
    /// The same for a submenu a button holds out: the button's element id.
    static POPUP: RefCell<Popup<&'static str>> = RefCell::new(Popup::default());
    /// The panels drawn last frame for the open drop, its flyout, and a
    /// submenu under a button, with the window each was drawn in. A press on
    /// one of them is the row's, never a press outside the menu. A hitbox
    /// from an older frame is never hovered, so an entry left by a panel that
    /// is no longer drawn reads as outside.
    static PANELS: RefCell<Vec<(&'static str, WindowId, Hitbox)>> =
        const { RefCell::new(Vec::new()) };
}

/// Reconcile the drawn drop with the state, and say which menu owns a drop this
/// frame. Call once per frame, before the titles are built.
///
/// Reduced motion under `policy` drops the exit rather than shortening it: under
/// reduced motion the animation schedules no frames, so a drop left mid-exit
/// would stay on screen until something else asked for a repaint.
pub(super) fn drop_phase(open: Option<AppMenu>, policy: MotionPolicy) -> Option<(AppMenu, Phase)> {
    DROP.with_borrow_mut(|popup| reconcile(popup, open, policy))
}

/// Bring `popup` to `open`, and say what it draws this frame and in which
/// phase. Calling it twice in a frame with the same `open` answers the same.
fn reconcile<T: Copy + PartialEq>(
    popup: &mut Popup<T>,
    open: Option<T>,
    policy: MotionPolicy,
) -> Option<(T, Phase)> {
    let span = policy.scale(presets::MENU_OUT.total());
    match open {
        Some(value) if popup.as_open() != Some(&value) => popup.open(value),
        Some(_) => {}
        None => {
            popup.begin_close();
            popup.finish_close(span);
        }
    }
    if let Some(value) = popup.as_open() {
        return Some((*value, Phase::Open));
    }
    if policy.reduced() {
        return None;
    }
    let since = popup.closing_since()?;
    let value = *popup.get()?;
    let elapsed = since.elapsed();
    let t = if span.is_zero() {
        1.0
    } else {
        (elapsed.as_secs_f32() / span.as_secs_f32()).clamp(0.0, 1.0)
    };
    Some((value, Phase::Closing(t)))
}

/// Whether the pointer is on a panel of the open menu in `window`: the drop,
/// its flyout, or a submenu under a button.
pub(super) fn pointer_on_drop(window: &Window) -> bool {
    let here = window.window_handle().window_id();
    PANELS.with_borrow(|panels| {
        panels
            .iter()
            .any(|(_, drawn_in, hitbox)| *drawn_in == here && hitbox.is_hovered(window))
    })
}

/// An empty layer over the whole of the panel it is a child of, which records
/// the panel as `slot` for [`pointer_on_drop`].
fn panel_mark(slot: &'static str) -> impl IntoElement {
    canvas(
        move |bounds, window, _| {
            let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
            let drawn_in = window.window_handle().window_id();
            PANELS.with_borrow_mut(|panels| {
                match panels.iter_mut().find(|(held, ..)| *held == slot) {
                    Some(entry) => *entry = (slot, drawn_in, hitbox),
                    None => panels.push((slot, drawn_in, hitbox)),
                }
            });
        },
        |_, _, _, _| {},
    )
    .absolute()
    .inset_0()
}

pub(super) fn menu_drop(
    menu: AppMenu,
    phase: Option<Phase>,
    theme: &Theme,
    state: &Entity<AppState>,
    cx: &mut Context<Shell>,
) -> impl IntoElement {
    let label = menu.label();
    let open = phase == Some(Phase::Open);
    let key = motion::MotionKey::new("menu-title", label);
    let rows: Vec<AnyElement> = match phase {
        Some(_) => {
            let drawn = drawn_entries(menu, state.read(cx));
            drawn
                .into_iter()
                .map(|entry| menu_row(entry, open, theme, state, cx).into_any_element())
                .collect()
        }
        None => Vec::new(),
    };
    div()
        .id(label)
        .debug_selector(move || format!("menu-title-{label}"))
        .relative()
        .px(px(Theme::SPACE_SM))
        .py(px(Theme::SPACE_XS))
        .rounded(px(Theme::CONTROL_RADIUS))
        .text_size(Theme::TEXT_UI)
        .text_color(theme.text)
        .cursor(CursorStyle::PointingHand)
        .active(|style| style.opacity(0.92))
        // An open title holds the selected wash; a shut one fades into it.
        .bg(if open {
            theme.glass_selected_bg()
        } else {
            motion::hover_blend(key, theme.wash(0.0), theme.element_hover)
        })
        .on_hover(motion::hover_listener(key))
        .child(label)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                this.state.update(cx, |state, _| state.toggle_menu(menu));
                cx.notify();
            }),
        )
        .when_some(phase, |el, phase| {
            el.child(drop_panel(label, phase, theme, rows))
        })
}

/// Everything one drawn entry needs, read from the state before any element is
/// built, so the drop takes one read of the state rather than one per row.
struct Drawn {
    /// The element id: the row's id, or the submenu's own.
    id: &'static str,
    caption: String,
    icon: Option<&'static str>,
    chord: Option<String>,
    /// Why the row cannot run now. A disabled row is drawn faded and states the
    /// reason as its tooltip.
    blocked: Option<&'static str>,
    /// `Some` for a toggle row, holding whether its check mark is drawn.
    checked: Option<bool>,
    /// The row to press, or `None` for a submenu.
    item: Option<MenuItem>,
    /// Where the entry sits in its list, for the keyboard light.
    index: usize,
    /// The keyboard lit this entry: Enter runs it. See [`crate::app::menu_keys`].
    lit: bool,
    /// The entry holds the keyboard focus and draws the focus ring. A submenu
    /// whose flyout has a lit row hands the ring to that row.
    ring: bool,
    /// The rows of a submenu entry, drawn in a flyout while it is open.
    flyout: Option<Vec<Drawn>>,
    /// The entry is a submenu, whether or not its flyout is open.
    sub: bool,
    /// The entry sits in a flyout rather than the drop.
    nested: bool,
}

/// The entries `menu` draws now, with the open submenu's rows inside it.
fn drawn_entries(menu: AppMenu, state: &AppState) -> Vec<Drawn> {
    // The bar draws the registry, not the shipped tables: a plugin's item is
    // drawn by the same code that draws File > New, and the keyboard walks the
    // same list.
    let items = state.menu_rows(menu);
    let plugin_rows =
        super::plugin_menus::rows_in(&state.ctx, super::plugin_menus::location_of(menu));
    let row = |item: &MenuItem, index: usize, lit: bool, nested: bool| {
        drawn_row(item, &plugin_rows, index, lit, nested, state)
    };
    let (flyout_at, flyout_lit) = match state.menu_flyout {
        Some((at, child)) => (Some(at), child),
        None => (None, None),
    };
    super::menu_items::drop_entries(&items)
        .into_iter()
        .enumerate()
        .map(|(index, entry)| {
            let lit = state.menu_row == Some(index);
            match entry {
                super::menu_items::DropEntry::Row(at) => row(&items[at], index, lit, false),
                super::menu_items::DropEntry::Sub { caption, rows } => {
                    let flyout = (flyout_at == Some(index)).then(|| {
                        rows.iter()
                            .enumerate()
                            .map(|(child, at)| {
                                row(&items[*at], child, flyout_lit == Some(child), true)
                            })
                            .collect()
                    });
                    Drawn {
                        id: crate::interned::intern(&format!(
                            "menu-sub-{}-{caption}",
                            menu.label()
                        )),
                        caption: caption.to_string(),
                        icon: None,
                        chord: None,
                        blocked: None,
                        checked: None,
                        item: None,
                        index,
                        lit,
                        ring: lit && !(flyout_at == Some(index) && flyout_lit.is_some()),
                        flyout,
                        sub: true,
                        nested: false,
                    }
                }
            }
        })
        .collect()
}

/// One command row of a drop or a flyout.
fn drawn_row(
    item: &MenuItem,
    plugin_rows: &[super::plugin_menus::PluginRow],
    index: usize,
    lit: bool,
    nested: bool,
    state: &AppState,
) -> Drawn {
    Drawn {
        id: item.id,
        caption: super::menu_items::row_caption(item, state),
        icon: super::plugin_menus::icon_of(plugin_rows, item),
        chord: item.chord(state),
        blocked: item.blocked_by(state),
        checked: item.checked.map(|_| item.is_checked(state)),
        item: Some(*item),
        index,
        lit,
        ring: lit,
        flyout: None,
        sub: false,
        nested,
    }
}

/// The rows of `menu`'s submenu `caption`, as its flyout draws them. While
/// `live`, the row the keyboard lit draws lit.
fn submenu_rows(menu: AppMenu, caption: &str, live: bool, state: &AppState) -> Vec<Drawn> {
    let items = state.menu_rows(menu);
    let plugin_rows =
        super::plugin_menus::rows_in(&state.ctx, super::plugin_menus::location_of(menu));
    let entries = super::menu_items::drop_entries(&items);
    let Some((at, held)) = entries
        .iter()
        .enumerate()
        .find_map(|(at, entry)| match entry {
            super::menu_items::DropEntry::Sub { caption: sub, rows } if *sub == caption => {
                Some((at, rows))
            }
            super::menu_items::DropEntry::Row(_) | super::menu_items::DropEntry::Sub { .. } => None,
        })
    else {
        return Vec::new();
    };
    let lit = match state.menu_flyout {
        Some((open, child)) if live && open == at => child,
        _ => None,
    };
    held.iter()
        .enumerate()
        .map(|(child, row)| {
            drawn_row(
                &items[*row],
                &plugin_rows,
                child,
                lit == Some(child),
                true,
                state,
            )
        })
        .collect()
}

/// The panel under a title: `menu_in` on the way in, `menu_out` on the way out.
fn drop_panel(
    label: &'static str,
    phase: Phase,
    theme: &Theme,
    rows: Vec<AnyElement>,
) -> impl IntoElement {
    let panel = div()
        .id(label)
        .debug_selector(move || format!("menu-drop-{label}"))
        .min_w(px(200.))
        .rounded(px(Theme::PANEL_RADIUS))
        .bg(theme.surface_overlay)
        .border_1()
        .border_color(theme.border)
        .p(px(Theme::SPACE_XS))
        .flex()
        .flex_col()
        // A press on the drop is the drop's, not the panel's drawn under it.
        .occlude()
        .children(rows)
        .child(panel_mark("drop"));
    let animated = match phase {
        Phase::Open => motion::menu_in(motion::MotionKey::new("menu", label), panel),
        Phase::Closing(t) => motion::menu_out(motion::MotionKey::new("menu", label), t, panel),
    };
    // Deferred so the drop paints over the tool bar and the panels drawn after
    // the bar, at the place the title lays it out.
    deferred(
        div()
            .absolute()
            .top(px(MENU_H))
            .left(px(0.))
            .child(crate::frost::frost(animated)),
    )
}

/// `button`, which opens `menu`'s submenu `caption` under itself, as the
/// timeline's display button opens View > Timeline Display Settings.
///
/// The rows are the ones the drop's flyout draws, read from the same registry
/// and run by the same press, so the button and the menu offer the same
/// choices. While `owner`, the button's element id, holds the submenu out, a
/// press outside the button and the rows closes it and still lands where it
/// was aimed; a press on the button is the button's, so a second press closes
/// it.
pub(super) fn submenu_under(
    owner: &'static str,
    menu: AppMenu,
    caption: &'static str,
    button: AnyElement,
    theme: &Theme,
    state: &Entity<AppState>,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let held = state.read(cx).menu_owner;
    let phase = POPUP
        .with_borrow_mut(|popup| reconcile(popup, held, cx.motion_policy()))
        .and_then(|(drawn, phase)| (drawn == owner).then_some(phase));
    let rows: Vec<AnyElement> = match phase {
        Some(phase) => {
            let live = phase == Phase::Open;
            submenu_rows(menu, caption, live, state.read(cx))
                .into_iter()
                .map(|row| menu_row(row, live, theme, state, cx).into_any_element())
                .collect()
        }
        None => Vec::new(),
    };
    div()
        .relative()
        .flex_none()
        .child(button)
        .when(held == Some(owner), |el| {
            el.on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, window, cx| {
                if pointer_on_drop(window) {
                    return;
                }
                this.state.update(cx, |state, _| state.close_menu());
                cx.notify();
            }))
        })
        .when_some(phase, move |el, phase| {
            el.child(popup_panel(owner, phase, theme, rows))
        })
        .into_any_element()
}

/// The panel of a submenu under its button. It opens downward from the
/// button's bottom left edge and moves back inside the window rather than
/// past its edge.
fn popup_panel(
    owner: &'static str,
    phase: Phase,
    theme: &Theme,
    rows: Vec<AnyElement>,
) -> impl IntoElement {
    let key = motion::MotionKey::new("menu-submenu", owner);
    let panel = div()
        .id(key.element_id("submenu"))
        .debug_selector(move || format!("{owner}-submenu"))
        .min_w(px(200.))
        .rounded(px(Theme::PANEL_RADIUS))
        .bg(theme.surface_overlay)
        .border_1()
        .border_color(theme.border)
        .p(px(Theme::SPACE_XS))
        .flex()
        .flex_col()
        .occlude()
        .children(rows)
        .child(panel_mark("submenu"));
    let animated = match phase {
        Phase::Open => motion::menu_in(key, panel).into_any_element(),
        Phase::Closing(t) => motion::menu_out(key, t, panel).into_any_element(),
    };
    deferred(
        div().absolute().top(gpui::relative(1.)).left(px(0.)).child(
            anchored()
                .snap_to_window_with_margin(Edges::all(px(Theme::SPACE_SM)))
                .child(crate::frost::frost(animated)),
        ),
    )
}

/// One entry of an open drop or of its flyout. A lit entry holds the hover
/// wash and the 1 px accent focus ring; the others fade into the wash under
/// the pointer, and a pointer moving over a lit drop moves the light with it. A disabled row is drawn at 40 % opacity and
/// states why in its tooltip; pressing it puts the reason on the status line and
/// leaves the drop open.
fn menu_row(
    entry: Drawn,
    live: bool,
    theme: &Theme,
    state: &Entity<AppState>,
    cx: &mut Context<Shell>,
) -> impl IntoElement {
    let Drawn {
        id,
        caption,
        icon,
        chord,
        blocked,
        checked,
        item,
        index,
        lit,
        ring,
        flyout,
        sub,
        nested,
    } = entry;
    let key = motion::MotionKey::new("menu-row", id);
    let hover = motion::hover_listener(key);
    let pointed = state.clone();
    let click = cx.listener(move |this, _: &MouseDownEvent, _, cx| {
        this.state.update(cx, |state, cx| match item {
            Some(item) => {
                if super::menu_items::press(&item, state, cx) {
                    state.close_menu();
                }
            }
            None => state.open_menu_flyout(index),
        });
        cx.notify();
    });
    let flyout = flyout.map(|rows| {
        let rows: Vec<AnyElement> = rows
            .into_iter()
            .map(|row| menu_row(row, live, theme, state, cx).into_any_element())
            .collect();
        div()
            .absolute()
            .top(px(-Theme::SPACE_XS))
            .left(gpui::relative(1.))
            .min_w(px(200.))
            .rounded(px(Theme::PANEL_RADIUS))
            .bg(theme.surface_overlay)
            .border_1()
            .border_color(theme.border)
            .p(px(Theme::SPACE_XS))
            .flex()
            .flex_col()
            .occlude()
            .children(rows)
            .child(panel_mark("flyout"))
    });
    div()
        .id(id)
        // The lit row is found as `focus-ring` instead: an element has one
        // selector, and the ring's is set below.
        .debug_selector(move || format!("menu-row-{id}"))
        .relative()
        .flex()
        .items_center()
        .gap(px(Theme::SPACE_SM))
        .px(px(Theme::SPACE_SM))
        .h(px(Theme::ROW_HEIGHT))
        .rounded(px(Theme::CONTROL_RADIUS))
        // Every row draws the border, so the ring moving between rows shifts
        // no caption. Only the keyboard lights a row, so a pointer never
        // leaves a ring.
        .border_1()
        .border_color(if ring {
            theme.accent
        } else {
            theme.accent.opacity(0.0)
        })
        .when(ring, |el| el.debug_selector(|| "focus-ring".into()))
        .text_size(Theme::TEXT_UI)
        .text_color(theme.text)
        .cursor(if blocked.is_some() {
            CursorStyle::Arrow
        } else {
            CursorStyle::PointingHand
        })
        .when(blocked.is_none(), |el| {
            el.active(|style| style.opacity(0.92))
        })
        .bg(if lit || flyout.is_some() {
            theme.element_hover
        } else {
            motion::hover_blend(key, theme.wash(0.0), theme.element_hover)
        })
        .on_hover(move |hovered, window, cx| {
            hover(hovered, window, cx);
            if *hovered {
                pointed.update(cx, |state, _| {
                    if nested {
                        state.point_flyout_row(index);
                    } else {
                        state.point_menu_row(index);
                    }
                });
            }
        })
        // The lead column is drawn on every row and holds the check mark or a
        // plugin row's icon. A checked row shows its check.
        .child(crate::components::menu_lead(
            checked == Some(true),
            icon,
            theme,
        ))
        .child(
            div()
                .flex_1()
                .when(blocked.is_some(), |el| el.opacity(0.4))
                .child(caption),
        )
        .when_some(chord, |el, chord| {
            el.child(
                div()
                    .pl(px(Theme::SPACE_LG))
                    .text_size(Theme::TEXT_UI)
                    .text_color(theme.text_faint)
                    .when(blocked.is_some(), |el| el.opacity(0.4))
                    .child(chord),
            )
        })
        .when(sub, |el| {
            el.child(
                div()
                    .pl(px(Theme::SPACE_LG))
                    .text_color(theme.text_muted)
                    .child("›"),
            )
        })
        .when_some(blocked, |el, reason| {
            el.with_tip(crate::tooltip::text_builder(reason))
        })
        .when(live, |el| el.on_mouse_down(MouseButton::Left, click))
        .children(flyout)
}

#[cfg(test)]
#[path = "menu_view_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "menu_grid_tests.rs"]
mod grid_tests;
