//! On-canvas transform, crop, mask, and clip type handles.
//!
//! The geometry lives in [`crate::app::canvas_overlay`]; this module only places
//! elements at the points it computes and routes the pointer. Two pieces are
//! needed because a pointer event carries window coordinates while an absolutely
//! placed element is offset from its parent: a measuring element records the
//! picture area during paint, and the handles are placed from that record.

use gpui::{
    Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    StatefulInteractiveElement, Styled, WeakEntity, canvas, div, px,
};

use crate::app::AppState;
use crate::app::canvas_overlay::{
    CropHandle, MaskHandle, Rect, TransformHandle, crop_handle_anchor, mask_box, mask_handle_anchor,
};
use crate::components::lit_icon;
use crate::icons;
use crate::surfaces::overlay_gesture::{self, Gesture, Step};
use crate::theme::Theme;
use crate::types::OverlayMode;
use gpui::prelude::FluentBuilder;

/// Handle hit size in pixels. Big enough to grab, small enough not to hide the
/// frame under it. The Program pane keeps half of it clear around the picture.
pub(super) const HANDLE: f32 = 12.0;

/// Records the picture area every paint, so a pointer position can be turned into
/// layer fractions. Draws nothing.
///
/// The repaint is requested only when the rectangle actually moved: a resize, a
/// pan or a transform change has to move the handles, and anything else would be
/// a repaint loop. It redraws what draws the Viewer's picture in `shell`'s
/// window, which holds every reader of the record.
pub(super) fn measure(
    state: &Entity<AppState>,
    shell: WeakEntity<crate::shell::Shell>,
) -> impl IntoElement {
    let state = state.clone();
    canvas(
        move |bounds, window, cx| {
            let rect = Rect::new(
                f32::from(bounds.origin.x),
                f32::from(bounds.origin.y),
                f32::from(bounds.size.width),
                f32::from(bounds.size.height),
            );
            let ratio = window.scale_factor();
            let moved = state.update(cx, |state, _| {
                state.preview_view.pixel_ratio = ratio;
                state.record_overlay_frame(rect)
            });
            if moved {
                crate::shell::redraw_after_draw(shell, crate::shell::Moved::Frame, cx);
            }
        },
        |_, _, _, _| {},
    )
    .absolute()
    .size_full()
}

/// The handle-set picker: one icon toggle per mode, the live one lit. The
/// toggles stand one row step apart, the step every control in the Viewer's
/// row keeps from the next.
///
/// Three toggles rather than one cycling control, because the sets are
/// mutually exclusive and the mask handles should not take two presses to
/// reach. The list comes from
/// [`OverlayMode::ALL`], and the match below has no default arm, so a fourth
/// handle set does not build until it has an icon and a tip.
pub(super) fn mode_picker(
    state: &Entity<AppState>,
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let live = state.read(cx).overlay_mode();
    let mut row = div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(Theme::SPACE_XS));
    for mode in OverlayMode::ALL {
        let on = mode == live;
        let icon = match mode {
            OverlayMode::Transform => {
                lit_icon("preview-handles-transform", icons::TRANSFORM, on, theme)
            }
            OverlayMode::Crop => lit_icon("preview-handles-crop", icons::CROP, on, theme),
            OverlayMode::Mask => lit_icon("preview-handles-mask", icons::MASK, on, theme),
        };
        row = row.child(icon.on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                this.state
                    .update(cx, |state, _| state.set_overlay_mode(mode));
                cx.notify();
            }),
        ));
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
    // A path mask is shaped by its vertices: the box outline and the size
    // handle draw nothing it uses. Its centre and rotation still apply.
    let path_mask = read
        .active_ui_mask()
        .is_some_and(|mask| mask.kind == aria_models::MaskKind::Path);
    // The picture under the Type tool takes a caret, and under the Pen a
    // vertex; with any other tool it moves.
    let move_cursor = if read.tool.types() {
        gpui::CursorStyle::IBeam
    } else if read.tool.pens() {
        gpui::CursorStyle::Crosshair
    } else {
        gpui::CursorStyle::OpenHand
    };
    let type_handles = read.overlay_type_handles();
    let type_colour = read
        .selected_ui_clip_ref()
        .and_then(|clip| clip.clip_type.as_ref())
        .and_then(|clip_type| clip_type.colour)
        .map_or(theme.accent, |colour| {
            theme.label(aria_models::LabelColor::from(colour), false)
        });

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
                    .cursor(move_cursor)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                            // The Type tool places a title and the Pen draws a
                            // mask path; neither moves the picture.
                            let tool = this.state.read(cx).tool;
                            if tool.types() || tool.pens() {
                                return;
                            }
                            let at = [f32::from(event.position.x), f32::from(event.position.y)];
                            this.state.update(cx, |state, _| {
                                overlay_gesture::run(state, Step::Grab(Gesture::Move, at))
                            });
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
                            overlay_gesture::run(state, Step::Grab(Gesture::Scale(handle), at))
                        });
                        cx.notify();
                    }),
                ));
            }
            // A clip type's handles sit above the move surface and the corners,
            // in the type's colour, so a point the generator draws from is
            // grabbed rather than the picture under it.
            for (index, value) in type_handles.into_iter().enumerate() {
                let point = frame.local(layer.at(value));
                root = root.child(handle_dot(
                    crate::motion::MotionKey::new("clip-type-handle", index).into(),
                    point,
                    type_colour,
                    theme,
                    cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                        let at = [f32::from(event.position.x), f32::from(event.position.y)];
                        this.state.update(cx, |state, _| {
                            overlay_gesture::run(state, Step::Grab(Gesture::TypeHandle(index), at))
                        });
                        cx.notify();
                    }),
                ));
            }
        }
        OverlayMode::Crop => {
            // The crop window itself, so the region is visible rather than only
            // the handles that move it.
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
                            overlay_gesture::run(state, Step::Grab(Gesture::Crop(handle), at))
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
                root = root.when(!path_mask, |root| {
                    root.child(
                        div()
                            .absolute()
                            .left(px(outline[0]))
                            .top(px(outline[1]))
                            .w(px(layer.w * mw))
                            .h(px(layer.h * mh))
                            .border_1()
                            .border_color(theme.playhead),
                    )
                });
                for handle in MaskHandle::ALL
                    .into_iter()
                    .filter(|handle| !path_mask || *handle != MaskHandle::Size)
                {
                    let point = frame.local(layer.at(mask_handle_anchor(center, size, handle)));
                    root = root.child(handle_dot(
                        crate::motion::ControlId::Fixed(handle.id()),
                        point,
                        theme.playhead,
                        theme,
                        cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                            let at = [f32::from(event.position.x), f32::from(event.position.y)];
                            this.state.update(cx, |state, _| {
                                overlay_gesture::run(state, Step::Grab(Gesture::Mask(handle), at))
                            });
                            cx.notify();
                        }),
                    ));
                }
            }
        }
    }

    // A drag the press began is carried and released by the shell
    // (`Shell::carry_gesture`), so it follows the pointer off the picture and
    // ends wherever the button comes up.
    Some(root.into_any_element())
}

/// One draggable dot, centered on `point`.
///
/// A press on a dot is the dot's alone. It lies over the move surface and the
/// picture, whose own presses start a move and pick the layer under the
/// pointer, and either would replace the drag the dot just started.
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
        .on_mouse_down(MouseButton::Left, move |event, window, cx| {
            cx.stop_propagation();
            on_down(event, window, cx);
        })
}
