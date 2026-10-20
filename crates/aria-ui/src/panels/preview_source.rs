//! The Source tab's stage and bar, the Program frame, and the safe-margin
//! guides.
//!
//! Each item is `pub(super)` so the parent and its other children reach it by
//! name.

use super::*;
use crate::surfaces::source_pick::{self, Pick};
use gpui::AppContext as _;

/// Press one row of the registry, reporting a refusal in the status line rather
/// than swallowing it.
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
    let texture = state.read(cx).preview_worker.shown_texture();
    let image = state.read(cx).last_render.clone();
    let gpu_probe = gpui::canvas(
        {
            let state = state.clone();
            move |_, window, cx| {
                if state
                    .read(cx)
                    .preview_worker
                    .set_gpu_device(window.gpu_device())
                {
                    cx.defer(move |app| {
                        state.update(app, |state, _| {
                            state.picture_changed();
                        });
                        crate::app::spawn_preview(state, app);
                    });
                }
            }
        },
        |_, _, _, _| {},
    )
    .size_0();
    let scale = view.scale();
    let (dx, dy) = view.pan;
    if let Some(texture) = texture {
        div()
            .id("preview-stage")
            .size_full()
            .overflow_hidden()
            .relative()
            .child(gpu_probe)
            .child(motion::fade_quick(
                seek::frame_id("preview-frame-fade", ticks),
                gpui::surface(texture)
                    .object_fit(ObjectFit::Contain)
                    .absolute()
                    .left(px(dx))
                    .top(px(dy))
                    .w(gpui::relative(scale))
                    .h(gpui::relative(scale)),
            ))
            .into_any_element()
    } else if let Some(image) = image {
        div()
            .id("preview-stage")
            .size_full()
            .overflow_hidden()
            .relative()
            .child(gpu_probe)
            .child(motion::fade_quick(
                // A discrete seek cross-fades; a drag-scrub and playback
                // do not, because a fade restarted every frame never
                // finishes and would leave the picture permanently dim.
                seek::frame_id("preview-frame-fade", ticks),
                // A zoomed frame is drawn larger than the pane and offset by
                // the pan; a fitted one is exactly the pane and `Contain`
                // does the rest.
                crate::components::picture(
                    img(image)
                        .id("preview-frame")
                        .object_fit(ObjectFit::Contain)
                        .absolute()
                        .left(px(dx))
                        .top(px(dy))
                        .w(gpui::relative(scale))
                        .h(gpui::relative(scale)),
                ),
            ))
            .into_any_element()
    } else {
        div()
            .flex()
            .items_center()
            .justify_center()
            .size_full()
            .child(gpu_probe)
            .child(
                div()
                    .text_size(Theme::TEXT_UI)
                    .text_color(theme.text_faint)
                    .child(if state.read(cx).last_frame.is_some() {
                        "Couldn't convert this frame for display. Try scrubbing again."
                    } else {
                        "Import media, then click a tile to place it on the timeline."
                    }),
            )
            .into_any_element()
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
    let mut position = format!("{} · frame {} of {}", view.name, view.tick + 1, frames);
    // A shuttle speed is the one thing about the source clock the frame number
    // does not show.
    let rate = state.read(cx).sources.play_rate().filter(|rate| *rate != 1);
    if let Some(rate) = rate {
        position.push_str(&format!(" · {rate}x"));
    }
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
            // The frame is the handle for a drag onto the timeline, which
            // drops the marked range with the same patching as the buttons.
            el.child(
                div()
                    .id("source-frame-drag")
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(motion::fade_quick(
                        seek::frame_id("source-frame-fade", view.tick),
                        crate::components::picture(
                            img(image)
                                .id("source-frame")
                                .object_fit(ObjectFit::Contain)
                                .size_full(),
                        ),
                    ))
                    .on_drag(
                        crate::types::MediaDrag {
                            media: view.media,
                            label: view.name.clone(),
                            from_source: true,
                        },
                        |drag, _offset, _window, cx| {
                            let label = drag.label.clone();
                            cx.new(|_| crate::panels::sidebar::DragGhost { label })
                        },
                    ),
            )
        })
        .child(
            // As wide as its text, up to the stage's width less the inset; a
            // longer name ends in an ellipsis before the Sequence button.
            div()
                .absolute()
                .top(px(Theme::SPACE_SM))
                .left(px(Theme::SPACE_SM))
                .right(px(Theme::SPACE_SM))
                .flex()
                .child(
                    div()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap(px(Theme::SPACE_SM))
                        .px(px(Theme::SPACE_SM))
                        .py(px(Theme::SPACE_XXS))
                        .rounded(px(Theme::CONTROL_RADIUS))
                        .bg(theme.surface_overlay)
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_size(Theme::TEXT_UI)
                                .text_color(theme.text_muted)
                                .child(gpui::SharedString::from(position)),
                        )
                        .child(
                            crate::components::ghost_button("source-close", "Sequence", theme)
                                .flex_none()
                                .when_some(crate::tooltip::builder("source-close"), |el, tip| {
                                    el.with_tip(tip)
                                })
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                        cx.stop_propagation();
                                        this.state.update(cx, |state, _| state.show_program());
                                        cx.notify();
                                    }),
                                ),
                        ),
                ),
        )
        .child(source_bar(&view, theme, cx))
        .into_any_element()
}

/// The Source tab's marks and cut-in controls under the picture: the marked
/// range, and the two edits that put that range in the sequence. The mini
/// ruler under the control row shows the range across the take and scrubs it.
pub(super) fn source_bar(
    view: &SourceView,
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let frames = view.duration_ticks.max(1);
    let start = view.mark_in.unwrap_or(0);
    let end = view.mark_out.unwrap_or(frames);
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
            // On the overlay's backing, as the name chip above, so the ghost
            // buttons read over a bright picture. Narrower than the label and
            // the buttons, the buttons take the next line at the right.
            div()
                .flex()
                .wraps()
                .items_center()
                .gap(px(Theme::SPACE_SM))
                .px(px(Theme::SPACE_SM))
                .py(px(Theme::SPACE_XXS))
                .rounded(px(Theme::CONTROL_RADIUS))
                .bg(theme.surface_overlay)
                .child(
                    div()
                        .text_size(Theme::TEXT_UI)
                        .text_color(theme.text_muted)
                        .child(gpui::SharedString::from(marked)),
                )
                .child(
                    div()
                        .flex()
                        .wraps()
                        .items_center()
                        .justify_end()
                        .gap(px(Theme::SPACE_SM))
                        .ml_auto()
                        .child(
                            crate::components::ghost_button("source-mark-in", "In", theme)
                                .when_some(crate::tooltip::builder("source-mark-in"), |el, tip| {
                                    el.with_tip(tip)
                                })
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                        cx.stop_propagation();
                                        this.state
                                            .update(cx, |state, _| press(state, Pick::MarkIn));
                                        cx.notify();
                                    }),
                                ),
                        )
                        .child(
                            crate::components::ghost_button("source-mark-out", "Out", theme)
                                .when_some(crate::tooltip::builder("source-mark-out"), |el, tip| {
                                    el.with_tip(tip)
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
                                .when_some(
                                    crate::tooltip::builder("source-mark-clear"),
                                    |el, tip| el.with_tip(tip),
                                )
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
                                    el.with_tip(tip)
                                })
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                        cx.stop_propagation();
                                        this.state
                                            .update(cx, |state, _| press(state, Pick::Insert));
                                        cx.notify();
                                    }),
                                ),
                        )
                        .child(
                            crate::components::button("source-overwrite", "Overwrite", theme)
                                .when_some(
                                    crate::tooltip::builder("source-overwrite"),
                                    |el, tip| el.with_tip(tip),
                                )
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
                ),
        )
        .into_any_element()
}

/// Title-safe, action-safe, and the thirds grid.
///
/// The percentages are SMPTE ST 2046-1's: action safe is the inner 93% of the
/// frame and title safe the inner 90%. Drawn as insets of the pane rather than
/// of the picture, which is what a fitted preview makes them.
pub(super) fn guides(
    view: crate::app::preview_view::PreviewView,
    theme: &Theme,
) -> Vec<gpui::AnyElement> {
    let mut out: Vec<gpui::AnyElement> = Vec::new();
    if view.action_safe {
        out.push(safe_box(theme, 3.5).into_any_element());
    }
    if view.title_safe {
        out.push(safe_box(theme, 5.0).into_any_element());
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
