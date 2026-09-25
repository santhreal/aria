//! Source: 7 helpers from [`super`].
//!
//! Split out of `preview.rs` to stay under the file ceiling. A pure
//! move. Each item is `pub(super)` so the parent and its other children can
//! reach it by name; nothing else changed.

use super::*;
use crate::surfaces::source_pick::{self, Pick};

/// Press one row of the registry, reporting a refusal where the operator is
/// looking rather than swallowing it.
fn press(state: &mut AppState, pick: Pick) {
    if let Err(err) = source_pick::apply(state, pick) {
        state.report_error(&err);
    }
}

pub(super) fn frame_view(
    state: &Entity<AppState>,
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let view = state.read(cx).preview_view;
    let ticks = state.read(cx).store.playhead_ticks;
    match state.read(cx).last_render.clone() {
        Some(image) => {
            let scale = view.scale();
            let (dx, dy) = view.pan;
            div()
                .id("preview-stage")
                .size_full()
                .overflow_hidden()
                .relative()
                .child(motion::fade_quick(
                    // A discrete seek cross-fades; a drag-scrub and playback
                    // do not, because a fade restarted every frame never
                    // finishes and would leave the picture permanently dim.
                    seek::frame_id("preview-frame-fade", ticks),
                    // A zoomed frame is drawn larger than the pane and offset by
                    // the pan; a fitted one is exactly the pane and `Contain`
                    // does the rest.
                    img(image)
                        .id("preview-frame")
                        .object_fit(ObjectFit::Contain)
                        .absolute()
                        .left(px(dx))
                        .top(px(dy))
                        .w(gpui::relative(scale))
                        .h(gpui::relative(scale)),
                ))
                .into_any_element()
        }
        None => div()
            .flex()
            .items_center()
            .justify_center()
            .size_full()
            .child(div().text_size(px(13.)).text_color(theme.text_faint).child(
                if state.read(cx).last_frame.is_some() {
                    "Couldn't convert this frame for display. Try scrubbing again."
                } else {
                    "Import media, then click a tile to place it on the timeline."
                },
            ))
            .into_any_element(),
    }
}

/// The monitor showing a pool source: the frame it is scrubbed to, which source
/// it is, and the way back to the sequence.
///
/// While a newly scrubbed tick renders, the frame already held for that source
/// stays on screen — a scrub that blanked the pane between frames would be
/// unusable.
pub(super) fn source_stage(
    view: SourceView,
    state: &Entity<AppState>,
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    pool_preview::request_source(state, cx, view.media, view.tick);
    let image = state
        .read(cx)
        .pool_preview
        .source(view.media, view.tick)
        .or_else(|| state.read(cx).pool_preview.source_stale(view.media))
        .cloned();
    let frames = view.duration_ticks.max(1);
    let position = format!("{} · frame {} of {}", view.name, view.tick + 1, frames);
    div()
        .id("source-stage")
        .size_full()
        .relative()
        .flex()
        .items_center()
        .justify_center()
        .when(image.is_none(), |el| {
            // A source render is a wait like any other: the decode is already
            // requested above, and nothing else on screen says so.
            el.child(crate::pending::working(
                theme,
                "preview.source",
                "Rendering this source…",
                cx,
            ))
        })
        .when_some(image, |el, image| {
            el.child(motion::fade_quick(
                seek::frame_id("source-frame-fade", view.tick),
                img(image)
                    .id("source-frame")
                    .object_fit(ObjectFit::Contain)
                    .size_full(),
            ))
        })
        .child(
            div()
                .absolute()
                .top(px(Theme::SPACE_SM))
                .left(px(Theme::SPACE_SM))
                .flex()
                .items_center()
                .gap(px(Theme::SPACE_SM))
                .px(px(Theme::SPACE_SM))
                .py(px(Theme::SPACE_XXS))
                .rounded(px(Theme::CONTROL_RADIUS))
                .bg(theme.surface_overlay)
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(theme.text_muted)
                        .child(gpui::SharedString::from(position)),
                )
                .child(
                    crate::components::ghost_button("source-close", "Sequence", theme)
                        .when_some(crate::tooltip::builder("source-close"), |el, tip| {
                            el.tooltip(tip)
                        })
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                this.state
                                    .update(cx, |state, _| state.close_source_monitor());
                                cx.notify();
                            }),
                        ),
                ),
        )
        .child(source_bar(&view, state, theme, cx))
        .into_any_element()
}

/// The monitor's scrub bar and cut-in controls: where the marked range is, which
/// frame is on screen, and the two edits that put that range in the sequence.
///
/// The bar is where the marks live because that is where they mean something —
/// a number in the corner does not tell you how much of a take you have kept.
pub(super) fn source_bar(
    view: &SourceView,
    state: &Entity<AppState>,
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let frames = view.duration_ticks.max(1);
    let span = frames as f32;
    let start = view.mark_in.unwrap_or(0);
    let end = view.mark_out.unwrap_or(frames);
    let measure = {
        let state = state.clone();
        canvas(
            move |bounds, _, cx| {
                let left = f32::from(bounds.origin.x);
                let width = f32::from(bounds.size.width);
                state.update(cx, |state, _| {
                    state.pool_preview.record_source_bar(left, width)
                });
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full()
    };
    let marked = format!(
        "In {} · Out {} · {} of {} frames",
        view.mark_in
            .map(|t| (t + 1).to_string())
            .unwrap_or_else(|| "—".into()),
        view.mark_out
            .map(|t| t.to_string())
            .unwrap_or_else(|| "—".into()),
        (end - start).max(0),
        frames,
    );
    div()
        .absolute()
        .bottom(px(Theme::SPACE_SM))
        .left(px(Theme::SPACE_SM))
        .right(px(Theme::SPACE_SM))
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .child(
            div()
                .id("source-scrub")
                .relative()
                .w_full()
                .h(px(10.))
                .rounded(px(Theme::CONTROL_RADIUS))
                .bg(theme.element_active)
                .child(measure)
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .left(relative(start as f32 / span))
                        .w(relative(((end - start).max(0) as f32 / span).min(1.0)))
                        .bg(theme.accent.opacity(0.35)),
                )
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .left(relative(view.tick as f32 / span))
                        .w(px(2.))
                        .bg(theme.playhead),
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, event: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        let x = f32::from(event.position.x);
                        this.state.update(cx, |state, _| state.scrub_source(x));
                        cx.notify();
                    }),
                )
                .on_mouse_move(cx.listener(|this, event: &gpui::MouseMoveEvent, _, cx| {
                    if !event.dragging() {
                        return;
                    }
                    let x = f32::from(event.position.x);
                    this.state.update(cx, |state, _| state.scrub_source(x));
                    cx.notify();
                })),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(Theme::SPACE_SM))
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(theme.text_muted)
                        .child(gpui::SharedString::from(marked)),
                )
                .child(div().flex_1())
                .child(
                    crate::components::ghost_button("source-mark-in", "In", theme)
                        .when_some(crate::tooltip::builder("source-mark-in"), |el, tip| {
                            el.tooltip(tip)
                        })
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                this.state.update(cx, |state, _| press(state, Pick::MarkIn));
                                cx.notify();
                            }),
                        ),
                )
                .child(
                    crate::components::ghost_button("source-mark-out", "Out", theme)
                        .when_some(crate::tooltip::builder("source-mark-out"), |el, tip| {
                            el.tooltip(tip)
                        })
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                this.state
                                    .update(cx, |state, _| press(state, Pick::MarkOut));
                                cx.notify();
                            }),
                        ),
                )
                .child(
                    crate::components::ghost_button("source-mark-clear", "Clear", theme)
                        .when_some(crate::tooltip::builder("source-mark-clear"), |el, tip| {
                            el.tooltip(tip)
                        })
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                this.state
                                    .update(cx, |state, _| press(state, Pick::ClearMarks));
                                cx.notify();
                            }),
                        ),
                )
                .child(
                    crate::components::button("source-insert", "Insert", theme)
                        .when_some(crate::tooltip::builder("source-insert"), |el, tip| {
                            el.tooltip(tip)
                        })
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                this.state.update(cx, |state, _| press(state, Pick::Insert));
                                cx.notify();
                            }),
                        ),
                )
                .child(
                    crate::components::button("source-overwrite", "Overwrite", theme)
                        .when_some(crate::tooltip::builder("source-overwrite"), |el, tip| {
                            el.tooltip(tip)
                        })
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                this.state
                                    .update(cx, |state, _| press(state, Pick::Overwrite));
                                cx.notify();
                            }),
                        ),
                ),
        )
        .into_any_element()
}

/// Title-safe, action-safe, and the thirds grid.
///
/// The percentages are the SMPTE ones: action safe is the inner 90% of the
/// frame and title safe the inner 80%. Drawn as insets of the pane rather than
/// of the picture, which is what a fitted preview makes them.
pub(super) fn guides(
    view: crate::app::preview_view::PreviewView,
    theme: &Theme,
) -> Vec<gpui::AnyElement> {
    let mut out: Vec<gpui::AnyElement> = Vec::new();
    if view.action_safe {
        out.push(safe_box(theme, 5.0).into_any_element());
    }
    if view.title_safe {
        out.push(safe_box(theme, 10.0).into_any_element());
    }
    if view.grid {
        for at in [33.333_f32, 66.667] {
            out.push(
                div()
                    .absolute()
                    .left(gpui::relative(at / 100.0))
                    .top_0()
                    .bottom_0()
                    .w(px(1.))
                    .bg(theme.border_strong)
                    .into_any_element(),
            );
            out.push(
                div()
                    .absolute()
                    .top(gpui::relative(at / 100.0))
                    .left_0()
                    .right_0()
                    .h(px(1.))
                    .bg(theme.border_strong)
                    .into_any_element(),
            );
        }
    }
    out
}

/// One safe-area rectangle, inset by `percent` of the pane on every side.
pub(super) fn safe_box(theme: &Theme, percent: f32) -> impl IntoElement {
    let inset = gpui::relative(percent / 100.0);
    div()
        .absolute()
        .left(inset)
        .right(inset)
        .top(inset)
        .bottom(inset)
        .border_1()
        .border_color(theme.border_strong)
}

pub(super) fn transport_btn(
    path: &'static str,
    id: &'static str,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let theme = Theme::of(cx);
    // The solid fill overwrites the base style `icon_button` set, so its fade
    // has to be read back and re-applied rather than silently dropped.
    let key = motion::MotionKey::new("icon", id);
    crate::components::icon_button(id, path, &theme)
        .bg(motion::hover_blend(key, theme.solid, theme.accent_strong))
        // The glyph travels with the fill under it: neutral on the solid plate,
        // accent-safe once the plate is accent.
        .text_color(motion::hover_blend(key, theme.on_solid, theme.on_accent))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                this.state.update(cx, |state, _| state.toggle_playback());
                start_playback(this.state.clone(), cx);
                spawn_preview(this.state.clone(), cx);
                cx.notify();
            }),
        )
}

/// A preview zoom step, drawn with the shipped zoom glyph.
pub(super) fn zoom_btn(
    id: impl Into<motion::ControlId>,
    icon: &'static str,
    cx: &mut gpui::Context<crate::shell::Shell>,
    step: fn(&mut AppState),
) -> impl IntoElement {
    let theme = Theme::of(cx);
    let id = id.into();
    let tip = id.fixed_name().and_then(crate::tooltip::builder);
    let key = id.motion("zoom");
    div()
        .id(id.element("zoom-button"))
        .w(px(Theme::SPACE_LG))
        .h(px(Theme::SPACE_LG))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(Theme::CONTROL_RADIUS))
        .text_size(px(13.))
        .text_color(motion::hover_blend(key, theme.text_muted, theme.text))
        .cursor(gpui::CursorStyle::PointingHand)
        .active(|style| style.opacity(0.92))
        .bg(motion::hover_blend(
            key,
            theme.wash(0.0),
            theme.element_hover,
        ))
        .on_hover(motion::hover_listener(key))
        .when_some(tip, |el, tip| el.tooltip(tip))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                this.state.update(cx, |state, _| step(state));
                cx.notify();
            }),
        )
        .child(crate::icons::icon(icon))
}
