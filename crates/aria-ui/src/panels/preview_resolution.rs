//! The Viewer's resolution controls: a chip stating the playback resolution,
//! the list of playback and paused choices it opens above itself, and the
//! proxy switch.

use crate::tooltip::WithTip as _;
use gpui::{
    AnyElement, Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    ParentElement, StatefulInteractiveElement, Styled, deferred, div, px,
};

use crate::app::AppState;
use crate::resolution::{PausedResolution, PlaybackResolution};
use crate::theme::Theme;
use gpui::prelude::FluentBuilder;

/// Height of the chip, which the list opens above: the control height every
/// control in the Viewer's row shares.
const CHIP_H: f32 = Theme::ICON_HIT;

/// The chip, and the list above it while it is open.
pub(super) fn picker(
    state: &Entity<AppState>,
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let read = state.read(cx);
    let open = read.resolution_list_open;
    let playback = read.ui_settings.playback_resolution;
    let paused = read.ui_settings.paused_resolution;
    let caption = read.playback_resolution_text();
    let list = open.then(|| list(playback, paused, theme, cx));
    // The outside press is the wrapper's, not the list's: the chip is outside
    // the list, and a press on it toggles the list rather than closing and
    // reopening it.
    div()
        .relative()
        .when(open, |el| {
            el.on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, _, cx| {
                this.state
                    .update(cx, |state, _| state.resolution_list_open = false);
                cx.notify();
            }))
        })
        .child(
            crate::components::list_chip("preview-resolution", caption, open, theme)
                .h(px(CHIP_H))
                .debug_selector(|| "preview-resolution".into())
                .when_some(crate::tooltip::builder("preview-resolution"), |el, tip| {
                    el.with_tip(tip)
                })
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        this.state.update(cx, |state, _| {
                            state.resolution_list_open = !state.resolution_list_open;
                        });
                        cx.notify();
                    }),
                ),
        )
        .when_some(list, |el, list| el.child(list))
}

/// The proxy switch: lit while the Viewer reads proxies, unlit while it reads
/// the original files.
pub(super) fn proxy_toggle(
    state: &Entity<AppState>,
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let on = state.read(cx).proxies_on();
    crate::components::lit_icon("preview-proxies-toggle", crate::icons::PROXY, on, theme)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                this.state.update(cx, AppState::toggle_proxies);
                cx.notify();
            }),
        )
}

/// The open list: the playback choices, then the paused ones.
fn list(
    playback: PlaybackResolution,
    paused: PausedResolution,
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement + use<> {
    let mut rows: Vec<AnyElement> =
        Vec::with_capacity(PlaybackResolution::ALL.len() + PausedResolution::ALL.len() + 2);
    rows.push(heading("Playback", theme));
    for (index, choice) in PlaybackResolution::ALL.into_iter().enumerate() {
        rows.push(row(
            ("preview-resolution-playback", index),
            choice.label(),
            choice == playback,
            theme,
            cx,
            move |state, cx| state.set_playback_resolution(choice, cx),
        ));
    }
    rows.push(heading("Paused", theme));
    for (index, choice) in PausedResolution::ALL.into_iter().enumerate() {
        rows.push(row(
            ("preview-resolution-paused", index),
            choice.label(),
            choice == paused,
            theme,
            cx,
            move |state, cx| state.set_paused_resolution(choice, cx),
        ));
    }
    let panel = div()
        .id("preview-resolution-list")
        .min_w(px(120.))
        .rounded(px(Theme::PANEL_RADIUS))
        .bg(theme.surface_overlay)
        .border_1()
        .border_color(theme.border)
        .p(px(Theme::SPACE_XS))
        .flex()
        .flex_col()
        // A press on the list is the list's, not the Viewer's drawn under it.
        .occlude()
        .children(rows);
    // Deferred so the list paints over the frame above the control row.
    deferred(
        div()
            .absolute()
            .bottom(px(CHIP_H + Theme::SPACE_XS))
            .left(px(0.))
            .child(crate::frost::frost(crate::motion::menu_in(
                crate::motion::MotionKey::new("menu", "preview-resolution"),
                panel,
            ))),
    )
}

/// A section label inside the list.
fn heading(text: &'static str, theme: &Theme) -> AnyElement {
    div()
        .px(px(Theme::SPACE_SM))
        .pt(px(Theme::SPACE_XS))
        .text_size(Theme::TEXT_UI)
        .text_color(theme.text_faint)
        .child(text)
        .into_any_element()
}

/// One choice. The selected one holds the selected wash.
fn row(
    id: (&'static str, usize),
    label: &'static str,
    selected: bool,
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
    choose: impl Fn(&mut AppState, &mut gpui::Context<AppState>) + 'static,
) -> AnyElement {
    let id = gpui::ElementId::from(id);
    let key = crate::motion::MotionKey::new("preview-resolution-row", &id);
    let (rest, hot) = if selected {
        (theme.glass_selected_bg(), theme.element_active)
    } else {
        (theme.wash(0.0), theme.element_hover)
    };
    div()
        .id(id)
        .px(px(Theme::SPACE_SM))
        .py(px(Theme::SPACE_XXS))
        .rounded(px(Theme::CONTROL_RADIUS))
        .text_size(Theme::TEXT_TITLE)
        .text_color(if selected {
            theme.text
        } else {
            theme.text_muted
        })
        .bg(crate::motion::hover_blend(key, rest, hot))
        .cursor(gpui::CursorStyle::PointingHand)
        .active(|style| style.opacity(0.92))
        .on_hover(crate::motion::hover_listener(key))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                this.state.update(cx, |state, cx| choose(state, cx));
                cx.notify();
            }),
        )
        .child(label)
        .into_any_element()
}
