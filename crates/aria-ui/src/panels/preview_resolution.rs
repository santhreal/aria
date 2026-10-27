//! The Viewer's resolution controls: a chip stating the playback resolution,
//! the list of playback and paused choices it opens above itself, and the
//! proxy switch.

use gpui::{AnyElement, Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent};

use super::menu::{heading, row};
use crate::app::AppState;
use crate::app::preview_view::ViewerMenu;
use crate::resolution::{PausedResolution, PlaybackResolution};
use crate::theme::Theme;

/// The chip, and the list above it while it is open.
pub(super) fn picker(
    state: &Entity<AppState>,
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let open = super::menu::is_open(state, ViewerMenu::Resolution, cx);
    let read = state.read(cx);
    let playback = read.ui_settings.playback_resolution;
    let paused = read.ui_settings.paused_resolution;
    let caption = read.playback_resolution_text();
    let chip = crate::components::list_chip("preview-resolution", caption, open, theme);
    let rows = open.then(|| choices(playback, paused, theme, cx));
    super::menu::picker(
        "preview-resolution",
        ViewerMenu::Resolution,
        chip,
        rows,
        theme,
        cx,
    )
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

/// The list's rows: the playback choices, then the paused ones.
fn choices(
    playback: PlaybackResolution,
    paused: PausedResolution,
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Vec<AnyElement> {
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
    rows
}
