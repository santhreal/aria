//! What a plugin's settings look like.
//!
//! [`SettingsService`](crate::SettingsService) lets a plugin read and write
//! `~/.aria/config.toml`. A plugin declares its fields here, and the host draws
//! them in Settings with the rows it draws its own with: a text box, a secret
//! shown as its length, a toggle, a bounded number, or a choice.
//!
//! Declaration only. The registry holds no values: a field states a settings
//! key, and reading and writing it stays with `SettingsService`, the single
//! owner of what is on disk. Values are stored as text: `true` or `false` for a
//! toggle, a decimal number, or the chosen option.

use std::sync::{Arc, Weak};

use parking_lot::RwLock;

use crate::Service;
use crate::context::PluginContext;
use crate::error::PluginError;
use crate::registration::{Contribution, ContributionKind, Registration};

/// What kind of value a field holds, and so which row draws it.
#[derive(Debug, Clone, PartialEq)]
pub enum SettingKind {
    /// Free text.
    Text,
    /// Text drawn as its length, never as its text.
    Secret,
    /// On or off, stored as `true` or `false`.
    Toggle,
    /// A number in `min..=max`, moved by `step`.
    Number { min: f64, max: f64, step: f64 },
    /// One of a fixed list of options.
    Choice(Vec<String>),
}

impl SettingKind {
    /// The kind's name, as `plugin.toml` spells it.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Secret => "secret",
            Self::Toggle => "toggle",
            Self::Number { .. } => "number",
            Self::Choice(_) => "choice",
        }
    }

    /// The text to store for `value`, or why the value does not fit the kind.
    ///
    /// Surrounding space is dropped. An empty value is always accepted: it
    /// clears the key, and the field reads as its default.
    pub fn check(&self, value: &str) -> Result<String, String> {
        let value = value.trim();
        if value.is_empty() {
            return Ok(String::new());
        }
        match self {
            Self::Text | Self::Secret => Ok(value.to_owned()),
            Self::Toggle => match value.to_ascii_lowercase().as_str() {
                "true" | "on" | "yes" => Ok("true".to_owned()),
                "false" | "off" | "no" => Ok("false".to_owned()),
                _ => Err(format!("{value} is not on or off")),
            },
            Self::Number { min, max, .. } => {
                let number: f64 = value
                    .parse()
                    .ok()
                    .filter(|number: &f64| number.is_finite())
                    .ok_or_else(|| format!("{value} is not a number"))?;
                if number < *min {
                    return Err(format!("{value} is below {min}"));
                }
                if number > *max {
                    return Err(format!("{value} is above {max}"));
                }
                Ok(number.to_string())
            }
            Self::Choice(options) => options
                .iter()
                .find(|option| option.eq_ignore_ascii_case(value))
                .cloned()
                .ok_or_else(|| format!("{value} is not one of {}", options.join(", "))),
        }
    }
}

/// One editable value.
#[derive(Debug, Clone, PartialEq)]
pub struct SettingSpec {
    /// The `SettingsService` key this row reads and writes, dotted:
    /// `com.example.notes.directory`.
    pub key: String,
    /// What the row is called.
    pub label: String,
    /// What an empty value means.
    pub placeholder: String,
    pub kind: SettingKind,
    /// The value an unset key reads as, in stored form.
    pub default: Option<String>,
}

impl SettingSpec {
    /// Whether the row shows the value's length rather than its text.
    pub fn secret(&self) -> bool {
        self.kind == SettingKind::Secret
    }

    /// Why the field cannot be drawn, if it cannot: an empty key or label, a
    /// number range that holds nothing or a step that never moves, a choice with
    /// no options or an option twice, or a default its own kind refuses.
    pub fn problem(&self) -> Option<String> {
        if self.key.trim().is_empty() {
            return Some("a setting needs a key".to_owned());
        }
        if self.label.trim().is_empty() {
            return Some(format!("{}: the setting needs a label", self.key));
        }
        match &self.kind {
            SettingKind::Number { min, max, step } => {
                if !(min.is_finite() && max.is_finite() && min <= max) {
                    return Some(format!(
                        "{}: the range {min} to {max} holds nothing",
                        self.key
                    ));
                }
                if !(step.is_finite() && *step > 0.0) {
                    return Some(format!("{}: the step {step} is not above zero", self.key));
                }
            }
            SettingKind::Choice(options) => {
                if options.is_empty() {
                    return Some(format!("{}: the choice has no options", self.key));
                }
                for (ix, option) in options.iter().enumerate() {
                    if option.trim().is_empty() {
                        return Some(format!("{}: an option is empty", self.key));
                    }
                    if options[..ix]
                        .iter()
                        .any(|held| held.eq_ignore_ascii_case(option))
                    {
                        return Some(format!("{}: the option {option} is listed twice", self.key));
                    }
                }
            }
            SettingKind::Text | SettingKind::Secret | SettingKind::Toggle => {}
        }
        let default = self.default.as_deref()?;
        match self.kind.check(default) {
            Ok(_) => None,
            Err(reason) => Some(format!("{}: the default is refused: {reason}", self.key)),
        }
    }
}

/// One plugin's section in Settings.
#[derive(Debug, Clone, PartialEq)]
pub struct SettingsSection {
    /// The plugin that declared it, for the row that states where a setting
    /// came from.
    pub plugin: String,
    pub title: String,
    pub fields: Vec<SettingSpec>,
}

/// Every declared section, in registration order.
#[derive(Debug, Default)]
pub struct SettingsSchema {
    sections: RwLock<Vec<Arc<SettingsSection>>>,
}

impl Service for SettingsSchema {}

impl SettingsSchema {
    /// Every section, in registration order.
    pub fn sections(&self) -> Vec<Arc<SettingsSection>> {
        self.sections.read().clone()
    }

    /// The section a plugin declared, if it declared one.
    pub fn section_of(&self, plugin: &str) -> Option<Arc<SettingsSection>> {
        self.sections
            .read()
            .iter()
            .find(|section| section.plugin == plugin)
            .cloned()
    }

    /// The field that owns one settings key, with the plugin that declared it.
    pub fn field(&self, key: &str) -> Option<(String, SettingSpec)> {
        self.sections.read().iter().find_map(|section| {
            section
                .fields
                .iter()
                .find(|spec| spec.key == key)
                .map(|spec| (section.plugin.clone(), spec.clone()))
        })
    }

    /// Add `section`, refusing one that cannot be drawn or that claims a key
    /// or plugin another section holds.
    fn insert(&self, section: SettingsSection) -> Result<Arc<SettingsSection>, String> {
        if section.title.trim().is_empty() {
            return Err(format!(
                "{}: the settings section needs a title",
                section.plugin
            ));
        }
        for (ix, field) in section.fields.iter().enumerate() {
            if let Some(problem) = field.problem() {
                return Err(problem);
            }
            if section.fields[..ix]
                .iter()
                .any(|held| held.key == field.key)
            {
                return Err(format!("{}: the setting is declared twice", field.key));
            }
        }
        let mut sections = self.sections.write();
        if sections.iter().any(|held| held.plugin == section.plugin) {
            return Err(format!("{} already has a settings section", section.plugin));
        }
        for field in &section.fields {
            if let Some(held) = sections
                .iter()
                .find(|held| held.fields.iter().any(|spec| spec.key == field.key))
            {
                return Err(format!("{} is declared by {}", field.key, held.plugin));
            }
        }
        let section = Arc::new(section);
        sections.push(Arc::clone(&section));
        Ok(section)
    }
}

/// Removes one section when its plugin's registration drops.
struct Undeclare {
    schema: Weak<SettingsSchema>,
    section: Arc<SettingsSection>,
}

impl Drop for Undeclare {
    fn drop(&mut self) {
        if let Some(schema) = self.schema.upgrade() {
            schema
                .sections
                .write()
                .retain(|held| !Arc::ptr_eq(held, &self.section));
        }
    }
}

impl PluginContext {
    /// Add a section of fields to Settings.
    ///
    /// Held like [`register_command`](Self::register_command). On a plugin's
    /// view the section is the plugin's own, whatever `section.plugin` holds,
    /// and every key must sit under the plugin id: `com.example.notes.directory`
    /// for `com.example.notes`. Refused when a field cannot be drawn, a key is
    /// declared twice or by another section, or the plugin already has a
    /// section. In a context whose host draws no Settings, nothing is held.
    pub fn register_settings(
        &self,
        mut section: SettingsSection,
    ) -> Result<Registration, PluginError> {
        let refused = PluginError::SettingsRefused;
        if let Some(manifest) = self.scope() {
            section.plugin = manifest.id.to_string();
            let prefix = format!("{}.", section.plugin);
            if let Some(field) = section
                .fields
                .iter()
                .find(|field| !field.key.starts_with(&prefix) || field.key.len() == prefix.len())
            {
                return Err(refused(format!(
                    "{}: a setting of {} sits under {prefix}",
                    field.key, section.plugin
                )));
            }
        }
        let Some(schema) = self.scoped(None).try_get::<SettingsSchema>() else {
            return Ok(Registration::none());
        };
        let contribution = Contribution::new(ContributionKind::Settings, section.title.clone());
        let section = schema.insert(section).map_err(refused)?;
        Ok(self.retain_if_mounting(Registration::holding(
            contribution,
            Undeclare {
                schema: Arc::downgrade(&schema),
                section,
            },
        )))
    }
}

/// The schema registry, or nothing when no host mounted one.
pub fn schema_of(ctx: &PluginContext) -> Option<Arc<SettingsSchema>> {
    ctx.try_get::<SettingsSchema>()
}

#[cfg(test)]
#[path = "settings_schema_tests.rs"]
mod tests;
