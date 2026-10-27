//! A plugin's own settings section, drawn from what it declared.
//!
//! Text, secret and number fields are typed rows, the same row a shipped field
//! draws with. A toggle is a switch and a choice is one chip per option; a
//! click saves the value at once through [`AppState::set_plugin_setting`],
//! which checks it against the field.

use aria_plugin::{SettingKind, SettingSpec, SettingsSection};
use gpui::{Entity, InteractiveElement, MouseButton, MouseDownEvent, Stateful};

use crate::app::AppState;
use crate::app::settings_form::SettingsField;
use crate::components::{self, ChipSize};
use crate::interned::intern;
use crate::motion::MotionKey;
use crate::theme::Theme;

/// One row per declared field, in declaration order.
pub(super) fn section_rows(
    theme: &Theme,
    state: &Entity<AppState>,
    section: &SettingsSection,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Vec<gpui::AnyElement> {
    section
        .fields
        .iter()
        .map(|spec| field(theme, state, spec, cx))
        .collect()
}

fn field(
    theme: &Theme,
    state: &Entity<AppState>,
    spec: &SettingSpec,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let key = intern(&spec.key);
    let label = intern(&spec.label);
    let current = state.read(cx).plugin_setting(key);
    match &spec.kind {
        SettingKind::Toggle => {
            let on = current == "true";
            let switch = components::switch(
                ("plugin-setting-switch", MotionKey::new(key, "switch")),
                Some(on),
                theme,
            );
            let stored = if on { "false" } else { "true" };
            super::widgets::chip_row(theme, label, [saving(switch, key, stored, cx)])
        }
        SettingKind::Choice(choices) => {
            let chips: Vec<_> = choices
                .iter()
                .map(|choice| {
                    let choice = intern(choice);
                    let chip = components::chip(
                        ("plugin-setting-option", MotionKey::new(key, choice)),
                        choice,
                        current == choice,
                        ChipSize::Regular,
                        theme,
                    );
                    saving(chip, key, choice, cx)
                })
                .collect();
            super::widgets::chip_row(theme, label, chips)
        }
        SettingKind::Text | SettingKind::Secret | SettingKind::Number { .. } => {
            super::ai::field_row(theme, state, SettingsField::Plugin(key), cx)
        }
    }
}

/// `control` with a press that saves `stored` as the plugin setting
/// `setting`, reporting a value the field rejects.
fn saving(
    control: Stateful<gpui::Div>,
    setting: &'static str,
    stored: &'static str,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Stateful<gpui::Div> {
    control.on_mouse_down(
        MouseButton::Left,
        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
            cx.stop_propagation();
            this.state.update(cx, |state, _| {
                if let Err(err) = state.set_plugin_setting(setting, &stored) {
                    state.report_error(err);
                }
            });
            cx.notify();
        }),
    )
}
