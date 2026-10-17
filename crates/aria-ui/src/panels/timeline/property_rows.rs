//! Timeline property rows, virtualized keyframe icon painting, graph editor,
//! and keyframe context menu.

use std::cell::Cell;
use std::rc::Rc;

use crate::fluent::FluentBuilder;
use aria_models::ClipId;
use gpui::{
    App, Bounds, ContentMask, Corners, CursorStyle, Edges, Entity, HitboxBehavior, Hsla,
    InteractiveElement, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    ParentElement, Pixels, SharedString, Styled, Window, canvas, div, fill, point, px, quad, size,
};

use crate::app::AppState;
use crate::app::property_rows::{
    GRAPH_EDITOR_H, GraphMode, KeyframeContextMenu, MaskPathKeyMenu, PROPERTY_ROW_H,
    TimelineKeyMark, TimelineMaskPathRow, TimelineOutlineRow, TimelinePropertyRow,
};
use crate::app::value_scrub::{ScrubTarget, ValueKind};
use crate::motion::MotionKey;
use crate::panels::keyframe_icon::{KeyInk, paint_keyframe_icon};
use crate::panels::value_readout;
use crate::shell::Shell;
use crate::surfaces::keyframe_rows::{self, Row};
use crate::theme::Theme;
use crate::tooltip::{WithTip, deferred_builder};

use super::clip::LaneGroupPaint;
use super::{ticks_to_px, ticks_to_px_f};

thread_local! {
    static GRAPH_DRAG_ACTIVE: Cell<bool> = const { Cell::new(false) };
}

/// Render the clip property-tree twirl triangle button (`▸` / `▾`) in the track
/// header column.
pub(super) fn track_outline_twirl_button(
    theme: &Theme,
    track_id: u64,
    clip_id: u64,
    open: bool,
    y_top: f32,
    clip_h: f32,
    cx: &mut gpui::Context<Shell>,
) -> impl IntoElement {
    let arrow = if open { "▾" } else { "▸" };
    div()
        .id(gpui::ElementId::named_usize(
            "track-prop-twirl",
            track_id as usize,
        ))
        .absolute()
        .left(px(52.0))
        .top(px(y_top + (clip_h - 16.0).max(0.0) * 0.5))
        .w(px(16.0))
        .h(px(16.0))
        .rounded(px(2.0))
        .flex()
        .items_center()
        .justify_center()
        .cursor(CursorStyle::PointingHand)
        .text_size(Theme::TEXT_SMALL)
        .text_color(if open { theme.accent } else { theme.text_muted })
        .child(arrow)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                this.state.update(cx, |state, _| {
                    state.select_clip(aria_models::ClipId::from_raw(clip_id));
                    state.toggle_clip_outline(clip_id);
                });
                cx.notify();
            }),
        )
}

/// Render one outline row header (either a group header or a property row) in
/// the outline column under its track header.
pub(super) fn outline_row_header(
    theme: &Theme,
    outline: &TimelineOutlineRow,
    y_top: f32,
    slot: usize,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<Shell>,
) -> impl IntoElement {
    let width = state.read(cx).ui_settings.outline_width;
    match outline {
        TimelineOutlineRow::GroupHeader {
            clip,
            group,
            label,
            expanded,
        } => {
            let clip_val = *clip;
            let group_val = *group;
            let text = label.clone();
            let arrow = if *expanded { "▾" } else { "▸" };
            div()
                .id(gpui::ElementId::named_usize("prop-group-hdr", slot))
                .absolute()
                .top(px(y_top))
                .left_0()
                .w(px(width))
                .h(px(PROPERTY_ROW_H))
                .pl(px(14.0))
                .pr(px(Theme::SPACE_XS))
                .flex()
                .items_center()
                .gap(px(4.0))
                .border_r_1()
                .border_b_1()
                .border_color(theme.hairline)
                .bg(theme.surface_header)
                .cursor(CursorStyle::PointingHand)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        this.state.update(cx, |state, _| {
                            state.toggle_property_group(clip_val, group_val);
                        });
                        cx.notify();
                    }),
                )
                .child(
                    div()
                        .text_size(Theme::TEXT_SMALL)
                        .text_color(theme.text_muted)
                        .child(arrow),
                )
                .child(
                    div()
                        .text_size(Theme::TEXT_UI)
                        .text_color(theme.text)
                        .child(text),
                )
                .into_any_element()
        }
        TimelineOutlineRow::Property(row) => {
            property_row_header(theme, row, y_top, width, slot, state, cx).into_any_element()
        }
        TimelineOutlineRow::MaskPath(row) => {
            mask_path_row_header(theme, row, y_top, width, slot, cx).into_any_element()
        }
    }
}

/// A press on a Path row control: the row's clip and mask, and whether the row
/// drew a keyframe under the playhead.
type PathPress = fn(&mut AppState, ClipId, u32, bool) -> Result<(), aria_engine::EngineError>;

/// The header of a mask's Path row: the stopwatch, the name, the vertex count
/// at the playhead, and the keyframe navigator. A Path keyframe holds the whole
/// path, so the row has no driver and no graph.
fn mask_path_row_header(
    theme: &Theme,
    row: &TimelineMaskPathRow,
    y_top: f32,
    width: f32,
    slot: usize,
    cx: &mut gpui::Context<Shell>,
) -> impl IntoElement {
    let (clip, mask) = (row.clip, row.mask);
    let keyed_here = row.keyed_at_playhead;
    let watch_color = if row.keyframed {
        theme.accent
    } else {
        theme.text_faint
    };
    let press = move |id: &'static str, cx: &mut gpui::Context<Shell>, run: PathPress| {
        div()
            .id(gpui::ElementId::named_usize(id, slot))
            .cursor(CursorStyle::PointingHand)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    this.state.update(cx, |state, _| {
                        if let Err(err) = run(state, clip, mask, keyed_here) {
                            state.report_error(&err);
                        }
                    });
                    crate::app::spawn_preview(this.state.clone(), cx);
                    cx.notify();
                }),
            )
    };
    let arrow = |on: bool| {
        if on {
            theme.text_muted
        } else {
            theme.text_faint.opacity(0.4)
        }
    };

    div()
        .id(gpui::ElementId::named_usize("path-row-hdr", slot))
        .absolute()
        .top(px(y_top))
        .left_0()
        .w(px(width))
        .h(px(PROPERTY_ROW_H))
        .pl(px(18.0))
        .pr(px(4.0))
        .flex()
        .items_center()
        .gap(px(3.0))
        .border_r_1()
        .border_b_1()
        .border_color(theme.hairline)
        .bg(theme.surface_header)
        // 1. Stopwatch: keyframe the path, or keep the path at the playhead.
        .child(
            press("path-watch", cx, |state, clip, mask, _| {
                state.animate_mask_path(clip, mask)
            })
            .debug_selector(|| "path-watch".into())
            .with_tip(deferred_builder("path-watch"))
            .w(px(14.0))
            .h(px(14.0))
            .rounded(px(7.0))
            .border_1()
            .border_color(watch_color)
            .bg(if row.keyframed {
                watch_color.opacity(0.24)
            } else {
                theme.bg.opacity(0.0)
            })
            .flex()
            .items_center()
            .justify_center()
            .text_size(Theme::TEXT_MICRO)
            .text_color(watch_color)
            .child("●"),
        )
        // 2. Name: makes this mask the one the Viewer handles.
        .child(
            press("path-name", cx, |state, clip, mask, _| {
                state.focus_mask_path(clip, mask);
                Ok(())
            })
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .text_size(Theme::TEXT_SMALL)
            .text_color(theme.text)
            .child(row.label.clone()),
        )
        // 3. The vertex count at the playhead.
        .child(
            div()
                .font_family(theme.font_mono.clone())
                .text_size(Theme::TEXT_SMALL)
                .text_color(theme.value)
                .child(row.value_label.clone()),
        )
        // 4. Keyframe navigator: < prev, ◆ add/remove at playhead, > next.
        .child(
            press("path-prev-kf", cx, |state, clip, mask, _| {
                state.step_mask_path_key(clip, mask, false);
                Ok(())
            })
            .with_tip(deferred_builder("kf-prev"))
            .w(px(10.0))
            .text_size(Theme::TEXT_MICRO)
            .text_color(arrow(row.has_prev_key))
            .child("‹"),
        )
        .child(
            press("path-at-kf", cx, |state, clip, mask, keyed| {
                state.key_mask_path(clip, mask, keyed)
            })
            .with_tip(deferred_builder("kf-at"))
            .w(px(12.0))
            .text_size(Theme::TEXT_MICRO)
            .text_color(if keyed_here {
                theme.accent
            } else {
                theme.text_muted
            })
            .child(if keyed_here { "◆" } else { "◇" }),
        )
        .child(
            press("path-next-kf", cx, |state, clip, mask, _| {
                state.step_mask_path_key(clip, mask, true);
                Ok(())
            })
            .with_tip(deferred_builder("kf-next"))
            .w(px(10.0))
            .text_size(Theme::TEXT_MICRO)
            .text_color(arrow(row.has_next_key))
            .child("›"),
        )
}

fn property_row_header(
    theme: &Theme,
    row: &TimelinePropertyRow,
    y_top: f32,
    width: f32,
    slot: usize,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<Shell>,
) -> impl IntoElement {
    let clip = row.clip;
    let param = row.param;
    let label = row.label.clone();
    let value = value_readout(theme, row, state, cx);
    let keyframed = row.keyframed;
    let keyed_here = row.keyed_at_playhead;
    let has_prev = row.has_prev_key;
    let has_next = row.has_next_key;
    let driver = row.driver_active;
    let off_default = row.off_default;
    let selected_graph = row.selected_for_graph;

    let watch_color = if driver {
        theme.warning
    } else if keyframed {
        theme.accent
    } else {
        theme.text_faint
    };

    div()
        .id(gpui::ElementId::named_usize("prop-row-hdr", slot))
        .absolute()
        .top(px(y_top))
        .left_0()
        .w(px(width))
        .h(px(PROPERTY_ROW_H))
        .pl(px(18.0))
        .pr(px(4.0))
        .flex()
        .items_center()
        .gap(px(3.0))
        .border_r_1()
        .border_b_1()
        .border_color(theme.hairline)
        .bg(if selected_graph {
            theme.surface_raised
        } else {
            theme.surface_header
        })
        // 1. Stopwatch toggle (click toggles keyframing, Alt+click toggles driver).
        .child(
            div()
                .id(gpui::ElementId::named_usize("prop-watch", slot))
                .debug_selector(|| "prop-watch".into())
                .with_tip(deferred_builder("prop-watch"))
                .w(px(14.0))
                .h(px(14.0))
                .rounded(px(7.0))
                .border_1()
                .border_color(watch_color)
                .bg(if keyframed || driver {
                    watch_color.opacity(0.24)
                } else {
                    theme.bg.opacity(0.0)
                })
                .flex()
                .items_center()
                .justify_center()
                .cursor(CursorStyle::PointingHand)
                .text_size(Theme::TEXT_MICRO)
                .text_color(watch_color)
                .child("●")
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        this.state.update(cx, |state, _| {
                            state.select_clip(clip);
                            let action = if event.modifiers.alt {
                                Row::Driver(param)
                            } else if keyframed {
                                Row::Flatten(param)
                            } else {
                                Row::Key {
                                    param,
                                    keyed: false,
                                }
                            };
                            if let Err(err) = keyframe_rows::apply(state, action) {
                                state.report_error(&err);
                            }
                        });
                        crate::app::spawn_preview(this.state.clone(), cx);
                        cx.notify();
                    }),
                ),
        )
        // 2. Property name (click selects property for graph editor).
        .child(
            div()
                .id(gpui::ElementId::named_usize("prop-name", slot))
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .text_size(Theme::TEXT_SMALL)
                .text_color(if selected_graph {
                    theme.accent
                } else {
                    theme.text
                })
                .cursor(CursorStyle::PointingHand)
                .child(label)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        this.state.update(cx, |state, _| {
                            state.select_clip(clip);
                            state.select_timeline_property(clip, param, event.modifiers.shift);
                        });
                        cx.notify();
                    }),
                ),
        )
        // 3. The value at the playhead: dragged or typed.
        .child(value)
        // 4. Keyframe navigator: < prev, ◆ add/remove at playhead, > next.
        .child(
            div()
                .id(gpui::ElementId::named_usize("prop-prev-kf", slot))
                .with_tip(deferred_builder("kf-prev"))
                .w(px(10.0))
                .text_size(Theme::TEXT_MICRO)
                .text_color(if has_prev {
                    theme.text_muted
                } else {
                    theme.text_faint.opacity(0.4)
                })
                .cursor(CursorStyle::PointingHand)
                .child("‹")
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        this.state.update(cx, |state, _| {
                            state.select_clip(clip);
                            let _ = keyframe_rows::apply(state, Row::Previous(param));
                        });
                        crate::app::spawn_preview(this.state.clone(), cx);
                        cx.notify();
                    }),
                ),
        )
        .child(
            div()
                .id(gpui::ElementId::named_usize("prop-at-kf", slot))
                .with_tip(deferred_builder("kf-at"))
                .w(px(12.0))
                .text_size(Theme::TEXT_MICRO)
                .text_color(if keyed_here {
                    theme.accent
                } else {
                    theme.text_muted
                })
                .cursor(CursorStyle::PointingHand)
                .child(if keyed_here { "◆" } else { "◇" })
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        this.state.update(cx, |state, _| {
                            state.select_clip(clip);
                            if let Err(err) = keyframe_rows::apply(
                                state,
                                Row::Key {
                                    param,
                                    keyed: keyed_here,
                                },
                            ) {
                                state.report_error(&err);
                            }
                        });
                        crate::app::spawn_preview(this.state.clone(), cx);
                        cx.notify();
                    }),
                ),
        )
        .child(
            div()
                .id(gpui::ElementId::named_usize("prop-next-kf", slot))
                .with_tip(deferred_builder("kf-next"))
                .w(px(10.0))
                .text_size(Theme::TEXT_MICRO)
                .text_color(if has_next {
                    theme.text_muted
                } else {
                    theme.text_faint.opacity(0.4)
                })
                .cursor(CursorStyle::PointingHand)
                .child("›")
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        this.state.update(cx, |state, _| {
                            state.select_clip(clip);
                            let _ = keyframe_rows::apply(state, Row::Next(param));
                        });
                        crate::app::spawn_preview(this.state.clone(), cx);
                        cx.notify();
                    }),
                ),
        )
        // 5. Reset to the default: a keyframe at the playhead when the
        // property is animated. Dimmed when the value is already there.
        .child(
            crate::components::icon_button(("prop-reset", slot), crate::icons::ROTATE_CCW, theme)
                .when(!off_default, |button| button.opacity(0.35))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        this.state.update(cx, |state, _| {
                            state.select_clip(clip);
                            if let Err(err) = keyframe_rows::apply(state, Row::Reset(param)) {
                                state.report_error(&err);
                            }
                        });
                        crate::app::spawn_preview(this.state.clone(), cx);
                        cx.notify();
                    }),
                ),
        )
}

/// A row's value at the playhead. A scalar or a pair (its first component) is
/// a scrubbable readout writing at the playhead, which keys the property when
/// it is keyframed; a colour or a rectangle only prints.
fn value_readout(
    theme: &Theme,
    row: &TimelinePropertyRow,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<Shell>,
) -> gpui::AnyElement {
    let look = value_readout::Look {
        size: Theme::TEXT_SMALL,
        ink: theme.value,
    };
    let Some(shape) = row.readout else {
        return div()
            .font_family(theme.font_mono.clone())
            .text_size(look.size)
            .text_color(look.ink)
            .child(row.value_label.clone())
            .into_any_element();
    };
    let (clip, param, stored) = (row.clip, row.param, row.scalar_at_playhead);
    let target = ScrubTarget {
        key: MotionKey::new("prop-val", clip.raw()).with(&row.id),
        value: stored * shape.scale,
        unit: shape.unit,
        kind: ValueKind::Number {
            decimals: shape.decimals,
        },
        write: Rc::new(move |state: &mut AppState, delta| {
            state.write_property_scalar_at_playhead(clip, param, stored + delta / shape.scale)
        }),
    };
    value_readout::readout(theme, state, target, row.value_label.clone(), look, cx)
}

/// Paint the virtualized property outline rows and their keyframe icons for one
/// lane inside the lane-group `gpui::canvas`.
#[allow(clippy::too_many_arguments)]
pub(super) fn paint_outline_rows(
    window: &mut Window,
    paint_calls: &mut usize,
    theme: &Theme,
    data: &LaneGroupPaint,
    outline_rows: &[TimelineOutlineRow],
    origin_x: f32,
    rows_top_y: f32,
    view_w: f32,
) {
    for (idx, outline) in outline_rows.iter().enumerate() {
        let row_y = rows_top_y + idx as f32 * PROPERTY_ROW_H;
        match outline {
            TimelineOutlineRow::GroupHeader { .. } => {
                paint_rect(
                    window,
                    paint_calls,
                    origin_x,
                    row_y,
                    view_w,
                    PROPERTY_ROW_H,
                    theme.surface_raised.opacity(0.55),
                );
                paint_rect(
                    window,
                    paint_calls,
                    origin_x,
                    row_y + PROPERTY_ROW_H - 1.0,
                    view_w,
                    1.0,
                    theme.border,
                );
            }
            TimelineOutlineRow::Property(row) => paint_key_row(
                window,
                paint_calls,
                theme,
                data,
                KeyRow {
                    span: (row.clip_start, row.clip_end),
                    keys: &row.keys,
                    lit: row.selected_for_graph,
                },
                origin_x,
                row_y,
                view_w,
            ),
            TimelineOutlineRow::MaskPath(row) => paint_key_row(
                window,
                paint_calls,
                theme,
                data,
                KeyRow {
                    span: (row.clip_start, row.clip_end),
                    keys: &row.keys,
                    lit: false,
                },
                origin_x,
                row_y,
                view_w,
            ),
        }
    }
}

/// What one keyframe row paints: its clip's timeline span, its keyframes, and
/// whether it is lit as the graph editor's property.
struct KeyRow<'a> {
    span: (i64, i64),
    keys: &'a [TimelineKeyMark],
    lit: bool,
}

/// Paint one keyframe row's band, its clip span, its hold bars, and its
/// keyframe icons at `row_y`.
#[allow(clippy::too_many_arguments)]
fn paint_key_row(
    window: &mut Window,
    paint_calls: &mut usize,
    theme: &Theme,
    data: &LaneGroupPaint,
    row: KeyRow<'_>,
    origin_x: f32,
    row_y: f32,
    view_w: f32,
) {
    paint_rect(
        window,
        paint_calls,
        origin_x,
        row_y,
        view_w,
        PROPERTY_ROW_H,
        if row.lit {
            theme.surface_raised.opacity(0.42)
        } else {
            theme.surface.opacity(0.32)
        },
    );
    let (start, end) = row.span;
    let cx0 = (ticks_to_px(start, data.fps, data.pps) - data.scroll_x).max(0.0);
    let cx1 = (ticks_to_px(end, data.fps, data.pps) - data.scroll_x).min(view_w);
    if cx1 > cx0 {
        paint_rect(
            window,
            paint_calls,
            origin_x + cx0,
            row_y + 2.0,
            cx1 - cx0,
            PROPERTY_ROW_H - 4.0,
            theme.surface_card.opacity(0.45),
        );
        paint_rect(
            window,
            paint_calls,
            origin_x + cx0,
            row_y + PROPERTY_ROW_H * 0.5,
            cx1 - cx0,
            1.0,
            theme.border_strong.opacity(0.4),
        );
    }

    let mid_y = row_y + PROPERTY_ROW_H * 0.5;
    let ink = KeyInk::of(theme);
    for (k_idx, mark) in row.keys.iter().enumerate() {
        let kx = ticks_to_px(mark.tick, data.fps, data.pps) - data.scroll_x;
        if mark.hold
            && let Some(next_mark) = row.keys.get(k_idx + 1)
        {
            let nx = ticks_to_px(next_mark.tick, data.fps, data.pps) - data.scroll_x;
            let hx0 = kx.max(0.0);
            let hx1 = nx.min(view_w);
            if hx1 > hx0 {
                paint_rect(
                    window,
                    paint_calls,
                    origin_x + hx0,
                    mid_y - 1.0,
                    hx1 - hx0,
                    2.0,
                    theme.warning.opacity(0.75),
                );
            }
        }
        if kx < -10.0 || kx > view_w + 10.0 {
            continue;
        }
        paint_keyframe_icon(
            window,
            paint_calls,
            ink,
            origin_x + kx,
            mid_y,
            mark.shape,
            mark.selected,
        );
    }

    paint_rect(
        window,
        paint_calls,
        origin_x,
        row_y + PROPERTY_ROW_H - 1.0,
        view_w,
        1.0,
        theme.border,
    );
}

/// Render the Timeline Graph Editor band (Value graph and Speed graph with
/// incoming/outgoing influence handles and spatial tangent handles).
pub(super) fn render_graph_editor(
    theme: &Theme,
    state: &Entity<AppState>,
    pps: f32,
    fps: f32,
    scroll_x: f32,
    playhead: f32,
    cx: &mut gpui::Context<Shell>,
) -> impl IntoElement {
    let width = state.read(cx).ui_settings.outline_width;
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

    let header_col = div()
        .id("timeline-graph-header")
        .w(px(width))
        .h_full()
        .border_r_1()
        .border_color(theme.border)
        .bg(theme.surface_header)
        .p(px(6.0))
        .flex()
        .flex_col()
        .justify_between()
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(Theme::TEXT_UI)
                        .text_color(theme.text)
                        .child(active_label),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(3.0))
                        .child(
                            div()
                                .id("graph-mode-value")
                                .px(px(5.0))
                                .py(px(1.5))
                                .rounded(px(2.0))
                                .bg(if mode == GraphMode::Value {
                                    theme.accent.opacity(0.28)
                                } else {
                                    theme.surface_raised
                                })
                                .text_size(Theme::TEXT_MICRO)
                                .text_color(if mode == GraphMode::Value {
                                    theme.accent
                                } else {
                                    theme.text_muted
                                })
                                .cursor(CursorStyle::PointingHand)
                                .child("Value")
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                        cx.stop_propagation();
                                        this.state.update(cx, |state, _| {
                                            state.set_timeline_graph_mode(GraphMode::Value);
                                        });
                                        cx.notify();
                                    }),
                                ),
                        )
                        .child(
                            div()
                                .id("graph-mode-speed")
                                .px(px(5.0))
                                .py(px(1.5))
                                .rounded(px(2.0))
                                .bg(if mode == GraphMode::Speed {
                                    theme.accent.opacity(0.28)
                                } else {
                                    theme.surface_raised
                                })
                                .text_size(Theme::TEXT_MICRO)
                                .text_color(if mode == GraphMode::Speed {
                                    theme.accent
                                } else {
                                    theme.text_muted
                                })
                                .cursor(CursorStyle::PointingHand)
                                .child("Speed")
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                        cx.stop_propagation();
                                        this.state.update(cx, |state, _| {
                                            state.set_timeline_graph_mode(GraphMode::Speed);
                                        });
                                        cx.notify();
                                    }),
                                ),
                        ),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(4.0))
                .child(
                    div()
                        .id("graph-btn-easy-ease")
                        .px(px(4.0))
                        .py(px(1.5))
                        .rounded(px(2.0))
                        .bg(theme.surface_raised)
                        .text_size(Theme::TEXT_MICRO)
                        .text_color(theme.text)
                        .cursor(CursorStyle::PointingHand)
                        .child("Easy Ease (F9)")
                        .on_mouse_down(
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
                        ),
                )
                .child(
                    div()
                        .id("graph-btn-hold")
                        .px(px(4.0))
                        .py(px(1.5))
                        .rounded(px(2.0))
                        .bg(theme.surface_raised)
                        .text_size(Theme::TEXT_MICRO)
                        .text_color(theme.text)
                        .cursor(CursorStyle::PointingHand)
                        .child("Hold")
                        .on_mouse_down(
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
                        ),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .font_family(theme.font_mono.clone())
                .text_size(Theme::TEXT_MICRO)
                .text_color(theme.text_faint)
                .child(bot_lbl)
                .child(mid_lbl)
                .child(top_lbl),
        );

    let canvas_bounds = Rc::new(Cell::new(Bounds::default()));
    let bounds_prepaint = canvas_bounds.clone();
    let bounds_down = canvas_bounds.clone();
    let bounds_move = canvas_bounds.clone();
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
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                let b = bounds_down.get();
                let lx = f32::from(event.position.x - b.origin.x);
                let ly = f32::from(event.position.y - b.origin.y);
                let started = this.state.update(cx, |state, _| {
                    state.begin_timeline_graph_drag(lx, ly, fps, pps, scroll_x, GRAPH_EDITOR_H)
                });
                if started {
                    cx.stop_propagation();
                    GRAPH_DRAG_ACTIVE.set(true);
                    cx.notify();
                }
            }),
        )
        .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
            if !GRAPH_DRAG_ACTIVE.get() || !event.dragging() {
                return;
            }
            let b = bounds_move.get();
            let lx = f32::from(event.position.x - b.origin.x);
            let ly = f32::from(event.position.y - b.origin.y);
            let changed = this.state.update(cx, |state, _| {
                state.update_timeline_graph_drag(lx, ly, fps, pps, scroll_x, GRAPH_EDITOR_H)
            });
            if changed {
                cx.notify();
            }
        }))
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseUpEvent, _, cx| {
                if !GRAPH_DRAG_ACTIVE.replace(false) {
                    return;
                }
                this.state.update(cx, |state, _| {
                    if let Err(err) = state.commit_timeline_graph_drag() {
                        state.report_error(&err);
                    }
                });
                crate::app::spawn_preview(this.state.clone(), cx);
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
                                state.open_keyframe_context_menu(KeyframeContextMenu {
                                    clip: k.clip,
                                    param: k.param,
                                    tick: k.tick,
                                    interior: k.interior,
                                    hold: k.hold,
                                    roving: k.roving,
                                    x: wx,
                                    y: wy,
                                });
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
        .border_t_1()
        .border_color(theme.border_strong)
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

        // Horizontal reference grid lines (top, mid, bottom).
        for frac in [0.15f32, 0.5, 0.85] {
            paint_rect(
                window,
                &mut calls,
                ox,
                oy + vh * frac,
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

            // 2. Spatial tangent handles on Position in Value graph.
            for sh in &curve.spatial_handles {
                if sh.anchor_x < -40.0 || sh.anchor_x > vw + 40.0 {
                    continue;
                }
                paint_segment(
                    window,
                    &mut calls,
                    ox + sh.anchor_x,
                    oy + sh.anchor_y,
                    ox + sh.handle_x,
                    oy + sh.handle_y,
                    theme.success.opacity(0.75),
                );
                paint_rect(
                    window,
                    &mut calls,
                    ox + sh.handle_x - 3.0,
                    oy + sh.handle_y - 3.0,
                    6.0,
                    6.0,
                    theme.success,
                );
            }

            // 3. Incoming and outgoing temporal influence handles (0.1%..=100%).
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
                    &theme.font_mono,
                );
            }

            // 4. Keyframe nodes.
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

/// Render the floating right-click context menu for a timeline keyframe.
pub(super) fn render_keyframe_context_menu(
    theme: &Theme,
    menu: KeyframeContextMenu,
    cx: &mut gpui::Context<Shell>,
) -> impl IntoElement {
    let clip = menu.clip;
    let param = menu.param;
    let tick = menu.tick;

    div()
        .id("keyframe-context-menu")
        .debug_selector(|| "keyframe-context-menu".into())
        .absolute()
        .left(px(menu.x.max(8.0)))
        .top(px((menu.y - 120.0).max(8.0)))
        .w(px(196.0))
        .rounded(px(4.0))
        .border_1()
        .border_color(theme.border_strong)
        .bg(theme.surface_raised)
        .p(px(4.0))
        .flex()
        .flex_col()
        .gap(px(2.0))
        .child(menu_item(theme, 0, "Linear", cx, move |state| {
            state.set_keyframe_linear_at(clip, param, tick)
        }))
        .child(menu_item(theme, 1, "Easy Ease (F9)", cx, move |state| {
            state.easy_ease_selected_or_playhead()
        }))
        .child(menu_item(
            theme,
            2,
            "Easy Ease In (Shift+F9)",
            cx,
            move |state| state.easy_ease_in_selected_or_playhead(),
        ))
        .child(menu_item(
            theme,
            3,
            "Easy Ease Out (Ctrl+Shift+F9)",
            cx,
            move |state| state.easy_ease_out_selected_or_playhead(),
        ))
        .child(menu_item(
            theme,
            4,
            if menu.hold {
                "Continuous (Ctrl+Alt+H)"
            } else {
                "Toggle Hold (Ctrl+Alt+H)"
            },
            cx,
            move |state| state.toggle_hold_selected_or_playhead(),
        ))
        .when(menu.interior, |el| {
            el.child(menu_item(
                theme,
                5,
                if menu.roving {
                    "Disable Roving"
                } else {
                    "Rove Across Time"
                },
                cx,
                move |state| state.toggle_roving_keyframe(clip, param, tick),
            ))
        })
        .child(menu_item(theme, 6, "Copy Keyframe", cx, move |state| {
            state.copy_selected_keyframes();
            Ok(())
        }))
        .child(menu_item(
            theme,
            7,
            "Paste Keyframe at Playhead",
            cx,
            move |state| state.paste_keyframes_at_playhead().map(drop),
        ))
        .child(menu_item(theme, 8, "Delete Keyframe", cx, move |state| {
            state.delete_selected_keyframes().map(drop)
        }))
}

/// Render the floating right-click menu of a keyframe on a mask's Path row:
/// the easing of the segment leaving it, and removing it.
pub(super) fn render_mask_path_key_menu(
    theme: &Theme,
    menu: MaskPathKeyMenu,
    cx: &mut gpui::Context<Shell>,
) -> impl IntoElement {
    let MaskPathKeyMenu {
        clip, mask, tick, ..
    } = menu;
    let ease = move |easing: aria_models::Easing| {
        move |state: &mut AppState| state.ease_mask_path_key(clip, mask, tick, easing)
    };
    div()
        .id("mask-path-key-menu")
        .debug_selector(|| "mask-path-key-menu".into())
        .absolute()
        .left(px(menu.x.max(8.0)))
        .top(px((menu.y - 80.0).max(8.0)))
        .w(px(196.0))
        .rounded(px(4.0))
        .border_1()
        .border_color(theme.border_strong)
        .bg(theme.surface_raised)
        .p(px(4.0))
        .flex()
        .flex_col()
        .gap(px(2.0))
        .child(menu_item(
            theme,
            0,
            "Linear",
            cx,
            ease(aria_models::Easing::Linear),
        ))
        .child(menu_item(
            theme,
            1,
            "Ease In and Out",
            cx,
            ease(aria_models::Easing::EaseInOut),
        ))
        .child(menu_item(
            theme,
            2,
            "Ease In",
            cx,
            ease(aria_models::Easing::EaseIn),
        ))
        .child(menu_item(
            theme,
            3,
            "Ease Out",
            cx,
            ease(aria_models::Easing::EaseOut),
        ))
        .child(menu_item(
            theme,
            4,
            if menu.hold { "Continuous" } else { "Hold" },
            cx,
            ease(if menu.hold {
                aria_models::Easing::Linear
            } else {
                aria_models::Easing::Hold
            }),
        ))
        .child(menu_item(theme, 5, "Delete Keyframe", cx, move |state| {
            state.remove_mask_path_key(clip, mask, tick)
        }))
}

fn menu_item(
    theme: &Theme,
    slot: usize,
    label: &'static str,
    cx: &mut gpui::Context<Shell>,
    run: impl Fn(&mut AppState) -> Result<(), aria_engine::EngineError> + 'static,
) -> impl IntoElement {
    div()
        .id(gpui::ElementId::named_usize("kf-menu-row", slot))
        .debug_selector(move || format!("kf-menu-row-{slot}"))
        .w_full()
        .px(px(6.0))
        .py(px(3.0))
        .rounded(px(2.0))
        .text_size(Theme::TEXT_UI)
        .text_color(theme.text)
        .cursor(CursorStyle::PointingHand)
        .hover(|s| s.bg(theme.selection))
        .child(label)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                this.state.update(cx, |state, _| {
                    if let Err(err) = run(state) {
                        state.report_error(&err);
                    }
                    state.close_keyframe_context_menu();
                    state.close_mask_path_key_menu();
                });
                crate::app::spawn_preview(this.state.clone(), cx);
                cx.notify();
            }),
        )
}

fn paint_rect(window: &mut Window, calls: &mut usize, x: f32, y: f32, w: f32, h: f32, color: Hsla) {
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    window.paint_quad(fill(
        Bounds {
            origin: point(px(x), px(y)),
            size: size(px(w), px(h)),
        },
        color,
    ));
    *calls += 1;
}

fn paint_segment(
    window: &mut Window,
    calls: &mut usize,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    color: Hsla,
) {
    let dx = x1 - x0;
    let dy = y1 - y0;
    let steps = ((dx.abs().max(dy.abs()) / 3.0).ceil() as usize).clamp(1, 24);
    for s in 0..steps {
        let t0 = s as f32 / steps as f32;
        let t1 = (s + 1) as f32 / steps as f32;
        let sx0 = x0 + dx * t0;
        let sy0 = y0 + dy * t0;
        let sx1 = x0 + dx * t1;
        let sy1 = y0 + dy * t1;
        let rx = sx0.min(sx1);
        let ry = sy0.min(sy1);
        let rw = (sx1 - sx0).abs().max(1.5);
        let rh = (sy1 - sy0).abs().max(1.5);
        paint_rect(window, calls, rx, ry, rw, rh, color);
    }
}
