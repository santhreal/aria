//! Preview canvas.

mod handles;
mod mask_path;
mod mini_ruler;
mod motion_path;
mod plugin_overlays;
pub(crate) mod scopes;
mod text_caret;
use crate::app::pool_preview::{self, SourceView};
use crate::app::{AppState, spawn_preview, start_playback};
use crate::components::Wraps as _;
use crate::motion;
use crate::shell::PreviewPan;
use crate::theme::Theme;
use crate::timecode;
use crate::tooltip::WithTip as _;
use crate::types::{ActivePanel, ContextTarget};
use gpui::prelude::FluentBuilder;
use gpui::{
    Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ObjectFit, ParentElement,
    ScrollWheelEvent, StatefulInteractiveElement, Styled, StyledImage, canvas, div, img, px,
    relative,
};
pub fn render(
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let theme = Theme::of(cx);
    let playing = state.read(cx).viewer_playing();
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
        .children(tabs::tab_row(state, &theme, cx))
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
            // transport, and the display controls. The two outer groups split
            // the room the transport leaves evenly, so the transport stands on
            // the Viewer's centre line whenever each outer group fits its
            // half. A Viewer too narrow for one line wraps between groups, so
            // every control stays inside the frame at the minimum window size.
            div()
                .debug_selector(|| "viewer-row".to_owned())
                .flex_none()
                .min_h(px(40.))
                .py(px(Theme::SPACE_XS))
                .px(px(Theme::SPACE_MD))
                .flex()
                .wraps()
                .items_center()
                .justify_center()
                .gap(px(Theme::SPACE_SM))
                .border_t_1()
                .border_color(theme.border)
                .bg(theme.surface)
                .child(
                    row_group("viewer-row-picture")
                        .flex_1()
                        .child(clock_field(state, &theme, cx))
                        .child(zoom_btn(
                            "preview-zoom-out",
                            crate::icons::ZOOM_OUT,
                            cx,
                            |state| state.preview_zoom_out(),
                        ))
                        .child(zoom::picker(state, &theme, cx))
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
                        .flex_1()
                        .justify_end()
                        .child(handles::mode_picker(state, &theme, cx))
                        .child(scopes::toggle(state, &theme, cx))
                        .child(peak_meter(state, &theme, cx))
                        .child(crate::chrome::tool_strip::display_btn(
                            "viewer-display-settings",
                            crate::chrome::menu_view_rows::VIEWER_DISPLAY,
                            Theme::ICON_HIT,
                            &theme,
                            state,
                            cx,
                        )),
                ),
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
/// plugin overlays, rulers, and viewer guides. A left press picks the clip
/// under the pointer; a left or middle press held pans a zoomed frame, and
/// the shell carries the pan and a held guide wherever the pointer goes
/// (`Shell::carry_gesture`). Ctrl+wheel (Cmd on macOS) zooms about the
/// pointer.
///
/// The picture area stands half a handle inside the pane on every side. A
/// fitted frame touches the picture area on two sides, and a handle on the
/// frame's edge stands half outside the frame, so the margin keeps every
/// handle of a fitted frame whole.
fn program_pane(
    state: &Entity<AppState>,
    theme: &crate::theme::ThemeHandle,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::Stateful<gpui::Div> {
    let view = state.read(cx).preview_view;
    let pannable = state.read(cx).preview_pannable();
    div()
        .id("preview-program")
        .flex_1()
        .min_w_0()
        .min_h_0()
        .flex()
        .p(px(handles::HANDLE / 2.0))
        .when(pannable, |el| el.cursor(gpui::CursorStyle::OpenHand))
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
        // One press does two things, and they do not conflict: it holds a pan,
        // and it picks the clip under the pointer. A miss leaves the
        // selection alone — a click on the letterbox while nudging a title
        // should not throw the title's selection away.
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, event: &MouseDownEvent, _, cx| {
                let at = [f32::from(event.position.x), f32::from(event.position.y)];
                this.preview_pan = Some(PreviewPan {
                    at,
                    button: MouseButton::Left,
                });
                this.state.update(cx, |state, _| {
                    state.select_layer_at(at, event.modifiers.shift);
                });
                cx.notify();
            }),
        )
        // The middle button pans and picks nothing.
        .on_mouse_down(
            MouseButton::Middle,
            cx.listener(|this, event: &MouseDownEvent, _, _| {
                this.preview_pan = Some(PreviewPan {
                    at: [f32::from(event.position.x), f32::from(event.position.y)],
                    button: MouseButton::Middle,
                });
            }),
        )
        // A level a notch, up or away zooming in; a trackpad's pixels add up
        // to whole notches. A plain wheel leaves the picture alone.
        .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _, cx| {
            if !event.modifiers.secondary() {
                return;
            }
            cx.stop_propagation();
            this.viewer_wheel += crate::transport::wheel_notches(event);
            let notches = this.viewer_wheel.trunc();
            this.viewer_wheel -= notches;
            let at = [f32::from(event.position.x), f32::from(event.position.y)];
            if notches != 0.0
                && this
                    .state
                    .update(cx, |state, _| state.zoom_preview_at(notches as i32, at))
            {
                cx.notify();
            }
        }))
        // The Info panel reads the pixel under the pointer, through the route
        // a probe reading from the preview worker takes.
        .on_mouse_move(cx.listener(|this, event: &gpui::MouseMoveEvent, _, cx| {
            let at = [f32::from(event.position.x), f32::from(event.position.y)];
            if this
                .state
                .update(cx, |state, _| state.point_viewer(Some(at)))
            {
                this.redraw(crate::shell::Moved::Frame, cx);
            }
        }))
        .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
            if !*hovered && this.state.update(cx, |state, _| state.point_viewer(None)) {
                this.redraw(crate::shell::Moved::Frame, cx);
            }
        }))
        .child(
            div()
                .relative()
                .flex_1()
                .min_w_0()
                .min_h_0()
                .flex()
                .items_center()
                .justify_center()
                .child(frame_view(state, theme, cx))
                .children(compare::wipe(state, theme, cx))
                .children(guides(view, theme))
                .child(handles::measure(state, cx.entity().downgrade()))
                .children(handles::overlay(state, theme, cx))
                .children(motion_path::overlay(state, theme, cx))
                .children(mask_path::overlay(state, theme, cx))
                .children(text_caret::overlay(state, theme, cx))
                .children(plugin_overlays::overlay(state, theme, cx))
                .children(rulers::rulers_and_guides(state, theme, cx)),
        )
}

/// One group of the Viewer's control row. A group keeps its controls on one
/// line; the row wraps between groups.
fn row_group(selector: &'static str) -> gpui::Div {
    div()
        .debug_selector(move || selector.to_owned())
        .flex_none()
        .flex()
        .items_center()
        .gap(px(Theme::SPACE_XS))
}

// Helpers live in sibling files, declared here. Each is `pub(super)`, so the
// parent and the siblings reach it by name and nothing is left unreachable.
#[path = "preview_compare.rs"]
mod compare;
#[path = "preview_controls.rs"]
mod controls;
#[path = "preview_menu.rs"]
mod menu;
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
#[cfg(test)]
#[path = "viewer_row_height_tests.rs"]
mod viewer_row_height_tests;
#[cfg(test)]
#[path = "viewer_seek_tests.rs"]
mod viewer_seek_tests;
#[cfg(test)]
#[path = "viewer_words_tests.rs"]
mod viewer_words_tests;
#[cfg(test)]
#[path = "viewer_zoom_tests.rs"]
mod viewer_zoom_tests;
#[path = "preview_zoom.rs"]
mod zoom;
use controls::{clock_field, jump_btn, loop_btn, mark_btn, step_btn, transport_btn, zoom_btn};
use peak::peak_meter;
use source::{frame_view, guides, source_stage};
