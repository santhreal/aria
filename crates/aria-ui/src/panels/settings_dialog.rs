//! Settings dialog: Appearance, Playback, Provider keys, Storage, Transcription, Cloud, Plugins, Keys, Workspaces, Canvas.
//!
//! The dialog is a list of sections, each generated from a table rather than written
//! out row by row, so a new provider, model, timeout, cache, or theme appears without
//! an edit here. Every section lives in its own module; this one only assembles them.

mod ai;
mod caches;
mod keyboard;
mod keys;
mod layouts;
mod pickers;
mod plugin_limits;
mod plugin_settings;
mod plugins;
mod providers;
mod storage;
mod transcription;
mod widgets;

#[cfg(test)]
#[path = "settings_dialog/settings_dialog_tests.rs"]
mod settings_dialog_tests;

use aria_models::{AudioLayout, CanvasAspect, Rational, TimeDisplay};
use gpui::{
    Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement, Styled,
    div, px,
};

use crate::fluent::FluentBuilder;

use crate::app::settings_form::SettingsField;
use crate::app::{AppState, DialogKind};
use crate::appearance::{self, AppearanceMode};
use crate::chrome::dialog::{DialogBody, dialog_surface};
use crate::components;
use crate::theme::Theme;

use widgets::{group_divider, kv, section};

pub fn render(
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let theme = Theme::of(cx);
    let mode = appearance::mode(cx);
    let filter = ai::field_row(&theme, state, SettingsField::Filter, cx);
    // Test the section title before evaluating its row expression. A search for
    // one section therefore constructs only that section's element tree.
    let mut sections: Vec<(&'static str, Vec<gpui::AnyElement>)> = Vec::new();
    macro_rules! push_visible {
        ($title:literal, $rows:expr) => {
            if state.read(cx).settings_section_visible($title) {
                sections.push(($title, $rows));
            }
        };
    }
    push_visible!("Appearance", appearance_rows(&theme, state, mode, cx));
    push_visible!("Playback", playback_rows(&theme, state, cx));
    push_visible!("AI", ai::ai_rows(&theme, state, cx));
    push_visible!("Provider keys", providers::provider_rows(&theme, state, cx));
    push_visible!("Storage", storage::storage_rows(&theme, state, cx));
    push_visible!("Caches", caches::cache_rows(&theme, state, cx));
    push_visible!(
        "Transcription",
        transcription::model_rows(&theme, state, cx)
    );
    push_visible!("Cloud", cloud_rows(&theme, state, cx));
    push_visible!("Plugins", {
        let mut rows = plugin_limits::limit_rows(&theme, state, cx);
        rows.extend(plugins::plugin_rows(&theme, state, cx));
        rows
    });
    // The Keys section lists its bindings under a filter that names one, so a
    // search for a command finds its key without the section's title.
    if let Some(rows) = keys::section(&theme, state, cx) {
        sections.push(("Keys", rows));
    }
    push_visible!("Workspaces", layouts::workspace_rows(&theme, state, cx));
    push_visible!("Catalogs", plugins::catalog_rows(&theme, state, cx));
    push_visible!("Configuration", ai::schema_rows(&theme, state, cx));
    push_visible!(
        "Application",
        vec![
            kv(
                &theme,
                "UI data",
                state.read(cx).data_dir.display().to_string(),
            )
            .into_any_element(),
        ]
    );
    // A plugin's own settings, declared through `SettingsSchema`. One section per
    // plugin, drawn with the same row as a shipped field: a plugin that declares a
    // setting is configurable without an edit here.
    for declared in state.read(cx).plugin_settings_sections() {
        if state.read(cx).settings_section_visible(&declared.title) {
            let rows = plugin_settings::section_rows(&theme, state, &declared, cx);
            sections.push((crate::interned::intern(&declared.title), rows));
        }
    }
    // Canvas is a project setting, so the section is absent with no project open
    // rather than editing a project that does not exist.
    if !state.read(cx).store.project_name.is_empty()
        && state.read(cx).settings_section_visible("Canvas")
    {
        sections.push(("Canvas", canvas_rows(&theme, state, cx)));
    }
    let visible = sections;
    let matched = !visible.is_empty();
    let done = components::button("settings-done", "Done", &theme).on_mouse_down(
        MouseButton::Left,
        cx.listener(|this, _: &MouseDownEvent, _, cx| {
            this.state
                .update(cx, |state, _| state.open_dialog(DialogKind::None));
            cx.notify();
        }),
    );
    let mut body = div().flex().flex_col().gap(px(Theme::SPACE_MD));
    for (title, rows) in visible {
        body = body.child(section(&theme, title, rows));
    }
    let body = body.when(!matched, |el| {
        el.child(kv(&theme, "No section", "Nothing matches that filter"))
    });
    dialog_surface(
        "settings",
        DialogKind::Settings,
        "Settings",
        state,
        cx,
        DialogBody::new(body).header(filter).action(done),
    )
}

/// Light/dark mode, the saved accent choice, the theme file, then how much
/// motion runs. Theme files are read from `<data dir>/themes`; one that cannot
/// be read is listed with the reason and cannot be chosen.
fn appearance_rows(
    theme: &Theme,
    state: &Entity<AppState>,
    mode: AppearanceMode,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Vec<gpui::AnyElement> {
    let mut rows: Vec<gpui::AnyElement> = AppearanceMode::ALL
        .into_iter()
        .map(|candidate| pickers::appearance_row(theme, candidate, mode, cx).into_any_element())
        .collect();
    rows.push(group_divider(theme));
    let current = state.read(cx).theme_choice();
    for candidate in aria_settings::ThemeChoice::ALL {
        rows.push(pickers::theme_row(theme, candidate, current, cx).into_any_element());
    }
    rows.push(group_divider(theme));
    let files = &state.read(cx).theme_files;
    let current_file = state.read(cx).ui_settings.theme_file.clone();
    let candidates: Vec<_> = std::iter::once(None)
        .chain(files.themes.iter().cloned().map(Some))
        .collect();
    let failed = files.failed.clone();
    for candidate in candidates {
        rows.push(
            pickers::theme_file_row(theme, candidate, current_file.as_deref(), cx)
                .into_any_element(),
        );
    }
    for (file, reason) in &failed {
        rows.push(pickers::theme_file_failure_row(theme, file, reason).into_any_element());
    }
    rows.push(group_divider(theme));
    let motion = state.read(cx).ui_settings.motion;
    for candidate in crate::motion::MotionPreference::ALL {
        rows.push(pickers::motion_row(theme, candidate, motion, cx).into_any_element());
    }
    rows
}

/// Playback toggles: audio scrubbing and dropped-frame logging, each drawn as a
/// label with On and Off chips that save immediately.
fn playback_rows(
    theme: &Theme,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Vec<gpui::AnyElement> {
    let (scrubbing, logging) = {
        let s = &state.read(cx).ui_settings;
        (s.audio_scrubbing, s.log_dropped_frames)
    };
    vec![
        on_off_row(
            theme,
            "settings-audio-scrubbing",
            "Audio scrubbing",
            scrubbing,
            |settings, on| settings.audio_scrubbing = on,
            cx,
        ),
        on_off_row(
            theme,
            "settings-log-dropped-frames",
            "Log dropped frames",
            logging,
            |settings, on| settings.log_dropped_frames = on,
            cx,
        ),
    ]
}

/// A label and an On and an Off chip; the lit chip is `value`, and a press
/// stores the other through `set` and saves it.
fn on_off_row(
    theme: &Theme,
    id: impl Into<crate::motion::ControlId>,
    label: &'static str,
    value: bool,
    set: fn(&mut crate::settings::UiSettings, bool),
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let id = id.into();
    let mut row = div()
        .id(id.element("settings-on-off"))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(Theme::SPACE_SM))
        .child(
            div()
                .w(px(140.0))
                .text_size(Theme::TEXT_UI)
                .text_color(theme.text_muted)
                .child(label),
        );
    for (shown, on) in [("On", true), ("Off", false)] {
        let key = id.motion(shown);
        let chip = if value == on {
            components::button(key, shown, theme)
        } else {
            components::ghost_button(key, shown, theme)
        };
        row = row.child(chip.on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                this.state.update(cx, |state, cx| {
                    set(&mut state.ui_settings, on);
                    state.persist_settings_later(cx);
                    state.status_message = Some(format!("{label}: {shown}"));
                });
                cx.notify();
            }),
        ));
    }
    row.into_any_element()
}

/// The cloud endpoint. One field, because everything else about cloud access is a
/// provider key, which has its own section.
fn cloud_rows(
    theme: &Theme,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Vec<gpui::AnyElement> {
    vec![ai::field_row(theme, state, SettingsField::CloudBaseUrl, cx)]
}

fn format_fps(rate: Rational) -> String {
    match (rate.num, rate.den) {
        (24000, 1001) => "23.976 fps (NTSC Cinema)".to_string(),
        (24, 1) => "24 fps (Cinema)".to_string(),
        (25, 1) => "25 fps (PAL)".to_string(),
        (30000, 1001) => "29.97 fps (NTSC Video)".to_string(),
        (30, 1) => "30 fps (Web / Video)".to_string(),
        (50, 1) => "50 fps (PAL High)".to_string(),
        (60000, 1001) => "59.94 fps (NTSC High)".to_string(),
        (60, 1) => "60 fps (Web High)".to_string(),
        _ if rate.den == 1 => format!("{} fps", rate.num),
        _ => format!("{:.3} fps ({}/{})", rate.as_f64(), rate.num, rate.den),
    }
}

/// The canvas size row that hands the size back to the aspect or the footage.
const DERIVED_SIZE: &str = "Derived from the aspect or the footage";

/// A delivery baseline's canvas size row: the baseline and its pixels.
fn size_label(baseline: u32, size: [u32; 2]) -> String {
    format!("{baseline}p — {}×{}", size[0], size[1])
}

/// The aspect presets, then the render size: what it is now, "Derived", and one
/// row per delivery baseline. Sizes are read from the project rather than
/// recomputed here, so the dialog and the exporter cannot name different pixels.
fn canvas_rows(
    theme: &Theme,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Vec<gpui::AnyElement> {
    let canvas = state.read(cx).canvas();
    let [w, h] = state.read(cx).canvas_size();
    let choices = state.read(cx).canvas_resolution_choices();
    let origin = if canvas.resolution.is_some() {
        "explicit"
    } else {
        "derived"
    };
    let mut rows = vec![kv(theme, "Size", format!("{w}×{h} ({origin})")).into_any_element()];
    rows.extend(CanvasAspect::ALL.into_iter().map(|candidate| {
        pickers::canvas_row(theme, candidate, canvas.aspect, cx).into_any_element()
    }));
    rows.push(
        pickers::resolution_row(
            theme,
            crate::motion::ControlId::Fixed("canvas-size-derived"),
            DERIVED_SIZE.into(),
            None,
            canvas.resolution.is_none(),
            cx,
        )
        .into_any_element(),
    );
    rows.extend(choices.into_iter().map(|(baseline, size)| {
        pickers::resolution_row(
            theme,
            crate::motion::MotionKey::new("canvas-size", baseline).into(),
            size_label(baseline, size).into(),
            Some(size),
            canvas.resolution == Some(size),
            cx,
        )
        .into_any_element()
    }));
    // Frame rate and pool mismatch
    let current_rate = state.read(cx).store.sequence.frame_rate;
    rows.push(kv(theme, "Frame rate", format_fps(current_rate)).into_any_element());

    if let Some(largest_rate) = state.read(cx).frame_rate_mismatch() {
        let note = format!(
            "Footage mismatch: largest source is {}",
            format_fps(largest_rate)
        );
        rows.push(kv(theme, "Mismatch", note).into_any_element());
    }

    for rate in crate::surfaces::settings_pick::RATES {
        rows.push(
            pickers::sequence_rate_row(theme, rate, current_rate, format_fps(rate).into(), cx)
                .into_any_element(),
        );
    }

    // Audio layout
    let current_layout = state.read(cx).audio_layout();
    rows.push(kv(theme, "Audio layout", current_layout.label()).into_any_element());
    for layout in AudioLayout::ALL {
        rows.push(pickers::audio_layout_row(theme, layout, current_layout, cx).into_any_element());
    }

    // Time display
    let current_display = state.read(cx).store.sequence.time_display;
    rows.push(kv(theme, "Time display", current_display.label()).into_any_element());
    for display in TimeDisplay::ALL {
        rows.push(
            pickers::time_display_row(theme, display, current_display, cx).into_any_element(),
        );
    }

    rows
}

#[cfg(test)]
#[path = "settings_dialog/rows_tests.rs"]
mod rows_tests;

#[cfg(test)]
#[path = "settings_dialog/choice_fit_tests.rs"]
mod choice_fit_tests;
