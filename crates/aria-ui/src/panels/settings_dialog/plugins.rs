//! The Plugins section: every built-in plugin, what it provides, and what failed.
//!
//! The rows are `AppState::plugin_rows`, built from the plugin host itself — the
//! built-in list, the mounted set, and the mount-failure ledger — so a plugin
//! added to the host appears here with nothing to update.
//!
//! A plugin the shell cannot run without is listed without a toggle. Everything
//! else can be turned off, which drops its services from the registry: the
//! surfaces that ask for them find nothing and hide, rather than erroring.

use gpui::{
    Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement, Styled,
    div, px,
};

use crate::app::AppState;
use crate::components;
use crate::theme::Theme;
use crate::types::{UiCatalogRow, UiPluginRow};

use super::widgets::kv;

/// One line describing a plugin: its version, then either what it registered or
/// why it is not running.
fn describe(row: &UiPluginRow) -> String {
    let mut parts = vec![format!("v{}", row.version)];
    if let Some(error) = &row.error {
        parts.push(format!("failed: {error}"));
    } else if !row.enabled {
        parts.push("disabled".to_string());
    } else if row.services.is_empty() {
        // A plugin can be all event handlers and no service; say so rather than
        // leaving the row looking truncated.
        parts.push("no services".to_string());
    } else {
        parts.push(row.services.join(", "));
    }
    if !row.requires.is_empty() {
        parts.push(format!("needs {}", row.requires.join(", ")));
    }
    if !row.denied.is_empty() {
        parts.push(format!("refused {}", row.denied.join(", ")));
    }
    // What it cost to start: a start is the sum of these, and the list that names
    // the plugins is the only place an operator can see which one is expensive.
    // Sub-millisecond mounts are the majority and read as "0.0 ms" rather than
    // being hidden, so a row with no number means nothing mounted.
    if let Some(ms) = row.mount_ms {
        parts.push(format!("{ms:.1} ms to mount"));
    }
    parts.join(" — ")
}

pub(super) fn plugin_rows(
    theme: &Theme,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Vec<gpui::AnyElement> {
    let rows = state.read(cx).plugin_rows();
    let mut elements: Vec<gpui::AnyElement> = Vec::new();
    for row in rows {
        elements.push(kv(theme, &row.name, describe(&row)).into_any_element());
        if !state.read(cx).plugin_can_toggle(&row.id) {
            continue;
        }
        let id = row.id.clone();
        let enabled = row.enabled;
        elements.push(
            div()
                .flex()
                .flex_row()
                .gap(px(Theme::SPACE_SM))
                .child(
                    components::ghost_button(
                        crate::motion::MotionKey::new("plugin-toggle", row.id),
                        if enabled { "Disable" } else { "Enable" },
                        theme,
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                            let id = id.clone();
                            this.state.update(cx, |state, _| {
                                state.set_plugin_enabled(&id, !enabled);
                            });
                            cx.notify();
                        }),
                    ),
                )
                .into_any_element(),
        );
    }
    elements
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
