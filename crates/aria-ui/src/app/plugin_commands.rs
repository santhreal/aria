//! Running a plugin's command from a menu row, a key, or the palette.
//!
//! A command runs here, on the window's thread, through the registering
//! plugin's view of the context. An edit it makes through the project host is
//! posted as `CommandApplied` and re-projected by
//! [`AppState::watch_edits`](super::AppState::watch_edits), like any edit made
//! away from the window.

use super::AppState;

impl AppState {
    /// Run the plugin command registered as `id`. A command that fails or
    /// panics is reported as the error line, prefixed with its title.
    pub(crate) fn run_plugin_command(&mut self, id: &str) {
        // Read before the run: a panic fails the plugin, which can take the
        // command with it.
        let spec = self.ctx.commands().into_iter().find(|spec| spec.id == id);
        let Err(err) = self.ctx.run_command(id) else {
            return;
        };
        match spec {
            Some(spec) => self.report_error(format!("{}: {err}", spec.title)),
            None => self.report_error(err.to_string()),
        }
    }
}

#[cfg(test)]
#[path = "plugin_commands_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "plugin_enablement_tests.rs"]
mod enablement_tests;
