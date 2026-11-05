//! A dock frame's tab row and the drop zone drawn over the frame a tab is
//! dragged onto.

use super::*;
use crate::shell::panel_view::TabRow;

impl Shell {
    pub(in crate::shell) fn render_tab_bar(
        &self,
        row: &TabRow,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let TabRow {
            frame,
            floating: is_floating,
            maximized: is_maximized,
            window,
            ..
        } = row;
        let (is_floating, is_maximized) = (*is_floating, *is_maximized);
        let active_idx = frame.active;
        let frame_id = frame.id;
        let active_panel = frame.active_panel();
        // The width the frame drew at last; a frame not drawn yet is not
        // compact and draws every tab.
        let width = self
            .state
            .read(cx)
            .dock_frame_rect(frame_id)
            .map(|[_, _, w, _]| w);
        let compact = width.is_some_and(|w| w < DOCK_TAB_COMPACT_W);
        let pad = if compact {
            Theme::SPACE_XS
        } else {
            Theme::SPACE_SM
        };
        let titles: Vec<&'static str> = {
            let read = self.state.read(cx);
            frame
                .tabs
                .iter()
                .map(|&p| read.dock_panel_title(p))
                .collect()
        };
        let font = tab_fit::tab_font(theme);
        let gap = if compact { 0.0 } else { Theme::SPACE_XS };
        let least: Vec<f32> = titles
            .iter()
            .map(|title| tab_fit::least_width(title, pad, gap, &font, cx))
            .collect();
        // The row less the frame's border and the controls after the tabs:
        // the menu button, and a floating or maximized frame's own buttons.
        let avail = width.map(|w| {
            let button = |label: &str, pad: f32| tab_fit::label_width(label, &font, cx) + 2.0 * pad;
            let mut controls = button("≡", pad);
            if is_floating {
                controls += button("Dock", 6.0) + button("Close Window", 6.0);
            }
            if is_maximized {
                controls += button("Restore", 6.0);
            }
            w - 2.0 - controls
        });
        let drawn = match avail {
            Some(avail) if frame.tabs.len() > 1 => tab_fit::fit(avail, &least, active_idx),
            _ => vec![true; frame.tabs.len()],
        };
        let hidden: Vec<PanelId> = frame
            .tabs
            .iter()
            .zip(&drawn)
            .filter(|&(_, &shown)| !shown)
            .map(|(&panel, _)| panel)
            .collect();
        // Short of room for the drawn tabs whole, every close gives up its
        // width before a title gives up a letter, and then the widest tabs
        // narrow to one shared width, so a short title stays whole while a
        // longer one takes the ellipsis. Shrinking in proportion, an active
        // close would stay a sliver and every title would lose its last
        // letters.
        let shown: Vec<usize> = (0..frame.tabs.len()).filter(|&at| drawn[at]).collect();
        let narrowed: Option<Vec<f32>> = avail.and_then(|avail| {
            let more = if hidden.is_empty() {
                0.0
            } else {
                tab_fit::MORE_W
            };
            let room = avail - more;
            let whole: Vec<f32> = shown
                .iter()
                .map(|&at| tab_fit::whole_width(titles[at], pad, gap, &font, cx))
                .collect();
            let closed: f32 = whole.iter().map(|width| width + DOCK_TAB_CLOSE).sum();
            (closed > room).then(|| {
                let narrowest: Vec<f32> = shown.iter().map(|&at| least[at]).collect();
                tab_fit::squeeze(room, &whole, &narrowest)
            })
        });
        let squeezed = narrowed.is_some();
        let mut tabs_out = Vec::with_capacity(shown.len());
        for (slot, &tab_idx) in shown.iter().enumerate() {
            let panel = frame.tabs[tab_idx];
            let is_active = tab_idx == active_idx;
            let title = titles[tab_idx];
            let tab_slot = MotionKey::new("dock-tab", panel);
            let close_slot = MotionKey::new("dock-tab-close", panel);
            tabs_out.push(
                div()
                    .relative()
                    .id(tab_slot.element_id("tab"))
                    .debug_selector(move || format!("dock-tab-{panel}"))
                    // A tab narrower than its title still reads it under the
                    // pointer.
                    .with_tip(crate::tooltip::text_builder(title))
                    .h_full()
                    // A tab shrinks to its title's first letters and stops:
                    // narrower than that, the row lists it behind the overflow
                    // control instead.
                    .min_w(px(least[tab_idx]))
                    // Squeezed, the tab stands at its share of the row; flex
                    // shrink takes up what the measured widths miss.
                    .when_some(narrowed.as_ref().map(|widths| widths[slot]), |el, width| {
                        el.w(px(width))
                    })
                    .flex()
                    .items_center()
                    .when(!compact, |el| el.gap(px(Theme::SPACE_XS)))
                    .px(px(pad))
                    .cursor(CursorStyle::PointingHand)
                    .bg(if is_active { theme.bg } else { theme.surface })
                    .border_r_1()
                    .border_color(theme.border)
                    .text_size(Theme::TEXT_UI)
                    // A hover fade redraws the row alone. A gpui hover style
                    // here would notify the view the row is built in, which is
                    // the Shell while the editor arrives, and a Shell
                    // notification rebuilds the panel bodies.
                    .text_color(if is_active {
                        theme.text
                    } else {
                        motion::hover_blend(tab_slot, theme.text_muted, theme.text)
                    })
                    .on_hover(motion::hover_listener(tab_slot))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                            cx.stop_propagation();
                            window.focus(&this.focus, cx);
                            this.frame_menu = None;
                            if event.click_count >= 2 {
                                this.state.update(cx, |state, cx| {
                                    state.dock_tree.toggle_maximize(frame_id);
                                    state.record_dock_edit(cx);
                                });
                                return;
                            }
                            this.state.update(cx, |state, cx| {
                                state.activate_dock_panel(panel, cx);
                            });
                            let x = f32::from(event.position.x);
                            let y = f32::from(event.position.y);
                            this.drag = Some(Drag::new(
                                DragKind::DockTab {
                                    panel,
                                    source_frame: frame_id,
                                    from_index: tab_idx,
                                    origin_y: y,
                                    moved: false,
                                },
                                x,
                                x,
                            ));
                        }),
                    )
                    .on_mouse_down(
                        MouseButton::Middle,
                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            this.state.update(cx, |state, cx| {
                                state.close_dock_panel(panel, cx);
                            });
                        }),
                    )
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            this.toggle_frame_menu(frame_id, panel);
                            cx.notify();
                        }),
                    )
                    .child(div().min_w_0().truncate().child(title))
                    .child(
                        div()
                            .id(close_slot.element_id("close"))
                            .debug_selector(move || format!("dock-tab-close-{panel}"))
                            // The close gives up its width before the title
                            // gives up a character, so a narrow tab reads as
                            // its name rather than as a lone close.
                            .flex_shrink(DOCK_TAB_CLOSE_SHRINK)
                            .min_w_0()
                            .w(px(if squeezed { 0.0 } else { DOCK_TAB_CLOSE }))
                            .h(px(DOCK_TAB_CLOSE))
                            .overflow_hidden()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(Theme::CONTROL_RADIUS))
                            .text_color(motion::hover_blend(
                                close_slot,
                                theme.text_faint,
                                theme.text,
                            ))
                            .bg(motion::hover_blend(
                                close_slot,
                                theme.wash(0.0),
                                theme.element_hover,
                            ))
                            .on_hover(motion::hover_listener(close_slot))
                            // Only the active tab shows its close at rest; the
                            // others fade it in under the pointer, in a place
                            // already laid out, so no title moves.
                            .when(!is_active, |el| el.opacity(motion::hover_t(tab_slot)))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                    cx.stop_propagation();
                                    this.frame_menu = None;
                                    this.state.update(cx, |state, cx| {
                                        state.close_dock_panel(panel, cx);
                                    });
                                }),
                            )
                            .child(
                                crate::icons::icon(crate::icons::CLOSE)
                                    .flex_none()
                                    .size(px(DOCK_TAB_CLOSE_GLYPH)),
                            ),
                    )
                    .when(is_active, |el| {
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
                    .into_any_element(),
            );
        }

        let bar_slot = MotionKey::new("dock-tabbar", frame_id);
        let menu_btn_slot = MotionKey::new("dock-frame-menu", frame_id);
        let redock_slot = MotionKey::new("dock-float-redock", frame_id);
        let close_float_slot = MotionKey::new("dock-float-close", frame_id);
        let restore_slot = MotionKey::new("dock-frame-restore", frame_id);

        div()
            .id(bar_slot.element_id("bar"))
            .h(px(Theme::TAB_ROW_HEIGHT))
            .w_full()
            .flex()
            .flex_row()
            .items_center()
            .bg(theme.surface)
            .border_b_1()
            .border_color(theme.border)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    window.focus(&this.focus, cx);
                    this.frame_menu = None;
                    if event.click_count >= 2 {
                        this.state.update(cx, |state, cx| {
                            state.dock_tree.toggle_maximize(frame_id);
                            state.record_dock_edit(cx);
                        });
                    }
                }),
            )
            // The tabs give up width before the controls do: a narrow frame
            // truncates its titles and keeps its menu.
            .child(
                div()
                    .h_full()
                    .min_w_0()
                    .flex()
                    .flex_row()
                    .overflow_hidden()
                    .children(tabs_out),
            )
            // The tabs that did not fit, behind one control at the row's end.
            .when(!hidden.is_empty(), |el| {
                el.child(self.render_tab_overflow(frame_id, &hidden, theme, cx))
            })
            // A floating window's own bar: the empty stretch moves the window
            // when the app draws its frame.
            .child(if is_floating {
                crate::chrome::drag_strip("dock-float-drag", *window, cx).into_any_element()
            } else {
                div().flex_1().h_full().into_any_element()
            })
            .when(is_floating, |el| {
                el.child(
                    div()
                        .id(redock_slot.element_id("btn"))
                        .flex_none()
                        .px(px(6.0))
                        .h_full()
                        .flex()
                        .items_center()
                        .text_size(Theme::TEXT_UI)
                        .cursor(CursorStyle::PointingHand)
                        .hover_fade(redock_slot, theme.text_muted, theme.text, theme)
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                                cx.stop_propagation();
                                this.state.update(cx, |state, cx| {
                                    state.redock_floating_frame(frame_id, cx);
                                });
                                window.remove_window();
                            }),
                        )
                        .child("Dock"),
                )
                .child(
                    div()
                        .id(close_float_slot.element_id("btn"))
                        .flex_none()
                        .px(px(6.0))
                        .h_full()
                        .flex()
                        .items_center()
                        .text_size(Theme::TEXT_UI)
                        .cursor(CursorStyle::PointingHand)
                        .hover_fade(close_float_slot, theme.text_muted, theme.text, theme)
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                                cx.stop_propagation();
                                this.state.update(cx, |state, cx| {
                                    state.close_floating_frame(frame_id, cx);
                                });
                                window.remove_window();
                            }),
                        )
                        .child("Close Window"),
                )
            })
            .when(is_maximized, |el| {
                el.child(
                    div()
                        .id(restore_slot.element_id("btn"))
                        .flex_none()
                        .px(px(6.0))
                        .h_full()
                        .flex()
                        .items_center()
                        .text_size(Theme::TEXT_UI)
                        .cursor(CursorStyle::PointingHand)
                        .hover_fade(restore_slot, theme.accent, theme.accent, theme)
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                this.state.update(cx, |state, cx| {
                                    state.dock_tree.toggle_maximize(frame_id);
                                    state.record_dock_edit(cx);
                                });
                            }),
                        )
                        .child("Restore"),
                )
            })
            .child(
                div()
                    .id(menu_btn_slot.element_id("btn"))
                    .debug_selector(move || format!("dock-frame-menu-{}", frame_id.0))
                    .flex_none()
                    .px(px(pad))
                    .h_full()
                    .flex()
                    .items_center()
                    .text_size(Theme::TEXT_UI)
                    .cursor(CursorStyle::PointingHand)
                    .hover_fade(menu_btn_slot, theme.text_muted, theme.text, theme)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            this.toggle_frame_menu(frame_id, active_panel);
                            cx.notify();
                        }),
                    )
                    .child("≡"),
            )
            .into_any_element()
    }

    /// The drop zone over the frame a dragged tab is over: a translucent accent
    /// rectangle on the half of the frame, or the whole frame, the panel takes
    /// when let go in `zone`. It follows the pointer between zones.
    pub(super) fn render_drop_zone(
        &self,
        frame_id: FrameId,
        zone: Zone,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let reduced = cx.reduce_motion();
        let (tl, tt, tw, th) = drop_zone_preview_fractions(zone);
        let base_key = MotionKey::new("dock-drop-follow", frame_id);
        let glide = |part: usize, to: f32| {
            motion::glide::glide(base_key.with(part), to, DROP_ZONE_FOLLOW_SPEC, reduced)
        };
        let (left, top) = (glide(0, tl), glide(1, tt));
        let (width, height) = (glide(2, tw).clamp(0.05, 1.0), glide(3, th).clamp(0.05, 1.0));
        let preview_key = MotionKey::new("dock-drop-preview", frame_id);
        div()
            .id(preview_key.element_id("preview"))
            .debug_selector(move || format!("dock-drop-zone-{}", frame_id.0))
            .absolute()
            .left(relative(left))
            .top(relative(top))
            .w(relative(width))
            .h(relative(height))
            .bg(theme.accent.opacity(0.10))
            .border_1()
            .border_color(theme.accent)
            .with_animation(
                preview_key.element_id("preview-anim"),
                motion::animation(DROP_ZONE_SPEC),
                |el, t| el.opacity(t),
            )
            .into_any_element()
    }
}
