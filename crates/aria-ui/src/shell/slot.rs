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
        // A plugin's declared panels follow the registry's, in the order the
        // plugins registered them.
        if let Some(declared) = host.declared() {
            out.extend(declared(location, &self.state, cx));
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
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A press on an empty stretch of the bar moves the window once the
        // pointer travels with the button down; the compositor has the
        // pointer from there.
        if std::mem::take(&mut self.window_move) && event.pressed_button == Some(MouseButton::Left)
        {
            window.start_window_move();
            return;
        }
        // A press on the Viewer's playhead field or mini ruler is held on the
        // state, so the drag keeps going off the field or the strip.
        if self.state.read(cx).viewer_scrub.press().is_some() {
            let state = &self.state;
            if event.pressed_button == Some(MouseButton::Left) {
                let dx = f32::from(event.delta.x);
                let x = f32::from(event.position.x);
                let mods = event.modifiers;
                let moved = state.update(cx, |state, _| {
                    state.drag_viewer(dx, x, mods.shift, mods.control || mods.platform)
                });
                // The playhead field reads relative travel, so past the slop
                // the cursor hides and holds its place, as on a value readout.
                if state.read(cx).viewer_scrub.drag_held() {
                    window.lock_pointer();
                }
                if moved {
                    spawn_preview(self.state.clone(), cx);
                    cx.notify();
                }
            } else if state.update(cx, |state, _| state.release_viewer()) {
                // The button came up outside the window: the gesture is over.
                window.unlock_pointer();
                cx.notify();
            }
            return;
        }
        // A press on the work area bar is held on the state, so a drag keeps
        // moving In and Out after the pointer leaves the 8 px bar.
        if self.state.read(cx).work_area_held() {
            let x = f32::from(event.position.x);
            let moved = if event.pressed_button == Some(MouseButton::Left) {
                self.state.update(cx, |state, _| state.drag_work_area(x))
            } else {
                // The button came up outside the window: the gesture is over.
                self.state.update(cx, |state, _| state.release_work_area())
            };
            if moved {
                cx.notify();
            }
            return;
        }
        // A pressed value readout is held on the state, not as a shell drag:
        // the drag keeps going when the pointer leaves the readout, and the
        // shell only routes the pointer.
        if self.state.read(cx).value_scrubs().pressed() {
            let dx = f32::from(event.delta.x);
            let mods = event.modifiers;
            let state = &self.state;
            if event.pressed_button == Some(MouseButton::Left) {
                let changed = state.update(cx, |state, _| {
                    state.drag_value(dx, mods.shift, mods.control || mods.platform)
                });
                // Past the slop the cursor hides and holds its place, so the
                // drag reads relative travel and never stops at a screen edge.
                if state.read(cx).value_scrubs().drag_held() {
                    window.lock_pointer();
                }
                if changed {
                    spawn_preview(self.state.clone(), cx);
                    cx.notify();
                }
            } else if state.update(cx, |state, _| state.release_value()) {
                // The button came up outside the window: the gesture is over.
                window.unlock_pointer();
                cx.notify();
            }
            return;
        }
        // A held colour wheel, likewise: the drag is the state's, so it keeps
        // going when the pointer leaves the wheel.
        if self.state.read(cx).wheel_held().is_some() {
            let at = [f32::from(event.position.x), f32::from(event.position.y)];
            let state = &self.state;
            let changed = if event.pressed_button == Some(MouseButton::Left) {
                state.update(cx, |state, _| state.drag_wheel(at))
            } else {
                // The button came up outside the window: the drag is over.
                state.update(cx, |state, _| state.release_wheel())
            };
            if changed {
                spawn_preview(self.state.clone(), cx);
                cx.notify();
            }
            return;
        }
        // A press on an Effect Controls time area, likewise: a keyframe drag or
        // a scrub keeps going when the pointer leaves the strip.
        if self.state.read(cx).key_strip_held() {
            let x = f32::from(event.position.x);
            let alt = event.modifiers.alt;
            let state = &self.state;
            let changed = if event.pressed_button == Some(MouseButton::Left) {
                state.update(cx, |state, _| state.drag_key_strip(x, alt))
            } else {
                // The button came up outside the window: the gesture is over.
                state.update(cx, |state, _| state.release_key_strip())
            };
            if changed {
                spawn_preview(self.state.clone(), cx);
                cx.notify();
            }
            return;
        }
        // A plugin panel's slider is held on the state, not as a shell drag:
        // its value is the panel's, and the shell only routes the pointer.
        if self.state.read(cx).plugin_panels.slide.is_some() {
            let at = f32::from(event.position.x);
            let state = &self.state;
            let changed = if event.pressed_button == Some(MouseButton::Left) {
                state.update(cx, |state, _| state.ride_panel_slider(at))
            } else {
                // The button came up outside the window: the drag is over.
                state.update(cx, |state, _| state.release_panel_slider())
            };
            if changed {
                cx.notify();
            }
            return;
        }
        // A viewer overlay's handle, likewise: the plugin moves it, and the
        // shell only routes the pointer.
        if self.state.read(cx).plugin_overlays.grab.is_some() {
            let at = [f32::from(event.position.x), f32::from(event.position.y)];
            let state = &self.state;
            if event.pressed_button == Some(MouseButton::Left) {
                state.update(cx, |state, _| state.carry_overlay_handle(at));
            } else {
                // The button came up outside the window: the drag is over.
                state.update(cx, |state, _| state.release_overlay_handle(at));
            }
            return;
        }
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
            DragKind::Timeline { max } => {
                let delta = f32::from(event.position.y) - drag.origin;
                let height = (drag.start - delta).clamp(TIMELINE_MIN_HEIGHT, max);
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
            DragKind::Outline => {
                let delta = f32::from(event.position.x) - drag.origin;
                self.state.update(cx, |state, _| {
                    state.set_outline_width(drag.start + delta);
                });
            }
            DragKind::DockSplit {
                path,
                depth,
                index,
                axis,
                span_px,
                w0,
                w1,
            } => {
                let pos = match axis {
                    crate::dock::Axis::Row => f32::from(event.position.x),
                    crate::dock::Axis::Column => f32::from(event.position.y),
                };
                let delta = (pos - drag.origin) / span_px.max(100.0);
                let full_path =
                    super::dock_view::active_split_drag_path(&path[..usize::from(depth)]);
                let slice: &[usize] = &full_path;
                self.state.update(cx, |state, _| {
                    if let Some(mut weights) = state.dock_tree.split_weights(slice)
                        && index + 1 < weights.len()
                    {
                        weights[index] = w0;
                        weights[index + 1] = w1;
                        state.dock_tree.set_weights(slice, &weights);
                    }
                    state.drag_split_clamped(slice, index, delta, span_px.max(100.0));
                });
            }
            DragKind::DockTab {
                panel,
                source_frame,
                from_index,
                origin_y,
                moved,
            } => {
                let x = f32::from(event.position.x);
                let y = f32::from(event.position.y);
                let now_moved = moved || (x - drag.origin).hypot(y - origin_y) > 5.0;
                if now_moved
                    && !moved
                    && let Some(d) = &mut self.drag
                {
                    d.kind = DragKind::DockTab {
                        panel,
                        source_frame,
                        from_index,
                        origin_y,
                        moved: true,
                    };
                }
                if now_moved {
                    let hit = {
                        let rects = self.state.read(cx).dock_frame_rects.lock();
                        rects
                            .iter()
                            .find(|(fid, [fx, fy, fw, fh])| {
                                fid.0 != 0
                                    && x >= *fx
                                    && x <= *fx + *fw
                                    && y >= *fy
                                    && y <= *fy + *fh
                            })
                            .map(|(fid, rect)| (*fid, *rect))
                    };
                    if let Some((target_frame, [fx, fy, fw, fh])) = hit {
                        if target_frame == source_frame && y <= fy + 26.0 {
                            let slot_idx = ((x - fx) / 96.0).floor().max(0.0) as usize;
                            self.state.update(cx, |state, _| {
                                state.dock_tree.reorder_panel(source_frame, panel, slot_idx);
                            });
                            self.tab_hover = Some((source_frame, crate::dock::Zone::Center));
                        } else {
                            let rx = ((x - fx) / fw.max(1.0)).clamp(0.0, 1.0);
                            let ry = ((y - fy) / fh.max(1.0)).clamp(0.0, 1.0);
                            let zone = if y <= fy + 26.0 {
                                crate::dock::Zone::Center
                            } else if rx < 0.22 {
                                crate::dock::Zone::Left
                            } else if rx > 0.78 {
                                crate::dock::Zone::Right
                            } else if ry < 0.25 {
                                crate::dock::Zone::Top
                            } else if ry > 0.78 {
                                crate::dock::Zone::Bottom
                            } else {
                                crate::dock::Zone::Center
                            };
                            self.tab_hover = Some((target_frame, zone));
                        }
                    } else {
                        self.tab_hover = None;
                    }
                    super::dock_view::set_active_tab_hover(self.tab_hover);
                }
            }
        }
        cx.notify();
    }

    pub(super) fn on_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = event.keystroke.key.as_str();
        let mods = &event.keystroke.modifiers;
        let ctrl = mods.control || mods.platform;
        // A capture in flight outranks every other consumer, text boxes
        // included: while Settings waits for a chord, the next keystroke *is*
        // the chord, and dispatching it would run the very command being
        // rebound.
        if self.state.read(cx).rebinding().is_some() {
            self.state.update(cx, |state, cx| {
                state.capture_rebind(key, ctrl, mods.shift, mods.alt);
                // Saved now, not at window close: a key rebound before a crash
                // is still rebound at the next launch.
                state.persist_settings_later(cx);
            });
            // A captured chord is not text for a box that holds focus.
            cx.stop_propagation();
            cx.notify();
            return;
        }
        // Escape with a value readout held puts the value back where the press
        // found it. The pointer stays down, but the gesture is over.
        if key == "escape" && self.state.update(cx, |state, _| state.cancel_value_drag()) {
            window.unlock_pointer();
            spawn_preview(self.state.clone(), cx);
            cx.stop_propagation();
            cx.notify();
            return;
        }
        // Escape with a clip bar's line held puts the line back.
        if key == "escape" && self.state.update(cx, |state, _| state.cancel_clip_line()) {
            spawn_preview(self.state.clone(), cx);
            cx.stop_propagation();
            cx.notify();
            return;
        }
        // Escape with the work area bar held puts In and Out back where the
        // press found them.
        if key == "escape"
            && self
                .state
                .update(cx, |state, _| state.cancel_work_area_drag())
        {
            cx.stop_propagation();
            cx.notify();
            return;
        }
        // Escape with a playhead field or the mini ruler held puts the tab in
        // front back on the frame the press found.
        if key == "escape" && self.state.update(cx, |state, _| state.cancel_viewer_drag()) {
            window.unlock_pointer();
            spawn_preview(self.state.clone(), cx);
            cx.stop_propagation();
            cx.notify();
            return;
        }
        // Escape with a colour wheel held puts the grade back where the press
        // found it.
        if key == "escape" && self.state.update(cx, |state, _| state.cancel_wheel_drag()) {
            spawn_preview(self.state.clone(), cx);
            cx.stop_propagation();
            cx.notify();
            return;
        }
        // Escape with a time area held drops the keyframe drag, or puts the
        // playhead back where the press found it.
        if key == "escape" && self.state.update(cx, |state, _| state.cancel_key_strip()) {
            spawn_preview(self.state.clone(), cx);
            cx.stop_propagation();
            cx.notify();
            return;
        }
        // An open drop holds the keyboard ahead of a focused box and the
        // bindings: with File open, `E` lights Export rather than typing or
        // running the E key. Keys it does not answer go on as usual.
        if !ctrl
            && !mods.alt
            && self.state.read(cx).open_menu.is_some()
            && let Some(nav) = crate::app::menu_keys::MenuNav::from_key(key)
        {
            self.state.update(cx, |state, cx| state.menu_key(nav, cx));
            cx.stop_propagation();
            cx.notify();
            return;
        }
        // A focused text box swallows the keystroke: while one holds focus every
        // printable key is text, not a shortcut. Which box, and in what order,
        // is `keys::fields` data — focus is explicit, so the bare-letter tool
        // shortcuts still work with a search tab open.
        if let Some(field) = crate::keys::fields::focused(self.state.read(cx)) {
            // A box with a caret moves it, selects, copies, and steps history
            // on its own keys.
            if let Some(caret) = field.caret {
                if let Some(caret_key) = crate::keys::fields::CaretKey::of(key, ctrl, mods.shift) {
                    self.state
                        .update(cx, |state, _| (caret.key)(state, caret_key, mods.shift));
                    if caret.redraws {
                        spawn_preview(self.state.clone(), cx);
                    }
                    cx.stop_propagation();
                    cx.notify();
                    return;
                }
                if ctrl && matches!(key, "c" | "x") {
                    if let Some(text) = (caret.selected)(self.state.read(cx)) {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
                        if key == "x" {
                            let cut = crate::keys::fields::CaretKey::Delete;
                            self.state
                                .update(cx, |state, _| (caret.key)(state, cut, false));
                            if caret.redraws {
                                spawn_preview(self.state.clone(), cx);
                            }
                        }
                    }
                    cx.stop_propagation();
                    cx.notify();
                    return;
                }
            }
            // Tab moves to the box's neighbour when the row has one; a box
            // without one lets Tab through.
            if key == "tab"
                && !ctrl
                && let Some(tab) = field.tab
            {
                self.state.update(cx, |state, _| tab(state, mods.shift));
                spawn_preview(self.state.clone(), cx);
                cx.stop_propagation();
                cx.notify();
                return;
            }
            // Paste is the same event as a run of keystrokes, so it goes through
            // the row's own `push` and keeps that box's own rules.
            if ctrl && key == "v" {
                if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                    self.state
                        .update(cx, |state, _| crate::keys::fields::insert(state, &text));
                    if field.caret.is_some_and(|caret| caret.redraws) {
                        spawn_preview(self.state.clone(), cx);
                    }
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
                if field.caret.is_some_and(|caret| caret.redraws) {
                    spawn_preview(self.state.clone(), cx);
                }
                cx.stop_propagation();
            }
            // A chord a box with a caret does not answer is the editor's, so
            // Ctrl+S saves mid-edit. A box that only appends keeps every key.
            if handled || field.caret.is_none() || !ctrl {
                cx.notify();
                return;
            }
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
                    crate::commands::dispatch(self, None, window, cx);
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
                self.tab_hover = None;
                super::dock_view::set_active_tab_hover(None);
                self.state.update(cx, |state, _| match drag.kind {
                    DragKind::Timeline { .. } => {
                        state.ui_settings.timeline_height = drag.start;
                    }
                    DragKind::Pan => {
                        state.scroll_timeline_to(drag.start, false);
                    }
                    DragKind::Lane(_) => {
                        state.take_lane_preview();
                    }
                    DragKind::Outline => {
                        state.set_outline_width(drag.start);
                    }
                    DragKind::DockSplit {
                        path,
                        depth,
                        index,
                        w0,
                        w1,
                        ..
                    } => {
                        let full_path =
                            super::dock_view::active_split_drag_path(&path[..usize::from(depth)]);
                        let slice: &[usize] = &full_path;
                        if let Some(mut weights) = state.dock_tree.split_weights(slice)
                            && index + 1 < weights.len()
                        {
                            weights[index] = w0;
                            weights[index + 1] = w1;
                            state.dock_tree.set_weights(slice, &weights);
                            state.sync_panes_from_dock_tree();
                        }
                    }
                    DragKind::DockTab {
                        panel,
                        source_frame,
                        from_index,
                        ..
                    } => {
                        state
                            .dock_tree
                            .reorder_panel(source_frame, panel, from_index);
                    }
                });
                cx.notify();
                return;
            }
            if self.frame_menu.take().is_some() {
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
            // A mask path the Pen is drawing is dropped, not committed.
            if self.state.read(cx).mask_path_drawing() {
                self.state.update(cx, |state, _| state.cancel_mask_path());
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
            // Fullscreen outranks the selection: leaving it is what Esc means
            // while the picture fills the window.
            self.state.update(cx, |state, _| {
                if state.dialog != DialogKind::None {
                    state.open_dialog(DialogKind::None);
                } else if state.context_menu.is_some() {
                    state.close_context();
                } else if state.open_menu.is_some() {
                    state.close_menu();
                } else if state.leave_preview_fullscreen() {
                } else if state.tool_variants_open().is_some() {
                    state.close_tool_variants();
                } else if !state.tool.selects() {
                    state.tool = EditorTool::default();
                } else {
                    state.clear_selection();
                }
            });
            cx.notify();
            return;
        }

        // Enter closes a mask path the Pen is drawing, as a click on its first
        // vertex does.
        if route == KeyRoute::Editor
            && matches!(
                crate::popover::classify_key(key, mods.platform, ctrl),
                crate::popover::MenuKey::Enter
            )
            && self.state.read(cx).mask_path_drawing()
        {
            self.state.update(cx, |state, _| {
                if let Err(err) = state.finish_mask_path() {
                    state.report_error(&err);
                }
            });
            cx.stop_propagation();
            cx.notify();
            return;
        }

        // Everything else is a binding. The shell holds no chord: the registry does,
        // so a plugin's key is dispatched by the code that dispatches Ctrl+S. The
        // focused panel's context answers first, then the window's.
        let binding = {
            let state = self.state.read(cx);
            let focus = crate::keys::focus_route(state);
            state
                .key_host()
                .and_then(|host| host.resolve_focused(focus, key, ctrl, mods.shift, mods.alt))
        };
        let Some(binding) = binding else {
            // An unbound digit or keypad sign over the Timeline or the Program
            // view opens the playhead timecode with that character; the field
            // takes the rest of the typing and Enter.
            if !ctrl && !mods.alt {
                let shift = mods.shift;
                if self
                    .state
                    .update(cx, |state, _| state.type_into_clock(key, shift))
                {
                    cx.stop_propagation();
                    cx.notify();
                }
            }
            return;
        };
        let is_held = event.is_held;
        self.state.update(cx, |state, cx| match binding.run {
            crate::keys::KeyBody::Plain(run) => {
                crate::keys::dispatch_plain_key_down(state, &binding, is_held, run);
            }
            crate::keys::KeyBody::Windowed(run) => run(state, cx),
            crate::keys::KeyBody::Command(_) => {
                state.run_plugin_command(binding.id);
            }
        });
        match binding.after {
            After::Play => {
                let state = self.state.read(cx);
                if state.playing || state.sources.play_rate().is_some() {
                    start_playback(self.state.clone(), window, cx);
                } else {
                    spawn_preview(self.state.clone(), cx);
                }
            }
            After::Preview => spawn_preview(self.state.clone(), cx),
            After::Nothing => {}
        }
        // A keystroke that ran a binding is not text, even for a box the
        // binding focused.
        cx.stop_propagation();
        cx.notify();
    }

    /// The press and release of a bare modifier. A lone Alt opens the menu bar
    /// in the editor, as F10 does, unless a dialog or a key capture holds the
    /// keyboard.
    pub(super) fn on_modifiers(
        &mut self,
        event: &gpui::ModifiersChangedEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ctrl = event.modifiers.control || event.modifiers.platform;
        let alt = event.modifiers.alt;
        let edge_changed = self.state.update(cx, |state, _| {
            state.update_edge_modifiers(state.tool_session.edge_hover, ctrl, alt)
        });
        if edge_changed {
            cx.notify();
        }
        if !self.lone_alt.modifiers(&event.modifiers) {
            return;
        }
        self.state.update(cx, |state, _| {
            if state.route == UiRoute::Editor
                && state.dialog == DialogKind::None
                && state.rebinding().is_none()
            {
                state.toggle_menu_from_keyboard();
            }
        });
        cx.notify();
    }

    pub(super) fn on_key_up(
        &mut self,
        event: &KeyUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = event.keystroke.key.as_str();
        let mods = &event.keystroke.modifiers;
        let ctrl = mods.control || mods.platform;
        let route = if self.state.read(cx).route == UiRoute::Home {
            KeyRoute::Home
        } else {
            KeyRoute::Editor
        };
        let changed = self.state.update(cx, |state, _| {
            crate::keys::dispatch_key_up(state, route, key, ctrl, mods.shift, mods.alt)
        });
        if changed {
            spawn_preview(self.state.clone(), cx);
            cx.notify();
        }
    }
}
