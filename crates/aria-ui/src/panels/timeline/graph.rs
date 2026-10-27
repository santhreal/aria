//! The Timeline graph editor band: a header column over the outline column
//! and the graph over the time area.
//!
//! The header column holds the property's name on its first 24 px row and the
//! graph tools on its second: the Value and Speed chips, then Easy Ease and
//! Hold. The value axis labels stand on the column's right edge at the ys the
//! curve maps the top, middle and bottom values to ([`graph_axis_ys`]), and the
//! graph draws its three grid lines at the same ys. The name row ends at the
//! axis gutter, beside the top label; the tools row lies between the top and
//! middle labels and spans the column.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    App, Bounds, ContentMask, Corners, Edges, Entity, HitboxBehavior, InteractiveElement,
    IntoElement, MouseButton, MouseDownEvent, ParentElement, Pixels, SharedString, Styled, Window,
    canvas, div, point, px, quad, size,
};

use crate::app::AppState;
use crate::app::property_rows::graph::graph_axis_ys;
use crate::app::property_rows::{GRAPH_EDITOR_H, GraphMode};
use crate::components::{self, ChipSize};
use crate::panels::keyframe_icon::{KeyInk, paint_keyframe_icon};
use crate::shell::{Hold, Shell};
use crate::theme::Theme;
use crate::tooltip::WithTip as _;
use crate::transport::ticks_to_px_f;
use crate::types::ContextTarget;
use gpui::prelude::FluentBuilder;

use super::header::HEADER_ROW_H;
use super::property_rows::{paint_rect, paint_segment};

/// Width of the value axis gutter on the header column's right edge. The name
/// row ends where it starts.
pub(super) const AXIS_GUTTER: f32 = 40.0;
/// Height of one value axis label, centred on its value's y.
const AXIS_LABEL_H: f32 = 12.0;

/// Render the Timeline Graph Editor band (Value graph and Speed graph with
/// incoming/outgoing influence handles and spatial tangent handles). `ruled`
/// draws the rule over the band; a band straight under the ruler leaves it to
/// the ruler's own.
#[allow(clippy::too_many_arguments)]
pub(super) fn render_graph_editor(
    theme: &Theme,
    state: &Entity<AppState>,
    pps: f32,
    fps: f32,
    scroll_x: f32,
    playhead: f32,
    ruled: bool,
    cx: &mut gpui::Context<Shell>,
) -> impl IntoElement {
    let width = state.read(cx).outline_drawn_width();
    let model = state
        .read(cx)
        .build_timeline_graph_model(fps, pps, scroll_x, GRAPH_EDITOR_H);
    let mode = model.mode;
    let active_label = model
        .curves
        .first()
        .map(|c| c.label.clone())
        .unwrap_or_else(|| SharedString::new_static("Property"));
    let top_lbl = model.top_label.clone();
    let mid_lbl = model.mid_label.clone();
    let bot_lbl = model.bottom_label.clone();

    let axis = |label: SharedString, y: f32| {
        div()
            .absolute()
            .right(px(Theme::SPACE_SM))
            .top(px(y - AXIS_LABEL_H * 0.5))
            .h(px(AXIS_LABEL_H))
            .flex()
            .items_center()
            .text_size(Theme::TEXT_MICRO)
            .text_color(theme.text_faint)
            .child(label)
    };
    let [top_y, mid_y, bot_y] = graph_axis_ys(GRAPH_EDITOR_H);
    let graph_mode_chip = |id: &'static str, label: &'static str, chip_mode: GraphMode| {
        components::chip(id, label, mode == chip_mode, ChipSize::Compact, theme)
            .debug_selector(move || id.into())
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    this.state.update(cx, |state, _| {
                        state.set_timeline_graph_mode(chip_mode);
                    });
                    cx.notify();
                }),
            )
    };
    let value_chip = graph_mode_chip("graph-mode-value", "Value", GraphMode::Value);
    let speed_chip = graph_mode_chip("graph-mode-speed", "Speed", GraphMode::Speed);
    let graph_tool = |id: &'static str, label: &'static str| {
        components::ghost_button(id, label, theme)
            .debug_selector(move || id.into())
            .px(px(Theme::SPACE_XS))
            .py(px(Theme::SPACE_XXS))
            .text_size(Theme::TEXT_SMALL)
            .when_some(crate::tooltip::builder(id), |el, tip| el.with_tip(tip))
    };
    let easy_ease = graph_tool("graph-btn-easy-ease", "Easy Ease").on_mouse_down(
        MouseButton::Left,
        cx.listener(|this, _: &MouseDownEvent, _, cx| {
            cx.stop_propagation();
            this.state.update(cx, |state, _| {
                if let Err(err) = state.easy_ease_selected_or_playhead() {
                    state.report_error(&err);
                }
            });
            crate::app::spawn_preview(this.state.clone(), cx);
            cx.notify();
        }),
    );
    let hold = graph_tool("graph-btn-hold", "Hold").on_mouse_down(
        MouseButton::Left,
        cx.listener(|this, _: &MouseDownEvent, _, cx| {
            cx.stop_propagation();
            this.state.update(cx, |state, _| {
                if let Err(err) = state.toggle_hold_selected_or_playhead() {
                    state.report_error(&err);
                }
            });
            crate::app::spawn_preview(this.state.clone(), cx);
            cx.notify();
        }),
    );
    let row = || {
        div()
            .h(px(HEADER_ROW_H))
            .flex_none()
            .flex()
            .items_center()
            .overflow_hidden()
    };

    let header_col = div()
        .id("timeline-graph-header")
        .debug_selector(|| "timeline-graph-header".into())
        .relative()
        .w(px(width))
        .flex_none()
        .h_full()
        .border_r_1()
        .border_color(theme.border)
        .bg(theme.surface_header)
        .px(px(Theme::SPACE_SM))
        .flex()
        .flex_col()
        .child(
            // The name row shares its height with the top axis label, so it
            // stops short of the label gutter; the tools row lies between the
            // top and middle labels and takes the column's width.
            row().pr(px(AXIS_GUTTER)).child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_size(Theme::TEXT_UI)
                    .text_color(theme.text)
                    .child(active_label),
            ),
        )
        .child(
            row()
                .gap(px(Theme::SPACE_XS))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(Theme::SPACE_XXS))
                        .child(value_chip)
                        .child(speed_chip),
                )
                .child(div().w(px(1.0)).h(px(Theme::SPACE_LG)).bg(theme.border))
                .child(easy_ease)
                .child(hold),
        )
        .child(axis(top_lbl, top_y).debug_selector(|| "graph-axis-top".into()))
        .child(axis(mid_lbl, mid_y).debug_selector(|| "graph-axis-mid".into()))
        .child(axis(bot_lbl, bot_y).debug_selector(|| "graph-axis-bottom".into()));

    let canvas_bounds = Rc::new(Cell::new(Bounds::default()));
    let bounds_prepaint = canvas_bounds.clone();
    let bounds_down = canvas_bounds.clone();
    let bounds_right = canvas_bounds;
    let paint_theme = theme.clone();
    let paint_model = model;

    let graph_canvas = div()
        .id("timeline-graph-canvas")
        .debug_selector(|| "timeline-graph-canvas".into())
        .flex_1()
        .h_full()
        .relative()
        .overflow_hidden()
        .bg(theme.bg)
        // The shell carries a drag the press began, so a key or a handle
        // follows the pointer off the graph and the release writes it
        // wherever the button comes up.
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                let b = bounds_down.get();
                let lx = f32::from(event.position.x - b.origin.x);
                let ly = f32::from(event.position.y - b.origin.y);
                let started = this.state.update(cx, |state, _| {
                    state.begin_timeline_graph_drag(lx, ly, fps, pps, scroll_x, GRAPH_EDITOR_H)
                });
                if !started {
                    return;
                }
                cx.stop_propagation();
                let bounds = bounds_down.clone();
                this.hold(Hold::new(
                    move |this, event, cx| {
                        let b = bounds.get();
                        let lx = f32::from(event.position.x - b.origin.x);
                        let ly = f32::from(event.position.y - b.origin.y);
                        let changed = this.state.update(cx, |state, _| {
                            state.update_timeline_graph_drag(
                                lx,
                                ly,
                                fps,
                                pps,
                                scroll_x,
                                GRAPH_EDITOR_H,
                            )
                        });
                        if changed {
                            cx.notify();
                        }
                    },
                    |this, _, cx| {
                        this.state.update(cx, |state, _| {
                            if let Err(err) = state.commit_timeline_graph_drag() {
                                state.report_error(&err);
                            }
                        });
                        crate::app::spawn_preview(this.state.clone(), cx);
                        cx.notify();
                    },
                ));
                cx.notify();
            }),
        )
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                let b = bounds_right.get();
                let lx = f32::from(event.position.x - b.origin.x);
                let ly = f32::from(event.position.y - b.origin.y);
                let wx = f32::from(event.position.x);
                let wy = f32::from(event.position.y);
                this.state.update(cx, |state, _| {
                    let gm = state.build_timeline_graph_model(fps, pps, scroll_x, GRAPH_EDITOR_H);
                    for curve in &gm.curves {
                        for k in &curve.keys {
                            let dx = lx - k.x;
                            let dy = ly - k.y;
                            if dx * dx + dy * dy <= 64.0 {
                                let target = ContextTarget::Keyframe {
                                    clip: k.clip.raw(),
                                    param: k.param,
                                    tick: k.tick,
                                };
                                state.open_context(target, wx, wy);
                                return;
                            }
                        }
                    }
                });
                cx.notify();
            }),
        )
        .child(
            canvas(
                move |bounds, window, _| {
                    bounds_prepaint.set(bounds);
                    window.insert_hitbox(bounds, HitboxBehavior::Normal)
                },
                move |bounds, _, window, cx| {
                    paint_graph_canvas(
                        bounds,
                        &paint_model,
                        &paint_theme,
                        fps,
                        pps,
                        scroll_x,
                        playhead,
                        window,
                        cx,
                    );
                },
            )
            .size_full(),
        );

    div()
        .id("timeline-graph-editor")
        .debug_selector(|| "timeline-graph-editor".into())
        .w_full()
        .h(px(GRAPH_EDITOR_H))
        .flex()
        .flex_row()
        .when(ruled, |el| {
            el.border_t_1().border_color(theme.border_strong)
        })
        .child(header_col)
        .child(graph_canvas)
}

#[allow(clippy::too_many_arguments)]
fn paint_graph_canvas(
    bounds: Bounds<Pixels>,
    model: &crate::app::property_rows::TimelineGraphModel,
    theme: &Theme,
    fps: f32,
    pps: f32,
    scroll_x: f32,
    playhead: f32,
    window: &mut Window,
    cx: &mut App,
) {
    let mut calls = 0usize;
    let ox = f32::from(bounds.origin.x);
    let oy = f32::from(bounds.origin.y);
    let vw = f32::from(bounds.size.width).max(1.0);
    let vh = f32::from(bounds.size.height).max(1.0);

    window.with_content_mask(Some(ContentMask::from(bounds)), |window| {
        paint_rect(window, &mut calls, ox, oy, vw, vh, theme.bg);

        // The value axis grid: the top, middle and bottom values, at the ys
        // the header column's labels stand on.
        for y in graph_axis_ys(GRAPH_EDITOR_H) {
            paint_rect(
                window,
                &mut calls,
                ox,
                oy + y,
                vw,
                1.0,
                theme.border.opacity(0.65),
            );
        }

        for curve in &model.curves {
            // 1. Sampled curve polyline.
            for pair in curve.samples.windows(2) {
                let (x0, y0) = pair[0];
                let (x1, y1) = pair[1];
                if x1 < -8.0 || x0 > vw + 8.0 {
                    continue;
                }
                paint_segment(
                    window,
                    &mut calls,
                    ox + x0,
                    oy + y0,
                    ox + x1,
                    oy + y1,
                    theme.accent,
                );
            }

            // 2. Incoming and outgoing temporal influence handles (0.1%..=100%).
            for h in &curve.influence_handles {
                if h.anchor_x < -80.0 && h.handle_x < -80.0 {
                    continue;
                }
                if h.anchor_x > vw + 80.0 && h.handle_x > vw + 80.0 {
                    continue;
                }
                paint_segment(
                    window,
                    &mut calls,
                    ox + h.anchor_x,
                    oy + h.anchor_y,
                    ox + h.handle_x,
                    oy + h.handle_y,
                    theme.warning.opacity(0.9),
                );
                window.paint_quad(quad(
                    Bounds {
                        origin: point(px(ox + h.handle_x - 3.5), px(oy + h.handle_y - 3.5)),
                        size: size(px(7.0), px(7.0)),
                    },
                    Corners::all(px(3.5)),
                    theme.warning,
                    Edges::all(px(1.0)),
                    theme.text,
                    gpui::BorderStyle::default(),
                ));
                calls += 1;

                let pct = (h.influence * 100.0).clamp(0.1, 100.0);
                let pct_text = format!("{pct:.0}%");
                super::clip::paint_label(
                    window,
                    cx,
                    &mut calls,
                    &pct_text,
                    ox + h.handle_x - 10.0,
                    oy + (h.handle_y - 13.0).max(2.0),
                    9.0,
                    theme.warning,
                    theme,
                );
            }

            // 3. Keyframe nodes.
            for k in &curve.keys {
                if k.x < -12.0 || k.x > vw + 12.0 {
                    continue;
                }
                paint_keyframe_icon(
                    window,
                    &mut calls,
                    KeyInk::of(theme),
                    ox + k.x,
                    oy + k.y,
                    k.shape,
                    k.selected,
                );
            }
        }

        // Playhead vertical line.
        let px_at = ticks_to_px_f(playhead, fps, pps) - scroll_x;
        if px_at >= -2.0 && px_at <= vw + 2.0 {
            paint_rect(window, &mut calls, ox + px_at, oy, 1.5, vh, theme.playhead);
        }
    });
}
