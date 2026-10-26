//! Timeline render-bar segments and the `aria-render-bar` thread that
//! computes them.
//!
//! Computing [`RenderSegment`]s runs on the `aria-render-bar` thread or at
//! the tail of a render-file job, never on the ruler's paint path and never
//! on the `aria-preview` thread, so a Viewer frame never waits behind it. The
//! thread starts at the session's first request and serves every later one,
//! waiting while none is pending, so an edit starts no thread; it ends when
//! the [`RenderBarState`] that started it drops. A newer request abandons the
//! pass in progress at its next frame.
//!
//! The thread reads the project one frame at a time under the engine lock and
//! holds no snapshot between two frames. A pass over a long sequence runs for
//! seconds and restarts after every edit; a snapshot held across it would make
//! every edit copy the whole project.

use std::path::PathBuf;
use std::sync::Arc;

use aria_core::{Rational, RationalTime};
use aria_engine::Engine;
use aria_models::Project;
use aria_plugin::ProjectHost;
use aria_render::{RenderFiles, Scene};
use parking_lot::{Condvar, Mutex, MutexGuard};

use super::render_files::{RenderQuality, RenderSegment};
use crate::handoff::Wake;
use crate::preview_worker::PreviewWorker;

#[cfg(test)]
#[path = "render_bar_tests.rs"]
mod tests;
#[cfg(test)]
#[path = "render_bar_thread_tests.rs"]
mod thread_tests;

/// Name of the thread that computes render-bar segments.
const THREAD_NAME: &str = "aria-render-bar";

/// Classify every frame of `project` against `files` and coalesce contiguous
/// runs of equal [`RenderQuality`]. Empty frames (no layers) produce no segment.
pub fn compute_render_segments(project: &Project, files: &RenderFiles) -> Vec<RenderSegment> {
    segments_unless(project, files, || false).unwrap_or_default()
}

/// [`compute_render_segments`], abandoned with `None` as soon as `superseded`
/// returns `true` between two frames.
pub(super) fn segments_unless(
    project: &Project,
    files: &RenderFiles,
    superseded: impl Fn() -> bool,
) -> Option<Vec<RenderSegment>> {
    let timeline = project.timeline();
    segments_reading(
        timeline.frame_rate,
        timeline.duration().value,
        files,
        superseded,
        |read| {
            read(project);
            true
        },
    )
}

/// [`segments_unless`] over the project `request` names, read one frame at a
/// time under the engine lock. `None` once `superseded` returns `true` or the
/// engine moves past the request's revision.
fn engine_segments_unless(
    request: &Request,
    superseded: impl Fn() -> bool,
) -> Option<Vec<RenderSegment>> {
    let Request {
        engine,
        revision,
        rate,
        frames,
        files,
        ..
    } = request;
    segments_reading(*rate, *frames, files, superseded, |read| {
        let guard = engine.lock();
        let current = guard.revision() == *revision;
        if current {
            read(guard.project());
        }
        current
    })
}

/// The segment pass: classify each of `duration_frames` frames and coalesce
/// contiguous runs of equal [`RenderQuality`]. `lend` runs a read against the
/// project and returns `false`, which abandons the pass, once the project has
/// moved on. The render-file lookup, a file-system call, runs between reads.
fn segments_reading(
    rate: Rational,
    duration_frames: i64,
    files: &RenderFiles,
    superseded: impl Fn() -> bool,
    mut lend: impl FnMut(&mut dyn FnMut(&Project)) -> bool,
) -> Option<Vec<RenderSegment>> {
    let mut segments: Vec<RenderSegment> = Vec::new();
    for f in 0..duration_frames.max(0) {
        if superseded() {
            return None;
        }
        let at = RationalTime::new(f, rate);
        let mut resolved = None;
        let read = lend(&mut |project: &Project| {
            resolved = aria_render::resolve(project, at)
                .ok()
                .filter(|scene| !scene.layers.is_empty())
                .map(|scene| {
                    let key = files.key(project, &scene);
                    (scene, key)
                });
        });
        if !read {
            return None;
        }
        let Some((mut scene, key)) = resolved else {
            continue;
        };
        let quality = if key.is_some_and(|key| files.contains(&key)) {
            RenderQuality::Green
        } else {
            let mut red = false;
            if !lend(&mut |project: &Project| red = is_red_scene(project, &mut scene)) {
                return None;
            }
            if red {
                RenderQuality::Red
            } else {
                RenderQuality::Yellow
            }
        };
        match segments.last_mut() {
            Some(last) if last.end_tick == f && last.quality == quality => {
                last.end_tick = f + 1;
            }
            _ => segments.push(RenderSegment {
                start_tick: f,
                end_tick: f + 1,
                quality,
            }),
        }
    }
    Some(segments)
}

fn is_red_scene(project: &Project, scene: &mut Scene) -> bool {
    if scene
        .layers
        .iter()
        .any(|layer| layer_has_heavy_or_nested(project, layer))
    {
        return true;
    }
    let picture_layers: usize = scene.layers.iter().map(picture_layer_count).sum();
    if picture_layers > 4 {
        return true;
    }
    aria_render::attach_motion_blur_passes(project, scene);
    scene.layers.iter().any(layer_has_motion_blur)
}

fn layer_has_heavy_or_nested(project: &Project, layer: &aria_render::SceneLayer) -> bool {
    if layer.effects.iter().any(|p| p.is_cpu() || p.is_stateful()) {
        return true;
    }
    if layer
        .clip
        .and_then(|id| project.clip(id))
        .is_some_and(|c| matches!(c.content, aria_models::ClipSource::Sequence { .. }))
    {
        return true;
    }
    match &layer.source {
        aria_render::LayerSource::Transition {
            outgoing, incoming, ..
        } => {
            layer_has_heavy_or_nested(project, outgoing)
                || layer_has_heavy_or_nested(project, incoming)
        }
        aria_render::LayerSource::Matte { layers, .. } => {
            layer_has_heavy_or_nested(project, &layers[0])
                || layer_has_heavy_or_nested(project, &layers[1])
        }
        _ => false,
    }
}

fn picture_layer_count(layer: &aria_render::SceneLayer) -> usize {
    match &layer.source {
        aria_render::LayerSource::Transition {
            outgoing, incoming, ..
        } => picture_layer_count(outgoing) + picture_layer_count(incoming),
        aria_render::LayerSource::Matte { layers, .. } => {
            picture_layer_count(&layers[0]) + picture_layer_count(&layers[1])
        }
        source => usize::from(source.spec().paints),
    }
}

fn layer_has_motion_blur(layer: &aria_render::SceneLayer) -> bool {
    if !layer.blur_passes.is_empty() {
        return true;
    }
    match &layer.source {
        aria_render::LayerSource::Transition {
            outgoing, incoming, ..
        } => layer_has_motion_blur(outgoing) || layer_has_motion_blur(incoming),
        aria_render::LayerSource::Matte { layers, .. } => {
            layer_has_motion_blur(&layers[0]) || layer_has_motion_blur(&layers[1])
        }
        _ => false,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BarStamp {
    pub revision: u64,
    pub mark_in: Option<i64>,
    pub mark_out: Option<i64>,
    pub generation: u64,
    pub env: String,
    pub dir: PathBuf,
}

/// A request for a segment pass.
struct Request {
    seq: u64,
    /// The engine whose project the pass reads, one frame at a time.
    engine: Arc<Mutex<Engine>>,
    /// The engine revision at the request. The pass abandons once the engine
    /// moves past it rather than record segments read from two projects.
    revision: u64,
    /// The sequence's frame rate and length in frames at `revision`.
    rate: Rational,
    frames: i64,
    files: Arc<RenderFiles>,
}

impl std::fmt::Debug for Request {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Request")
            .field("seq", &self.seq)
            .field("revision", &self.revision)
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
struct WorkerState {
    observed: bool,
    /// The `aria-render-bar` thread is started and serves [`Self::pending`].
    running: bool,
    /// The thread is computing a request it took from [`Self::pending`].
    busy: bool,
    /// The [`RenderBarState`] that started the thread has dropped: the thread
    /// abandons its pass and ends.
    closed: bool,
    /// The newest request the thread has not taken yet.
    pending: Option<Request>,
    submitted_stamp: Option<BarStamp>,
    latest_seq: u64,
    completed_seq: u64,
    completed: Arc<Vec<RenderSegment>>,
    /// How many times an `aria-render-bar` thread was started.
    #[cfg(test)]
    starts: usize,
}

#[derive(Debug)]
pub(crate) struct RenderBarShared {
    state: Mutex<WorkerState>,
    cv: Condvar,
}

impl RenderBarShared {
    /// Serve requests until the worker is closed: compute the newest request,
    /// drop a request the moment a newer one replaces it, and wait while none
    /// is pending. Wakes the window each time it goes idle with new segments
    /// recorded.
    fn serve(&self, wake: &Wake<()>) {
        let _unwinding = Unwinding(self);
        let mut updated = false;
        let mut state = self.state.lock();
        loop {
            if state.closed {
                state.running = false;
                state.busy = false;
                self.cv.notify_all();
                return;
            }
            let Some(request) = state.pending.take() else {
                // A pass the project moved past completes nothing, so a wait
                // for its request ends here, on the thread recorded as idle.
                state.busy = false;
                self.cv.notify_all();
                if std::mem::take(&mut updated) {
                    wake.wake(());
                }
                self.cv.wait(&mut state);
                continue;
            };
            state.busy = true;
            let seq = request.seq;
            // The request, and the engine it holds, drops with the lock
            // released.
            let segments = MutexGuard::unlocked(&mut state, move || {
                engine_segments_unless(&request, || {
                    let state = self.state.lock();
                    state.pending.is_some() || state.closed
                })
            });
            match segments {
                Some(segments) if seq >= state.completed_seq => {
                    state.completed_seq = seq;
                    state.completed = Arc::new(segments);
                    updated = true;
                }
                _ => {}
            }
            self.cv.notify_all();
        }
    }

    /// Start the `aria-render-bar` thread. Called with `running` already set.
    fn spawn_serve(self: &Arc<Self>, wake: Wake<()>) {
        let shared = Arc::clone(self);
        let spawned = std::thread::Builder::new()
            .name(THREAD_NAME.to_owned())
            .spawn(move || shared.serve(&wake));
        if let Err(err) = spawned {
            self.state.lock().running = false;
            self.cv.notify_all();
            tracing::error!(
                "could not start the {THREAD_NAME} thread: {err}; the render bar keeps its last \
                 segments until the next edit retries"
            );
        }
    }

    pub(super) fn record_from_job(&self, job_stamp: BarStamp, segments: Arc<Vec<RenderSegment>>) {
        let mut state = self.state.lock();
        let compatible = state.submitted_stamp.as_ref().is_none_or(|s| {
            s.revision == job_stamp.revision
                && s.mark_in == job_stamp.mark_in
                && s.mark_out == job_stamp.mark_out
                && s.env == job_stamp.env
                && s.dir == job_stamp.dir
        });
        if compatible && state.pending.is_none() {
            state.latest_seq += 1;
            let seq = state.latest_seq;
            state.completed_seq = seq;
            state.completed = segments;
            state.submitted_stamp = Some(job_stamp);
            self.cv.notify_all();
        }
    }
}

#[cfg(test)]
impl RenderBarShared {
    /// The segments of request `target_seq`, once a pass completes it, or
    /// `None` once nothing is left to complete it: the thread went idle with
    /// the pass abandoned, it panicked, or it could not be started.
    pub(crate) fn wait_for(&self, target_seq: u64) -> Option<Arc<Vec<RenderSegment>>> {
        let mut state = self.state.lock();
        while state.completed_seq < target_seq {
            if !state.running || (!state.busy && state.pending.is_none()) {
                return None;
            }
            self.cv.wait(&mut state);
        }
        Some(Arc::clone(&state.completed))
    }

    /// [`Self::wait_for`] bounded by `limit`; `None` when it ran out.
    pub(crate) fn wait_within(
        &self,
        target_seq: u64,
        limit: std::time::Duration,
    ) -> Option<Arc<Vec<RenderSegment>>> {
        let deadline = std::time::Instant::now() + limit;
        let mut state = self.state.lock();
        while state.completed_seq < target_seq {
            if self.cv.wait_until(&mut state, deadline).timed_out() {
                return None;
            }
        }
        Some(Arc::clone(&state.completed))
    }

    /// The sequence number of the newest request.
    pub(crate) fn latest_seq(&self) -> u64 {
        self.state.lock().latest_seq
    }

    /// Whether a request waits for the thread to take it.
    pub(crate) fn is_pending(&self) -> bool {
        self.state.lock().pending.is_some()
    }
}

/// Clears [`WorkerState::running`] and [`WorkerState::busy`] when the serving
/// thread panics, so a wait for its request ends and the next request starts
/// a new thread.
struct Unwinding<'a>(&'a RenderBarShared);

impl Drop for Unwinding<'_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            let mut state = self.0.state.lock();
            state.running = false;
            state.busy = false;
            drop(state);
            self.0.cv.notify_all();
        }
    }
}

/// The session's handle on the render-bar worker. Dropping it ends the
/// `aria-render-bar` thread.
#[derive(Debug)]
pub(crate) struct RenderBarState {
    pub(crate) shared: Arc<RenderBarShared>,
}

impl Default for RenderBarState {
    fn default() -> Self {
        Self {
            shared: Arc::new(RenderBarShared {
                state: Mutex::new(WorkerState {
                    observed: false,
                    running: false,
                    busy: false,
                    closed: false,
                    pending: None,
                    submitted_stamp: None,
                    latest_seq: 0,
                    completed_seq: 0,
                    completed: Arc::new(Vec::new()),
                    #[cfg(test)]
                    starts: 0,
                }),
                cv: Condvar::new(),
            }),
        }
    }
}

impl Drop for RenderBarState {
    fn drop(&mut self) {
        self.shared.state.lock().closed = true;
        self.shared.cv.notify_all();
    }
}

impl RenderBarState {
    pub(super) fn is_observed(&self) -> bool {
        self.shared.state.lock().observed
    }

    pub(super) fn ensure_submitted(
        &self,
        preview_worker: &PreviewWorker,
        host: &ProjectHost,
        files: &Arc<RenderFiles>,
        stamp: BarStamp,
    ) -> u64 {
        let mut state = self.shared.state.lock();
        state.observed = true;
        if state.submitted_stamp.as_ref() == Some(&stamp) {
            return state.latest_seq;
        }
        state.latest_seq += 1;
        let seq = state.latest_seq;
        state.submitted_stamp = Some(stamp);
        let revision = host.revision();
        let (rate, frames) = {
            let project = host.snapshot();
            let timeline = project.timeline();
            (timeline.frame_rate, timeline.duration().value)
        };
        if frames <= 0 {
            state.pending = None;
            state.completed_seq = seq;
            state.completed = Arc::new(Vec::new());
            self.shared.cv.notify_all();
            return seq;
        }
        state.pending = Some(Request {
            seq,
            engine: Arc::clone(host.engine()),
            revision,
            rate,
            frames,
            files: Arc::clone(files),
        });
        let start = !state.running;
        state.running = true;
        #[cfg(test)]
        {
            state.starts += usize::from(start);
        }
        drop(state);
        if start {
            self.shared.spawn_serve(preview_worker.clone_wake());
        } else {
            self.shared.cv.notify_all();
        }
        seq
    }

    pub(super) fn latest_nonblocking(&self) -> Arc<Vec<RenderSegment>> {
        Arc::clone(&self.shared.state.lock().completed)
    }
}
