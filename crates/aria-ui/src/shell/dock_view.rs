//! Rendering and interactive docking for [`crate::dock::DockTree`] inside [`Shell`].

use gpui::motion::{Easing, MotionSpec};
use gpui::{
    AnimationExt as _, AnyElement, AppContext, Context, CursorStyle, InteractiveElement,
    IntoElement, KeyDownEvent, MouseButton, MouseDownEvent, ParentElement, SharedString,
    StatefulInteractiveElement as _, Styled, Window, WindowBounds, WindowKind, WindowOptions, div,
    point, px, relative, size,
};

use aria_plugin::PanelLocation;

use crate::dock::{
    Axis, FloatRect, Frame, FrameId, Node, PANEL_INSPECTOR, PANEL_TIMELINE, PANEL_VIEWER, PanelId,
    Zone,
};
use crate::fluent::FluentBuilder;
use crate::motion::{self, MotionKey};
use crate::plugins::panel_host::host_of;
use crate::settings::{MIN_WINDOW_HEIGHT, MIN_WINDOW_WIDTH};
use crate::theme::Theme;
use crate::types::ActivePanel;

use super::Shell;
use super::drag::{Drag, DragKind};
use super::strip::split_row;

const SPLITTER_HIT_PX: f32 = 6.0;
const SPLITTER_LINE_PX: f32 = 1.0;

/// Drop-zone preview fade-in (`docs/spec/layout.md:51-53`: appears 80 ms after
/// the pointer enters the frame, with a 120 ms fade).
pub const DROP_ZONE_SPEC: MotionSpec = MotionSpec::new(120, Easing::EaseOut).with_delay(80);
/// Drop-zone preview follow between zones (`docs/spec/layout.md:52-53`: 120 ms fade).
pub const DROP_ZONE_FOLLOW_SPEC: MotionSpec = MotionSpec::new(120, Easing::EaseOut);
/// Maximize / restore frame transition (`docs/spec/layout.md`: 160 ms decelerating).
pub const MAXIMIZE_SPEC: MotionSpec = MotionSpec::new(160, Easing::EaseOut);
/// Workspace switch split weight morph (`docs/spec/layout.md:81`: 180 ms;
/// `docs/spec/look.md` Motion: panel docking and workspace changes on `ease_out`).
pub const WORKSPACE_MORPH_SPEC: MotionSpec = MotionSpec::new(180, Easing::EaseOut);

std::thread_local! {
    static ACTIVE_SPLIT_DRAG_PATH: std::cell::RefCell<Vec<usize>> =
        const { std::cell::RefCell::new(Vec::new()) };
    static ACTIVE_TAB_HOVER: std::cell::Cell<Option<(FrameId, Zone)>> =
        const { std::cell::Cell::new(None) };
}

/// Store the full split path for an in-flight splitter drag so splits nested
/// deeper than 4 levels are never truncated.
pub(crate) fn set_active_split_drag_path(path: &[usize]) {
    ACTIVE_SPLIT_DRAG_PATH.with(|cell| {
        let mut buf = cell.borrow_mut();
        buf.clear();
        buf.extend_from_slice(path);
    });
}

/// Read the full split path for an in-flight splitter drag, falling back to
/// `fallback` when none was recorded.
pub(crate) fn active_split_drag_path(fallback: &[usize]) -> Vec<usize> {
    ACTIVE_SPLIT_DRAG_PATH.with(|cell| {
        let buf = cell.borrow();
        if buf.is_empty() {
            fallback.to_vec()
        } else {
            buf.clone()
        }
    })
}

/// Share the hovered `(FrameId, Zone)` across the main shell and any floating
/// panel windows so dragging a floating window's tab over a main-window frame
/// highlights and docks into that frame's drop zones.
pub(super) fn set_active_tab_hover(hover: Option<(FrameId, Zone)>) {
    ACTIVE_TAB_HOVER.with(|cell| cell.set(hover));
}

pub(super) fn active_tab_hover() -> Option<(FrameId, Zone)> {
    ACTIVE_TAB_HOVER.with(|cell| cell.get())
}

/// Unique [`MotionKey`] for a splitter at `(path, index)`, encoding every step
/// of `path` so splitters in sibling nested splits at the same depth never
/// collide.
pub fn splitter_motion_key(path: &[usize], index: usize) -> MotionKey {
    let mut key = MotionKey::new("dock-split", path.len());
    for &step in path {
        key = key.with(step);
    }
    key.with(index)
}

/// Fractional `(left, top, width, height)` in `[0.0, 1.0]` of the region a
/// docked panel will occupy when dropped onto `zone` (`docs/spec/layout.md:38-41`).
pub fn drop_zone_preview_fractions(zone: Zone) -> (f32, f32, f32, f32) {
    match zone {
        Zone::Left => (0.0, 0.0, 0.5, 1.0),
        Zone::Right => (0.5, 0.0, 0.5, 1.0),
        Zone::Top => (0.0, 0.0, 1.0, 0.5),
        Zone::Bottom => (0.0, 0.5, 1.0, 0.5),
        Zone::Center => (0.0, 0.0, 1.0, 1.0),
    }
}

/// Clamp a floating window rectangle against available monitor rectangles
/// `[x, y, w, h]`, moving an off-screen rectangle back onto the primary or
/// nearest monitor (`docs/spec/layout.md:48`).
pub fn clamp_float_rect_to_displays(
    mut rect: FloatRect,
    min_size: (f32, f32),
    displays: &[[f32; 4]],
) -> FloatRect {
    let min_w = min_size.0.max(160.0);
    let min_h = min_size.1.max(80.0);
    rect.w = if rect.w.is_finite() {
        rect.w.max(min_w)
    } else {
        480.0_f32.max(min_w)
    };
    rect.h = if rect.h.is_finite() {
        rect.h.max(min_h)
    } else {
        320.0_f32.max(min_h)
    };
    if !rect.x.is_finite() {
        rect.x = 80.0;
    }
    if !rect.y.is_finite() {
        rect.y = 80.0;
    }

    if displays.is_empty() {
        rect.x = rect.x.max(0.0);
        rect.y = rect.y.max(0.0);
        return rect;
    }

    let overlaps_any = displays.iter().any(|&[dx, dy, dw, dh]| {
        let ox = (rect.x + rect.w).min(dx + dw) - rect.x.max(dx);
        let oy = (rect.y + rect.h).min(dy + dh) - rect.y.max(dy);
        ox >= 64.0 && oy >= 32.0
    });
    if overlaps_any {
        return rect;
    }

    let [dx, dy, dw, dh] = displays[0];
    if dw > 0.0 && dh > 0.0 {
        rect.w = rect.w.min(dw.max(min_w));
        rect.h = rect.h.min(dh.max(min_h));
        let max_x = (dx + dw - rect.w).max(dx);
        let max_y = (dy + dh - rect.h).max(dy);
        rect.x = rect.x.clamp(dx, max_x);
        rect.y = rect.y.clamp(dy, max_y);
    }
    rect
}

impl Shell {
    /// Render a single dockable panel's content by id.
    pub(super) fn render_panel_body(
        &self,
        panel: PanelId,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let host = {
            let read = self.state.read(cx);
            match host_of(&read.ctx) {
                Some(host) => host,
                None => return Vec::new(),
            }
        };
        let mut out = Vec::new();
        if let Some(entry) = host.get(panel) {
            out.push((entry.render)(&self.state, cx));
            if panel == PANEL_VIEWER {
                out.extend(self.slot(PanelLocation::Overlay, cx));
            } else if panel == PANEL_INSPECTOR {
                let subs = {
                    let read = self.state.read(cx);
                    host.sub_entries_at(PanelLocation::Inspector, read)
                };
                for sub in subs {
                    out.push(sub(&self.state, cx));
                }
            }
            return out;
        }
        if let Some(declared_one) = host.declared_one()
            && let Some(el) = declared_one(panel, &self.state, cx)
        {
            out.push(el);
        }
        out
    }

    /// Render the editor's docked workspace area (maximized frame, small-window
    /// two-row fallback, or the full split tree).
    pub(super) fn render_editor_dock(
        &mut self,
        theme: &Theme,
        window_w: f32,
        window_h: f32,
        reduced: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if window_w > 0.0 && window_h > 0.0 {
            let (prev_w, prev_h) = self.state.read(cx).last_window_size();
            if (window_w - prev_w).abs() > 0.5 || (window_h - prev_h).abs() > 0.5 {
                self.state.update(cx, |state, _| {
                    state.observe_main_window_size(window_w, window_h);
                });
            }
        }
        if !self.state.read(cx).dock_tree.floating.is_empty() || !self.floating_windows.is_empty() {
            self.sync_floating_windows(cx);
        }

        let (maximized_frame, ws_epoch) = {
            let read = self.state.read(cx);
            let max_frame = read
                .dock_tree
                .maximized
                .and_then(|id| read.dock_tree.frame(id).cloned())
                .filter(|f| f.tabs.iter().any(|&tab| read.is_dock_panel_registered(tab)));
            (max_frame, read.workspace_transition_epoch)
        };

        if let Some(frame) = maximized_frame {
            let max_key = MotionKey::new("dock-maximized", frame.id);
            let inner = self.render_frame(&frame, false, true, theme, cx);
            return div()
                .id(max_key.element_id("max"))
                .flex()
                .flex_1()
                .min_w_0()
                .min_h_0()
                .size_full()
                .child(inner)
                .with_animation(
                    max_key.element_id("max-anim"),
                    motion::animation(MAXIMIZE_SPEC),
                    |el, t| el.opacity(0.4 + 0.6 * t),
                )
                .into_any_element();
        }

        if window_w > 0.0
            && window_h > 0.0
            && (window_w < MIN_WINDOW_WIDTH || window_h < MIN_WINDOW_HEIGHT)
        {
            return self.render_small_window_fallback(theme, reduced, cx);
        }

        let root_el = self.render_dock_node(&[], theme, reduced, cx);
        let ws_key = MotionKey::new("dock-ws-epoch", ws_epoch);
        div()
            .id(ws_key.element_id("ws"))
            .flex()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .size_full()
            .child(root_el)
            .with_animation(
                ws_key.element_id("ws-anim"),
                motion::animation(WORKSPACE_MORPH_SPEC),
                |el, t| el.opacity(0.65 + 0.35 * t),
            )
            .into_any_element()
    }

    /// Below [`MIN_WINDOW_WIDTH`] or [`MIN_WINDOW_HEIGHT`]
    /// (`docs/spec/layout.md:24-31`), show the Viewer (55 % height) and the
    /// Timeline (45 % height) only; the other frames return when the window
    /// grows back to the minimum.
    fn render_small_window_fallback(
        &mut self,
        theme: &Theme,
        _reduced: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let upper = Frame::new(FrameId::CENTER, vec![PANEL_VIEWER]);
        let lower = Frame::new(FrameId::BOTTOM, vec![PANEL_TIMELINE]);
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .size_full()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .h(relative(0.55))
                    .w_full()
                    .min_w_0()
                    .min_h_0()
                    .overflow_hidden()
                    .child(self.render_frame(&upper, false, false, theme, cx)),
            )
            .child(split_row(cx, DragKind::Timeline))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .h(relative(0.45))
                    .w_full()
                    .min_w_0()
                    .min_h_0()
                    .overflow_hidden()
                    .child(self.render_frame(&lower, false, false, theme, cx)),
            )
            .into_any_element()
    }

    fn render_dock_node(
        &mut self,
        path: &[usize],
        theme: &Theme,
        reduced: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        enum ShallowNode {
            Empty,
            Frame(Frame),
            Split {
                axis: Axis,
                all_weights: Vec<f32>,
                drawn_indices: Vec<(usize, (f32, f32))>,
            },
        }
        let shallow = {
            let read = self.state.read(cx);
            match read.dock_tree.node_at(path) {
                None => ShallowNode::Empty,
                Some(Node::Frame(frame)) => ShallowNode::Frame(frame.clone()),
                Some(Node::Split { axis, children }) => {
                    let all_weights: Vec<f32> = children.iter().map(|(w, _)| *w).collect();
                    let drawn_indices: Vec<(usize, (f32, f32))> = children
                        .iter()
                        .enumerate()
                        .filter(|(_, c)| read.node_has_registered_panel(&c.1))
                        .map(|(idx, c)| (idx, read.node_min_size(&c.1)))
                        .collect();
                    ShallowNode::Split {
                        axis: *axis,
                        all_weights,
                        drawn_indices,
                    }
                }
            }
        };
        match shallow {
            ShallowNode::Empty => div().size_full().into_any_element(),
            ShallowNode::Frame(frame) => self.render_frame(&frame, false, false, theme, cx),
            ShallowNode::Split {
                axis,
                all_weights,
                drawn_indices,
            } => {
                if drawn_indices.is_empty() {
                    return div().size_full().into_any_element();
                }
                if drawn_indices.len() == 1 {
                    let (only_idx, _) = drawn_indices[0];
                    let mut child_path = Vec::with_capacity(path.len() + 1);
                    child_path.extend_from_slice(path);
                    child_path.push(only_idx);
                    return self.render_dock_node(&child_path, theme, reduced, cx);
                }

                let splitter_dragging = matches!(
                    self.drag.as_ref().map(|d| &d.kind),
                    Some(DragKind::DockSplit { .. })
                );
                let mut glided_weights = Vec::with_capacity(drawn_indices.len());
                for &(orig_idx, _) in &drawn_indices {
                    let w_key = splitter_motion_key(path, orig_idx);
                    let target_w = all_weights.get(orig_idx).copied().unwrap_or(0.5).max(0.02);
                    let glided = motion::glide::glide(
                        w_key,
                        target_w,
                        WORKSPACE_MORPH_SPEC,
                        splitter_dragging || reduced,
                    );
                    glided_weights.push(glided.max(0.02));
                }
                let weight_sum: f32 = glided_weights.iter().sum::<f32>().max(0.001);

                let mut container = match axis {
                    Axis::Row => div()
                        .flex()
                        .flex_row()
                        .flex_1()
                        .min_w_0()
                        .min_h_0()
                        .size_full(),
                    Axis::Column => div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w_0()
                        .min_h_0()
                        .size_full(),
                };

                for (drawn_pos, &(orig_idx, (min_w, min_h))) in drawn_indices.iter().enumerate() {
                    let mut child_path = Vec::with_capacity(path.len() + 1);
                    child_path.extend_from_slice(path);
                    child_path.push(orig_idx);

                    let inner = self.render_dock_node(&child_path, theme, reduced, cx);
                    let share = (glided_weights[drawn_pos] / weight_sum).clamp(0.02, 0.98);
                    let child_el = match axis {
                        Axis::Row => div()
                            .flex()
                            .flex_col()
                            .w(relative(share))
                            .h_full()
                            .min_w(px(min_w))
                            .min_h(px(min_h))
                            .overflow_hidden()
                            .child(inner)
                            .into_any_element(),
                        Axis::Column => div()
                            .flex()
                            .flex_col()
                            .h(relative(share))
                            .w_full()
                            .min_w(px(min_w))
                            .min_h(px(min_h))
                            .overflow_hidden()
                            .child(inner)
                            .into_any_element(),
                    };
                    container = container.child(child_el);

                    if drawn_pos + 1 < drawn_indices.len() {
                        let next_orig_idx = drawn_indices[drawn_pos + 1].0;
                        if next_orig_idx == orig_idx + 1 {
                            container = container.child(self.render_dock_splitter(
                                path,
                                orig_idx,
                                axis,
                                all_weights.get(orig_idx).copied().unwrap_or(0.5),
                                all_weights.get(next_orig_idx).copied().unwrap_or(0.5),
                                theme,
                                cx,
                            ));
                        } else {
                            container = container.child(self.render_dock_splitter(
                                path,
                                orig_idx,
                                axis,
                                all_weights.get(orig_idx).copied().unwrap_or(0.5),
                                all_weights.get(orig_idx + 1).copied().unwrap_or(0.5),
                                theme,
                                cx,
                            ));
                        }
                    }
                }
                container.into_any_element()
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn render_dock_splitter(
        &self,
        path: &[usize],
        index: usize,
        axis: Axis,
        w0: f32,
        w1: f32,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let full_path: std::sync::Arc<[usize]> = std::sync::Arc::from(path);
        let mut path_arr = [0usize; 4];
        let depth = path.len().min(4);
        path_arr[..depth].copy_from_slice(&path[..depth]);
        let cursor = match axis {
            Axis::Row => CursorStyle::ResizeLeftRight,
            Axis::Column => CursorStyle::ResizeUpDown,
        };
        let motion_slot = splitter_motion_key(path, index);
        let line_color = motion::hover_blend(motion_slot, theme.border, theme.accent);
        let line = match axis {
            Axis::Row => div().w(px(SPLITTER_LINE_PX)).h_full().bg(line_color),
            Axis::Column => div().h(px(SPLITTER_LINE_PX)).w_full().bg(line_color),
        };
        let strip = match axis {
            Axis::Row => div().w(px(SPLITTER_HIT_PX)).h_full(),
            Axis::Column => div().h(px(SPLITTER_HIT_PX)).w_full(),
        };
        strip
            .id(motion_slot.element_id("bar"))
            .flex()
            .items_center()
            .justify_center()
            .flex_none()
            .cursor(cursor)
            .on_hover(motion::hover_listener(motion_slot))
            .child(line)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    if event.click_count >= 2 {
                        let p = full_path.clone();
                        this.state.update(cx, |state, cx| {
                            state.restore_split_weights(&p, cx);
                        });
                        return;
                    }
                    set_active_split_drag_path(&full_path);
                    let root_extent = match axis {
                        Axis::Row => f32::from(window.viewport_size().width).max(200.0),
                        Axis::Column => f32::from(window.viewport_size().height).max(200.0),
                    };
                    let (span_frac, splitters_px) = {
                        let read = this.state.read(cx);
                        let frac = read.dock_tree.split_span_fraction(&full_path, axis);
                        let drawn_count = match read.dock_tree.node_at(&full_path) {
                            Some(Node::Split { children, .. }) => children
                                .iter()
                                .filter(|c| read.node_has_registered_panel(&c.1))
                                .count(),
                            _ => 2,
                        };
                        (
                            frac,
                            (drawn_count.saturating_sub(1) as f32) * SPLITTER_HIT_PX,
                        )
                    };
                    let span_px = (root_extent * span_frac - splitters_px).max(100.0);
                    let origin = match axis {
                        Axis::Row => f32::from(event.position.x),
                        Axis::Column => f32::from(event.position.y),
                    };
                    this.drag = Some(Drag::new(
                        DragKind::DockSplit {
                            path: path_arr,
                            depth: depth as u8,
                            index,
                            axis,
                            span_px,
                            w0,
                            w1,
                        },
                        origin,
                        w0,
                    ));
                }),
            )
            .into_any_element()
    }

    /// Render a single [`Frame`] with its 24 px tab bar, active panel content,
    /// context menu, and drop-zone indicators.
    pub(super) fn render_frame(
        &mut self,
        frame: &Frame,
        is_floating: bool,
        is_maximized: bool,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let frame_id = frame.id;
        let (focused, registered_tabs, min_size) = {
            let read = self.state.read(cx);
            let reg: Vec<PanelId> = frame
                .tabs
                .iter()
                .copied()
                .filter(|&tab| read.is_dock_panel_registered(tab))
                .collect();
            let min_s = read.node_min_size(&Node::Frame(frame.clone()));
            (read.dock_tree.focused == frame_id, reg, min_s)
        };
        if registered_tabs.is_empty() {
            return div().size_full().into_any_element();
        }
        let requested_active = frame.active_panel();
        let active_idx = registered_tabs
            .iter()
            .position(|&p| p == requested_active)
            .unwrap_or(0);
        let active_panel = registered_tabs[active_idx];
        let drawn_frame = Frame {
            id: frame_id,
            tabs: registered_tabs,
            active: active_idx,
        };

        let tab_drag_active = matches!(
            self.drag.as_ref().map(|d| &d.kind),
            Some(DragKind::DockTab { moved: true, .. })
        ) || active_tab_hover().is_some();
        let hovered_zone = self
            .tab_hover
            .or_else(active_tab_hover)
            .filter(|(fid, _)| *fid == frame_id)
            .map(|(_, z)| z);

        let rects = self.state.read(cx).dock_frame_rects.clone();
        let measure = gpui::canvas(
            move |bounds, _, _| {
                rects.lock().insert(
                    frame_id,
                    [
                        (f32::from(bounds.origin.x) - 1.0).max(0.0),
                        (f32::from(bounds.origin.y) - 1.0).max(0.0),
                        f32::from(bounds.size.width) + 2.0,
                        f32::from(bounds.size.height) + 2.0,
                    ],
                );
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full();

        let tab_bar = self.render_tab_bar(
            &drawn_frame,
            active_idx,
            is_floating,
            is_maximized,
            theme,
            cx,
        );
        let body_children = self.render_panel_body(active_panel, cx);
        let menu_overlay = if self.frame_menu.map(|(fid, _)| fid) == Some(frame_id) {
            let target_panel = self.frame_menu.map(|(_, p)| p).unwrap_or(active_panel);
            Some(self.render_frame_menu(frame_id, target_panel, theme, cx))
        } else {
            None
        };
        let zones_overlay = if tab_drag_active {
            Some(self.render_drop_zones(frame_id, hovered_zone, theme, cx))
        } else {
            None
        };

        let frame_slot = MotionKey::new("dock-frame", frame_id);
        div()
            .id(frame_slot.element_id("box"))
            .flex()
            .flex_col()
            .size_full()
            .min_w(px(min_size.0))
            .min_h(px(min_size.1))
            .relative()
            .overflow_hidden()
            .bg(theme.bg)
            .border_1()
            .border_color(if focused { theme.accent } else { theme.border })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                    window.focus(&this.focus, cx);
                    this.state.update(cx, |state, _| {
                        state.dock_tree.focused = frame_id;
                        if let Some(active) = ActivePanel::from_panel_id(active_panel) {
                            state.active_panel = active;
                        }
                    });
                    cx.notify();
                }),
            )
            .child(measure)
            .child(tab_bar)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .relative()
                    .overflow_hidden()
                    .children(body_children)
                    .when_some(menu_overlay, |el, menu| el.child(menu))
                    .when_some(zones_overlay, |el, zones| el.child(zones)),
            )
            .into_any_element()
    }

    fn render_tab_bar(
        &self,
        frame: &Frame,
        active_idx: usize,
        is_floating: bool,
        is_maximized: bool,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let frame_id = frame.id;
        let active_panel = frame.active_panel();
        let mut tabs_out = Vec::with_capacity(frame.tabs.len());
        for (tab_idx, &panel) in frame.tabs.iter().enumerate() {
            let is_active = tab_idx == active_idx;
            let title = self.state.read(cx).dock_panel_title(panel);
            let tab_slot = MotionKey::new("dock-tab", panel);
            let close_slot = MotionKey::new("dock-tab-close", panel);
            tabs_out.push(
                div()
                    .id(tab_slot.element_id("tab"))
                    .debug_selector(move || format!("dock-tab-{panel}"))
                    .h_full()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .px(px(10.0))
                    .cursor(CursorStyle::PointingHand)
                    .bg(if is_active { theme.bg } else { theme.surface })
                    .border_r_1()
                    .border_color(theme.border)
                    .when(is_active, |el| el.border_b_2().border_color(theme.accent))
                    .text_size(Theme::TEXT_UI)
                    .text_color(if is_active {
                        theme.text
                    } else {
                        theme.text_muted
                    })
                    .hover(|s| s.text_color(theme.text))
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
                            if this.frame_menu == Some((frame_id, panel)) {
                                this.frame_menu = None;
                            } else {
                                this.frame_menu = Some((frame_id, panel));
                            }
                            cx.notify();
                        }),
                    )
                    .child(div().min_w_0().truncate().child(title))
                    .child(
                        div()
                            .id(close_slot.element_id("close"))
                            .debug_selector(move || format!("dock-tab-close-{panel}"))
                            .flex_none()
                            .px(px(2.0))
                            .rounded(px(2.0))
                            .text_size(Theme::TEXT_SMALL)
                            .text_color(theme.text_faint)
                            .hover(|s| s.text_color(theme.text).bg(theme.element_hover))
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
                            .child("×"),
                    )
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
            // A floating window's own bar: the empty stretch moves the window
            // when the app draws its frame.
            .child(if is_floating {
                crate::chrome::drag_strip("dock-float-drag", self.frame, cx).into_any_element()
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
                        .text_color(theme.text_muted)
                        .cursor(CursorStyle::PointingHand)
                        .hover(|s| s.text_color(theme.text).bg(theme.element_hover))
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
                        .text_color(theme.text_muted)
                        .cursor(CursorStyle::PointingHand)
                        .hover(|s| s.text_color(theme.text).bg(theme.element_hover))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                                cx.stop_propagation();
                                this.state.update(cx, |state, cx| {
                                    if let Some(pos) = state
                                        .dock_tree
                                        .floating
                                        .iter()
                                        .position(|fl| fl.frame.id == frame_id)
                                    {
                                        state.dock_tree.floating.remove(pos);
                                        state.record_dock_edit(cx);
                                    }
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
                        .text_color(theme.accent)
                        .cursor(CursorStyle::PointingHand)
                        .hover(|s| s.bg(theme.element_hover))
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
                    .px(px(8.0))
                    .h_full()
                    .flex()
                    .items_center()
                    .text_size(Theme::TEXT_UI)
                    .text_color(theme.text_muted)
                    .cursor(CursorStyle::PointingHand)
                    .hover(|s| s.text_color(theme.text).bg(theme.element_hover))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            if this.frame_menu == Some((frame_id, active_panel)) {
                                this.frame_menu = None;
                            } else {
                                this.frame_menu = Some((frame_id, active_panel));
                            }
                            cx.notify();
                        }),
                    )
                    .child("≡"),
            )
            .into_any_element()
    }

    fn render_frame_menu(
        &self,
        frame_id: FrameId,
        panel: PanelId,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let close_slot = MotionKey::new("dock-menu-close", panel);
        let others_slot = MotionKey::new("dock-menu-close-others", panel);
        let undock_slot = MotionKey::new("dock-menu-undock", panel);
        let max_slot = MotionKey::new("dock-menu-maximize", panel);

        let openable: Vec<(PanelId, &'static str)> = {
            let read = self.state.read(cx);
            let ids = read.registered_panel_ids();
            let in_frame: Vec<PanelId> = read
                .dock_tree
                .frame(frame_id)
                .map(|f| f.tabs.clone())
                .unwrap_or_default();
            ids.into_iter()
                .filter(|id| !in_frame.contains(id))
                .map(|id| (id, read.dock_panel_title(id)))
                .collect()
        };

        let mut add_rows = Vec::with_capacity(openable.len());
        for (add_panel, title) in openable {
            let row_slot = MotionKey::new("dock-menu-open", add_panel);
            add_rows.push(
                div()
                    .id(row_slot.element_id("row"))
                    .px(px(10.0))
                    .py(px(4.0))
                    .text_size(Theme::TEXT_TITLE)
                    .text_color(theme.text)
                    .cursor(CursorStyle::PointingHand)
                    .hover(|s| s.bg(theme.element_hover))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            this.frame_menu = None;
                            this.state.update(cx, |state, cx| {
                                state.dock_tree.dock(add_panel, frame_id, Zone::Center);
                                state.record_dock_edit(cx);
                            });
                        }),
                    )
                    .flex()
                    .flex_row()
                    .gap(px(4.0))
                    .child("Add")
                    .child(title)
                    .into_any_element(),
            );
        }

        motion::fade_quick(
            MotionKey::new("dock-frame-menu-pop", frame_id),
            div()
                .id(MotionKey::new("dock-menu-card", frame_id).element_id("card"))
                .absolute()
                .top(px(4.0))
                .right(px(4.0))
                .w(px(200.0))
                .py(px(4.0))
                .rounded(px(Theme::CONTROL_RADIUS))
                .bg(theme.surface_card)
                .border_1()
                .border_color(theme.border_strong)
                .flex()
                .flex_col()
                .child(
                    div()
                        .id(close_slot.element_id("row"))
                        .px(px(10.0))
                        .py(px(4.0))
                        .text_size(Theme::TEXT_TITLE)
                        .text_color(theme.text)
                        .cursor(CursorStyle::PointingHand)
                        .hover(|s| s.bg(theme.element_hover))
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
                        .child("Close Panel"),
                )
                .child(
                    div()
                        .id(others_slot.element_id("row"))
                        .px(px(10.0))
                        .py(px(4.0))
                        .text_size(Theme::TEXT_TITLE)
                        .text_color(theme.text)
                        .cursor(CursorStyle::PointingHand)
                        .hover(|s| s.bg(theme.element_hover))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                this.frame_menu = None;
                                this.state.update(cx, |state, cx| {
                                    state.close_other_dock_tabs(frame_id, panel, cx);
                                });
                            }),
                        )
                        .child("Close Other Tabs in Frame"),
                )
                .child(
                    div()
                        .id(undock_slot.element_id("row"))
                        .px(px(10.0))
                        .py(px(4.0))
                        .text_size(Theme::TEXT_TITLE)
                        .text_color(theme.text)
                        .cursor(CursorStyle::PointingHand)
                        .hover(|s| s.bg(theme.element_hover))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                this.frame_menu = None;
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
                            }),
                        )
                        .child("Undock Panel"),
                )
                .child(
                    div()
                        .id(max_slot.element_id("row"))
                        .px(px(10.0))
                        .py(px(4.0))
                        .text_size(Theme::TEXT_TITLE)
                        .text_color(theme.text)
                        .cursor(CursorStyle::PointingHand)
                        .hover(|s| s.bg(theme.element_hover))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                this.frame_menu = None;
                                this.state.update(cx, |state, cx| {
                                    state.dock_tree.toggle_maximize(frame_id);
                                    state.record_dock_edit(cx);
                                });
                            }),
                        )
                        .child("Maximize Frame"),
                )
                .when(!add_rows.is_empty(), |el| {
                    el.child(div().h(px(1.0)).w_full().my(px(3.0)).bg(theme.border))
                        .children(add_rows)
                }),
        )
        .into_any_element()
    }

    fn render_drop_zones(
        &self,
        frame_id: FrameId,
        hovered: Option<Zone>,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let reduced = cx.reduce_motion();
        let preview_overlay = hovered.map(|zone| {
            let (tl, tt, tw, th) = drop_zone_preview_fractions(zone);
            let base_key = MotionKey::new("dock-drop-follow", frame_id);
            let left_f =
                motion::glide::glide(base_key.with(0usize), tl, DROP_ZONE_FOLLOW_SPEC, reduced);
            let top_f =
                motion::glide::glide(base_key.with(1usize), tt, DROP_ZONE_FOLLOW_SPEC, reduced);
            let width_f =
                motion::glide::glide(base_key.with(2usize), tw, DROP_ZONE_FOLLOW_SPEC, reduced)
                    .clamp(0.05, 1.0);
            let height_f =
                motion::glide::glide(base_key.with(3usize), th, DROP_ZONE_FOLLOW_SPEC, reduced)
                    .clamp(0.05, 1.0);
            let preview_key = MotionKey::new("dock-drop-preview", frame_id);
            div()
                .id(preview_key.element_id("preview"))
                .absolute()
                .left(relative(left_f))
                .top(relative(top_f))
                .w(relative(width_f))
                .h(relative(height_f))
                .bg(theme.accent.opacity(0.10))
                .border_1()
                .border_color(theme.accent)
                .with_animation(
                    preview_key.element_id("preview-anim"),
                    motion::animation(DROP_ZONE_SPEC),
                    |el, t| el.opacity(t),
                )
        });

        let zone_box = |zone: Zone, label: &'static str| {
            let active = hovered == Some(zone);
            let bg = if active {
                theme.accent.opacity(0.28)
            } else {
                theme.surface_card.opacity(0.68)
            };
            let border = if active {
                theme.accent
            } else {
                theme.border_strong
            };
            let base = div()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(Theme::CONTROL_RADIUS))
                .bg(bg)
                .border_1()
                .border_color(border)
                .text_size(Theme::TEXT_SMALL)
                .text_color(theme.text)
                .child(label);
            match zone {
                Zone::Left => base
                    .absolute()
                    .left(px(8.0))
                    .top(relative(0.25))
                    .w(px(44.0))
                    .h(relative(0.5)),
                Zone::Right => base
                    .absolute()
                    .right(px(8.0))
                    .top(relative(0.25))
                    .w(px(44.0))
                    .h(relative(0.5)),
                Zone::Top => base
                    .absolute()
                    .top(px(8.0))
                    .left(relative(0.25))
                    .w(relative(0.5))
                    .h(px(32.0)),
                Zone::Bottom => base
                    .absolute()
                    .bottom(px(8.0))
                    .left(relative(0.25))
                    .w(relative(0.5))
                    .h(px(32.0)),
                Zone::Center => base
                    .absolute()
                    .left(relative(0.32))
                    .top(relative(0.32))
                    .w(relative(0.36))
                    .h(relative(0.36)),
            }
        };

        motion::fade_quick(
            MotionKey::new("dock-zones", frame_id),
            div()
                .debug_selector(|| "dock-drop-zones".into())
                .absolute()
                .size_full()
                .when_some(preview_overlay, |el, preview| el.child(preview))
                .child(zone_box(Zone::Left, "Left"))
                .child(zone_box(Zone::Right, "Right"))
                .child(zone_box(Zone::Top, "Top"))
                .child(zone_box(Zone::Bottom, "Bottom"))
                .child(zone_box(Zone::Center, "Tab")),
        )
        .into_any_element()
    }

    /// Open a GPUI window for any newly floated frame in `dock_tree.floating`
    /// that does not yet have a window handle.
    pub(super) fn sync_floating_windows(&mut self, cx: &mut Context<Self>) {
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
                move |_, app| app.new(|cx| Shell::new_floating(state, frame_id, cx)),
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
    pub(super) fn render_floating_window(
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

        // A release outside the window ends a gesture as one inside it does:
        // the press began in this window, so its release is this window's.
        let release = move |this: &mut Self,
                            _: &gpui::MouseUpEvent,
                            window: &mut Window,
                            cx: &mut Context<Self>| {
            // A press on the tab row that never travelled moves nothing.
            this.window_move = false;
            let Some(drag) = this.drag.take() else {
                return;
            };
            let hover = this.tab_hover.take().or_else(active_tab_hover);
            set_active_tab_hover(None);
            if let DragKind::DockTab {
                panel,
                source_frame,
                moved: true,
                ..
            } = drag.kind
                && let Some((target_frame, zone)) = hover
                && !(target_frame == source_frame && zone == Zone::Center)
            {
                let still_floating = this.state.update(cx, |state, cx| {
                    state.dock_tree.dock(panel, target_frame, zone);
                    state.record_dock_edit(cx);
                    state
                        .dock_tree
                        .floating
                        .iter()
                        .any(|fl| fl.frame.id == frame_id)
                });
                if !still_floating {
                    window.remove_window();
                    return;
                }
            }
            this.persist(cx);
            cx.notify();
        };
        div()
            .id("floating-shell")
            .track_focus(&self.focus)
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(release))
            .on_mouse_up_out(MouseButton::Left, cx.listener(release))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                let key = event.keystroke.key.as_str();
                if key.eq_ignore_ascii_case("escape") {
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
