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

use crate::app::DialogKind;
use crate::types::ActivePanel;

use super::host::{CommandEntry, WindowedEntry};

/// Rows that act on the assistant, the provider, or the configuration file.
pub(super) const AI_ROWS: &[CommandEntry] = &[
    CommandEntry {
        id: "pal-ai-tools",
        label: "Assistant: what the assistant can do",
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
        label: "Assistant: show the system prompt",
        run: |state| state.show_ai_system_prompt(),
    },
    CommandEntry {
        id: "pal-ai-slash",
        label: "Assistant: list the slash commands",
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
        label: "AI: choose the model",
        run: |state| open_settings(state, "AI"),
    },
    CommandEntry {
        id: "pal-ai-connection",
        label: "AI: test the connection",
        run: |state| open_settings(state, "AI"),
    },
    CommandEntry {
        id: "pal-ai-schema",
        label: "Configuration: what this build reads",
        run: |state| {
            let status = state.settings_schema_status();
            open_settings(state, "Configuration");
            state.status_message = Some(status);
        },
    },
    CommandEntry {
        id: "pal-cloud-checksum",
        label: "Cloud: checksum a downloaded file",
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
        label: "AI: ask the endpoint which models it serves",
        run: |state, cx| state.probe_ai_models(cx),
    },
    WindowedEntry {
        id: "pal-ai-test-connection",
        label: "AI: test the provider connection now",
        run: |state, cx| state.test_ai_connection(cx),
    },
];

/// Open Settings with the filter naming one section.
///
/// The filter rather than a scroll position: the dialog already narrows itself by
/// section title, so setting it is how a row points at a pane without the dialog
/// needing to know who asked.
fn open_settings(state: &mut crate::app::AppState, section: &str) {
    state.settings_form.filter = section.to_string();
    state.settings_form.focused = None;
    state.settings_form.draft.clear();
    state.open_dialog(DialogKind::Settings);
}
