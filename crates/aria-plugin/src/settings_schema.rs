//! What a plugin's settings look like.
//!
//! [`SettingsService`](crate::SettingsService) let a plugin read and write
//! `~/.aria/config.toml`, but a value nobody can see is a value nobody sets: a
//! plugin's configuration meant hand-editing the file. A plugin declares its
//! fields here instead, and the host renders them with the same rows it renders
//! its own with.
//!
//! Declaration only. The registry holds no values: a field names a settings key,
//! and reading and writing it stays with `SettingsService`, so there is one owner
//! of what is on disk.

use parking_lot::RwLock;

use crate::Service;
use crate::context::PluginContext;

/// One editable value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettingSpec {
    /// The `SettingsService` key this row reads and writes, dotted:
    /// `notes.directory`.
    pub key: &'static str,
    /// What the row is called.
    pub label: &'static str,
    /// What an empty value means, so a blank row is not a mystery.
    pub placeholder: &'static str,
    /// A secret is shown as its length, never as its text.
    pub secret: bool,
}

/// One plugin's section in the settings surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettingsSection {
    /// The plugin that declared it, for the row that says where a setting came
    /// from.
    pub plugin: &'static str,
    pub title: &'static str,
    pub fields: &'static [SettingSpec],
}

/// Every declared section, in registration order.
#[derive(Default)]
pub struct SettingsSchema {
    sections: RwLock<Vec<SettingsSection>>,
}

impl Service for SettingsSchema {}

impl SettingsSchema {
    /// Declare a section. A second declaration for the same plugin replaces the
    /// first, so a remount does not double the rows.
    pub fn declare(&self, section: SettingsSection) {
        let mut sections = self.sections.write();
        if let Some(held) = sections
            .iter_mut()
            .find(|held| held.plugin == section.plugin)
        {
            *held = section;
            return;
        }
        sections.push(section);
    }

    /// Every section, in registration order.
    pub fn sections(&self) -> Vec<SettingsSection> {
        self.sections.read().clone()
    }

    /// The section a plugin declared, if it declared one.
    pub fn section_of(&self, plugin: &str) -> Option<SettingsSection> {
        self.sections
            .read()
            .iter()
            .find(|section| section.plugin == plugin)
            .copied()
    }

    /// The field that owns one settings key, with the plugin that declared it.
    ///
    /// The host edits by key: a row hands back the key it was drawn from, and the
    /// declaring plugin is what the status line names.
    pub fn field(&self, key: &str) -> Option<(&'static str, SettingSpec)> {
        for section in self.sections.read().iter() {
            if let Some(spec) = section.fields.iter().find(|spec| spec.key == key) {
                return Some((section.plugin, *spec));
            }
        }
        None
    }
}

/// The schema registry, or nothing when no host mounted one.
pub fn schema_of(ctx: &PluginContext) -> Option<std::sync::Arc<SettingsSchema>> {
    ctx.try_get::<SettingsSchema>()
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOTES: &[SettingSpec] = &[
        SettingSpec {
            key: "notes.directory",
            label: "Notes directory",
            placeholder: "the project directory",
            secret: false,
        },
        SettingSpec {
            key: "notes.token",
            label: "Sync token",
            placeholder: "no token stored",
            secret: true,
        },
    ];

    fn section() -> SettingsSection {
        SettingsSection {
            plugin: "example.notes",
            title: "Notes",
            fields: NOTES,
        }
    }

    #[test]
    fn a_declared_section_is_listed_with_its_fields() {
        let schema = SettingsSchema::default();
        schema.declare(section());
        let sections = schema.sections();
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].fields.len(), 2);
        assert_eq!(
            schema.section_of("example.notes").map(|s| s.title),
            Some("Notes")
        );
        assert!(schema.section_of("example.other").is_none());
    }

    /// A remount re-declares, and doubling the rows would show every field twice.
    #[test]
    fn declaring_twice_replaces_the_section() {
        let schema = SettingsSchema::default();
        schema.declare(section());
        schema.declare(SettingsSection {
            title: "Notes v2",
            ..section()
        });
        assert_eq!(schema.sections().len(), 1);
        assert_eq!(schema.sections()[0].title, "Notes v2");
    }

    /// The host edits by key, so a key has to resolve to exactly one field and to
    /// the plugin that owns it.
    #[test]
    fn a_key_resolves_to_the_field_and_the_plugin_that_declared_it() {
        let schema = SettingsSchema::default();
        schema.declare(section());
        let (plugin, spec) = schema.field("notes.token").expect("declared");
        assert_eq!(plugin, "example.notes");
        assert!(spec.secret, "a token must not be drawn as text");
        assert!(schema.field("notes.missing").is_none());
    }
}
