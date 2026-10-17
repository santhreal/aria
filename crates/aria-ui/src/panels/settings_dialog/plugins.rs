//! The Plugins section: every built-in plugin and installed package, what it
//! provides, what it may reach, what it cost, and what failed.
//!
//! The rows are `AppState::plugin_rows`, built from the plugin host itself and
//! the plugin folder, so a plugin added to either appears here with nothing to
//! update. A plugin the shell cannot run without is listed without a toggle.
//! An installed package also offers Reload, Uninstall and Open folder; the
//! development package offers Reload and Open folder. A plugin that logged
//! offers Log, which shows its lines, newest last, coloured by level, with Copy.

use gpui::{
    Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement, Styled,
    div, px,
};

use aria_plugin::LogLevel;

use crate::app::AppState;
use crate::components;
use crate::motion::MotionKey;
use crate::theme::Theme;
use crate::types::{UiCatalogRow, UiPluginRow};

use super::widgets::kv;

/// One line describing a plugin: version, tier, state, then what it
/// registered, what it may reach and what it cost.
fn describe(row: &UiPluginRow) -> String {
    let mut parts = Vec::new();
    if !row.version.is_empty() {
        parts.push(format!("v{}", row.version));
    }
    if !row.tier.is_empty() {
        parts.push(row.tier.to_string());
    }
    if row.development {
        parts.push("in development, reloads on change".to_owned());
    }
    parts.push(match &row.error {
        Some(error) => format!("failed: {error}"),
        None if !row.waiting_for.is_empty() => {
            format!("waits for {}", row.waiting_for.join(", "))
        }
        None if row.enabled => "active".to_owned(),
        None => "disabled".to_owned(),
    });
    if !row.services.is_empty() {
        parts.push(format!("provides {}", row.services.join(", ")));
    }
    if !row.contributions.is_empty() {
        parts.push(format!("adds {}", row.contributions.join(", ")));
    }
    if !row.requires.is_empty() {
        parts.push(format!("needs {}", row.requires.join(", ")));
    }
    if row.installed {
        parts.push(if row.permissions.is_empty() {
            "no permissions".to_owned()
        } else {
            format!("may {}", row.permissions.join("; "))
        });
    }
    if !row.denied.is_empty() {
        parts.push(format!("refused {}", row.denied.join(", ")));
    }
    if let Some(fault) = &row.fault {
        parts.push(format!("fault: {fault}"));
    }
    // Sub-millisecond mounts are the majority and read as "0.0 ms" rather than
    // being hidden, so a row with no number means nothing mounted.
    if let Some(ms) = row.mount_ms {
        parts.push(format!("{ms:.1} ms to mount"));
    }
    if let Some(ms) = row.frame_ms {
        parts.push(format!("{} per frame", format_cost_ms(ms)));
    }
    if let Some(ms) = row.block_ms {
        parts.push(format!("{} per audio block", format_cost_ms(ms)));
    }
    parts.join(" — ")
}

/// Format a non-negative runtime cost in milliseconds without collapsing
/// sub-millisecond audio blocks or shader encodes to `"0.0 ms"`.
fn format_cost_ms(ms: f32) -> String {
    if ms >= 0.1 {
        format!("{ms:.1} ms")
    } else if ms >= 0.01 {
        format!("{ms:.2} ms")
    } else if ms >= 0.001 {
        format!("{ms:.3} ms")
    } else if ms > 0.0 {
        "<0.001 ms".to_owned()
    } else {
        "0.0 ms".to_owned()
    }
}

/// What one of a row's buttons does.
#[derive(Clone, Copy)]
enum RowAction {
    Toggle(bool),
    Reload,
    Uninstall,
    OpenFolder,
    ToggleLog,
}

impl RowAction {
    fn run(self, state: &mut AppState, id: &str) {
        match self {
            Self::Toggle(enabled) => {
                state.set_plugin_enabled(id, !enabled);
            }
            Self::Reload => state.reload_plugin(id),
            Self::Uninstall => state.uninstall_plugin(id),
            Self::OpenFolder => state.open_plugin_folder(Some(id)),
            Self::ToggleLog => state.toggle_plugin_log(id),
        }
    }
}

pub(super) fn plugin_rows(
    theme: &Theme,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Vec<gpui::AnyElement> {
    let rows = state.read(cx).plugin_rows();
    let folder = state
        .read(cx)
        .plugin_installs
        .installed
        .dir()
        .display()
        .to_string();
    let mut elements: Vec<gpui::AnyElement> = vec![
        kv(theme, "Plugin folder", folder).into_any_element(),
        div()
            .flex()
            .flex_row()
            .gap(px(Theme::SPACE_SM))
            .child(
                components::ghost_button("plugin-install", "Install Plugin…", theme).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        this.state
                            .update(cx, |state, _| state.pick_plugin_archive());
                        cx.notify();
                    }),
                ),
            )
            .child(
                components::ghost_button("plugin-folder", "Open Folder", theme).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        this.state
                            .update(cx, |state, _| state.open_plugin_folder(None));
                        cx.notify();
                    }),
                ),
            )
            .into_any_element(),
    ];
    for row in rows {
        elements.push(kv(theme, &row.name, describe(&row)).into_any_element());
        let mut actions = Vec::new();
        if row.can_toggle {
            let label = if row.enabled { "Disable" } else { "Enable" };
            actions.push(("plugin-toggle", label, RowAction::Toggle(row.enabled)));
        }
        if row.installed {
            actions.push(("plugin-reload", "Reload", RowAction::Reload));
            if !row.development {
                actions.push(("plugin-uninstall", "Uninstall", RowAction::Uninstall));
            }
            actions.push(("plugin-open", "Open Folder", RowAction::OpenFolder));
        }
        if row.log_lines > 0 {
            let label = if row.log_open { "Hide Log" } else { "Log" };
            actions.push(("plugin-log", label, RowAction::ToggleLog));
        }
        if actions.is_empty() {
            continue;
        }
        let mut buttons = div().flex().flex_row().gap(px(Theme::SPACE_SM));
        for (key, label, action) in actions {
            let id = row.id.clone();
            buttons = buttons.child(
                components::ghost_button(MotionKey::new(key, row.id.clone()), label, theme)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                            this.state.update(cx, |state, _| action.run(state, &id));
                            cx.notify();
                        }),
                    ),
            );
        }
        elements.push(buttons.into_any_element());
        if row.log_open && row.log_lines > 0 {
            elements.push(log_block(theme, state, &row.id, cx));
        }
    }
    elements
}

/// The colour a log line of `level` is drawn in.
fn level_color(theme: &Theme, level: LogLevel) -> gpui::Hsla {
    match level {
        LogLevel::Trace | LogLevel::Debug => theme.text_faint,
        LogLevel::Info => theme.text_muted,
        LogLevel::Warn => theme.warning,
        LogLevel::Error => theme.danger,
    }
}

/// Plugin `plugin`'s log, oldest first, with a Copy button that puts every
/// line on the clipboard.
fn log_block(
    theme: &Theme,
    state: &Entity<AppState>,
    plugin: &gpui::SharedString,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let lines = state.read(cx).ctx.plugin_logs(plugin);
    let copied = plugin.clone();
    div()
        .flex()
        .flex_col()
        .gap(px(2.))
        .p(px(Theme::SPACE_XS))
        .rounded(px(Theme::CONTROL_RADIUS))
        .bg(theme.surface)
        .border_1()
        .border_color(theme.border)
        .font_family(theme.font_mono.clone())
        .text_size(Theme::TEXT_UI)
        .children(lines.into_iter().map(|line| {
            div()
                .flex()
                .flex_row()
                .gap(px(Theme::SPACE_XS))
                .text_color(level_color(theme, line.level))
                .child(div().flex_none().w(px(40.)).child(line.level.as_str()))
                .child(div().flex_1().child(gpui::SharedString::from(line.text)))
        }))
        .child(
            div().flex().flex_row().child(
                components::ghost_button(
                    MotionKey::new("plugin-log-copy", plugin.clone()),
                    "Copy",
                    theme,
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        let text = this.state.read(cx).plugin_log_text(&copied);
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
                    }),
                ),
            ),
        )
        .into_any_element()
}

/// One line per catalog: how many entries are in effect and where they came
/// from. A refused file says so with its reason, because a bad override
/// otherwise looks identical to having installed nothing.
fn describe_catalog(row: &UiCatalogRow) -> String {
    let source = if row.overridden {
        "your file"
    } else {
        "shipped"
    };
    let mut text = format!("{} entries — {source}", row.entries);
    if let Some(problem) = &row.problem {
        text.push_str(&format!(" — refused {problem}"));
    }
    text
}

pub(super) fn catalog_rows(
    theme: &Theme,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Vec<gpui::AnyElement> {
    let session = state.read(cx);
    let rows = session.catalog_rows();
    // The session's directory, not the default one: a process pointed at another
    // profile mounts its catalogs from there, and the row that names the
    // directory has to name the one that was read.
    let dir = crate::plugins::catalog_dir(&session.data_dir)
        .display()
        .to_string();
    let mut elements = vec![kv(theme, "Override directory", dir).into_any_element()];
    for row in rows {
        let text = describe_catalog(&row);
        elements.push(kv(theme, &row.name, text).into_any_element());
    }
    elements
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_row() -> UiPluginRow {
        UiPluginRow {
            id: "com.example.fx".into(),
            name: "Example FX".into(),
            version: "1.2.0".into(),
            tier: "native".into(),
            installed: true,
            development: false,
            can_toggle: true,
            requires: Vec::new(),
            services: Vec::new(),
            contributions: vec!["effect com.example.glow".into()],
            permissions: Vec::new(),
            denied: Vec::new(),
            enabled: true,
            waiting_for: Vec::new(),
            mount_ms: Some(1.4),
            frame_ms: None,
            frame_max_ms: None,
            block_ms: None,
            block_max_ms: None,
            error: None,
            fault: None,
            log_lines: 0,
            log_open: false,
        }
    }

    /// WHY: The Plugins panel row description must keep `{ms:.1} ms to mount`,
    /// append per-frame and per-audio-block costs only when the plugin has
    /// executed them, and preserve sub-millisecond audio block precision
    /// instead of printing `0.0 ms` for non-zero timings.
    #[test]
    fn describe_formats_mount_frame_and_sub_millisecond_audio_block_costs() {
        let mut row = sample_row();
        let idle = describe(&row);
        assert!(idle.contains("1.4 ms to mount"), "idle={idle}");
        assert!(!idle.contains("per frame"), "idle={idle}");
        assert!(!idle.contains("per audio block"), "idle={idle}");

        row.frame_ms = Some(2.1);
        row.frame_max_ms = Some(4.8);
        row.block_ms = Some(0.04);
        row.block_max_ms = Some(0.09);
        let active = describe(&row);
        assert!(
            active.contains("1.4 ms to mount — 2.1 ms per frame — 0.04 ms per audio block"),
            "active={active}"
        );

        row.block_ms = Some(0.004);
        let micro = describe(&row);
        assert!(micro.contains("0.004 ms per audio block"), "micro={micro}");

        row.block_ms = Some(0.000_3);
        let nano = describe(&row);
        assert!(nano.contains("<0.001 ms per audio block"), "nano={nano}");
    }

    /// WHY: a package that waits for an activation event is enabled but holds
    /// no code yet, so its row reads "waits for" with each event instead of
    /// "active", and a recorded failure still outranks the waiting state.
    #[test]
    fn describe_names_the_events_a_waiting_package_loads_on() {
        let mut row = sample_row();
        row.mount_ms = None;
        row.waiting_for = vec!["file:cube".into(), "command:fx.reset".into()];
        assert_eq!(
            describe(&row),
            "v1.2.0 — native — waits for file:cube, command:fx.reset — adds effect \
             com.example.glow — no permissions"
        );
        row.error = Some("did not load".into());
        assert!(describe(&row).contains("failed: did not load"));
        assert!(!describe(&row).contains("waits for"));
    }
}
