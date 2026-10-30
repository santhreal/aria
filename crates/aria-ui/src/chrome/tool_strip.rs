//! The editor tool bar: the tools, the edit mode, the workspace switcher, and
//! Export. Snapping, linked selection and the timeline display settings are
//! the Timeline header's (`panels/timeline/header.rs`), drawn there only.
//!
//! Every control here fades between its rest and hover colours through
//! [`crate::motion::hover_blend`], keyed by the element id, so the strip never
//! flips a colour under the pointer.

use gpui::{
    AnyElement, Context, CursorStyle, Entity, InteractiveElement, IntoElement, MouseButton,
    MouseDownEvent, ParentElement, StatefulInteractiveElement, Styled, div, px,
};

use crate::app::{AppState, DialogKind};
use crate::motion;
use crate::shell::Shell;
use crate::theme::Theme;
use crate::tool_catalog::ToolGroup;
use crate::tooltip::WithTip as _;
use crate::types::{AppMenu, EditMode};
use gpui::prelude::FluentBuilder;

pub(super) const TOOL_H: f32 = 36.0;

pub(super) fn tool_bar(
    theme: &Theme,
    state: &Entity<AppState>,
    cx: &mut Context<Shell>,
) -> impl IntoElement {
    let (tool, variants_open, pen_variant, mode) = {
        let state = state.read(cx);
        (
            state.tool,
            state.tool_variants_open(),
            state.pen_variant(),
            state.edit_mode,
        )
    };

    // One button per group, drawn from `tool_catalog::ToolGroup::ALL` and the
    // rows behind it, so a new tool cannot exist without a button, a glyph, or
    // a tip: the strip is the table, not a copy of it. A button shows the armed
    // tool when it is in the group, else the group's first row. A loop rather
    // than `map` because each button borrows `cx`, which cannot escape a
    // `FnMut`.
    let mut tools = Vec::with_capacity(ToolGroup::ALL.len());
    for group in ToolGroup::ALL {
        let shown = group.shown(tool);
        tools.push(
            spec_tool_btn(
                theme,
                shown.spec(),
                tool == shown,
                variants_open == Some(shown),
                pen_variant,
                cx,
            )
            .into_any_element(),
        );
    }

    // The tools and the Export button stand in two side groups that split the
    // room the switcher leaves evenly, so the switcher stands on the window's
    // centre line rather than midway between the two groups' inner edges.
    div()
        .id("tool-bar")
        .debug_selector(|| "tool-bar".into())
        .h(px(TOOL_H))
        .w_full()
        .flex()
        .flex_row()
        .items_center()
        .px(px(Theme::SPACE_MD))
        .gap(px(Theme::SPACE_XS))
        .border_t_1()
        .border_color(theme.border)
        .child(
            div()
                .flex_1()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(Theme::SPACE_XS))
                .children(tools)
                .child(rule(theme))
                .child(mode_btn(theme, mode, cx)),
        )
        .child(workspace_switcher(theme, state, cx))
        .child(
            div().flex_1().flex().flex_row().justify_end().child(
                word_icon("export", crate::icons::UPLOAD, "Export", cx, |s| {
                    s.open_dialog(DialogKind::Export);
                })
                .debug_selector(|| "tool-bar-export".into()),
            ),
        )
}

fn spec_tool_btn(
    theme: &Theme,
    spec: &crate::tool_catalog::ToolSpec,
    on: bool,
    popover_open: bool,
    pen_variant: crate::types::PenVariant,
    cx: &mut Context<Shell>,
) -> impl IntoElement {
    let id = spec.id;
    let item = spec.tool;
    let has_variants = item.has_variants();
    crate::components::icon_button(spec.id, spec.icon, theme)
        .size(px(Theme::ICON_HIT_LARGE))
        .debug_selector(move || id.into())
        .relative()
        .when(on, |el| el.text_color(theme.accent))
        .when(has_variants, |el| {
            el.child(
                div()
                    .id((id, 1usize))
                    .absolute()
                    .right(px(2.0))
                    .bottom(px(2.0))
                    .w(px(3.0))
                    .h(px(3.0))
                    .bg(if on { theme.accent } else { theme.text_muted })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            this.state.update(cx, |state, _| {
                                if state.tool_variants_open() == Some(item) {
                                    state.close_tool_variants();
                                } else {
                                    state.open_tool_variants(item);
                                }
                            });
                            cx.notify();
                        }),
                    ),
            )
        })
        .when(popover_open, |el| {
            el.child(variants_popover(theme, spec, on, pen_variant, cx))
        })
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                let seq = this.state.update(cx, |state, _| {
                    state.tool = item;
                    state.close_tool_variants();
                    state.begin_tool_press(item)
                });
                if has_variants {
                    cx.spawn(async move |this, cx| {
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(
                                crate::app::clip_text::tool_hold::TOOL_HOLD_MS,
                            ))
                            .await;
                        let _ = this.update(cx, |shell, cx| {
                            let opened = shell
                                .state
                                .update(cx, |state, _| state.hold_open_tool_variants(item, seq));
                            if opened {
                                cx.notify();
                            }
                        });
                    })
                    .detach();
                }
                cx.notify();
            }),
        )
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(|this, _: &gpui::MouseUpEvent, _, cx| {
                this.state.update(cx, |state, _| state.end_tool_press());
            }),
        )
}

fn variants_popover(
    theme: &Theme,
    spec: &crate::tool_catalog::ToolSpec,
    tool_active: bool,
    pen_variant: crate::types::PenVariant,
    cx: &mut Context<Shell>,
) -> impl IntoElement {
    use crate::app::clip_text::tool_hold::ToolVariant;
    let variants = spec.tool.variants();
    let mut rows = Vec::with_capacity(variants.len());
    for (idx, &variant) in variants.iter().enumerate() {
        let active = match variant {
            ToolVariant::Tool(t) => tool_active && t == spec.tool,
            ToolVariant::Pen(pv) => tool_active && pv == pen_variant,
        };
        let shortcut = variant
            .shortcut_control()
            .and_then(crate::tooltip::tip_for)
            .and_then(|tip| crate::tooltip::chord_label(&tip, cx));
        rows.push(
            div()
                .id((spec.id, idx))
                .px(px(Theme::SPACE_SM))
                .py(px(Theme::SPACE_XS))
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(Theme::SPACE_MD))
                .rounded(px(Theme::CONTROL_RADIUS))
                .cursor(CursorStyle::PointingHand)
                .bg(if active {
                    theme.element_active
                } else {
                    theme.surface
                })
                .text_color(if active { theme.accent } else { theme.text })
                .text_size(Theme::TEXT_UI)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        this.state.update(cx, |state, _| {
                            state.select_tool_variant(variant);
                        });
                        cx.notify();
                    }),
                )
                .child(variant.label())
                .children(shortcut.map(|chord| div().text_color(theme.text_muted).child(chord)))
                .into_any_element(),
        );
    }
    div()
        .id((spec.id, 2usize))
        .occlude()
        .absolute()
        .top(px(TOOL_H - 2.0))
        .left(px(0.0))
        .w(px(168.0))
        .p(px(4.0))
        .flex()
        .flex_col()
        .gap(px(2.0))
        .rounded(px(Theme::CONTROL_RADIUS))
        .bg(theme.surface)
        .border_1()
        .border_color(theme.border)
        .children(rows)
}

/// The display button `id`: a wrench `size` px square that holds the View
/// menu's submenu `caption` out under itself while pressed. The Timeline
/// header draws one for Timeline Display Settings, the Viewer one for Viewer
/// Display Settings.
pub(crate) fn display_btn(
    id: &'static str,
    caption: &'static str,
    size: f32,
    theme: &Theme,
    state: &Entity<AppState>,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let on = state.read(cx).menu_owner == Some(id);
    let button = tool_btn(id, crate::icons::WRENCH, on, cx, move |s| {
        s.toggle_display_menu(id, caption)
    })
    .size(px(size))
    .into_any_element();
    super::menu_view::submenu_under(id, AppMenu::View, caption, button, theme, state, cx)
}

/// A toggle on a bar: an icon that reads lit while `on`, and runs `action` on
/// a press. The Timeline header draws its toggles with it.
pub(crate) fn tool_btn(
    id: &'static str,
    path: &'static str,
    on: bool,
    cx: &mut Context<Shell>,
    action: impl Fn(&mut AppState) + 'static,
) -> gpui::Stateful<gpui::Div> {
    crate::components::lit_icon(id, path, on, &Theme::of(cx)).on_mouse_down(
        MouseButton::Left,
        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
            this.state.update(cx, |state, _| action(state));
            cx.notify();
        }),
    )
}

fn word_icon(
    id: impl Into<motion::ControlId>,
    path: &'static str,
    label: &'static str,
    cx: &mut Context<Shell>,
    action: impl Fn(&mut AppState) + 'static,
) -> gpui::Stateful<gpui::Div> {
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
        .child(
            div()
                .text_size(Theme::TEXT_UI)
                .text_color(theme.text)
                .child(label),
        )
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
        .debug_selector(|| "edit-mode".into())
        .when_some(crate::tooltip::builder("edit-mode"), |el, tip| {
            el.with_tip(tip)
        })
        .px(px(Theme::SPACE_SM))
        .py(px(Theme::SPACE_XS))
        .rounded(px(Theme::CONTROL_RADIUS))
        .text_size(Theme::TEXT_UI)
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

fn workspace_switcher(
    theme: &Theme,
    state: &Entity<AppState>,
    cx: &mut Context<Shell>,
) -> impl IntoElement {
    let rows: Vec<(&'static str, &'static str, bool)> = state
        .read(cx)
        .workspace_rows()
        .into_iter()
        .map(|row| (row.id, row.name, row.active))
        .collect();
    let mut tabs = Vec::with_capacity(rows.len());
    for (ws_id, name, active) in rows {
        tabs.push(
            crate::components::underline_tab(("workspace-tab", ws_id), name, active, theme)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        this.state.update(cx, |state, cx| {
                            state.apply_workspace(ws_id, cx);
                        });
                    }),
                )
                .into_any_element(),
        );
    }
    // A row of tabs on the bar itself, as a dock's tab row is, with no frame
    // around it: the open workspace stands in full text over the accent
    // underline on the bar's bottom edge.
    div()
        .id("workspace-switcher")
        .debug_selector(|| "workspace-switcher".into())
        .h_full()
        .flex()
        .flex_row()
        .items_center()
        .children(tabs)
}

fn rule(theme: &Theme) -> impl IntoElement {
    div()
        .w(px(1.))
        .h(px(Theme::SPACE_LG))
        .bg(theme.border)
        .mx(px(Theme::SPACE_XS))
}
