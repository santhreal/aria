//! A plugin's own settings section, drawn from what it declared.
//!
//! Text, secret and number fields are typed rows, the same row a shipped field
//! draws with. A toggle is an On and Off pair and a choice is one chip per
//! option; a click saves the value at once through
//! [`AppState::set_plugin_setting`], which checks it against the field.

use aria_plugin::{SettingKind, SettingSpec, SettingsSection};
use gpui::{
    Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    SharedString, Styled, div, px,
};

use crate::app::AppState;
use crate::app::settings_form::SettingsField;
use crate::components;
use crate::interned::intern;
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
    match &spec.kind {
        SettingKind::Toggle => {
            let options = [("On", "true"), ("Off", "false")];
            options_row(theme, state, key, &spec.label, &options, cx)
        }
        SettingKind::Choice(choices) => {
            let options: Vec<(&str, &str)> = choices
                .iter()
                .map(|choice| (choice.as_str(), choice.as_str()))
                .collect();
            options_row(theme, state, key, &spec.label, &options, cx)
        }
        SettingKind::Text | SettingKind::Secret | SettingKind::Number { .. } => {
            super::ai::field_row(theme, state, SettingsField::Plugin(key), cx)
        }
    }
}

/// The label of the field that edits `setting`, then one chip per
/// `(shown, stored)` option. The chip whose stored text is the current value is
/// filled.
fn options_row(
    theme: &Theme,
    state: &Entity<AppState>,
    setting: &'static str,
    label: &str,
    options: &[(&str, &str)],
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let current = state.read(cx).plugin_setting(setting);
    let mut row = div()
        .id(crate::motion::MotionKey::new("plugin-setting", setting).element_id("plugin-setting"))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(Theme::SPACE_SM))
        .child(
            div()
                .w(px(140.0))
                .text_size(Theme::TEXT_UI)
                .text_color(theme.text_muted)
                .child(SharedString::from(label.to_owned())),
        );
    for (shown, stored) in options {
        let id = crate::motion::MotionKey::new(setting, stored);
        let shown = SharedString::from((*shown).to_owned());
        let chip = if current == *stored {
            components::button(id, shown, theme)
        } else {
            components::ghost_button(id, shown, theme)
        };
        let stored = (*stored).to_owned();
        row = row.child(chip.on_mouse_down(
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
        ));
    }
    row.into_any_element()
}
