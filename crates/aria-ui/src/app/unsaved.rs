//! The prompt that stands between unsaved work and the door.
//!
//! Home, New, Open, Close and Quit all leave the session. Each used to do it
//! immediately, so an hour of edits went with one mis-click and no question
//! asked. Every one of them now goes through [`AppState::request`], which either
//! runs the intent (nothing to lose) or parks it behind the prompt.
//!
//! The parked intent is the whole mechanism: the dialog does not know what it is
//! interrupting, and each of its three buttons resolves the same field, so a new
//! way of leaving the project is one variant here and no work in the panel.

use std::path::PathBuf;

use super::{AppState, DialogKind};

/// The requested action, held until the prompt is answered.
///
/// Every variant replaces or leaves the loaded project. Anything that does not
/// is not an intent and must not be parked: the prompt would ask about work the
/// action was never going to touch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingIntent {
    /// Leave the project for Home, keeping it loaded.
    GoHome,
    /// Start an empty project.
    NewProject,
    /// Open a project file that is already chosen.
    OpenProject(PathBuf),
    /// Ask for a project file, then open it. The pick happens after the answer,
    /// so cancelling the prompt costs no file dialog.
    PickProject,
    /// Load the bundled sample project.
    OpenSample,
    /// Take what was dropped on Home: a project file opens, anything else lands
    /// in a new project.
    DropOnHome(Vec<PathBuf>),
    /// Close the window and end the session.
    Quit,
}

impl PendingIntent {
    /// What answering the prompt does, as the prompt's second sentence.
    ///
    /// A row per variant rather than a sentence in the panel: a new way of
    /// leaving the project is one arm here, and the compiler asks for it before
    /// the prompt can be drawn without it.
    pub fn outcome(&self) -> &'static str {
        match self {
            Self::GoHome => "Either answer goes to Home.",
            Self::NewProject => "Either answer goes to a new project.",
            Self::OpenProject(_) | Self::PickProject => "Either answer goes to another project.",
            Self::OpenSample => "Either answer goes to the sample project.",
            Self::DropOnHome(_) => "Either answer goes to what you dropped.",
            Self::Quit => "Either answer quits Aria.",
        }
    }
}

impl AppState {
    /// Run `intent`, or park it behind the unsaved-changes prompt.
    ///
    /// Returns true when the intent ran. The caller does not branch on it: it is
    /// here so a test can tell "ran" from "asked" without reading two fields.
    pub fn request(&mut self, intent: PendingIntent) -> bool {
        if !self.store.is_dirty {
            self.run_intent(intent);
            return true;
        }
        self.pending_intent = Some(intent);
        // Set directly rather than through `open_dialog`: this prompt is the one
        // dialog that must open on Home too, because a dirty session stays
        // loaded on the way back to Home.
        self.dialog = DialogKind::UnsavedChanges;
        self.context_menu = None;
        self.close_menu();
        false
    }

    pub fn request_go_home(&mut self) {
        self.request(PendingIntent::GoHome);
    }

    pub fn request_new_project(&mut self) {
        self.request(PendingIntent::NewProject);
    }

    pub fn request_open_project(&mut self, path: PathBuf) {
        self.request(PendingIntent::OpenProject(path));
    }

    pub fn request_pick_project(&mut self) {
        self.request(PendingIntent::PickProject);
    }

    pub fn request_open_sample(&mut self) {
        self.request(PendingIntent::OpenSample);
    }

    /// Ctrl+Q, File > Quit, and the window's close control. The shell quits on
    /// the frame after [`AppState::quit_requested`] is set.
    pub fn request_quit(&mut self) {
        self.request(PendingIntent::Quit);
    }

    /// Whether leaving has been answered, or needed no answer.
    pub fn quit_requested(&self) -> bool {
        self.quit_requested
    }

    /// A drop on Home. A plugin package installs without replacing the
    /// session, so it is taken before the unsaved prompt is asked.
    pub fn request_drop_on_home(&mut self, paths: Vec<PathBuf>) {
        let paths = self.take_plugin_archives(paths);
        if !paths.is_empty() {
            self.request(PendingIntent::DropOnHome(paths));
        }
    }

    /// The intent waiting on an answer, if the prompt is up.
    pub fn pending_intent(&self) -> Option<&PendingIntent> {
        self.pending_intent.as_ref()
    }

    /// What the prompt says it is about to close: the project's own name.
    pub fn unsaved_subject(&self) -> String {
        let name = self.store.project_name.trim();
        if name.is_empty() {
            "This project".into()
        } else {
            name.to_string()
        }
    }

    /// Save, then continue. A failed save keeps the prompt up and the intent
    /// parked: the whole point is that the work survives the answer.
    pub fn save_and_continue(&mut self) {
        if let Err(err) = self.save_project() {
            self.report_error(&err);
            return;
        }
        self.resolve_pending();
    }

    /// Continue and lose the edits, as the Discard button requests.
    pub fn discard_and_continue(&mut self) {
        self.resolve_pending();
    }

    /// Answer "not yet": the intent is dropped and the project is untouched.
    pub fn cancel_pending(&mut self) {
        self.pending_intent = None;
        self.dialog = DialogKind::None;
    }

    fn resolve_pending(&mut self) {
        let Some(intent) = self.pending_intent.take() else {
            self.dialog = DialogKind::None;
            return;
        };
        self.dialog = DialogKind::None;
        self.run_intent(intent);
    }

    fn run_intent(&mut self, intent: PendingIntent) {
        match intent {
            PendingIntent::GoHome => self.go_home(),
            PendingIntent::NewProject => match self.new_project() {
                Ok(()) => self.status_message = Some("New project".into()),
                Err(err) => self.report_error(&err),
            },
            PendingIntent::OpenProject(path) => {
                if let Err(err) = self.open_project(path) {
                    self.report_error(&err);
                }
            }
            PendingIntent::PickProject => self.pick_and_open_project(),
            PendingIntent::OpenSample => {
                if let Err(err) = self.open_sample_project() {
                    self.report_error(&err);
                }
            }
            PendingIntent::DropOnHome(paths) => {
                if let Err(err) = self.drop_on_home(paths) {
                    self.report_error(&err);
                }
            }
            PendingIntent::Quit => self.quit_requested = true,
        }
    }
}
