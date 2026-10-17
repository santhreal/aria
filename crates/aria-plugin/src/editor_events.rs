//! Editor notifications: the session, the view, media, renders, exports, jobs,
//! and plugins.
//!
//! Every type here is posted with [`EventBus::post`], except
//! [`ContextMenuRequested`], a waterfall the editor runs when a menu opens.
//! The project host posts the session and media events from the outcome of each
//! command; the editor posts the view events once per drawn frame through
//! [`EditorView::publish`].

use std::path::PathBuf;
use std::sync::Arc;

use aria_core::RationalTime;
use aria_models::{ClipId, MediaId, Project, TrackId};
use parking_lot::RwLock;

use crate::events::{Event, EventBus, WaterfallEvent};

/// A project became the session: opened, loaded, filled from a template, or
/// started new. Follows the [`ProjectClosed`] for the session it replaced.
pub struct ProjectOpened {
    /// The file the session mirrors; `None` for a new or template-filled
    /// project.
    pub path: Option<PathBuf>,
    pub project: Arc<Project>,
}

/// The session is about to be replaced by the [`ProjectOpened`] that follows.
pub struct ProjectClosed {
    /// The file the replaced session mirrored; `None` if it was never saved.
    pub path: Option<PathBuf>,
}

/// The project was written to `path`.
pub struct ProjectSaved {
    pub path: PathBuf,
}

/// The clip selection changed. Holds the whole selection, in selection order.
pub struct SelectionChanged {
    pub clips: Vec<ClipId>,
}

/// The playhead moved. Posted at most once per drawn frame.
pub struct PlayheadMoved {
    pub at: RationalTime,
}

/// The sequence In or Out point was set or cleared.
pub struct InOutChanged {
    pub mark_in: Option<RationalTime>,
    pub mark_out: Option<RationalTime>,
}

/// Playback started, or its speed changed while playing.
pub struct PlaybackStarted {
    pub at: RationalTime,
    /// A multiple of real time; negative plays backwards.
    pub speed: i32,
}

/// Playback stopped.
pub struct PlaybackStopped {
    pub at: RationalTime,
}

/// A file was added to the media pool. Posted once the undo step that added it
/// can no longer roll back.
pub struct MediaImported {
    pub media: MediaId,
    pub path: PathBuf,
}

/// A media pool item now reads `path`.
pub struct MediaRelinked {
    pub media: MediaId,
    pub path: PathBuf,
}

/// A media file the project uses is not on disk. Posted when the editor first
/// shows it as missing.
pub struct MediaOffline {
    pub media: MediaId,
    pub path: PathBuf,
}

/// How a render or an export ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderResult {
    Written,
    Failed(String),
    Cancelled,
}

/// What a render wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderKind {
    /// A clip rendered to a file and swapped in for the clip.
    Clip(ClipId),
    /// The sequence, one numbered image per frame.
    ImageSequence,
}

/// A render job ended.
pub struct RenderFinished {
    pub kind: RenderKind,
    /// The file or folder the render wrote to.
    pub path: PathBuf,
    pub result: RenderResult,
}

/// An export started writing `path`.
pub struct ExportStarted {
    pub path: PathBuf,
}

/// An export that started has ended.
pub struct ExportFinished {
    pub path: PathBuf,
    pub result: RenderResult,
}

/// A background job was queued, started, or reported progress.
pub struct JobProgress {
    pub job: u64,
    pub label: String,
    /// `0.0..=1.0`, or `None` while the job has not measured its progress.
    pub progress: Option<f32>,
    pub detail: String,
}

/// How a background job ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobOutcome {
    Done,
    Failed,
    Cancelled,
}

/// A background job ended.
pub struct JobFinished {
    pub job: u64,
    pub label: String,
    pub outcome: JobOutcome,
    /// The result summary, or the reason it failed.
    pub detail: String,
}

/// A plugin finished mounting.
pub struct PluginActivated {
    pub id: std::borrow::Cow<'static, str>,
}

/// A plugin was unmounted; its handlers and services are gone.
pub struct PluginDeactivated {
    pub id: std::borrow::Cow<'static, str>,
}

impl Event for ProjectOpened {}
impl Event for ProjectClosed {}
impl Event for ProjectSaved {}
impl Event for SelectionChanged {}
impl Event for PlayheadMoved {}
impl Event for InOutChanged {}
impl Event for PlaybackStarted {}
impl Event for PlaybackStopped {}
impl Event for MediaImported {}
impl Event for MediaRelinked {}
impl Event for MediaOffline {}
impl Event for RenderFinished {}
impl Event for ExportStarted {}
impl Event for ExportFinished {}
impl Event for JobProgress {}
impl Event for JobFinished {}
impl Event for PluginActivated {}
impl Event for PluginDeactivated {}

/// What the editor shows besides the project.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewState {
    /// The selected clips, in selection order.
    pub selection: Vec<ClipId>,
    pub playhead: RationalTime,
    pub mark_in: Option<RationalTime>,
    pub mark_out: Option<RationalTime>,
    pub playing: bool,
    /// Playback speed as a multiple of real time; negative plays backwards.
    pub speed: i32,
}

/// One frame's [`ViewState`], borrowed, so a frame that changed nothing copies
/// nothing.
#[derive(Debug, Clone, Copy)]
pub struct ViewFrame<'a> {
    pub selection: &'a [ClipId],
    pub playhead: RationalTime,
    pub mark_in: Option<RationalTime>,
    pub mark_out: Option<RationalTime>,
    pub playing: bool,
    pub speed: i32,
}

impl ViewFrame<'_> {
    fn matches(&self, state: &ViewState) -> bool {
        self.selection == state.selection.as_slice()
            && self.playhead == state.playhead
            && self.mark_in == state.mark_in
            && self.mark_out == state.mark_out
            && self.playing == state.playing
            && self.speed == state.speed
    }

    fn to_state(self) -> ViewState {
        ViewState {
            selection: self.selection.to_vec(),
            playhead: self.playhead,
            mark_in: self.mark_in,
            mark_out: self.mark_out,
            playing: self.playing,
            speed: self.speed,
        }
    }
}

/// The editor's [`ViewState`] as of the last drawn frame. The editor registers
/// it; a plugin declares `uses: &["EditorView"]` to read it.
#[derive(Default)]
pub struct EditorView {
    state: RwLock<Option<ViewState>>,
}

impl EditorView {
    pub fn new() -> Self {
        Self::default()
    }

    /// The view as of the last drawn frame; `None` before the first.
    pub fn current(&self) -> Option<ViewState> {
        self.state.read().clone()
    }

    /// Record `frame`, and post [`SelectionChanged`], [`PlayheadMoved`],
    /// [`InOutChanged`], [`PlaybackStarted`], or [`PlaybackStopped`] for each
    /// part that differs from the last recorded frame. The first frame is
    /// recorded without posting.
    pub fn publish(&self, frame: ViewFrame<'_>, bus: &EventBus) {
        if self
            .state
            .read()
            .as_ref()
            .is_some_and(|state| frame.matches(state))
        {
            return;
        }
        let mut state = self.state.write();
        if let Some(previous) = state.as_ref() {
            if previous.selection.as_slice() != frame.selection {
                bus.post(SelectionChanged {
                    clips: frame.selection.to_vec(),
                });
            }
            if previous.playhead != frame.playhead {
                bus.post(PlayheadMoved { at: frame.playhead });
            }
            if (previous.mark_in, previous.mark_out) != (frame.mark_in, frame.mark_out) {
                bus.post(InOutChanged {
                    mark_in: frame.mark_in,
                    mark_out: frame.mark_out,
                });
            }
            match (previous.playing, frame.playing) {
                (false, true) => bus.post(PlaybackStarted {
                    at: frame.playhead,
                    speed: frame.speed,
                }),
                (true, true) if previous.speed != frame.speed => bus.post(PlaybackStarted {
                    at: frame.playhead,
                    speed: frame.speed,
                }),
                (true, false) => bus.post(PlaybackStopped { at: frame.playhead }),
                (true, true) | (false, false) => {}
            }
        }
        *state = Some(frame.to_state());
    }
}

/// What a context menu was opened on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuTarget {
    Clip {
        clip: ClipId,
        track: TrackId,
    },
    Track {
        track: TrackId,
    },
    /// The junction at the right edge of `clip`.
    Transition {
        clip: ClipId,
    },
    Viewer,
    Media {
        media: MediaId,
    },
    /// A recent project on Home.
    Recent {
        path: PathBuf,
    },
    /// The effect at `index` in `clip`'s chain, from its header in the
    /// Inspector's Effects tab.
    Effect {
        clip: ClipId,
        index: usize,
    },
    /// The fx badge on `clip`'s timeline bar, which lists the properties the
    /// bar can draw as its line.
    ClipBadge {
        clip: ClipId,
        track: TrackId,
    },
}

/// A row a plugin adds to a context menu.
#[derive(Clone)]
pub struct MenuEntry {
    pub label: String,
    /// Runs on a background thread when the row is clicked.
    pub run: Arc<dyn Fn() + Send + Sync>,
}

/// A context menu is opening. Each handler may add rows; they are drawn after
/// the editor's own, in mount order.
///
/// Handlers run on the UI thread while the menu opens, so a handler reads
/// `project` and returns; the work belongs in the row's `run`.
pub struct ContextMenuRequested {
    pub target: MenuTarget,
    /// The project the menu was opened over.
    pub project: Arc<Project>,
    pub entries: Vec<MenuEntry>,
}

impl ContextMenuRequested {
    pub fn new(target: MenuTarget, project: Arc<Project>) -> Self {
        Self {
            target,
            project,
            entries: Vec::new(),
        }
    }

    /// Add a row labelled `label` that runs `run` when clicked.
    pub fn add(&mut self, label: impl Into<String>, run: impl Fn() + Send + Sync + 'static) {
        self.entries.push(MenuEntry {
            label: label.into(),
            run: Arc::new(run),
        });
    }
}

impl WaterfallEvent for ContextMenuRequested {
    fn should_stop(&self) -> bool {
        false
    }
}

#[cfg(test)]
#[path = "editor_events_tests.rs"]
mod tests;
