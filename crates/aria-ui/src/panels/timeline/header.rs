//! The timeline header over the outline column: the sequence timecode, the
//! property search, and the toggles for snapping, linked selection, markers,
//! the display settings, and the graph editor.
//!
//! The header is two columns over two 24 px rows. The left column holds the
//! timecode, centred on the two rows, its first digit on the column's text
//! edge. The right column is five toggles wide and holds the search field
//! over the toggles, so the field and the toggles share both edges. The right
//! column keeps its width at every outline width; the left column takes the
//! rest, and the timecode steps down to 14 px when that is narrower than an
//! 18 px timecode.
//!
//! The timecode is a playhead field like the Viewer's (`app/viewer_scrub.rs`):
//! a drag scrubs, a click opens it for typing, and a press anywhere else
//! closes it. At rest it is plain figures; the field's border and fill fade
//! in under the pointer, and the accent border and caret show while it holds
//! the keys. The toggles are the header's own controls for snapping, linked
//! selection, markers, the display settings, and the Graph Editor.

use gpui::{
    CursorStyle, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    Pixels, SharedString, Styled, div, px,
};

use crate::app::AppState;
use crate::app::clock::ClockSite;
use crate::chrome::tool_strip::{display_btn, tool_btn};
use crate::components;
use crate::fluent::FluentBuilder;
use crate::icons;
use crate::motion;
use crate::theme::Theme;
use crate::tooltip::WithTip as _;
use crate::types::ActivePanel;

/// Height of each header row.
pub(super) const HEADER_ROW_H: f32 = Theme::ICON_HIT;
/// The toggles in the right column, left to right.
const TOGGLES: usize = 5;
/// Width of the right column: the toggles, edge to edge.
pub(super) const TOOLS_W: f32 = TOGGLES as f32 * Theme::ICON_HIT;
/// Outline width under which the timecode is drawn at `TEXT_HEADING`: below
/// it the left column is narrower than an 18 px timecode.
pub(super) const TIMECODE_FULL_MIN: f32 = 272.0;
/// The timecode field's border plus its inner padding: the field steps left by
/// this much so its first digit stands on the column's text edge.
const FIELD_INSET: f32 = 1.0 + Theme::SPACE_XXS;

/// The timecode's text size at an outline column `outline` px wide.
pub(super) fn timecode_size(outline: f32) -> Pixels {
    if outline >= TIMECODE_FULL_MIN {
        Theme::TEXT_TIMECODE
    } else {
        Theme::TEXT_HEADING
    }
}

pub(super) fn header_controls(
    theme: &Theme,
    state: &gpui::Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let app = state.read(cx);
    let outline = app.outline_drawn_width();
    let (tc_open, tc_text) = app.clock_field(ClockSite::Timeline);
    let query = SharedString::from(app.property_query().to_owned());
    let searching = !query.is_empty();
    let search_open = app.property_search_focused();
    let snap = app.snap_enabled;
    let linked = app.linked_selection;
    let graph_open = app.timeline_graph_open();

    let tc_id = motion::ControlId::from("timeline-tc");
    let lit = motion::hover_t(tc_id.motion("field"));
    let timecode = components::text_box(tc_id, tc_text, "", tc_open, theme)
        .debug_selector(|| "timeline-tc".into())
        .h(px(HEADER_ROW_H))
        .ml(px(-FIELD_INSET))
        .px(px(Theme::SPACE_XXS))
        .flex_shrink_0()
        .text_size(timecode_size(outline))
        .font_features(crate::theme::tabular_figures())
        // The line stands inside the field's 1 px border: the font's own line
        // height would run past the row above and below it.
        .line_height(px(HEADER_ROW_H - 2.0))
        .text_color(theme.playhead)
        // At rest the timecode is figures, not a box: the frame and the fill
        // fade in under the pointer and hold with the caret.
        .when(!tc_open, |el| {
            el.bg(motion::mix(
                theme.wash(0.0),
                theme.surface_raised_hover,
                lit,
            ))
            .border_color(motion::mix(theme.wash(0.0), theme.border_strong, lit))
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
        )
        // A press anywhere else drops the draft, as Escape does, and still
        // lands where it was aimed.
        .when(tc_open, |el| {
            el.on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, _, cx| {
                this.state.update(cx, |state, _| state.focus_clock(false));
                cx.notify();
            }))
        });

    let search = components::text_box("property-search", query, "Search", search_open, theme)
        .debug_selector(|| "property-search".into())
        .flex_1()
        .min_w_0()
        .h(px(Theme::INPUT_HEIGHT))
        .truncate()
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

    let row = || div().h(px(HEADER_ROW_H)).flex_none().flex().items_center();

    let clock_column = div()
        .flex_1()
        .min_w_0()
        .h(px(2.0 * HEADER_ROW_H))
        .flex()
        .items_center()
        .child(timecode);

    let tools_column = div()
        .w(px(TOOLS_W))
        .flex_none()
        .flex()
        .flex_col()
        .child(row().child(search).when(searching, |el| {
            el.child(
                components::icon_button("property-search-clear", icons::CLOSE, theme)
                    .debug_selector(|| "property-search-clear".into())
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseDownEvent, _, cx| {
                            this.state
                                .update(cx, |state, _| state.clear_property_query());
                            cx.notify();
                        }),
                    ),
            )
        }))
        .child(
            row()
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
                .child(display_btn(
                    "timeline-display-settings",
                    crate::chrome::menu_items::TIMELINE_DISPLAY,
                    theme,
                    state,
                    cx,
                ))
                .child(tool_btn(
                    "timeline-graph",
                    icons::GRAPH,
                    graph_open,
                    cx,
                    |s| s.toggle_timeline_graph_editor(),
                )),
        );

    div()
        .id("timeline-header-controls")
        .debug_selector(|| "timeline-header-controls".into())
        .w(px(outline))
        .h_full()
        .flex_shrink_0()
        .border_r_1()
        .border_color(theme.border)
        .px(px(Theme::SPACE_SM))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(Theme::SPACE_SM))
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
        .child(clock_column)
        .child(tools_column)
}
