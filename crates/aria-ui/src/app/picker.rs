//! The one way the editor prompts for a path.
//!
//! Fifteen call sites used to open a native dialog with `rfd`'s blocking API on
//! the thread that draws. Two defects came with that: the window stopped
//! redrawing until the dialog was answered, and a command that picked a file
//! could not be driven in a test at all, because a headless run waits for an
//! answer that never comes.
//!
//! A pick is a request and a continuation instead. `AppState::request_pick`
//! records what to ask for and what to do with the answer, and one of two things
//! happens:
//!
//! 1. an answer has been supplied in advance — `AppState::supply_pick` — and
//!    the continuation runs at once, which is how a test drives a file-picking
//!    command;
//! 2. nothing has been supplied, so the request goes to the window's picker
//!    task, which opens the dialog as a future and applies the answer to the
//!    state on the foreground executor.
//!
//! The request path is a channel because `AppState` has no `gpui` context of its
//! own: a palette row, a key chord and a menu item all reach the state through
//! `&mut AppState` alone. [`watch_picks`] installs the one task that answers
//! them, at the moment the state gets its entity.
//!
//! Nothing here exposes a blocking call, and `rfd` is not a dependency of this
//! crate: the dialogs live in `aria-dialog`, which offers futures only, so a new
//! call site cannot reintroduce a dialog that freezes the window.

use std::collections::VecDeque;
use std::path::PathBuf;

use gpui::{App, Entity};

use super::AppState;

pub use aria_dialog::{DialogError, FileSpec};

/// What one pick prompts for.
#[derive(Debug)]
pub enum Ask {
    /// One file that already exists.
    OpenFile(FileSpec),
    /// Any number of files that already exist.
    OpenFiles(FileSpec),
    /// Where to write a file.
    SaveFile(FileSpec),
    /// A directory.
    Folder(FileSpec),
    /// A question with two answers.
    Confirm { title: String, question: String },
}

impl Ask {
    /// Open the dialog and wait for an answer. Awaited on a background task:
    /// the answer arrives whenever it arrives, and the window keeps drawing.
    pub(crate) async fn answer(self) -> Result<Answer, DialogError> {
        match self {
            Ask::OpenFile(spec) => Ok(Answer::Paths(
                aria_dialog::open_file(spec).await?.into_iter().collect(),
            )),
            Ask::OpenFiles(spec) => Ok(Answer::Paths(aria_dialog::open_files(spec).await?)),
            Ask::SaveFile(spec) => Ok(Answer::Paths(
                aria_dialog::save_file(spec).await?.into_iter().collect(),
            )),
            Ask::Folder(spec) => Ok(Answer::Paths(
                aria_dialog::open_folder(spec).await?.into_iter().collect(),
            )),
            Ask::Confirm { title, question } => Ok(Answer::Confirmed(
                aria_dialog::confirm(title, question).await?,
            )),
        }
    }

    /// The title the dialog opens with.
    pub(crate) fn title(&self) -> &str {
        match self {
            Ask::OpenFile(spec)
            | Ask::OpenFiles(spec)
            | Ask::SaveFile(spec)
            | Ask::Folder(spec) => spec.title(),
            Ask::Confirm { title, .. } => title,
        }
    }
}

/// The dialog's answer.
///
/// Cancelling is an answer: no paths, or no. A continuation that reads the
/// answer with [`Answer::one`] or [`Answer::all`] handles the cancel by handling
/// the empty case, so there is no separate path for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// The paths chosen, empty when the dialog was cancelled.
    Paths(Vec<PathBuf>),
    /// Whether a question was confirmed.
    Confirmed(bool),
}

impl Answer {
    /// One chosen path: a file, a save destination, or a folder.
    pub fn file(path: impl Into<PathBuf>) -> Self {
        Answer::Paths(vec![path.into()])
    }

    /// Several chosen paths.
    pub fn files(paths: impl IntoIterator<Item = PathBuf>) -> Self {
        Answer::Paths(paths.into_iter().collect())
    }

    /// The dialog was dismissed without choosing anything.
    pub fn cancelled() -> Self {
        Answer::Paths(Vec::new())
    }

    /// The first path chosen, or `None` when the pick was cancelled.
    pub fn one(self) -> Option<PathBuf> {
        match self {
            Answer::Paths(paths) => paths.into_iter().next(),
            Answer::Confirmed(_) => None,
        }
    }

    /// Every path chosen, in the order the dialog reported them.
    pub fn all(self) -> Vec<PathBuf> {
        match self {
            Answer::Paths(paths) => paths,
            Answer::Confirmed(_) => Vec::new(),
        }
    }

    /// Whether a question was answered yes.
    pub fn is_yes(&self) -> bool {
        matches!(self, Answer::Confirmed(true))
    }
}

/// One pick: what to ask, and what to do with the answer.
///
/// The continuation is `Send` because the request crosses to the picker task,
/// and takes no context because it must also run in a plain unit test.
struct PickRequest {
    waiting: String,
    ask: Ask,
    then: Continuation,
}

/// What runs with a pick's answer.
type Continuation = Box<dyn FnOnce(&mut AppState, Answer) + Send>;

impl super::selection::ReportableError for DialogError {
    fn into_error_message(self) -> String {
        self.to_string()
    }
}

/// The picks a session has asked for.
///
/// The queue is unbounded and the reader is kept here so [`watch_picks`] can
/// take a handle: a request made before the window installed its task waits in
/// the queue rather than being dropped.
pub struct Picks {
    to_window: crate::handoff::Wake<PickRequest>,
    from_state: crate::handoff::Wakes<PickRequest>,
    /// Answers supplied in advance, consumed in order. Only a test fills this.
    supplied: VecDeque<Answer>,
    /// Status line posted while a native dialog is open, kept until the dialog
    /// answers or errors so [`AppState::age_status`] does not expire it while
    /// the dialog is still open.
    waiting: Option<String>,
    /// How many picks have been asked for, so a test can tell "asked and not
    /// answered" from "never asked".
    asked: usize,
}

impl Picks {
    /// Whether `status` is the waiting line of an in-flight picker dialog.
    pub(crate) fn is_waiting_status(&self, status: &str) -> bool {
        self.waiting.as_deref() == Some(status)
    }
}

impl std::fmt::Debug for Picks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Picks")
            .field("to_window", &self.to_window)
            .field("asked", &self.asked)
            .finish_non_exhaustive()
    }
}

impl Default for Picks {
    fn default() -> Self {
        let (to_window, from_state) = crate::handoff::wakes();
        Self {
            to_window,
            from_state,
            supplied: VecDeque::new(),
            waiting: None,
            asked: 0,
        }
    }
}

impl AppState {
    /// Prompt for a path, then run `then` against the state with the answer.
    ///
    /// Returns immediately. `then` runs on the thread that draws, either now
    /// (an answer was supplied) or once the dialog is answered.
    ///
    /// A save answered with a name that has no extension is completed with the
    /// first extension the dialog offered before `then` reads it, so `cut` in
    /// Save As writes `cut.aria`: the Linux portal and zenity return a typed
    /// name as it was typed.
    pub(crate) fn request_pick(
        &mut self,
        ask: Ask,
        then: impl FnOnce(&mut AppState, Answer) + Send + 'static,
    ) {
        #[cfg(test)]
        hold_title_case(&ask);
        #[cfg(test)]
        hold_save_extension(&ask);
        let mut then: Continuation = Box::new(then);
        if let Ask::SaveFile(spec) = &ask
            && let Some(extension) = spec.first_extension()
        {
            let extension = extension.to_owned();
            let named = then;
            then = Box::new(move |state, answer| {
                state.complete_save_answer(&extension, answer, named);
            });
        }
        self.picks.asked += 1;
        if let Some(answer) = self.picks.supplied.pop_front() {
            then(self, answer);
            return;
        }
        let waiting = format!("Waiting for the {} picker", ask.title());
        self.status_message = Some(format!("Waiting for the {} picker", ask.title()));
        self.status_seen = None;
        self.picks.waiting = Some(waiting.clone());
        // A wake fails only when the reader is gone, which cannot happen while
        // the state holds it. Nothing is lost either way: an unanswered pick is
        // a dialog that never opened.
        self.picks
            .to_window
            .wake(PickRequest { waiting, ask, then });
    }

    /// Hand `then` the path a save answer names, with `extension` added when
    /// the chosen name has none.
    ///
    /// The dialog checked the name it was given for an existing file, not the
    /// completed one, so a file already at the completed path is replaced only
    /// once a Replace File question is answered yes; no is a cancelled save.
    fn complete_save_answer(&mut self, extension: &str, answer: Answer, then: Continuation) {
        let completed = match &answer {
            Answer::Paths(paths)
                if paths.len() == 1
                    && paths[0].extension().is_none_or(|typed| typed.is_empty()) =>
            {
                paths[0].with_extension(extension)
            }
            _ => return then(self, answer),
        };
        if std::fs::symlink_metadata(&completed).is_err() {
            return then(self, Answer::file(completed));
        }
        let question = format!("{} already exists. Replace it?", completed.display());
        let ask = Ask::Confirm {
            title: "Replace File".into(),
            question,
        };
        self.request_pick(ask, move |state, reply| {
            let answer = if reply.is_yes() {
                Answer::file(completed)
            } else {
                Answer::cancelled()
            };
            then(state, answer);
        });
    }

    /// Apply the outcome of a native dialog pick: clear its waiting status and
    /// either run the continuation with the answer or post the dialog error.
    pub(crate) fn finish_pick(
        &mut self,
        waiting: &str,
        outcome: Result<Answer, DialogError>,
        then: impl FnOnce(&mut AppState, Answer),
    ) {
        if self.picks.waiting.as_deref() == Some(waiting) {
            self.picks.waiting = None;
        }
        if self.status_message.as_deref() == Some(waiting) {
            self.status_message = None;
            self.status_seen = None;
        }
        match outcome {
            Ok(answer) => then(self, answer),
            Err(err) => self.report_error(err),
        }
    }

    /// Answer the next pick with `answer` instead of opening a dialog.
    ///
    /// The seam every file-picking command is driven through in a test. Answers
    /// queue, so a command that asks twice — a template with two slots — is
    /// answered twice.
    #[cfg(test)]
    pub(crate) fn supply_pick(&mut self, answer: Answer) {
        self.picks.supplied.push_back(answer);
    }

    /// How many picks this session has asked for.
    #[cfg(test)]
    pub(crate) fn picks_asked(&self) -> usize {
        self.picks.asked
    }

    /// How many supplied answers are still waiting to be used.
    #[cfg(test)]
    pub(crate) fn picks_unused(&self) -> usize {
        self.picks.supplied.len()
    }
}

/// Refuse a pick whose title is not in Title Case.
///
/// Every pick passes through [`AppState::request_pick`], so the title is checked
/// in whichever test first reaches the pick, with no list of call sites for a
/// new pick to be missing from. The menu row or palette entry that opens a
/// dialog is in Title Case, and the title of the dialog it opens is held to the
/// same rule.
///
/// A title states its subject after `: `, as `Choose Media: Logo` does. The
/// subject is the project's own data, a slot label or a plugin name, so only
/// the words before it are checked.
///
/// Not caught: a pick that no test reaches.
#[cfg(test)]
fn hold_title_case(ask: &Ask) {
    let title = ask.title();
    let authored = title.split_once(": ").map_or(title, |(head, _)| head);
    if let Err(reason) = crate::commands::casing_tests::check_title_case(authored) {
        panic!("the pick titled `{title}` is not in Title Case: {reason}");
    }
}

/// Refuse a save pick that offers no extension: a name typed without one would
/// be written bare, a file no dialog filtered by type lists and no viewer
/// opens. Checked at the seam, as [`hold_title_case`] is, so a new save pick
/// fails in whichever test first reaches it.
///
/// Not caught: a save pick that no test reaches.
#[cfg(test)]
fn hold_save_extension(ask: &Ask) {
    if let Ask::SaveFile(spec) = ask {
        assert!(
            spec.first_extension().is_some(),
            "the save pick titled `{}` offers no extension to complete a typed name with",
            spec.title()
        );
    }
}

#[cfg(test)]
#[path = "picker_save_tests.rs"]
mod save_tests;

/// Open the dialogs the session asks for, one at a time.
///
/// Installed once, when the state gets its entity, so a pick asked for by a
/// palette row, a key chord, a menu item or a panel button is opened the same
/// way. One at a time because two native dialogs at once is two questions with
/// one keyboard between them.
pub fn watch_picks(state: &Entity<AppState>, cx: &mut App) {
    let requests = crate::handoff::Wakes::clone(&state.read(cx).picks.from_state);
    let state = state.downgrade();
    cx.spawn(async move |cx| {
        loop {
            // Every writer is gone: the session ended.
            let Some(request) = requests.next().await else {
                break;
            };
            let outcome = cx.background_executor().spawn(request.ask.answer()).await;
            let applied = state.update(cx, |state, cx| {
                state.finish_pick(&request.waiting, outcome, request.then);
                cx.notify();
            });
            if applied.is_err() {
                break;
            }
            // A pick that relinked or imported media changed the picture under
            // the playhead, and the pick is what the window was waiting for.
            // The update above just succeeded and nothing is awaited in
            // between, so the app is still alive here.
            let Some(entity) = state.upgrade() else {
                break;
            };
            cx.update(|app| crate::app::spawn_preview(entity, app));
        }
    })
    .detach();
}
