//! `Shell` slot: 4 methods moved out of `shell.rs`.
//!
//! A pure move to stay under the file ceiling. Inherent methods keep their
//! paths wherever they are written, because a method belongs to the type and
//! not to the file, so no caller changes.

use super::*;

impl Shell {
    /// The panels registered at `location` that this state puts on screen, in
    /// registration order.
    ///
    /// The shell draws whatever the registry holds: a built-in panel and a
    /// contributed one take the same path, and a slot with nothing registered
    /// draws nothing rather than falling back to a hardcoded body. A body slot
    /// (the viewer, Home) holds several entries and each one's own predicate says
    /// whether it is the one showing.
    pub(super) fn slot(
        &self,
        location: PanelLocation,
        cx: &mut Context<Self>,
    ) -> Vec<gpui::AnyElement> {
        let host = {
            let read = self.state.read(cx);
            match host_of(&read.ctx) {
                Some(host) => host,
                None => return Vec::new(),
            }
        };
        let entries = host.cached_at(location);
        let mut out = Vec::with_capacity(entries.len());
        for entry in entries.iter() {
            let visible = (entry.visible)(self.state.read(cx));
            if visible {
                out.push((entry.render)(&self.state, cx));
            }
        }
        out
    }

    pub(super) fn overlay(
        &self,
        when: overlays::OverlayWhen,
        active: bool,
        cx: &mut Context<Self>,
    ) -> Vec<gpui::AnyElement> {
        let Some(host) = overlays::host_of(&self.state.read(cx).ctx) else {
            return Vec::new();
        };
        let renders = match when {
            overlays::OverlayWhen::Dialog(kind) => host.dialog(kind, active).into_iter().collect(),
            other => host.matching(other, active),
        };
        let mut out = Vec::with_capacity(renders.len());
        for render in renders {
            out.push(render(&self.state, cx));
        }
        out
    }

    pub(super) fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Mutable, because a pan has to remember the rate it is moving at: the
        // frame the pointer leaves is the only one that can know it, and by then
        // the positions are gone. The borrow ends before the state is touched.
        let Some(drag) = &mut self.drag else {
            return;
        };
        if matches!(drag.kind, DragKind::Pan) {
            drag.sample(f32::from(event.position.x));
        }
        let drag = *drag;
        match drag.kind {
            DragKind::Sidebar => {
                let delta = f32::from(event.position.x) - drag.origin;
                let width = (drag.start + delta).clamp(SIDEBAR_MIN, SIDEBAR_MAX);
                self.state.update(cx, |state, _| {
                    state.ui_settings.sidebar_width = width;
                    state.ui_settings.sidebar_collapsed = false;
                });
            }
            DragKind::Inspector => {
                let delta = drag.origin - f32::from(event.position.x);
                let width = (drag.start + delta).clamp(INSPECTOR_MIN, INSPECTOR_MAX);
                self.state.update(cx, |state, _| {
                    state.ui_settings.inspector_width = width;
                    state.ui_settings.inspector_collapsed = false;
                });
            }
            DragKind::Timeline => {
                let delta = f32::from(event.position.y) - drag.origin;
                let height = (drag.start - delta).max(TIMELINE_MIN_HEIGHT);
                self.state.update(cx, |state, _| {
                    state.ui_settings.timeline_height = height;
                });
            }
            DragKind::Pan => {
                let delta = f32::from(event.position.x) - drag.origin;
                let scroll = (drag.start - delta).max(0.0);
                self.state.update(cx, |state, _| {
                    state.scroll_timeline_to(scroll, false);
                });
            }
            DragKind::Lane(track) => {
                let delta = f32::from(event.position.y) - drag.origin;
                self.state.update(cx, |state, _| {
                    state.set_lane_preview(track, drag.start + delta);
                });
            }
        }
        cx.notify();
    }

    pub(super) fn on_key(
        &mut self,
        event: &KeyDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = event.keystroke.key.as_str();
        let mods = &event.keystroke.modifiers;
        let ctrl = mods.control || mods.platform;
        // A capture in flight outranks every other consumer, text boxes
        // included: while Settings waits for a chord, the next keystroke *is*
        // the chord, and dispatching it would run the very command the operator
        // is trying to move.
        if self.state.read(cx).rebinding().is_some() {
            self.state.update(cx, |state, _| {
                state.capture_rebind(key, ctrl, mods.shift, mods.alt);
            });
            // A captured chord is not text for a box that holds focus.
            cx.stop_propagation();
            cx.notify();
            return;
        }
        // A focused text box swallows the keystroke: while one holds focus every
        // printable key is text, not a shortcut. Which box, and in what order,
        // is `keys::fields` data — focus is explicit, so the bare-letter tool
        // shortcuts still work with a search tab open.
        if let Some(field) = crate::keys::fields::focused(self.state.read(cx)) {
            // Paste is the same event as a run of keystrokes, so it goes through
            // the row's own `push` and keeps that box's own rules.
            if ctrl && key == "v" {
                if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                    self.state
                        .update(cx, |state, _| crate::keys::fields::insert(state, &text));
                }
                cx.stop_propagation();
                cx.notify();
                return;
            }
            let handled = match crate::popover::classify_key(key, mods.platform, ctrl) {
                crate::popover::MenuKey::Escape => {
                    self.state.update(cx, |state, _| (field.escape)(state));
                    true
                }
                crate::popover::MenuKey::Backspace => {
                    self.state.update(cx, |state, _| (field.backspace)(state));
                    true
                }
                crate::popover::MenuKey::Enter => {
                    self.state.update(cx, |state, cx| (field.submit)(state, cx));
                    true
                }
                crate::popover::MenuKey::Up if field.history.is_some() => {
                    let walk = field.history.expect("checked");
                    self.state.update(cx, |state, _| walk(state, -1));
                    true
                }
                crate::popover::MenuKey::Down if field.history.is_some() => {
                    let walk = field.history.expect("checked");
                    self.state.update(cx, |state, _| walk(state, 1));
                    true
                }
                // Printable text is not handled here: the key-down propagates,
                // and the platform commits its `key_char` through the input
                // handler in `text_input.rs`, case and layout intact. Taking the
                // key name here as well would type each character twice.
                _ => false,
            };
            // A key handled here is not text as well: Enter's `key_char` is a
            // newline.
            if handled {
                cx.stop_propagation();
            }
            cx.notify();
            return;
        }

        if self.state.read(cx).dialog == DialogKind::Palette {
            if ctrl && key == "v" {
                if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                    self.state.update(cx, |state, _| {
                        for ch in text.chars().filter(|ch| !ch.is_control()) {
                            state.palette_push(ch);
                        }
                    });
                }
                cx.stop_propagation();
                cx.notify();
                return;
            }
            match crate::popover::classify_key(key, mods.platform, ctrl) {
                crate::popover::MenuKey::Escape => {
                    self.state
                        .update(cx, |state, _| state.open_dialog(DialogKind::None));
                }
                crate::popover::MenuKey::Backspace => {
                    self.state.update(cx, |state, _| state.palette_pop());
                }
                crate::popover::MenuKey::Down => {
                    self.state.update(cx, |state, _| state.palette_move(1));
                }
                crate::popover::MenuKey::Up => {
                    self.state.update(cx, |state, _| state.palette_move(-1));
                }
                crate::popover::MenuKey::Enter => {
                    crate::commands::dispatch(self, None, cx);
                }
                // The input handler writes only to a field, so the palette
                // takes the keystroke's own text: `F` for Shift+F, where the key
                // name is `f`.
                crate::popover::MenuKey::Other if !ctrl => {
                    if let Some(text) = event.keystroke.key_char.as_deref() {
                        self.state.update(cx, |state, _| {
                            for ch in text.chars().filter(|ch| !ch.is_control()) {
                                state.palette_push(ch);
                            }
                        });
                    }
                }
                _ => {}
            }
            cx.stop_propagation();
            cx.notify();
            return;
        }

        // A dialog that is up owns the keyboard, so Enter is its primary action
        // and not the editor's key. Which action, or why there is none, is the
        // dialog registry's answer: the shell presses whatever the row names, so
        // a new dialog is reachable from the keyboard the moment it declares
        // one. The key is consumed either way — a dialog holding the keyboard
        // must not let a chord through to the timeline behind it.
        if self.state.read(cx).dialog != DialogKind::None
            && matches!(
                crate::popover::classify_key(key, mods.platform, ctrl),
                crate::popover::MenuKey::Enter
            )
        {
            self.state.update(cx, |state, _| state.run_dialog_primary());
            cx.stop_propagation();
            cx.notify();
            return;
        }

        let route = if self.state.read(cx).route == UiRoute::Home {
            KeyRoute::Home
        } else {
            KeyRoute::Editor
        };

        // Escape on the editor route is the shell's own key: it cancels the gesture
        // in flight, and a splitter drag lives here rather than on the state, so it
        // cannot be a binding that only sees `AppState`.
        if route == KeyRoute::Editor && key == "escape" && !ctrl {
            if let Some(drag) = self.drag.take() {
                self.state.update(cx, |state, _| match drag.kind {
                    DragKind::Sidebar => state.ui_settings.sidebar_width = drag.start,
                    DragKind::Inspector => state.ui_settings.inspector_width = drag.start,
                    DragKind::Timeline => state.ui_settings.timeline_height = drag.start,
                    DragKind::Pan => {
                        state.scroll_timeline_to(drag.start, false);
                    }
                    DragKind::Lane(_) => {
                        state.take_lane_preview();
                    }
                });
                cx.notify();
                return;
            }
            // A rubber band in flight is a gesture too: abandoning it leaves the
            // selection exactly as the band found it.
            if self.state.read(cx).marquee.is_some() {
                self.state.update(cx, |state, _| state.cancel_marquee());
                cx.notify();
                return;
            }
            // An on-canvas handle drag outranks the rest: it is the gesture in
            // flight, and abandoning it must cost nothing.
            if self.state.read(cx).overlay_dragging() {
                self.state
                    .update(cx, |state, _| state.cancel_overlay_drag());
                cx.notify();
                return;
            }
            if self.state.read(cx).track_dragging() {
                self.state.update(cx, |state, _| state.cancel_track_drag());
                cx.notify();
                return;
            }
            if self.state.read(cx).ramp_dragging().is_some() {
                self.state.update(cx, |state, _| state.cancel_ramp_drag());
                cx.notify();
                return;
            }
            if self.state.read(cx).curve_dragging().is_some() {
                self.state.update(cx, |state, _| state.cancel_curve_drag());
                cx.notify();
                return;
            }
            // Fullscreen outranks the selection: leaving it is what Esc means to
            // an operator who filled the window with the picture.
            self.state.update(cx, |state, _| {
                if state.dialog != DialogKind::None {
                    state.open_dialog(DialogKind::None);
                } else if state.context_menu.is_some() {
                    state.close_context();
                } else if state.open_menu.is_some() {
                    state.close_menu();
                } else if state.leave_preview_fullscreen() {
                } else if !state.tool.selects() {
                    state.tool = EditorTool::default();
                } else {
                    state.clear_selection();
                }
            });
            cx.notify();
            return;
        }

        // Everything else is a binding. The shell holds no chord: the registry does,
        // so a plugin's key is dispatched by the code that dispatches Ctrl+S.
        let binding = crate::keys::host_of(&self.state.read(cx).ctx)
            .and_then(|host| host.resolve(route, key, ctrl, mods.shift, mods.alt));
        let Some(binding) = binding else {
            return;
        };
        self.state.update(cx, |state, cx| match binding.run {
            crate::keys::KeyBody::Plain(run) => run(state),
            crate::keys::KeyBody::Windowed(run) => run(state, cx),
        });
        match binding.after {
            After::Play => start_playback(self.state.clone(), cx),
            After::Preview => spawn_preview(self.state.clone(), cx),
            After::Nothing => {}
        }
        // A keystroke that ran a binding is not text, even for a box the
        // binding focused.
        cx.stop_propagation();
        cx.notify();
    }
}
