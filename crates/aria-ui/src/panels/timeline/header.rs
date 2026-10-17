//! The timeline header over the outline column: the sequence timecode with
//! the playhead's frame count under it, the property search, and the toggles
//! for snapping, linked selection, markers, the display settings, and the
//! graph editor.
//!
//! The timecode is a playhead field like the Viewer's (`app/viewer_scrub.rs`):
//! a drag scrubs, a click opens it for typing. The toggles are the tool bar's
//! toggles under their own ids, so each lights on its own.

use gpui::{
    CursorStyle, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    SharedString, Styled, div, px,
};

use crate::app::AppState;
use crate::app::clock::ClockSite;
use crate::chrome::tool_strip::{display_btn, tool_btn};
use crate::components;
use crate::fluent::FluentBuilder;
use crate::icons;
use crate::theme::Theme;
use crate::tooltip::WithTip as _;
use crate::types::ActivePanel;

/// Height of the timecode row, which holds the 18 px timecode.
const TIMECODE_ROW_H: f32 = 22.0;
/// Height of the frame count and toggle row.
const TOGGLE_ROW_H: f32 = 16.0;

pub(super) fn header_controls(
    theme: &Theme,
    state: &gpui::Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let app = state.read(cx);
    let outline = app.ui_settings.outline_width;
    let (tc_open, tc_text) = app.clock_field(ClockSite::Timeline);
    let frames = frame_count(app.store.playhead_ticks);
    let query = SharedString::from(app.property_query().to_owned());
    let searching = !query.is_empty();
    let search_open = app.property_search_focused();
    let snap = app.snap_enabled;
    let linked = app.linked_selection;
    let graph_open = app.timeline_graph_open();

    let timecode = components::text_box("timeline-tc", tc_text, "", tc_open, theme)
        .debug_selector(|| "timeline-tc".into())
        .h(px(TIMECODE_ROW_H))
        .px(px(Theme::SPACE_XXS))
        .flex_shrink_0()
        .font_family(theme.font_mono.clone())
        .text_size(Theme::TEXT_TIMECODE)
        .text_color(theme.playhead)
        // Closed, the timecode reads as a number rather than a box: the
        // frame and the fill come back with the caret.
        .when(!tc_open, |el| {
            el.bg(theme.wash(0.0))
                .border_color(theme.wash(0.0))
                .cursor(CursorStyle::ResizeLeftRight)
        })
        .when_some(crate::tooltip::builder("timeline-tc"), |el, tip| {
            el.with_tip(tip)
        })
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                this.state
                    .update(cx, |state, _| state.press_clock(ClockSite::Timeline));
                cx.notify();
            }),
        );

    let search = components::text_box(
        "property-search",
        query,
        "Search properties",
        search_open,
        theme,
    )
    .debug_selector(|| "property-search".into())
    .flex_1()
    .min_w_0()
    .h(px(Theme::INPUT_HEIGHT))
    .overflow_hidden()
    .on_mouse_down(
        MouseButton::Left,
        cx.listener(|this, _: &MouseDownEvent, _, cx| {
            this.state
                .update(cx, |state, _| state.focus_property_search(true));
            cx.notify();
        }),
    )
    // A press anywhere else gives the keys back and keeps the filter, and
    // still lands where it was aimed.
    .when(search_open, |el| {
        el.on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, _, cx| {
            this.state
                .update(cx, |state, _| state.focus_property_search(false));
            cx.notify();
        }))
    });

    div()
        .id("timeline-header-controls")
        .w(px(outline))
        .h_full()
        .flex_shrink_0()
        .border_r_1()
        .border_color(theme.border)
        .px(px(Theme::SPACE_SM))
        .flex()
        .flex_col()
        .justify_center()
        .gap(px(Theme::SPACE_XXS))
        .overflow_hidden()
        // A press here is the header's: the pan and zoom tools act on the time
        // area, not on these controls.
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                this.state
                    .update(cx, |state, _| state.active_panel = ActivePanel::Timeline);
            }),
        )
        .child(
            div()
                .h(px(TIMECODE_ROW_H))
                .flex()
                .items_center()
                .gap(px(Theme::SPACE_XS))
                .child(timecode)
                .child(search)
                .when(searching, |el| {
                    el.child(
                        components::icon_button("property-search-clear", icons::CLOSE, theme)
                            .debug_selector(|| "property-search-clear".into())
                            .flex_shrink_0()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                    this.state
                                        .update(cx, |state, _| state.clear_property_query());
                                    cx.notify();
                                }),
                            ),
                    )
                }),
        )
        .child(
            div()
                .h(px(TOGGLE_ROW_H))
                .flex()
                .items_center()
                .gap(px(Theme::SPACE_XXS))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .font_family(theme.font_mono.clone())
                        .text_size(Theme::TEXT_MICRO)
                        .text_color(theme.text_faint)
                        .child(frames),
                )
                .child(tool_btn("timeline-snap", icons::MAGNET, snap, cx, |s| {
                    s.toggle_snapping()
                }))
                .child(tool_btn("timeline-linked", icons::LINK, linked, cx, |s| {
                    s.toggle_linked_selection()
                }))
                .child(tool_btn("timeline-marker", icons::FLAG, false, cx, |s| {
                    if let Err(err) = s.add_or_edit_marker() {
                        s.report_error(&err);
                    }
                }))
                .child(display_btn("timeline-display-settings", theme, state, cx))
                .child(tool_btn(
                    "timeline-graph",
                    icons::GRAPH,
                    graph_open,
                    cx,
                    |s| s.toggle_timeline_graph_editor(),
                )),
        )
}

/// The playhead's frame number as the header prints it under the timecode.
fn frame_count(frame: i64) -> String {
    if frame == 1 {
        "1 frame".to_owned()
    } else {
        format!("{frame} frames")
    }
}
