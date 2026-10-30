//! Plugin viewer overlays over the sequence frame.
//!
//! Lines, rectangles, ellipses and paths are tessellated by
//! [`crate::app::plugin_overlays`] once per scene and canvas rectangle and
//! painted from that cache. Text and handles are placed as elements above
//! them, since a handle takes the pointer. Everything is clipped to the
//! picture area. While a handle is held the shell routes the pointer, so a
//! drag keeps tracking off the Viewer.

use std::sync::Arc;

use aria_plugin::{HandleLook, OverlayColor, OverlayShape};
use gpui::{
    ContentMask, Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    ParentElement, StatefulInteractiveElement, Styled, canvas, div, px,
};

use crate::app::AppState;
use crate::app::canvas_overlay::{Rect, view_frame};
use crate::app::plugin_overlays::{CanvasMap, Paint};
use crate::motion::MotionKey;
use crate::theme::{Theme, ThemeHandle};
use crate::types::ActivePanel;

/// Handle hit size in pixels, the same as the transform handles.
const HANDLE: f32 = 12.0;
/// The thickness of a cross handle's bars.
const BAR: f32 = 2.0;

/// The screen colour of `color`.
fn hsla(theme: &Theme, color: OverlayColor) -> gpui::Hsla {
    match color {
        OverlayColor::Accent => theme.accent,
        OverlayColor::Text => theme.text,
        OverlayColor::Muted => theme.text_muted,
        OverlayColor::Success => theme.success,
        OverlayColor::Warning => theme.warning,
        OverlayColor::Danger => theme.danger,
        OverlayColor::Rgba([r, g, b, a]) => gpui::Rgba {
            r: f32::from(r) / 255.0,
            g: f32::from(g) / 255.0,
            b: f32::from(b) / 255.0,
            a: f32::from(a) / 255.0,
        }
        .into(),
    }
}

/// Every overlay's scene, or nothing when no overlay has drawn one.
pub(super) fn overlay(
    state: &Entity<AppState>,
    theme: &ThemeHandle,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Option<gpui::AnyElement> {
    let read = state.read(cx);
    if !read.plugin_overlays.any_scene() {
        return None;
    }
    let view = read.preview_view;
    let size = read.canvas_size();
    let frame = read.overlay_frame;
    let map = read.overlay_map();
    let shown = Arc::clone(read.plugin_overlays.shown());

    let painter = state.clone();
    let colors = theme.clone();
    let shapes = canvas(
        move |bounds, _, cx| {
            let container = Rect::new(
                f32::from(bounds.origin.x),
                f32::from(bounds.origin.y),
                f32::from(bounds.size.width),
                f32::from(bounds.size.height),
            );
            let map = CanvasMap::new(view_frame(container, view, size), size)?;
            Some(painter.update(cx, |state, _| state.overlay_batches(map, container)))
        },
        move |bounds, batches, window, _| {
            let Some(batches) = batches else {
                return;
            };
            window.with_content_mask(Some(ContentMask::from(bounds)), |window| {
                for batch in batches.iter() {
                    let (Paint::Fill(color) | Paint::Stroke(color, _)) = batch.paint;
                    window.paint_path(batch.path.clone(), hsla(&colors, color));
                }
            });
        },
    )
    .absolute()
    .size_full();

    let mut root = div().absolute().size_full().overflow_hidden().child(shapes);
    // Placed from the rectangle the last paint measured, as the transform
    // handles are; the first paint records it and asks for the frame that
    // places these.
    let Some(map) = map else {
        return Some(root.into_any_element());
    };
    for state in shown.iter() {
        let Some(scene) = &state.scene else {
            continue;
        };
        for shape in &scene.shapes {
            match shape {
                OverlayShape::Text {
                    at,
                    text,
                    color,
                    size: text_size,
                } => {
                    let [x, y] = frame.local(map.to_window(*at));
                    root = root.child(
                        div()
                            .absolute()
                            .left(px(x))
                            .top(px(y))
                            .whitespace_nowrap()
                            .text_size(px(*text_size))
                            .text_color(hsla(theme, *color))
                            .child(gpui::SharedString::from(text)),
                    );
                }
                OverlayShape::Handle {
                    id,
                    at,
                    color,
                    look,
                } => {
                    let point = frame.local(map.to_window(*at));
                    let key = MotionKey::new("plugin-overlay", &state.spec.id).with(id);
                    let spec = Arc::clone(&state.spec);
                    let handle = Arc::clone(id);
                    root = root.child(handle_mark(
                        key,
                        point,
                        hsla(theme, *color),
                        *look,
                        theme,
                        cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                            // The press is the handle's: no pan anchor, and no
                            // pick of the clip under it.
                            cx.stop_propagation();
                            let at = [f32::from(event.position.x), f32::from(event.position.y)];
                            this.state.update(cx, |state, _| {
                                state.active_panel = ActivePanel::Preview;
                                state.close_context();
                                state.grab_overlay_handle(&spec.id, &handle, at);
                            });
                            cx.notify();
                        }),
                    ));
                }
                OverlayShape::Line { .. }
                | OverlayShape::Rect { .. }
                | OverlayShape::Ellipse { .. }
                | OverlayShape::Path { .. } => {}
            }
        }
    }
    Some(root.into_any_element())
}

/// One draggable handle, centred on `point`.
fn handle_mark(
    key: MotionKey,
    point: [f32; 2],
    color: gpui::Hsla,
    look: HandleLook,
    theme: &Theme,
    on_down: impl Fn(&MouseDownEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> impl IntoElement {
    let hit = div()
        .id(key.element_id("handle"))
        .absolute()
        .left(px(point[0] - HANDLE * 0.5))
        .top(px(point[1] - HANDLE * 0.5))
        .w(px(HANDLE))
        .h(px(HANDLE))
        .cursor(gpui::CursorStyle::PointingHand)
        .active(|style| style.opacity(0.92))
        .on_mouse_down(MouseButton::Left, on_down);
    match look {
        HandleLook::Dot => hit
            .rounded(px(HANDLE * 0.5))
            .bg(color)
            .border_1()
            .border_color(theme.on_solid),
        HandleLook::Square => hit
            .rounded(px(Theme::CONTROL_RADIUS))
            .bg(color)
            .border_1()
            .border_color(theme.on_solid),
        HandleLook::Cross => hit
            .child(
                div()
                    .absolute()
                    .left(px(0.0))
                    .top(px((HANDLE - BAR) * 0.5))
                    .w(px(HANDLE))
                    .h(px(BAR))
                    .bg(color),
            )
            .child(
                div()
                    .absolute()
                    .left(px((HANDLE - BAR) * 0.5))
                    .top(px(0.0))
                    .w(px(BAR))
                    .h(px(HANDLE))
                    .bg(color),
            ),
    }
}
