//! The frame menu a tab's right-click or the frame's `≡` opens: the commands
//! for one panel, and the panels the frame does not hold behind one Add Panel
//! row. Also the tab overflow menu, which lists the tabs a frame's tab row has
//! no room for.
//!
//! The menu draws the menu bar's rows: 22 px of UI text on the overlay
//! surface. It opens under the frame's tab row at its right end and moves
//! back inside the window rather than past its edge. Add Panel opens a flyout beside it,
//! toward the frame's body unless the window's left edge is nearer than the
//! flyout's width, and the flyout moves inside the window the same way. The
//! overflow menu opens under its control, on the same rows.

use gpui::{
    Anchor, AnyElement, App, Context, CursorStyle, Edges, InteractiveElement, IntoElement,
    MouseButton, MouseDownEvent, ParentElement, StatefulInteractiveElement as _, Styled, Window,
    anchored, deferred, div, px, relative,
};

use crate::dock::{FloatRect, FrameId, PanelId, Zone};
use crate::motion::{self, ControlId, MotionKey};
use crate::theme::Theme;
use gpui::prelude::FluentBuilder;

use super::Shell;

/// Width of the menu and of its Add Panel flyout.
const MENU_W: f32 = 200.0;

/// The menu a window has open on one of its frames. A window has at most
/// one: opening either kind closes the other.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FrameMenu {
    /// The frame menu of `panel`'s tab in `frame`, with its Add Panel flyout
    /// out while `add_open`.
    Panel {
        frame: FrameId,
        panel: PanelId,
        add_open: bool,
    },
    /// The tab overflow menu of `frame`.
    Overflow(FrameId),
}

/// One menu row: the caption on the menu bar's 22 px row. Its wash fades in
/// under the pointer, redrawing the window chrome alone, and `also` runs on
/// each hover change after the fade is set.
fn row(
    id: impl Into<ControlId>,
    caption: &'static str,
    theme: &Theme,
    also: impl Fn(&bool, &mut Window, &mut App) + 'static,
) -> gpui::Stateful<gpui::Div> {
    let id = id.into();
    let key = id.motion("row");
    let fade = motion::hover_listener(key);
    div()
        .id(id.element("row"))
        .flex()
        .items_center()
        .gap(px(Theme::SPACE_SM))
        .h(px(Theme::ROW_HEIGHT))
        .px(px(Theme::SPACE_SM))
        .rounded(px(Theme::CONTROL_RADIUS))
        .text_size(Theme::TEXT_UI)
        .text_color(theme.text)
        .cursor(CursorStyle::PointingHand)
        .bg(motion::hover_blend(
            key,
            theme.wash(0.0),
            theme.element_hover,
        ))
        .on_hover(move |hovered, window, cx| {
            fade(hovered, window, cx);
            also(hovered, window, cx);
        })
        .child(div().flex_1().min_w_0().truncate().child(caption))
}

/// The hover action of a row that does nothing past its fade.
fn no_hover(_: &bool, _: &mut Window, _: &mut App) {}

/// The surface the menu and its flyout draw on.
fn card(theme: &Theme) -> gpui::Div {
    div()
        .w(px(MENU_W))
        .rounded(px(Theme::PANEL_RADIUS))
        .bg(theme.surface_overlay)
        .border_1()
        .border_color(theme.border)
        .p(px(Theme::SPACE_XS))
        .flex()
        .flex_col()
        // A press on the menu is the menu's, not the panel's under it.
        .occlude()
}

/// `row` running `act` and closing the menu when pressed.
fn pressed(
    row: gpui::Stateful<gpui::Div>,
    cx: &mut Context<Shell>,
    act: impl Fn(&mut Shell, &mut Context<Shell>) + 'static,
) -> gpui::Stateful<gpui::Div> {
    row.on_mouse_down(
        MouseButton::Left,
        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
            cx.stop_propagation();
            this.frame_menu = None;
            act(this, cx);
            cx.notify();
        }),
    )
}

/// A row that runs `act` and closes the menu when pressed.
fn command_row(
    id: impl Into<ControlId>,
    caption: &'static str,
    theme: &Theme,
    cx: &mut Context<Shell>,
    act: impl Fn(&mut Shell, &mut Context<Shell>) + 'static,
) -> gpui::Stateful<gpui::Div> {
    pressed(row(id, caption, theme, no_hover), cx, act)
}

/// A command row of the frame menu itself, beside Add Panel: the pointer on
/// it takes the flyout in. The flyout's own rows keep it out.
fn menu_row(
    id: impl Into<ControlId>,
    caption: &'static str,
    theme: &Theme,
    cx: &mut Context<Shell>,
    act: impl Fn(&mut Shell, &mut Context<Shell>) + 'static,
) -> gpui::Stateful<gpui::Div> {
    let shut_flyout = cx.listener(|this, hovered: &bool, _, cx| {
        if !*hovered {
            return;
        }
        if let Some(FrameMenu::Panel { add_open, .. }) = &mut this.frame_menu
            && std::mem::take(add_open)
        {
            this.redraw_chrome(cx);
        }
    });
    pressed(row(id, caption, theme, shut_flyout), cx, act)
}

impl Shell {
    /// Open the frame menu on `panel` in `frame_id`, or close it when it is
    /// open there.
    pub(super) fn toggle_frame_menu(&mut self, frame_id: FrameId, panel: PanelId) {
        self.frame_menu = match self.frame_menu {
            Some(FrameMenu::Panel {
                frame, panel: open, ..
            }) if frame == frame_id && open == panel => None,
            _ => Some(FrameMenu::Panel {
                frame: frame_id,
                panel,
                add_open: false,
            }),
        };
    }

    /// Open the tab overflow menu of `frame_id`, or close it when it is open.
    pub(super) fn toggle_tab_overflow(&mut self, frame_id: FrameId) {
        self.frame_menu = if self.frame_menu == Some(FrameMenu::Overflow(frame_id)) {
            None
        } else {
            Some(FrameMenu::Overflow(frame_id))
        };
    }

    /// Put out the Add Panel flyout of the frame menu open on `panel` in
    /// `frame_id`; `true` when that changed what is drawn.
    fn open_add_panel(&mut self, frame_id: FrameId, panel: PanelId) -> bool {
        let out = Some(FrameMenu::Panel {
            frame: frame_id,
            panel,
            add_open: true,
        });
        let changed = self.frame_menu != out;
        self.frame_menu = out;
        changed
    }

    /// The overflow control at the end of a tab row and, while it is open, its
    /// menu: one row per tab in `hidden`, in the frame's order. Picking a row
    /// activates that tab, which the row then draws.
    pub(super) fn render_tab_overflow(
        &self,
        frame_id: FrameId,
        hidden: &[PanelId],
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let open = self.frame_menu == Some(FrameMenu::Overflow(frame_id));
        let menu = open.then(|| {
            let rows: Vec<AnyElement> = hidden
                .iter()
                .map(|&panel| {
                    let title = self.state.read(cx).dock_panel_title(panel);
                    command_row(
                        MotionKey::new("dock-tab-more-row", panel),
                        title,
                        theme,
                        cx,
                        move |this, cx| {
                            this.state.update(cx, |state, cx| {
                                state.activate_dock_panel(panel, cx);
                            });
                        },
                    )
                    .debug_selector(move || format!("dock-tab-more-{panel}"))
                    .into_any_element()
                })
                .collect();
            deferred(
                div().absolute().top(relative(1.)).right_0().child(
                    anchored()
                        .anchor(Anchor::TopRight)
                        .snap_to_window_with_margin(Edges::all(px(Theme::SPACE_SM)))
                        .child(motion::fade_quick(
                            MotionKey::new("dock-tab-more-pop", frame_id),
                            card(theme)
                                .id(MotionKey::new("dock-tab-more-card", frame_id)
                                    .element_id("card"))
                                .debug_selector(|| "dock-tab-more-card".into())
                                .children(rows),
                        )),
                ),
            )
        });
        div()
            .flex_none()
            .relative()
            .h_full()
            .flex()
            .items_center()
            .px(px(Theme::SPACE_XXS))
            .child(
                crate::components::icon_button(
                    ("dock-tab-more", MotionKey::new("dock-tab-more", frame_id)),
                    crate::icons::CHEVRONS_RIGHT,
                    theme,
                )
                .debug_selector(move || format!("dock-tab-more-button-{}", frame_id.0))
                .size(px(Theme::ICON_HIT_ROW))
                .when(open, |el| el.bg(theme.element_hover))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        this.toggle_tab_overflow(frame_id);
                        cx.notify();
                    }),
                ),
            )
            .children(menu)
            .into_any_element()
    }

    pub(super) fn render_frame_menu(
        &self,
        frame_id: FrameId,
        panel: PanelId,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (openable, rect) = {
            let read = self.state.read(cx);
            let in_frame: Vec<PanelId> = read
                .dock_tree
                .frame(frame_id)
                .map(|f| f.tabs.clone())
                .unwrap_or_default();
            let openable: Vec<(PanelId, &'static str)> = read
                .registered_panel_ids()
                .into_iter()
                .filter(|id| !in_frame.contains(id))
                .map(|id| (id, read.dock_panel_title(id)))
                .collect();
            (openable, read.dock_frame_rect(frame_id))
        };
        let add_open = !openable.is_empty()
            && self.frame_menu
                == Some(FrameMenu::Panel {
                    frame: frame_id,
                    panel,
                    add_open: true,
                });
        // The menu's left edge is its width in from the frame's right edge,
        // less the inset; the flyout goes left of that while it fits.
        let flyout_left = rect
            .is_none_or(|[x, _, w, _]| x + w - Theme::SPACE_XS - 2.0 * MENU_W >= Theme::SPACE_SM);

        let close = menu_row(
            MotionKey::new("dock-menu-close", panel),
            "Close Panel",
            theme,
            cx,
            move |this, cx| {
                this.state.update(cx, |state, cx| {
                    state.close_dock_panel(panel, cx);
                });
            },
        );
        let others = menu_row(
            MotionKey::new("dock-menu-close-others", panel),
            "Close Other Tabs in Frame",
            theme,
            cx,
            move |this, cx| {
                this.state.update(cx, |state, cx| {
                    state.close_other_dock_tabs(frame_id, panel, cx);
                });
            },
        );
        let undock = menu_row(
            MotionKey::new("dock-menu-undock", panel),
            "Undock Panel",
            theme,
            cx,
            move |this, cx| {
                this.state.update(cx, |state, cx| {
                    state.float_dock_panel(
                        panel,
                        FloatRect {
                            x: 120.0,
                            y: 120.0,
                            w: 520.0,
                            h: 360.0,
                        },
                        cx,
                    );
                });
                this.sync_floating_windows(cx);
            },
        );
        let maximize = menu_row(
            MotionKey::new("dock-menu-maximize", panel),
            "Maximize Frame",
            theme,
            cx,
            move |this, cx| {
                this.state.update(cx, |state, cx| {
                    state.dock_tree.toggle_maximize(frame_id);
                    state.record_dock_edit(cx);
                });
            },
        );

        let add = (!openable.is_empty()).then(|| {
            let flyout = add_open.then(|| {
                let rows: Vec<AnyElement> = openable
                    .iter()
                    .map(|&(add_panel, title)| {
                        command_row(
                            MotionKey::new("dock-menu-open", add_panel),
                            title,
                            theme,
                            cx,
                            move |this, cx| {
                                this.state.update(cx, |state, cx| {
                                    state.dock_tree.dock(add_panel, frame_id, Zone::Center);
                                    state.record_dock_edit(cx);
                                });
                            },
                        )
                        .debug_selector(move || format!("dock-menu-open-{add_panel}"))
                        .into_any_element()
                    })
                    .collect();
                // Level with the Add Panel row: the card's padding and border
                // above the row are the flyout's own.
                div()
                    .absolute()
                    .top(px(-(Theme::SPACE_XS + 1.0)))
                    .when(flyout_left, |el| el.left_0())
                    .when(!flyout_left, |el| el.left(relative(1.)))
                    .child(
                        anchored()
                            .anchor(if flyout_left {
                                Anchor::TopRight
                            } else {
                                Anchor::TopLeft
                            })
                            .snap_to_window_with_margin(Edges::all(px(Theme::SPACE_SM)))
                            .child(
                                card(theme)
                                    .id(MotionKey::new("dock-menu-add", frame_id)
                                        .element_id("flyout"))
                                    .debug_selector(|| "dock-menu-add-flyout".into())
                                    .children(rows),
                            ),
                    )
            });
            let open_flyout = cx.listener(move |this, hovered: &bool, _, cx| {
                if *hovered && this.open_add_panel(frame_id, panel) {
                    this.redraw_chrome(cx);
                }
            });
            row(
                MotionKey::new("dock-menu-add", frame_id),
                "Add Panel",
                theme,
                open_flyout,
            )
            .debug_selector(|| "dock-menu-add".into())
            .relative()
            .when(add_open, |el| el.bg(theme.element_hover))
            .child(div().flex_none().text_color(theme.text_muted).child("›"))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    if this.open_add_panel(frame_id, panel) {
                        this.redraw_chrome(cx);
                    }
                }),
            )
            .children(flyout)
        });

        let menu = card(theme)
            .id(MotionKey::new("dock-menu-card", frame_id).element_id("card"))
            .debug_selector(|| "dock-frame-menu-card".into())
            .child(close)
            .child(others)
            .child(undock)
            .child(maximize)
            .children(add);
        // At the top of the body, under the tab row, so the frame's menu
        // button stays clear to close it. Deferred so the menu paints over the frames drawn after this one
        // and past this frame's clip.
        deferred(
            div().absolute().top_0().right(px(Theme::SPACE_XS)).child(
                anchored()
                    .anchor(Anchor::TopRight)
                    .snap_to_window_with_margin(Edges::all(px(Theme::SPACE_SM)))
                    .child(motion::fade_quick(
                        MotionKey::new("dock-frame-menu-pop", frame_id),
                        menu,
                    )),
            ),
        )
        .into_any_element()
    }
}

#[cfg(test)]
#[path = "frame_menu_tests.rs"]
mod tests;
