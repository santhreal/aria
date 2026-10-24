//! The plugin install prompt.
//!
//! Shows the package an `.ariaplugin` holds: name, publisher, version, tier and
//! the permissions it asks for, and for a native package that its code can
//! reach the system beyond them. Install commits it; Cancel deletes the
//! unpacked files. Both then show the next archive dropped with it.

use gpui::{
    Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement, Styled,
    div, px,
};

use crate::app::plugin_install::InstallPrompt;
use crate::app::{AppState, DialogKind};
use crate::chrome::dialog::{DialogBody, dialog_surface};
use crate::components;
use crate::theme::Theme;

/// One line the prompt shows under the title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptLine<'a> {
    /// A sentence about the package.
    Text(String),
    /// The permissions the package asks for, one to a row, drawn indented
    /// under the line that introduces them.
    Permissions(&'a [String]),
}

/// The lines the prompt shows under the title, in order.
pub fn lines(prompt: &InstallPrompt) -> Vec<PromptLine<'_>> {
    let mut lines = vec![
        PromptLine::Text(format!("{} {}", prompt.name, prompt.version)),
        PromptLine::Text(format!(
            "{} plugin from {}, id {}",
            prompt.tier, prompt.publisher, prompt.id
        )),
    ];
    if let Some(old) = &prompt.replaces {
        lines.push(PromptLine::Text(format!(
            "Replaces the installed version {old}."
        )));
    }
    lines.push(PromptLine::Text(
        match (prompt.permissions.is_empty(), prompt.replaces.is_some()) {
            (true, _) => "It asks for no permissions.".to_owned(),
            (false, false) => "It asks for:".to_owned(),
            (false, true) => "This version also asks for:".to_owned(),
        },
    ));
    if !prompt.permissions.is_empty() {
        lines.push(PromptLine::Permissions(&prompt.permissions));
    }
    if prompt.waiting > 0 {
        lines.push(PromptLine::Text(format!(
            "{} more waiting after this one.",
            prompt.waiting
        )));
    }
    lines
}

/// The prompt's title: Update Plugin for a package that replaces an
/// installed version, otherwise Install Plugin.
pub fn title(prompt: Option<&InstallPrompt>) -> &'static str {
    if prompt.is_some_and(|prompt| prompt.replaces.is_some()) {
        "Update Plugin"
    } else {
        "Install Plugin"
    }
}

/// The warning a native package carries.
pub const NATIVE_WARNING: &str = "Native code runs inside Aria. Its permissions are checked at \
     the plugin API, but it can reach your files, network and processes beyond them. Install it \
     only if you trust the publisher.";

pub fn render(
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let theme = Theme::of(cx);
    let prompt = state.read(cx).install_prompt();

    let cancel = components::ghost_button("install-plugin-cancel", "Cancel", &theme).on_mouse_down(
        MouseButton::Left,
        cx.listener(|this, _: &MouseDownEvent, _, cx| {
            this.state
                .update(cx, |state, _| state.cancel_plugin_install());
            cx.notify();
        }),
    );
    let install = components::button(
        "install-plugin-confirm",
        if prompt.as_ref().is_some_and(|p| p.replaces.is_some()) {
            "Update"
        } else {
            "Install"
        },
        &theme,
    )
    .on_mouse_down(
        MouseButton::Left,
        cx.listener(|this, _: &MouseDownEvent, _, cx| {
            this.state
                .update(cx, |state, _| state.confirm_plugin_install());
            cx.notify();
        }),
    );

    let mut body = div().flex().flex_col().gap(px(Theme::SPACE_SM));
    match &prompt {
        Some(prompt) => {
            for (index, line) in lines(prompt).into_iter().enumerate() {
                body = body.child(match line {
                    PromptLine::Text(text) => div()
                        .text_size(if index == 0 {
                            Theme::TEXT_TITLE
                        } else {
                            Theme::TEXT_UI
                        })
                        .text_color(if index == 0 {
                            theme.text
                        } else {
                            theme.text_muted
                        })
                        .child(text),
                    PromptLine::Permissions(permissions) => div()
                        .flex()
                        .flex_col()
                        .gap(px(Theme::SPACE_XS))
                        .pl(px(Theme::SPACE_MD))
                        .text_size(Theme::TEXT_UI)
                        .text_color(theme.text_muted)
                        .children(
                            permissions
                                .iter()
                                .map(|permission| div().child(permission.clone())),
                        ),
                });
            }
            if prompt.native {
                body = body.child(
                    div()
                        .text_size(Theme::TEXT_TITLE)
                        .text_color(theme.warning)
                        .child(NATIVE_WARNING),
                );
            }
        }
        // The prompt closes itself once nothing is pending; a frame drawn in
        // between shows this rather than an empty sheet.
        None => {
            body = body.child(
                div()
                    .text_size(Theme::TEXT_TITLE)
                    .text_color(theme.text_faint)
                    .child("No plugin is waiting to be installed."),
            );
        }
    }
    let mut actions = DialogBody::new(body).action(cancel);
    if prompt.is_some() {
        actions = actions.action(install);
    }
    dialog_surface(
        "install-plugin",
        DialogKind::InstallPlugin,
        title(prompt.as_ref()),
        state,
        cx,
        actions,
    )
}

#[cfg(test)]
#[path = "install_dialog_tests.rs"]
mod tests;
