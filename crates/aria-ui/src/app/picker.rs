//! The one way the editor asks the operator for a path.
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

pub use aria_dialog::FileSpec;

/// What one pick asks the operator for.
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
    /// Open the dialog and wait for the operator. Awaited on a background task:
    /// the answer arrives whenever it arrives, and the window keeps drawing.
    async fn answer(self) -> Answer {
        match self {
            Ask::OpenFile(spec) => {
                Answer::Paths(aria_dialog::open_file(spec).await.into_iter().collect())
            }
            Ask::OpenFiles(spec) => Answer::Paths(aria_dialog::open_files(spec).await),
            Ask::SaveFile(spec) => {
                Answer::Paths(aria_dialog::save_file(spec).await.into_iter().collect())
            }
            Ask::Folder(spec) => {
                Answer::Paths(aria_dialog::open_folder(spec).await.into_iter().collect())
            }
            Ask::Confirm { title, question } => {
                Answer::Confirmed(aria_dialog::confirm(title, question).await)
            }
        }
    }
}

/// What the operator answered.
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
    ask: Ask,
    then: Box<dyn FnOnce(&mut AppState, Answer) + Send>,
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
    /// How many picks have been asked for, so a test can tell "asked and not
    /// answered" from "never asked".
    asked: usize,
}

impl Default for Picks {
    fn default() -> Self {
        let (to_window, from_state) = crate::handoff::wakes();
        Self {
            to_window,
            from_state,
            supplied: VecDeque::new(),
            asked: 0,
        }
    }
}

impl AppState {
    /// Ask the operator for a path, then run `then` against the state with what
    /// they answered.
    ///
    /// Returns immediately. `then` runs on the thread that draws, either now
    /// (an answer was supplied) or once the dialog is answered.
    pub(crate) fn request_pick(
        &mut self,
        ask: Ask,
        then: impl FnOnce(&mut AppState, Answer) + Send + 'static,
    ) {
        self.picks.asked += 1;
        if let Some(answer) = self.picks.supplied.pop_front() {
            then(self, answer);
            return;
        }
        // A wake fails only when the reader is gone, which cannot happen while
        // the state holds it. Nothing is lost either way: an unanswered pick is
        // a dialog the operator never saw.
        self.picks.to_window.wake(PickRequest {
            ask,
            then: Box::new(then),
        });
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
            let answer = cx.background_executor().spawn(request.ask.answer()).await;
            let applied = state.update(cx, |state, cx| {
                (request.then)(state, answer);
                cx.notify();
            });
            if applied.is_err() {
                break;
            }
            // A pick that relinked or imported media changed the picture under
            // the playhead, and the pick is what the operator was waiting for.
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
