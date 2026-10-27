//! Floating panel windows: opening a window for each floated frame and drawing
//! a floated frame inside its window.

use super::*;

impl Shell {
    /// Open a GPUI window for any newly floated frame in `dock_tree.floating`
    /// that does not yet have a window handle.
    pub(in crate::shell) fn sync_floating_windows(&mut self, cx: &mut Context<Self>) {
        let unopened: Vec<crate::dock::Floating> = {
            let read = self.state.read(cx);
            self.floating_windows
                .retain(|(fid, _)| read.dock_tree.floating.iter().any(|fl| fl.frame.id == *fid));
            read.dock_tree
                .floating
                .iter()
                .filter(|fl| {
                    !self
                        .floating_windows
                        .iter()
                        .any(|(fid, _)| *fid == fl.frame.id)
                })
                .cloned()
                .collect()
        };
        if unopened.is_empty() {
            return;
        }

        let displays: Vec<[f32; 4]> = cx
            .displays()
            .into_iter()
            .map(|d| {
                let b = d.bounds();
                [
                    f32::from(b.origin.x),
                    f32::from(b.origin.y),
                    f32::from(b.size.width),
                    f32::from(b.size.height),
                ]
            })
            .collect();

        for fl in unopened {
            let (min_size, title) = {
                let read = self.state.read(cx);
                let ms = read.node_min_size(&Node::Frame(fl.frame.clone()));
                let active_title = read.dock_panel_title(fl.frame.active_panel());
                (ms, active_title)
            };
            let clamped = clamp_float_rect_to_displays(
                FloatRect {
                    x: fl.x,
                    y: fl.y,
                    w: fl.w,
                    h: fl.h,
                },
                min_size,
                &displays,
            );
            if (clamped.x - fl.x).abs() > 0.5
                || (clamped.y - fl.y).abs() > 0.5
                || (clamped.w - fl.w).abs() > 0.5
                || (clamped.h - fl.h).abs() > 0.5
            {
                self.state.update(cx, |state, _| {
                    if let Some(entry) = state
                        .dock_tree
                        .floating
                        .iter_mut()
                        .find(|entry| entry.frame.id == fl.frame.id)
                    {
                        entry.x = clamped.x;
                        entry.y = clamped.y;
                        entry.w = clamped.w;
                        entry.h = clamped.h;
                    }
                });
            }

            let state = self.state.clone();
            let frame_id = fl.frame.id;
            let bounds = gpui::Bounds::new(
                point(px(clamped.x), px(clamped.y)),
                size(px(clamped.w), px(clamped.h)),
            );
            match cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(gpui::TitlebarOptions {
                        title: Some(SharedString::from(title)),
                        ..Default::default()
                    }),
                    window_min_size: Some(size(px(min_size.0), px(min_size.1))),
                    kind: WindowKind::Normal,
                    ..Default::default()
                },
                move |window, app| {
                    // The window manager's close control closes the frame's
                    // panels, as Close Window does. Without it the frame stays
                    // floating with no window, and its panels cannot be reached.
                    let closing = state.clone();
                    window.on_window_should_close(app, move |_, cx| {
                        closing.update(cx, |state, cx| {
                            state.close_floating_frame(frame_id, cx);
                        });
                        true
                    });
                    app.new(|cx| Shell::new_floating(state, frame_id, cx))
                },
            ) {
                Ok(handle) => {
                    self.floating_windows.push((frame_id, handle.into()));
                }
                Err(err) => {
                    self.state.update(cx, |state, cx| {
                        state.handle_float_open_failure(frame_id, err.to_string(), cx);
                    });
                }
            }
        }
    }

    /// Render a floating panel window (`self.floating_frame == Some(frame_id)`).
    pub(in crate::shell) fn render_floating_window(
        &mut self,
        frame_id: FrameId,
        theme: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let fl_opt = {
            let read = self.state.read(cx);
            read.dock_tree
                .floating
                .iter()
                .find(|fl| fl.frame.id == frame_id)
                .cloned()
        };
        let Some(fl) = fl_opt else {
            window.remove_window();
            return div().size_full().into_any_element();
        };

        let active_title = self
            .state
            .read(cx)
            .dock_panel_title(fl.frame.active_panel());
        self.set_title(
            crate::chrome::window_frame::WindowTitle::Panel(active_title),
            window,
        );

        let wb = window.bounds();
        let wx = f32::from(wb.origin.x);
        let wy = f32::from(wb.origin.y);
        let ww = f32::from(wb.size.width);
        let wh = f32::from(wb.size.height);
        if ww >= 160.0
            && wh >= 80.0
            && ((wx - fl.x).abs() > 1.0
                || (wy - fl.y).abs() > 1.0
                || (ww - fl.w).abs() > 1.0
                || (wh - fl.h).abs() > 1.0)
        {
            self.state.update(cx, |state, _| {
                if let Some(entry) = state
                    .dock_tree
                    .floating
                    .iter_mut()
                    .find(|entry| entry.frame.id == frame_id)
                {
                    entry.x = wx;
                    entry.y = wy;
                    entry.w = ww;
                    entry.h = wh;
                }
            });
        }

        div()
            .id("floating-shell")
            .track_focus(&self.focus)
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .map(|root| Self::ends_drags(root, cx))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                let key = event.keystroke.key.as_str();
                if key.eq_ignore_ascii_case("escape") {
                    // A gesture in flight is abandoned first, as in the main
                    // window.
                    if this.cancel_gesture(window, cx) {
                        cx.stop_propagation();
                        cx.notify();
                        return;
                    }
                    let has_modal = {
                        let s = this.state.read(cx);
                        s.dialog != crate::app::DialogKind::None
                            || s.rename.is_some()
                            || s.context_menu.is_some()
                            || this.frame_menu.is_some()
                    };
                    if !has_modal {
                        // Return focus to the main window (`docs/spec/layout.md:50`).
                        this.state.update(cx, |state, _| {
                            state.dock_tree.focused = FrameId::CENTER;
                        });
                        for w in cx.windows() {
                            if w != window.window_handle() {
                                let _ = w.update(cx, |_, main_win, _| {
                                    main_win.activate_window();
                                });
                                break;
                            }
                        }
                        cx.notify();
                        return;
                    }
                }
                this.on_key(event, window, cx);
            }))
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.bg)
            .text_color(theme.text)
            .font_family(theme.font_sans.clone())
            .text_size(Theme::TEXT_UI)
            .child(self.render_frame(&fl.frame, true, false, theme, cx))
            .into_any_element()
    }
}
