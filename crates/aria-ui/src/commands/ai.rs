//! Palette rows for the assistant, the AI provider, and the configuration file.
//!
//! One of the groups [`super::BUILTIN_COMMANDS`] joins. Split by
//! concern like its siblings: a row here acts on the assistant, on the provider
//! the assistant talks to, or on the file that configures both, and none of them
//! needs a clip selected.
//!
//! Two of these rows open Settings with its filter already set rather than
//! performing the work. That is deliberate and not a placeholder: choosing a
//! model and testing a key are *controls* — a list to pick from and a button with
//! a result line — and a palette row that fired them blind would give the
//! operator no way to read the answer. The row is the name; the pane is the
//! control. `AI_JOB_ROWS` holds the two that can be fired directly, because
//! they need the entity context to start a request.

use crate::types::ActivePanel;

use super::host::{CommandEntry, WindowedEntry};

/// Rows that act on the assistant, the provider, or the configuration file.
pub(super) const AI_ROWS: &[CommandEntry] = &[
    CommandEntry {
        id: "pal-ai-tools",
        label: "Assistant: What the Assistant Can Do",
        run: |state| {
            // The capability panel is docked beside the assistant and shows
            // itself when the assistant is open, so opening the assistant is
            // what makes the list reachable.
            state.active_panel = ActivePanel::Ai;
            state.status_message = Some("The assistant's tools are listed beside it".into());
        },
    },
    CommandEntry {
        id: "pal-ai-system-prompt",
        label: "Assistant: Show the System Prompt",
        run: |state| state.show_ai_system_prompt(),
    },
    CommandEntry {
        id: "pal-ai-slash",
        label: "Assistant: List the Slash Commands",
        run: |state| {
            let names = state.assistant_slash_commands();
            state.status_message = Some(if names.is_empty() {
                "No slash commands: put a template in the agent directory's commands/".into()
            } else {
                format!("/{}", names.join("  /"))
            });
        },
    },
    CommandEntry {
        id: "pal-ai-models",
        label: "AI: Choose the Model",
        run: |state| state.open_settings_at("AI"),
    },
    CommandEntry {
        id: "pal-ai-connection",
        label: "AI: Test the Connection",
        run: |state| state.open_settings_at("AI"),
    },
    CommandEntry {
        id: "pal-ai-schema",
        label: "Configuration: What This Build Reads",
        run: |state| {
            let status = state.settings_schema_status();
            state.open_settings_at("Configuration");
            state.status_message = Some(status);
        },
    },
    CommandEntry {
        id: "pal-cloud-checksum",
        label: "Cloud: Checksum a Downloaded File",
        run: |state| state.checksum_picked_file(),
    },
];

/// The two rows that start a request, so they need the entity context.
///
/// Kept apart for the same reason `BUILTIN_JOB_COMMANDS` is: a command that
/// cannot run against a headless state must not sit in a table the tests drive
/// without a window.
pub(super) const AI_JOB_ROWS: &[WindowedEntry] = &[
    WindowedEntry {
        id: "pal-ai-list-models",
        label: "AI: Ask the Endpoint Which Models It Serves",
        run: |state, cx| state.probe_ai_models(cx),
    },
    WindowedEntry {
        id: "pal-ai-test-connection",
        label: "AI: Test the Provider Connection Now",
        run: |state, cx| state.test_ai_connection(cx),
    },
];
