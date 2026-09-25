//! A complete external plugin, built as a `cdylib` and loaded from disk.
//!
//! Build it and drop the library in `<data dir>/plugins`:
//!
//! ```text
//! cargo build -p aria-notes-plugin --release
//! cp target/release/libaria_notes_plugin.so ~/.aria/plugins/
//! ```
//!
//! It does the two things an external plugin can do without the UI crate: it
//! claims a file type, and it sits in front of every command. A plugin that also
//! draws a panel depends on `aria-ui` and adds a `PanelEntry`; see
//! `docs/plugin-development.md`.

use std::sync::atomic::{AtomicUsize, Ordering};

use aria_plugin::{
    Command, CommandProposed, DropAction, Plugin, PluginContext, PluginError, PluginManifest,
    ResolveDroppedPath, SettingSpec, SettingsSection,
};

/// Counts the commands this plugin saw. A plugin's own state lives in the plugin,
/// not in the host.
static SEEN: AtomicUsize = AtomicUsize::new(0);

/// What this plugin lets a user configure. Declared, not stored: the host owns
/// the file, and each key is read back through `SettingsService`.
const SETTINGS: &[SettingSpec] = &[
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

pub struct NotesPlugin;

impl Plugin for NotesPlugin {
    fn manifest(&self) -> &PluginManifest {
        &PluginManifest {
            id: "example.notes",
            name: "Notes",
            version: "0.1.0",
            requires: &[],
            // Declares its own settings, so it reads the schema registry.
            uses: &["SettingsSchema"],
        }
    }

    fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        // Claim a file type. The first plugin to claim one wins, in mount order,
        // and an external plugin is mounted after the built-ins.
        ctx.on_serial::<ResolveDroppedPath, DropAction, _>(|event| {
            event
                .path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("srt"))
                .then_some(DropAction::ImportMedia)
        });

        // Sit in front of every command. Refusing here stops the edit before the
        // engine sees it, and the reason is shown to the user as written.
        ctx.on_waterfall::<CommandProposed, _>(|event| {
            SEEN.fetch_add(1, Ordering::Relaxed);
            let Command::Edit(edit) = &event.command else {
                return;
            };
            if edit.targets().tracks.len() > 8 {
                event.refuse("Notes will not touch more than eight tracks at once.");
            }
        });

        // Declare the settings this plugin reads. The host draws them in its own
        // Settings surface and stores them in the same config file; a value is read
        // back through `SettingsService` under the declared key.
        if let Some(schema) = aria_plugin::settings_schema::schema_of(ctx) {
            schema.declare(SettingsSection {
                plugin: "example.notes",
                title: "Notes",
                fields: SETTINGS,
            });
        }

        Ok(())
    }
}

aria_plugin::export_plugin!(NotesPlugin);
