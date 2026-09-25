//! The editor tool strip: tools, transport, the timecode field, the edit mode,
//! and the rail toggles.
//!
//! Moved out of `chrome.rs` so the bar's frame and the strip's contents are
//! separate files. Every control here fades between its rest and hover colours
//! through [`crate::motion::hover_blend`], keyed by the element id, so the strip
//! never flips a colour under the pointer.

use gpui::{
    Context, CursorStyle, Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    ParentElement, StatefulInteractiveElement, Styled, div, px,
};

use crate::app::{AppState, DialogKind, spawn_preview, start_playback};
use crate::fluent::FluentBuilder;
use crate::motion;
use crate::shell::Shell;
use crate::theme::Theme;
use crate::tool_catalog::tool_specs;
use crate::types::EditMode;

const TOOL_H: f32 = 36.0;

pub(super) fn tool_bar(
    theme: &Theme,
    state: &Entity<AppState>,
    cx: &mut Context<Shell>,
) -> impl IntoElement {
    let tool = state.read(cx).tool;
    let snap = state.read(cx).snap_enabled;
    let mode = state.read(cx).edit_mode;
    let playing = state.read(cx).playing;
    let sidebar_on = !state.read(cx).ui_settings.sidebar_collapsed;
    let inspector_on = !state.read(cx).ui_settings.inspector_collapsed;

    // Drawn from `tool_catalog::tool_specs()`, so a new tool cannot exist
    // without a button, a glyph, or a tip: the strip is the table, not a copy of
    // it. A loop rather than `map` because each button borrows `cx`, which
    // cannot escape a `FnMut`.
    let mut tools = Vec::with_capacity(tool_specs().len());
    for spec in tool_specs() {
        let item = spec.tool;
        tools.push(
            tool_btn(spec.id, spec.icon, tool == item, cx, move |s| s.tool = item)
                .into_any_element(),
        );
    }

    div()
        .id("tool-bar")
        .h(px(TOOL_H))
        .w_full()
        .flex()
        .flex_row()
        .items_center()
        .px(px(Theme::SPACE_MD))
        .gap(px(Theme::SPACE_XS))
        .border_t_1()
        .border_color(theme.border)
        .children(tools)
        .child(rule(theme))
        .child(icon_act("go-start", crate::icons::SKIP_BACK, cx, |s| {
            s.set_playhead(0)
        }))
        .child(icon_act(
            "prev-frame",
            crate::icons::CHEVRON_LEFT,
            cx,
            |s| {
                s.set_playhead(s.playhead.value - 1);
            },
        ))
        // The transport button names what it will do next, not what it did: the
        // id carries the state, so its tip reads "Pause" while the sequence runs.
        .child(tool_btn(
            if playing { "pause" } else { "play" },
            if playing {
                crate::icons::PAUSE
            } else {
                crate::icons::PLAY
            },
            playing,
            cx,
            |s| s.toggle_playback(),
        ))
        .child(icon_act(
            "next-frame",
            crate::icons::CHEVRON_RIGHT,
            cx,
            |s| {
                s.set_playhead(s.playhead.value + 1);
            },
        ))
        .child(icon_act("go-end", crate::icons::SKIP_FORWARD, cx, |s| {
            s.set_playhead(s.store.sequence.duration_ticks);
        }))
        .child({
            let focused = state.read(cx).clock_focused();
            let text = state.read(cx).clock_text();
            crate::components::text_box("toolbar-tc", text, "00:00:00:00", focused, theme)
                .font_family(theme.font_mono.clone())
                .text_size(px(11.))
                .when_some(crate::tooltip::builder("toolbar-tc"), |el, tip| {
                    el.tooltip(tip)
                })
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        this.state.update(cx, |state, _| state.focus_clock(true));
                        cx.notify();
                    }),
                )
        })
        .child(rule(theme))
        .child(tool_btn("snap", crate::icons::MAGNET, snap, cx, |s| {
            s.snap_enabled = !s.snap_enabled;
        }))
        .child(mode_btn(theme, mode, cx))
        .child(div().flex_1())
        .child(word_icon(
            "export",
            crate::icons::UPLOAD,
            "Export",
            cx,
            |s| {
                s.open_dialog(DialogKind::Export);
            },
        ))
        .child(icon_act("settings", crate::icons::SETTINGS, cx, |s| {
            s.open_dialog(DialogKind::Settings);
        }))
        .child(rail_toggle(sidebar_on, true, cx))
        .child(rail_toggle(inspector_on, false, cx))
}

fn tool_btn(
    id: &'static str,
    path: &'static str,
    on: bool,
    cx: &mut Context<Shell>,
    action: impl Fn(&mut AppState) + 'static,
) -> impl IntoElement {
    let theme = Theme::of(cx);
    // The same key `icon_button` fades on: it owns the hover listener, and a
    // second one on the same element is not supported, so the selected look
    // reads that fade rather than starting another.
    let key = motion::MotionKey::new("icon", id);
    crate::components::icon_button(id, path, &theme)
        .when(on, |el| {
            el.bg(motion::hover_blend(
                key,
                theme.glass_selected_bg(),
                theme.element_active,
            ))
            .text_color(theme.text)
        })
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                this.state.update(cx, |state, _| action(state));
                if matches!(id, "play" | "pause") {
                    start_playback(this.state.clone(), cx);
                    spawn_preview(this.state.clone(), cx);
                }
                cx.notify();
            }),
        )
}

fn icon_act(
    id: &'static str,
    path: &'static str,
    cx: &mut Context<Shell>,
    action: impl Fn(&mut AppState) + 'static,
) -> impl IntoElement {
    tool_btn(id, path, false, cx, action)
}

fn word_icon(
    id: impl Into<motion::ControlId>,
    path: &'static str,
    label: &'static str,
    cx: &mut Context<Shell>,
    action: impl Fn(&mut AppState) + 'static,
) -> impl IntoElement {
    let theme = Theme::of(cx);
    let id = id.into();
    let key = id.motion("word");
    div()
        .id(id.element("word-icon"))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(Theme::SPACE_XS))
        .px(px(Theme::SPACE_SM))
        .h(px(Theme::SPACE_LG + Theme::SPACE_XS))
        .rounded(px(Theme::CONTROL_RADIUS))
        .cursor(CursorStyle::PointingHand)
        .bg(motion::hover_blend(
            key,
            theme.wash(0.0),
            theme.element_hover,
        ))
        .text_color(motion::hover_blend(key, theme.text_muted, theme.text))
        // Press feedback: dim while held, matching the shared controls.
        .active(move |style| style.opacity(0.92))
        .on_hover(motion::hover_listener(key))
        .child(crate::icons::icon(path))
        .child(div().text_size(px(11.)).text_color(theme.text).child(label))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                this.state.update(cx, |state, _| action(state));
                cx.notify();
            }),
        )
}

fn mode_btn(theme: &Theme, mode: EditMode, cx: &mut Context<Shell>) -> impl IntoElement {
    let label = match mode {
        EditMode::Ripple => "Ripple",
        EditMode::Overwrite => "Overwrite",
    };
    let key = motion::MotionKey::new("mode", "edit-mode");
    div()
        .id("edit-mode")
        .px(px(Theme::SPACE_SM))
        .py(px(Theme::SPACE_XS))
        .rounded(px(Theme::CONTROL_RADIUS))
        .text_size(px(11.))
        .text_color(theme.text)
        .cursor(CursorStyle::PointingHand)
        .active(|style| style.opacity(0.92))
        .bg(motion::hover_blend(
            key,
            theme.wash(0.0),
            theme.element_hover,
        ))
        .on_hover(motion::hover_listener(key))
        .child(label)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                this.state.update(cx, |state, _| {
                    state.edit_mode = match state.edit_mode {
                        EditMode::Ripple => EditMode::Overwrite,
                        EditMode::Overwrite => EditMode::Ripple,
                    };
                });
                cx.notify();
            }),
        )
}

fn rail_toggle(on: bool, sidebar: bool, cx: &mut Context<Shell>) -> impl IntoElement {
    let theme = Theme::of(cx);
    let id = if sidebar {
        "rail-sidebar"
    } else {
        "rail-inspector"
    };
    let key = motion::MotionKey::new("icon", id);
    crate::components::icon_button(
        id,
        if sidebar {
            crate::icons::FOLDER_OPEN
        } else {
            crate::icons::SETTINGS
        },
        &theme,
    )
    .when(on, |el| {
        el.bg(motion::hover_blend(
            key,
            theme.glass_selected_bg(),
            theme.element_active,
        ))
        .text_color(theme.text)
    })
    .on_mouse_down(
        MouseButton::Left,
        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
            this.state.update(cx, |state, _| {
                if sidebar {
                    state.ui_settings.sidebar_collapsed = !state.ui_settings.sidebar_collapsed;
                } else {
                    state.ui_settings.inspector_collapsed = !state.ui_settings.inspector_collapsed;
                }
            });
            this.persist(cx);
            cx.notify();
        }),
    )
}

fn rule(theme: &Theme) -> impl IntoElement {
    div()
        .w(px(1.))
        .h(px(Theme::SPACE_LG))
        .bg(theme.border)
        .mx(px(Theme::SPACE_XS))
}
