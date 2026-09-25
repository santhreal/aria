//! Light/dark switching.
//!
//! Ported from Comet (<https://github.com/zeronsh/comet>)
//! MIT License — see LICENSES/comet-MIT.txt

use std::path::{Path, PathBuf};

use gpui::{App, Global, Subscription, Window};
use serde::{Deserialize, Serialize};

use crate::settings::UiSettings;
use crate::theme::{Appearance, Theme};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AppearanceMode {
    #[default]
    System,
    Light,
    Dark,
}

impl AppearanceMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }

    pub const ALL: [Self; 3] = [Self::System, Self::Light, Self::Dark];
}

pub struct AppearanceState {
    pub mode: AppearanceMode,
    pub system: Appearance,
    pub data_dir: PathBuf,
    /// The saved accent choice. It lives here rather than in `UiSettings` because
    /// `config.toml` already owns it and the two files must not disagree.
    pub choice: aria_settings::ThemeChoice,
}

impl Global for AppearanceState {}

pub fn resolve(mode: AppearanceMode, system: Appearance) -> Appearance {
    match mode {
        AppearanceMode::System => system,
        AppearanceMode::Light => Appearance::Light,
        AppearanceMode::Dark => Appearance::Dark,
    }
}

/// The appearance `mode` would resolve to right now.
///
/// A control that offers a mode can then draw the palette the click would
/// install, rather than naming it: "System" is not a colour, and which one it
/// means is exactly what the operator cannot see from the word.
pub fn preview(mode: AppearanceMode, cx: &App) -> Appearance {
    let system = cx
        .try_global::<AppearanceState>()
        .map(|state| state.system)
        .unwrap_or_else(|| Appearance::from_window(cx.window_appearance()));
    resolve(mode, system)
}

pub fn init(mode: AppearanceMode, data_dir: impl Into<PathBuf>, cx: &mut App) {
    let system = Appearance::from_window(cx.window_appearance());
    let data_dir = data_dir.into();
    // A config that will not parse still has to open the window: the accent falls back
    // to the shipped one rather than blocking startup on a settings file.
    let choice = aria_settings::load(&data_dir.join("config.toml"))
        .unwrap_or_default()
        .appearance
        .theme;
    tracing::debug!(?mode, ?system, ?choice, "appearance: initial");
    cx.set_global(AppearanceState {
        mode,
        system,
        data_dir,
        choice,
    });
    Theme::install(resolve(mode, system), choice, cx);
}

/// Install a new accent choice. The config file is written by `AppState`, which owns
/// every settings write; this only repaints.
pub fn set_theme_choice(choice: aria_settings::ThemeChoice, cx: &mut App) {
    if !cx.has_global::<AppearanceState>() {
        return;
    }
    let state = cx.global_mut::<AppearanceState>();
    if state.choice == choice {
        return;
    }
    state.choice = choice;
    let appearance = resolve(state.mode, state.system);
    Theme::install(appearance, choice, cx);
    cx.refresh_windows();
    reapply_window_background(cx);
}

/// The saved accent choice.
pub fn theme_choice(cx: &App) -> aria_settings::ThemeChoice {
    cx.try_global::<AppearanceState>()
        .map(|state| state.choice)
        .unwrap_or_default()
}

pub fn mode(cx: &App) -> AppearanceMode {
    cx.try_global::<AppearanceState>()
        .map(|s| s.mode)
        .unwrap_or_default()
}

pub fn set_mode(mode: AppearanceMode, cx: &mut App) {
    if !cx.has_global::<AppearanceState>() {
        return;
    }
    let state = cx.global_mut::<AppearanceState>();
    if state.mode == mode {
        return;
    }
    state.mode = mode;
    let data_dir = state.data_dir.clone();
    apply(cx);
    persist(mode, &data_dir);
}

fn persist(mode: AppearanceMode, data_dir: &Path) {
    let mut settings = UiSettings::load(data_dir);
    settings.appearance = mode;
    if let Err(err) = settings.save(data_dir) {
        tracing::warn!(error = %err, "could not persist appearance");
    }
}

pub fn observe_window(window: &mut Window, cx: &mut App) -> Subscription {
    sync(Appearance::from_window(window.appearance()), cx);
    window.observe_window_appearance(|window, cx| {
        sync(Appearance::from_window(window.appearance()), cx);
    })
}

fn sync(system: Appearance, cx: &mut App) {
    if !cx.has_global::<AppearanceState>() {
        return;
    }
    let state = cx.global_mut::<AppearanceState>();
    if state.system == system {
        return;
    }
    tracing::debug!(?system, "appearance: system changed");
    state.system = system;
    apply(cx);
}

pub fn apply(cx: &mut App) {
    let Some(state) = cx.try_global::<AppearanceState>() else {
        return;
    };
    let wanted = resolve(state.mode, state.system);
    let choice = state.choice;
    let changed = !cx
        .try_global::<crate::theme::ThemeHandle>()
        .is_some_and(|t| t.appearance == wanted);
    if changed {
        tracing::debug!(?wanted, "appearance: installing palette");
        Theme::install(wanted, choice, cx);
        cx.refresh_windows();
    }
    reapply_window_background(cx);
}

pub fn reapply_window_background(cx: &mut App) {
    let Some(wanted) = cx
        .try_global::<crate::theme::ThemeHandle>()
        .map(|theme| theme.window_background_appearance())
    else {
        return;
    };
    for window in cx.windows() {
        window
            .update(cx, |_, window, _| {
                window.set_background_appearance(wanted);
            })
            .ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_mode_follows_the_os() {
        assert_eq!(
            resolve(AppearanceMode::System, Appearance::Light),
            Appearance::Light
        );
        assert_eq!(
            resolve(AppearanceMode::System, Appearance::Dark),
            Appearance::Dark
        );
    }

    #[test]
    fn pinned_modes_ignore_the_os() {
        for system in [Appearance::Light, Appearance::Dark] {
            assert_eq!(resolve(AppearanceMode::Light, system), Appearance::Light);
            assert_eq!(resolve(AppearanceMode::Dark, system), Appearance::Dark);
        }
    }

    #[test]
    fn default_mode_is_system() {
        assert_eq!(AppearanceMode::default(), AppearanceMode::System);
    }

    #[test]
    fn mode_serialises_stably() {
        for (mode, json) in [
            (AppearanceMode::System, "\"system\""),
            (AppearanceMode::Light, "\"light\""),
            (AppearanceMode::Dark, "\"dark\""),
        ] {
            assert_eq!(serde_json::to_string(&mode).unwrap(), json);
            assert_eq!(
                serde_json::from_str::<AppearanceMode>(json).unwrap(),
                mode,
                "{json} should parse back"
            );
        }
    }
}
