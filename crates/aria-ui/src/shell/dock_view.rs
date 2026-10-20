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
    Axis, DOCK_GUTTER, FloatRect, Frame, FrameId, Node, PANEL_TIMELINE, PANEL_VIEWER, PanelId, Zone,
};
use crate::fluent::FluentBuilder;
use crate::motion::{self, MotionKey};
use crate::plugins::panel_host::host_of;
use crate::settings::{MIN_WINDOW_HEIGHT, MIN_WINDOW_WIDTH};
use crate::theme::Theme;
use crate::tooltip::WithTip as _;
use crate::types::ActivePanel;

use super::Shell;
use super::drag::{Drag, DragKind};
use super::strip::split_row;

#[path = "dock_floating.rs"]
mod dock_floating;
#[path = "dock_tabs.rs"]
mod dock_tabs;
#[path = "frame_menu.rs"]
pub(super) mod frame_menu;
#[path = "frame_page.rs"]
mod frame_page;
#[path = "tab_fit.rs"]
mod tab_fit;

const SPLITTER_LINE_PX: f32 = 1.0;

/// A dock tab's close: its box and its glyph.
const DOCK_TAB_CLOSE: f32 = 16.0;
const DOCK_TAB_CLOSE_GLYPH: f32 = 12.0;
/// How much faster a tab's close shrinks than its title. The close is
/// `DOCK_TAB_CLOSE` px against a title of several times that, so a factor this
/// large hands it all of a narrow tab's deficit until it is gone.
const DOCK_TAB_CLOSE_SHRINK: f32 = 64.0;
/// Frame width under which the tab row is compact: tabs and the menu button
/// keep `SPACE_XS` of padding instead of `SPACE_SM`, and a title stands with no
/// gap before its close, so a frame as narrow as the 64 px Audio Meters frame
/// still reads the start of its title.
const DOCK_TAB_COMPACT_W: f32 = 120.0;

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

/// A frame header control that fades from `rest` text on no wash to `hover`
/// text on the hover wash under the pointer. The fade redraws the window
/// chrome alone, where a gpui hover style on Shell-drawn chrome notifies the
/// Shell and rebuilds every panel body.
trait HoverFade: Styled + gpui::StatefulInteractiveElement + Sized {
    fn hover_fade(
        self,
        key: MotionKey,
        rest: gpui::Hsla,
        hover: gpui::Hsla,
        theme: &Theme,
    ) -> Self {
        self.text_color(motion::hover_blend(key, rest, hover))
            .bg(motion::hover_blend(
                key,
                theme.wash(0.0),
                theme.element_hover,
            ))
            .on_hover(motion::hover_listener(key))
    }
}

impl<E: Styled + gpui::StatefulInteractiveElement> HoverFade for E {}

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
    /// Render a single dockable panel's content by id, on its page when it
    /// has one (see [`frame_page`]). The page's scroller is drawn inside the
    /// panel's body, so a scroll redraws that body alone.
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
            }
            return frame_page::page(panel, out);
        }
        if let Some(declared_one) = host.declared_one()
            && let Some(el) = declared_one(panel, &self.state, cx)
        {
            out.push(el);
        }
        frame_page::page(panel, out)
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
        // At rest the gutter is the only mark between two frames: a line in it
        // would draw a third rule beside the two frame borders.
        let line_color = motion::hover_blend(motion_slot, theme.wash(0.0), theme.accent);
        let line = match axis {
            Axis::Row => div().w(px(SPLITTER_LINE_PX)).h_full().bg(line_color),
            Axis::Column => div().h(px(SPLITTER_LINE_PX)).w_full().bg(line_color),
        };
        let strip = match axis {
            Axis::Row => div().w(px(DOCK_GUTTER)).h_full(),
            Axis::Column => div().h(px(DOCK_GUTTER)).w_full(),
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
                    // The docked frames stand a gutter in from the window's
                    // edges on both sides.
                    let root_extent = match axis {
                        Axis::Row => f32::from(window.viewport_size().width),
                        Axis::Column => f32::from(window.viewport_size().height),
                    };
                    let root_extent = (root_extent - 2.0 * DOCK_GUTTER).max(200.0);
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
                        (frac, (drawn_count.saturating_sub(1) as f32) * DOCK_GUTTER)
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
            move |bounds, window, _| {
                let rect = [
                    (f32::from(bounds.origin.x) - 1.0).max(0.0),
                    (f32::from(bounds.origin.y) - 1.0).max(0.0),
                    f32::from(bounds.size.width) + 2.0,
                    f32::from(bounds.size.height) + 2.0,
                ];
                let was = rects.lock().insert(frame_id, rect);
                // The tab row fits its tabs to the width drawn last, so a frame
                // drawn at a new width draws once more.
                if was.is_none_or(|was| (was[2] - rect[2]).abs() > 0.5) {
                    window.request_animation_frame();
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full();

        let row = super::panel_view::TabRow {
            overflow: self.overflow_open(frame_id),
            window: self.frame,
            frame: drawn_frame,
            floating: is_floating,
            maximized: is_maximized,
        };
        let tab_bar = self.tab_row(row, theme, cx);
        let frame_slot = MotionKey::new("dock-frame", frame_id);
        let mut body_children = vec![self.panel_body(active_panel, cx)];
        // The timeline's playhead is painted over its body on every frame the
        // shell draws, so a move of the playhead alone replays the body.
        if active_panel == PANEL_TIMELINE {
            let reduced = cx.reduce_motion();
            let layer = crate::timeline_playhead::layer(self.state.read(cx), theme, reduced);
            body_children.push(layer.into_any_element());
        }
        let menu_overlay = match self.frame_menu {
            Some(frame_menu::FrameMenu::Panel { frame, panel, .. }) if frame == frame_id => {
                Some(self.render_frame_menu(frame_id, panel, theme, cx))
            }
            _ => None,
        };
        let zones_overlay = if tab_drag_active {
            Some(self.render_drop_zones(frame_id, hovered_zone, theme, cx))
        } else {
            None
        };

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
}

#[cfg(test)]
#[path = "dock_tab_tests.rs"]
mod dock_tab_tests;

#[cfg(test)]
#[path = "panel_fit_tests.rs"]
mod panel_fit_tests;

#[cfg(test)]
#[path = "frame_height_tests.rs"]
mod frame_height_tests;
#[cfg(test)]
#[path = "spacing_tests.rs"]
mod spacing_tests;
#[cfg(test)]
#[path = "tab_overflow_tests.rs"]
mod tab_overflow_tests;
#[cfg(test)]
#[path = "workspace_fit_tests.rs"]
mod workspace_fit_tests;
