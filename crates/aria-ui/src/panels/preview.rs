//! Preview canvas.

mod handles;
mod mask_path;
mod mini_ruler;
mod motion_path;
mod plugin_overlays;
pub(crate) mod scopes;
mod seek;
mod text_caret;
use crate::app::pool_preview::{self, SourceView};
use crate::app::{AppState, spawn_preview, start_playback};
use crate::fluent::FluentBuilder;
use crate::motion;
use crate::theme::Theme;
use crate::timecode;
use crate::tooltip::WithTip as _;
use crate::types::{ActivePanel, ContextTarget};
use gpui::{
    Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ObjectFit, ParentElement,
    StatefulInteractiveElement, Styled, StyledImage, canvas, div, img, px, relative,
};
pub fn render(
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let theme = Theme::of(cx);
    let playing = state.read(cx).viewer_playing();
    let error = state.read(cx).last_error.clone();
    let zoom_label = state.read(cx).preview_zoom_label();
    let focused = state.read(cx).sources.focused().cloned();
    let split = state.read(cx).sources.split_source().cloned();
    let scopes_open = state.read(cx).scopes_open;
    // Split: the Source tab focused last on the left, Program on the right.
    // Otherwise the focused tab fills the frame.
    let program = (focused.is_none() || split.is_some())
        .then(|| program_pane(state, &theme, cx))
        .map(|pane| compare::beside(pane, state, &theme, cx));
    let program_front = focused.is_none();
    let source = split.clone().or(focused);

    div()
        .id("preview")
        .size_full()
        .flex()
        .flex_col()
        .bg(theme.bg)
        .child(tabs::tab_row(state, &theme, cx))
        .child(
            div()
                .flex_1()
                .min_h_0()
                .flex()
                // A source frame is not the sequence: the guides describe the
                // delivery canvas and the handles edit a clip, so neither is
                // drawn over a pool source.
                .when_some(source, |el, open| {
                    el.child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .relative()
                            .child(source_stage(open, state, &theme, cx)),
                    )
                })
                .when(split.is_some(), |el| {
                    el.child(div().w(px(1.)).h_full().flex_none().bg(theme.border))
                })
                .when_some(program, |el, pane| el.child(pane))
                .when(scopes_open, |el| el.child(scopes::strip(state, &theme, cx))),
        )
        .child(
            // Three groups that never split: what the picture shows, the
            // transport, and the display controls. A Viewer too narrow for
            // one line wraps between groups, so every control stays inside
            // the frame at the minimum window size.
            div()
                .debug_selector(|| "viewer-row".to_owned())
                .flex_none()
                .min_h(px(40.))
                .py(px(Theme::SPACE_XS))
                .px(px(Theme::SPACE_MD))
                .flex()
                .flex_wrap()
                .items_center()
                .justify_between()
                .gap(px(Theme::SPACE_SM))
                .border_t_1()
                .border_color(theme.border)
                .bg(theme.surface)
                .child(
                    row_group("viewer-row-picture")
                        .child(clock_field(state, &theme, cx))
                        .child(zoom_btn(
                            "preview-zoom-out",
                            crate::icons::ZOOM_OUT,
                            cx,
                            |state| state.preview_zoom_out(),
                        ))
                        .child(
                            div()
                                .id("preview-zoom-label")
                                .px(px(Theme::SPACE_XS))
                                .text_size(Theme::TEXT_UI)
                                .text_color(theme.text_muted)
                                .cursor(gpui::CursorStyle::PointingHand)
                                .active(|style| style.opacity(0.92))
                                .when_some(
                                    crate::tooltip::builder("preview-zoom-label"),
                                    |el, tip| el.with_tip(tip),
                                )
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                        // The label is the fit control: clicking the
                                        // number is how an operator gets back to whole.
                                        this.state.update(cx, |state, _| state.preview_zoom_fit());
                                        cx.notify();
                                    }),
                                )
                                .child(zoom_label),
                        )
                        .child(zoom_btn(
                            "preview-zoom-in",
                            crate::icons::ZOOM_IN,
                            cx,
                            |state| state.preview_zoom_in(),
                        ))
                        .child(resolution::picker(state, &theme, cx))
                        .child(resolution::proxy_toggle(state, &theme, cx)),
                )
                .child(
                    row_group("viewer-row-transport")
                        .child(jump_btn("go-start", crate::icons::SKIP_BACK, cx, |state| {
                            state.go_to_start_here()
                        }))
                        .child(step_btn("prev-frame", crate::icons::CHEVRON_LEFT, -1, cx))
                        .child(transport_btn(
                            if playing {
                                crate::icons::PAUSE
                            } else {
                                crate::icons::PLAY
                            },
                            if playing { "pause" } else { "play" },
                            cx,
                        ))
                        .child(step_btn("next-frame", crate::icons::CHEVRON_RIGHT, 1, cx))
                        .child(jump_btn(
                            "go-end",
                            crate::icons::SKIP_FORWARD,
                            cx,
                            |state| state.go_to_end_here(),
                        ))
                        .child(loop_btn(state, &theme, cx))
                        .children(shuttle::badge(state, &theme, cx))
                        .when(program_front, |el| {
                            el.child(mark_btn(
                                "program-mark-in",
                                "In",
                                &theme,
                                cx,
                                AppState::set_mark_in,
                            ))
                            .child(mark_btn(
                                "program-mark-out",
                                "Out",
                                &theme,
                                cx,
                                AppState::set_mark_out,
                            ))
                        }),
                )
                .child(
                    row_group("viewer-row-display")
                        .child(
                            div()
                                .px(px(Theme::SPACE_XS))
                                .text_size(Theme::TEXT_UI)
                                .text_color(theme.text_faint)
                                .child("Handles"),
                        )
                        .child(handles::mode_picker(state, &theme, cx))
                        .child(scopes::toggle(state, &theme, cx))
                        .child(peak_meter(state, &theme, cx)),
                )
                // The last error takes what the row has left and ends in an
                // ellipsis; its tooltip reads in full.
                .when_some(error, |el, err| {
                    el.child(
                        div()
                            .id("viewer-row-error")
                            .max_w_full()
                            .min_w_0()
                            .truncate()
                            .text_size(Theme::TEXT_UI)
                            .text_color(theme.danger)
                            .with_tip(crate::tooltip::text_builder(err.clone()))
                            .child(err),
                    )
                }),
        )
        .child(mini_ruler::strip(state, &theme, cx))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                this.state.update(cx, |state, _| {
                    state.active_panel = ActivePanel::Preview;
                    state.close_context();
                });
                spawn_preview(this.state.clone(), cx);
                cx.notify();
            }),
        )
}

/// The Program picture: the sequence frame with its safe guides, handles,
/// plugin overlays, rulers, and viewer guides. A press anchors a pan and picks
/// the clip under the pointer; a held guide follows the pointer and drops on
/// release.
fn program_pane(
    state: &Entity<AppState>,
    theme: &crate::theme::ThemeHandle,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::Stateful<gpui::Div> {
    let view = state.read(cx).preview_view;
    div()
        .id("preview-program")
        .flex_1()
        .min_w_0()
        .min_h_0()
        .flex()
        .items_center()
        .justify_center()
        .relative()
        .when(view.pannable(), |el| el.cursor(gpui::CursorStyle::OpenHand))
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(|this, event: &MouseDownEvent, _, cx| {
                this.state.update(cx, |state, _| {
                    state.open_context(
                        ContextTarget::Preview,
                        f32::from(event.position.x),
                        f32::from(event.position.y),
                    );
                });
                cx.notify();
            }),
        )
        // One press does two things, and they do not conflict: it anchors a
        // pan, and it picks the clip under the pointer. A miss leaves the
        // selection alone — a click on the letterbox while nudging a title
        // should not throw the title's selection away.
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, event: &MouseDownEvent, _, cx| {
                let point = [f32::from(event.position.x), f32::from(event.position.y)];
                this.preview_pan_anchor = (point[0], point[1]);
                this.state.update(cx, |state, _| {
                    state.select_layer_at(point, event.modifiers.shift);
                });
                cx.notify();
            }),
        )
        // A held guide follows the pointer. Otherwise a zoomed frame is
        // dragged by the pointer: the delta is applied straight to the pan,
        // and a fitted view ignores it. A drag of a handle moves the handle,
        // not the frame under it.
        .on_mouse_move(cx.listener(|this, event: &gpui::MouseMoveEvent, _, cx| {
            let at = [f32::from(event.position.x), f32::from(event.position.y)];
            // The Info panel reads the pixel under the pointer.
            if this
                .state
                .update(cx, |state, _| state.point_viewer(Some(at)))
            {
                cx.notify();
            }
            if this.state.read(cx).viewer_guides.dragging.is_some() {
                this.state.update(cx, |state, _| state.drag_guide(at));
                cx.notify();
                return;
            }
            let read = this.state.read(cx);
            if !event.dragging()
                || !read.preview_view.pannable()
                || read.overlay_dragging()
                || read.plugin_overlays.grab.is_some()
            {
                return;
            }
            let dx = at[0] - this.preview_pan_anchor.0;
            let dy = at[1] - this.preview_pan_anchor.1;
            this.preview_pan_anchor = (at[0], at[1]);
            this.state.update(cx, |state, _| state.pan_preview(dx, dy));
            cx.notify();
        }))
        .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
            if !*hovered && this.state.update(cx, |state, _| state.point_viewer(None)) {
                cx.notify();
            }
        }))
        .on_mouse_up(MouseButton::Left, cx.listener(drop_guide))
        .on_mouse_up_out(MouseButton::Left, cx.listener(drop_guide))
        .child(frame_view(state, theme, cx))
        .children(compare::wipe(state, theme, cx))
        .children(guides(view, theme))
        .child(handles::measure(state))
        .children(handles::overlay(state, theme, cx))
        .children(motion_path::overlay(state, theme, cx))
        .children(mask_path::overlay(state, theme, cx))
        .children(text_caret::overlay(state, theme, cx))
        .children(plugin_overlays::overlay(state, theme, cx))
        .children(rulers::rulers_and_guides(state, theme, cx))
}

/// Release a held guide where the pointer came up: over its ruler or off the
/// frame it is deleted, anywhere else it stays.
fn drop_guide(
    this: &mut crate::shell::Shell,
    event: &gpui::MouseUpEvent,
    _: &mut gpui::Window,
    cx: &mut gpui::Context<crate::shell::Shell>,
) {
    let at = [f32::from(event.position.x), f32::from(event.position.y)];
    let held = this.state.update(cx, |state, _| {
        let held = state.viewer_guides.dragging.is_some();
        if held {
            state.drag_guide(at);
            state.release_guide(at);
        }
        held
    });
    if held {
        cx.notify();
    }
}

/// One group of the Viewer's control row. A group keeps its controls on one
/// line; the row wraps between groups.
fn row_group(selector: &'static str) -> gpui::Div {
    div()
        .debug_selector(move || selector.to_owned())
        .flex_none()
        .flex()
        .items_center()
        .gap(px(Theme::SPACE_SM))
}

// Helpers live in sibling files, declared here. Each is `pub(super)`, so the
// parent and the siblings reach it by name and nothing is left unreachable.
#[path = "preview_compare.rs"]
mod compare;
#[path = "preview_controls.rs"]
mod controls;
#[path = "preview_peak.rs"]
mod peak;
#[path = "preview_resolution.rs"]
mod resolution;
#[path = "preview_rulers.rs"]
mod rulers;
#[path = "preview_shuttle.rs"]
mod shuttle;
#[path = "preview_source.rs"]
mod source;
#[path = "preview_tabs.rs"]
mod tabs;
use controls::{clock_field, jump_btn, loop_btn, mark_btn, step_btn, transport_btn, zoom_btn};
use peak::peak_meter;
use source::{frame_view, guides, source_stage};
