//! On-canvas crop and mask handles.
//!
//! The geometry lives in [`crate::app::canvas_overlay`]; this module only places
//! elements at the points it computes and routes the pointer. Two pieces are
//! needed because a pointer event carries window coordinates while an absolutely
//! placed element is offset from its parent: a measuring element records the
//! picture area during paint, and the handles are placed from that record.

use gpui::{
    Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, ParentElement, StatefulInteractiveElement, Styled, canvas, div, px,
};

use crate::app::AppState;
use crate::app::canvas_overlay::{
    CropHandle, MaskHandle, Rect, TransformHandle, crop_handle_anchor, mask_box, mask_handle_anchor,
};
use crate::fluent::FluentBuilder;
use crate::surfaces::overlay_gesture::{self, Gesture, Step};
use crate::theme::Theme;
use crate::types::OverlayMode;

/// Handle hit size in pixels. Big enough to grab, small enough not to hide the
/// frame under it.
const HANDLE: f32 = 12.0;

/// One step of a drag, reported rather than swallowed.
///
/// Every handle below hands its step to the registry: what a corner edits is one
/// table there, not a closure here, and a refusal on release reaches the
/// operator instead of the floor.
fn step(state: &mut AppState, step: Step) {
    if let Err(err) = overlay_gesture::apply(state, step) {
        state.report_error(&err);
    }
}

/// Records the picture area every paint, so a pointer position can be turned into
/// layer fractions. Draws nothing.
///
/// The repaint is requested only when the rectangle actually moved: a resize or a
/// transform change has to move the handles, and anything else would be a repaint
/// loop.
pub(super) fn measure(state: &Entity<AppState>) -> impl IntoElement {
    let state = state.clone();
    canvas(
        move |bounds, _, cx| {
            let rect = Rect::new(
                f32::from(bounds.origin.x),
                f32::from(bounds.origin.y),
                f32::from(bounds.size.width),
                f32::from(bounds.size.height),
            );
            state.update(cx, |state, cx| {
                if state.record_overlay_frame(rect) {
                    cx.notify();
                }
            });
        },
        |_, _, _, _| {},
    )
    .absolute()
    .size_full()
}

/// The handle-set picker: one chip per mode, the live one plated.
///
/// Three chips rather than one cycling label, because the sets are mutually
/// exclusive and an operator reaching for the mask handles should not have to
/// read a label and press it twice to get there. The list comes from
/// [`OverlayMode::ALL`], so a fourth handle set arrives with a chip.
pub(super) fn mode_picker(
    state: &Entity<AppState>,
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let live = state.read(cx).overlay_mode();
    let mut row = div()
        .id("preview-handles")
        .flex()
        .flex_row()
        .items_center()
        .gap(px(Theme::SPACE_XXS))
        .when_some(crate::tooltip::builder("preview-handles"), |el, tip| {
            el.tooltip(tip)
        });
    for (index, mode) in OverlayMode::ALL.into_iter().enumerate() {
        let id = gpui::ElementId::from(("preview-handle-mode", index));
        let key = crate::motion::MotionKey::new("handles", &id);
        let selected = mode == live;
        let (rest, hot) = if selected {
            (theme.glass_selected_bg(), theme.element_active)
        } else {
            (theme.wash(0.0), theme.element_hover)
        };
        row = row.child(
            div()
                .id(id)
                .px(px(Theme::SPACE_XS))
                .py(px(Theme::SPACE_XXS))
                .rounded(px(Theme::CONTROL_RADIUS))
                .text_size(px(11.))
                .text_color(crate::motion::hover_blend(
                    key,
                    if selected {
                        theme.text
                    } else {
                        theme.text_muted
                    },
                    theme.text,
                ))
                .bg(crate::motion::hover_blend(key, rest, hot))
                .cursor(gpui::CursorStyle::PointingHand)
                .active(|style| style.opacity(0.92))
                .on_hover(crate::motion::hover_listener(key))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        this.state
                            .update(cx, |state, _| state.set_overlay_mode(mode));
                        cx.notify();
                    }),
                )
                .child(mode.label()),
        );
    }
    row
}

/// The handles for the selection, or nothing when there is no selection, no
/// painted geometry yet, or a clip whose quad the resolver leaves relative (a
/// text or shape generator).
///
/// Which set is drawn is the state's answer, not this module's: a crop corner and
/// a scale corner land on the same pixel when the crop is full.
pub(super) fn overlay(
    state: &Entity<AppState>,
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Option<gpui::AnyElement> {
    let read = state.read(cx);
    if !read.selected_has_framing() {
        return None;
    }
    let frame = read.overlay_frame;
    let layer = frame.layer?;
    let mode = read.overlay_mode();
    let crop = read.overlay_crop()?;
    let mask = read.overlay_mask();
    let dragging = read.overlay_dragging();

    let mut root = div().absolute().size_full();

    match mode {
        OverlayMode::Transform => {
            // The quad itself is the move surface: pressing the picture and
            // dragging is what every editor means by moving a layer.
            let origin = frame.local(layer.at([0.0, 0.0]));
            root = root.child(
                div()
                    .id("transform-move")
                    .absolute()
                    .left(px(origin[0]))
                    .top(px(origin[1]))
                    .w(px(layer.w))
                    .h(px(layer.h))
                    .border_1()
                    .border_color(theme.accent)
                    .cursor(gpui::CursorStyle::OpenHand)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                            let at = [f32::from(event.position.x), f32::from(event.position.y)];
                            this.state
                                .update(cx, |state, _| step(state, Step::Grab(Gesture::Move, at)));
                            cx.notify();
                        }),
                    ),
            );
            for handle in TransformHandle::ALL {
                let point = frame.local(layer.at(handle.anchor()));
                root = root.child(handle_dot(
                    crate::motion::ControlId::Fixed(handle.id()),
                    point,
                    theme.accent,
                    theme,
                    cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                        let at = [f32::from(event.position.x), f32::from(event.position.y)];
                        this.state.update(cx, |state, _| {
                            step(state, Step::Grab(Gesture::Scale(handle), at))
                        });
                        cx.notify();
                    }),
                ));
            }
        }
        OverlayMode::Crop => {
            // The crop window itself, so the operator sees the region rather than
            // only the handles that move it.
            let window_origin = frame.local(layer.at([crop.x, crop.y]));
            root = root.child(
                div()
                    .absolute()
                    .left(px(window_origin[0]))
                    .top(px(window_origin[1]))
                    .w(px(layer.w * crop.w))
                    .h(px(layer.h * crop.h))
                    .border_1()
                    .border_color(theme.accent),
            );
            for handle in CropHandle::ALL {
                let point = frame.local(layer.at(crop_handle_anchor(crop, handle)));
                root = root.child(handle_dot(
                    crate::motion::ControlId::Fixed(handle.id()),
                    point,
                    theme.accent,
                    theme,
                    cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                        let at = [f32::from(event.position.x), f32::from(event.position.y)];
                        this.state.update(cx, |state, _| {
                            step(state, Step::Grab(Gesture::Crop(handle), at))
                        });
                        cx.notify();
                    }),
                ));
            }
        }
        OverlayMode::Mask => {
            if let Some((center, size, _rotation)) = mask {
                let [mx, my, mw, mh] = mask_box(center, size);
                let outline = frame.local(layer.at([mx, my]));
                root = root.child(
                    div()
                        .absolute()
                        .left(px(outline[0]))
                        .top(px(outline[1]))
                        .w(px(layer.w * mw))
                        .h(px(layer.h * mh))
                        .border_1()
                        .border_color(theme.playhead),
                );
                for handle in MaskHandle::ALL {
                    let point = frame.local(layer.at(mask_handle_anchor(center, size, handle)));
                    root = root.child(handle_dot(
                        crate::motion::ControlId::Fixed(handle.id()),
                        point,
                        theme.playhead,
                        theme,
                        cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                            let at = [f32::from(event.position.x), f32::from(event.position.y)];
                            this.state.update(cx, |state, _| {
                                step(state, Step::Grab(Gesture::Mask(handle), at))
                            });
                            cx.notify();
                        }),
                    ));
                }
            }
        }
    }

    // Pointer routing lives on the whole picture area rather than on each handle:
    // a drag that runs off a 12-pixel dot must keep tracking, and the geometry
    // clamps it to the layer anyway.
    let root = root
        .when(dragging, |el| {
            el.on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                this.state.update(cx, |state, _| {
                    step(
                        state,
                        Step::Carry([f32::from(event.position.x), f32::from(event.position.y)]),
                    );
                });
                cx.notify();
            }))
        })
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(|this, _: &MouseUpEvent, _, cx| {
                this.state.update(cx, |state, _| step(state, Step::Release));
                cx.notify();
            }),
        );
    Some(root.into_any_element())
}

/// One draggable dot, centered on `point`.
fn handle_dot(
    id: crate::motion::ControlId,
    point: [f32; 2],
    color: gpui::Hsla,
    theme: &Theme,
    on_down: impl Fn(&MouseDownEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> impl IntoElement {
    div()
        .id(id.element("handle"))
        .absolute()
        .left(px(point[0] - HANDLE * 0.5))
        .top(px(point[1] - HANDLE * 0.5))
        .w(px(HANDLE))
        .h(px(HANDLE))
        .rounded(px(HANDLE * 0.5))
        .bg(color)
        .border_1()
        .border_color(theme.on_solid)
        .cursor(gpui::CursorStyle::PointingHand)
        .active(|style| style.opacity(0.92))
        .on_mouse_down(MouseButton::Left, on_down)
}
