//! Preview canvas.

mod handles;
mod scopes;
mod seek;
use crate::app::pool_preview::{self, SourceView};
use crate::app::{AppState, spawn_preview, start_playback};
use crate::fluent::FluentBuilder;
use crate::motion;
use crate::theme::Theme;
use crate::timecode;
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
    let playing = state.read(cx).playing;
    let error = state.read(cx).last_error.clone();
    let ticks = state.read(cx).store.playhead_ticks;
    let name = state.read(cx).store.project_name.clone();
    let rate = state.read(cx).store.sequence.frame_rate;
    let tc = timecode::format_ticks(ticks, rate.num, rate.den);
    let view = state.read(cx).preview_view;
    let zoom_label = state.read(cx).preview_zoom_label();
    let source = state.read(cx).source_view.clone();
    let scopes_open = state.read(cx).scopes_open;

    div()
        .id("preview")
        .size_full()
        .flex()
        .flex_col()
        .bg(theme.bg)
        .child(
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .child(
                    div()
                        .flex_1()
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
                        // One press does two things, and they do not conflict:
                        // it anchors a pan, and it picks the clip under the
                        // pointer. A miss leaves the selection alone — a click
                        // on the letterbox while nudging a title should not
                        // throw the title's selection away.
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, event: &MouseDownEvent, _, cx| {
                                let point =
                                    [f32::from(event.position.x), f32::from(event.position.y)];
                                this.preview_pan_anchor = (point[0], point[1]);
                                this.state.update(cx, |state, _| {
                                    state.select_layer_at(point);
                                });
                                cx.notify();
                            }),
                        )
                        // A zoomed frame is dragged by the pointer: the delta is applied
                        // straight to the pan, and a fitted view ignores it.
                        .on_mouse_move(cx.listener(|this, event: &gpui::MouseMoveEvent, _, cx| {
                            if !event.dragging() || !this.state.read(cx).preview_view.pannable() {
                                return;
                            }
                            let dx = f32::from(event.position.x) - this.preview_pan_anchor.0;
                            let dy = f32::from(event.position.y) - this.preview_pan_anchor.1;
                            this.preview_pan_anchor =
                                (f32::from(event.position.x), f32::from(event.position.y));
                            this.state.update(cx, |state, _| state.pan_preview(dx, dy));
                            cx.notify();
                        }))
                        // A source frame is not the sequence: the guides describe the
                        // delivery canvas and the handles edit a clip, so neither is
                        // drawn over a pool source.
                        .when_some(source.clone(), |el, open| {
                            el.child(source_stage(open, state, &theme, cx))
                        })
                        .when(source.is_none(), |el| {
                            el.child(frame_view(state, &theme, cx))
                                .children(guides(view, &theme))
                                .child(handles::measure(state))
                                .children(handles::overlay(state, &theme, cx))
                        }),
                )
                .when(scopes_open, |el| el.child(scopes::strip(state, &theme, cx))),
        )
        .child(
            div()
                .h(px(40.))
                .px(px(Theme::SPACE_MD))
                .flex()
                .items_center()
                .gap(px(Theme::SPACE_SM))
                .border_t_1()
                .border_color(theme.border)
                .bg(theme.surface)
                .child(transport_btn(
                    if playing {
                        crate::icons::PAUSE
                    } else {
                        crate::icons::PLAY
                    },
                    if playing { "pause" } else { "play" },
                    cx,
                ))
                .children(shuttle::badge(state, &theme, cx))
                .child(
                    crate::components::icon_button("fit-timeline", crate::icons::FIT, &theme)
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                this.state.update(cx, |state, _| state.zoom_fit_sequence());
                                cx.notify();
                            }),
                        ),
                )
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
                        .text_size(px(11.))
                        .text_color(theme.text_muted)
                        .cursor(gpui::CursorStyle::PointingHand)
                        .active(|style| style.opacity(0.92))
                        .when_some(crate::tooltip::builder("preview-zoom-label"), |el, tip| {
                            el.tooltip(tip)
                        })
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
                .child(
                    div()
                        .px(px(Theme::SPACE_XS))
                        .text_size(px(11.))
                        .text_color(theme.text_faint)
                        .child("Handles"),
                )
                .child(handles::mode_picker(state, &theme, cx))
                .child(scopes::toggle(state, &theme, cx))
                .child(
                    div()
                        .flex_1()
                        .text_size(px(11.))
                        .font_family(theme.font_mono.clone())
                        .text_color(theme.text_muted)
                        .child(format!("{name}  ·  {tc}")),
                )
                .child(peak_meter(state, &theme, cx))
                .when_some(error, |el, err| {
                    el.child(div().text_size(px(11.)).text_color(theme.danger).child(err))
                }),
        )
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

// Helpers live in sibling files, declared here. Each is `pub(super)`, so the
// parent and the siblings reach it by name and nothing is left unreachable.
#[path = "preview_peak.rs"]
mod peak;
#[path = "preview_shuttle.rs"]
mod shuttle;
#[path = "preview_source.rs"]
mod source;
use peak::peak_meter;
use source::{frame_view, guides, source_stage, transport_btn, zoom_btn};
