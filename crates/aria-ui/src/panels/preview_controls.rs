//! The Viewer's control-row buttons: transport, the playhead field, marks,
//! loop, and zoom.
//!
//! Each item is `pub(super)` so the parent and its other children reach it by
//! name.

use super::*;

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
        .debug_selector(move || id.to_string())
        .bg(motion::hover_blend(key, theme.solid, theme.accent_strong))
        // The glyph travels with the fill under it: neutral on the solid plate,
        // accent-safe once the plate is accent.
        .text_color(motion::hover_blend(key, theme.on_solid, theme.on_accent))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                this.state
                    .update(cx, |state, _| state.toggle_viewer_playback());
                start_playback(this.state.clone(), window, cx);
                spawn_preview(this.state.clone(), cx);
                cx.notify();
            }),
        )
}

/// A transport jump to either end of the tab in front of the Viewer. The press
/// makes the Viewer the active panel first, so the jump reaches a Source tab in
/// front of it the way the keys do.
pub(super) fn jump_btn(
    id: &'static str,
    path: &'static str,
    cx: &mut gpui::Context<crate::shell::Shell>,
    jump: fn(&mut AppState),
) -> impl IntoElement {
    let theme = Theme::of(cx);
    crate::components::icon_button(id, path, &theme)
        .debug_selector(move || id.to_string())
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                this.state.update(cx, |state, _| {
                    state.active_panel = ActivePanel::Preview;
                    jump(state);
                });
                spawn_preview(this.state.clone(), cx);
                cx.notify();
            }),
        )
}

/// A one-frame step on the tab in front of the Viewer, `frames` signed. Held,
/// it repeats at that tab's frame rate until the button is released.
pub(super) fn step_btn(
    id: &'static str,
    path: &'static str,
    frames: i64,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let theme = Theme::of(cx);
    crate::components::icon_button(id, path, &theme)
        .debug_selector(move || id.to_string())
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                this.state
                    .update(cx, |state, _| state.active_panel = ActivePanel::Preview);
                crate::app::step_hold::hold_step(this.state.clone(), frames, cx);
                cx.notify();
            }),
        )
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(|this, _: &gpui::MouseUpEvent, _, cx| {
                this.state.update(cx, |state, _| state.end_step_hold());
            }),
        )
        .on_mouse_up_out(
            MouseButton::Left,
            cx.listener(|this, _: &gpui::MouseUpEvent, _, cx| {
                this.state.update(cx, |state, _| state.end_step_hold());
            }),
        )
}

/// Mark In or Mark Out on the sequence, drawn while Program is in front. A
/// Source tab marks its take from its own bar.
pub(super) fn mark_btn(
    id: &'static str,
    label: &'static str,
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
    mark: fn(&mut AppState),
) -> impl IntoElement {
    row_text(id, label, theme.text_muted, theme).on_mouse_down(
        MouseButton::Left,
        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
            cx.stop_propagation();
            this.state.update(cx, |state, _| mark(state));
            cx.notify();
        }),
    )
}

/// The sequence playhead as a field. A drag scrubs the playhead a frame per
/// two pixels; a click opens the field for typing, where Enter seeks to a
/// timecode and a signed duration moves the selection or the playhead. The
/// press is the state's, see [`crate::app::viewer_scrub`].
pub(super) fn clock_field(
    state: &Entity<AppState>,
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let (focused, text) = state
        .read(cx)
        .clock_field(crate::app::clock::ClockSite::Viewer);
    // The row's controls are one height, so the field is an icon button's.
    crate::components::text_box("playhead-tc", text, "00:00:00:00", focused, theme)
        .h(px(Theme::ICON_HIT))
        .text_size(Theme::TEXT_UI)
        .font_features(crate::theme::tabular_figures())
        .debug_selector(|| "playhead-tc".into())
        .when_some(crate::tooltip::builder("playhead-tc"), |el, tip| {
            el.with_tip(tip)
        })
        .when(!focused, |el| el.cursor(gpui::CursorStyle::ResizeLeftRight))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                this.state.update(cx, |state, _| {
                    state.press_clock(crate::app::clock::ClockSite::Viewer);
                });
                cx.notify();
            }),
        )
        // A press anywhere else drops the draft, as Escape does, and still
        // lands where it was aimed.
        .when(focused, |el| {
            el.on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, _, cx| {
                this.state.update(cx, |state, _| state.focus_clock(false));
                cx.notify();
            }))
        })
}

/// A worded control in the Viewer's row: an icon button's height, a hover
/// wash, and the label in `color`.
pub(super) fn row_text(
    id: impl Into<motion::ControlId>,
    label: impl Into<gpui::SharedString>,
    color: gpui::Hsla,
    theme: &Theme,
) -> gpui::Stateful<gpui::Div> {
    let id = id.into();
    let key = id.motion("row-text");
    div()
        .id(id.element("row-text"))
        .when_some(id.fixed_name(), |el, name| {
            el.debug_selector(move || name.to_owned())
        })
        .flex_none()
        .h(px(Theme::ICON_HIT))
        .px(px(Theme::SPACE_XS))
        .flex()
        .items_center()
        .rounded(px(Theme::CONTROL_RADIUS))
        .text_size(Theme::TEXT_UI)
        .text_color(color)
        .bg(motion::hover_blend(
            key,
            theme.wash(0.0),
            theme.element_hover,
        ))
        .on_hover(motion::hover_listener(key))
        .cursor(gpui::CursorStyle::PointingHand)
        .active(|style| style.opacity(0.92))
        .when_some(
            id.fixed_name().and_then(crate::tooltip::builder),
            |el, tip| crate::tooltip::WithTip::with_tip(el, tip),
        )
        .child(label.into())
}

/// Loop playback, lit while the tab in front loops.
pub(super) fn loop_btn(
    state: &Entity<AppState>,
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let on = state.read(cx).looping;
    crate::chrome::tool_strip::lit_icon("preview-loop", crate::icons::LOOP, on, theme)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                this.state.update(cx, |state, _| state.toggle_loop());
                cx.notify();
            }),
        )
}

/// A preview zoom step, drawn with the shipped zoom glyph in an icon button's
/// box.
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
        .flex_none()
        .size(px(Theme::ICON_HIT))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(Theme::CONTROL_RADIUS))
        .text_size(Theme::TEXT_UI)
        .text_color(motion::hover_blend(key, theme.text_muted, theme.text))
        .cursor(gpui::CursorStyle::PointingHand)
        .active(|style| style.opacity(0.92))
        .bg(motion::hover_blend(
            key,
            theme.wash(0.0),
            theme.element_hover,
        ))
        .on_hover(motion::hover_listener(key))
        .when_some(tip, |el, tip| el.with_tip(tip))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                this.state.update(cx, |state, _| step(state));
                cx.notify();
            }),
        )
        .child(crate::icons::icon(icon))
}
