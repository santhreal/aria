//! Render In to Out cache wiring and timeline render-bar segment computation.
//!
//! [`RenderFiles`] is owned by `aria-render` and shared between the session's
//! `aria_engine::Engine` and background render jobs. Computing
//! [`RenderSegment`] runs off the UI thread on the session's preview worker or
//! at the tail of a render-file job, because resolving every frame's scene and
//! checking disk presence must never run on the ruler's paint path.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use aria_core::RationalTime;
use aria_jobs::JobCompletion;
use aria_models::Project;
use aria_plugin::{PluginContext, ProjectHost};
use aria_render::{RenderFiles, Scene};
use aria_storage::CacheId;
use parking_lot::{Condvar, Mutex};

use super::AppState;
use crate::preview_worker::PreviewWorker;

/// Playback readiness of a timeline frame range in the ruler's `3px` render bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderQuality {
    /// Frame has a matching render file on disk.
    Green,
    /// Frame is unrendered, has at least one layer, and is expected to play at
    /// the sequence rate without a pre-render.
    Yellow,
    /// Frame is unrendered and has a CPU effect, a stateful effect, motion blur
    /// on a moving clip, or a heavy stack (> 2 layers / transitions / track
    /// mattes).
    Red,
}

/// What a render job covers, for its status line and its Jobs row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RenderScope {
    /// The work area, or the whole sequence without one.
    InToOut,
    /// The frames the selected clips cover.
    Selection,
}

impl RenderScope {
    fn what(self) -> &'static str {
        match self {
            Self::InToOut => "in to out",
            Self::Selection => "of the selection",
        }
    }

    fn job_label(self) -> &'static str {
        match self {
            Self::InToOut => "Rendering In to Out",
            Self::Selection => "Rendering Selection",
        }
    }
}

/// A contiguous half-open `[start_tick, end_tick)` span of equal [`RenderQuality`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderSegment {
    pub start_tick: i64,
    pub end_tick: i64,
    pub quality: RenderQuality,
}

/// Deterministic environment string for [`RenderFiles`]: mounted plugins formatted
/// as `id@version`, sorted and `;`-joined.
pub(crate) fn render_files_environment(ctx: &PluginContext) -> String {
    let mut plugins: Vec<String> = ctx
        .mounted()
        .into_iter()
        .map(|info| format!("{}@{}", info.manifest.id, info.manifest.version))
        .collect();
    plugins.sort();
    plugins.join(";")
}

/// Classify every frame of `project` against `files` and coalesce contiguous
/// runs of equal [`RenderQuality`]. Empty frames (no layers) produce no segment.
pub fn compute_render_segments(project: &Project, files: &RenderFiles) -> Vec<RenderSegment> {
    let rate = project.timeline().frame_rate;
    let duration_frames = project.timeline().duration().value.max(0);
    if duration_frames <= 0 {
        return Vec::new();
    }
    let mut segments: Vec<RenderSegment> = Vec::new();
    for f in 0..duration_frames {
        let start_tick = f;
        let end_tick = f + 1;
        let at = RationalTime::new(f, rate);
        let Ok(mut scene) = aria_render::resolve(project, at) else {
            continue;
        };
        if scene.layers.is_empty() {
            continue;
        }
        let quality = if files
            .key(project, &scene)
            .is_some_and(|key| files.contains(&key))
        {
            RenderQuality::Green
        } else if is_red_scene(project, &mut scene) {
            RenderQuality::Red
        } else {
            RenderQuality::Yellow
        };
        match segments.last_mut() {
            Some(last) if last.end_tick == start_tick && last.quality == quality => {
                last.end_tick = end_tick;
            }
            _ => segments.push(RenderSegment {
                start_tick,
                end_tick,
                quality,
            }),
        }
    }
    segments
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

#[derive(Debug)]
struct WorkerState {
    observed: bool,
    pending: Option<(u64, Arc<Project>, Arc<RenderFiles>)>,
    submitted_stamp: Option<BarStamp>,
    latest_seq: u64,
    completed_seq: u64,
    completed: Arc<Vec<RenderSegment>>,
}

#[derive(Debug)]
pub(crate) struct RenderBarShared {
    state: Mutex<WorkerState>,
    cv: Condvar,
}

impl RenderBarShared {
    fn drain_pending(&self) -> bool {
        let mut updated = false;
        loop {
            let next = {
                let mut state = self.state.lock();
                state.pending.take()
            };
            let Some((seq, project, files)) = next else {
                break;
            };
            let segments = Arc::new(compute_render_segments(&project, &files));
            let mut state = self.state.lock();
            if seq >= state.completed_seq {
                state.completed_seq = seq;
                state.completed = segments;
                updated = true;
            }
            self.cv.notify_all();
        }
        updated
    }

    fn record_from_job(&self, job_stamp: BarStamp, segments: Arc<Vec<RenderSegment>>) {
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

#[derive(Debug, Clone)]
pub(crate) struct RenderBarState {
    pub(crate) shared: Arc<RenderBarShared>,
}

impl Default for RenderBarState {
    fn default() -> Self {
        Self {
            shared: Arc::new(RenderBarShared {
                state: Mutex::new(WorkerState {
                    observed: false,
                    pending: None,
                    submitted_stamp: None,
                    latest_seq: 0,
                    completed_seq: 0,
                    completed: Arc::new(Vec::new()),
                }),
                cv: Condvar::new(),
            }),
        }
    }
}

impl RenderBarState {
    fn is_observed(&self) -> bool {
        self.shared.state.lock().observed
    }

    fn ensure_submitted(
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
        let project = host.snapshot();
        if project.timeline().duration().value <= 0 {
            state.pending = None;
            state.completed_seq = seq;
            state.completed = Arc::new(Vec::new());
            self.shared.cv.notify_all();
            return seq;
        }
        let need_schedule = state.pending.is_none();
        state.pending = Some((seq, project, Arc::clone(files)));
        drop(state);
        if need_schedule {
            let shared = Arc::clone(&self.shared);
            preview_worker.schedule_mail(move || shared.drain_pending());
        }
        seq
    }

    fn latest_nonblocking(&self) -> Arc<Vec<RenderSegment>> {
        Arc::clone(&self.shared.state.lock().completed)
    }

    fn wait_for(&self, target_seq: u64) -> Arc<Vec<RenderSegment>> {
        let mut state = self.shared.state.lock();
        while state.completed_seq < target_seq {
            self.shared.cv.wait(&mut state);
        }
        Arc::clone(&state.completed)
    }
}

impl AppState {
    /// Resolved disk directory for [`CacheId::RenderFiles`].
    pub(crate) fn render_files_cache_dir(&self) -> PathBuf {
        self.storage_layout()
            .resolve(CacheId::RenderFiles)
            .expect("RenderFiles is a disk cache")
    }

    /// Build or refresh the [`RenderFiles`] handle from the current storage layout
    /// and mounted plugin list, installing it on the engine and waking the render
    /// bar worker.
    pub(crate) fn refresh_render_files(&mut self) {
        let env = render_files_environment(&self.ctx);
        let dir = self.render_files_cache_dir();
        let rf = Arc::new(RenderFiles::new(dir, env));
        self.engine.lock().set_render_files(Some(Arc::clone(&rf)));
        self.render_files_handle = rf;
        self.preview_worker.clear();
        self.bump_render_files_generation();
    }

    /// Ensure [`Self::render_files_handle`] and the engine's [`RenderFiles`]
    /// reflect the current storage layout and mounted plugins, returning the
    /// active handle.
    pub(crate) fn sync_render_files_handle(&mut self) -> Arc<RenderFiles> {
        let env = render_files_environment(&self.ctx);
        let dir = self.render_files_cache_dir();
        if self.render_files_handle.environment() != env || self.render_files_handle.dir() != dir {
            let rf = Arc::new(RenderFiles::new(dir, env));
            self.engine.lock().set_render_files(Some(Arc::clone(&rf)));
            self.render_files_handle = rf;
            self.preview_worker.clear();
            self.render_files_generation.fetch_add(1, Ordering::AcqRel);
        }
        Arc::clone(&self.render_files_handle)
    }

    /// The session's [`RenderFiles`] cache handle.
    pub fn render_files(&self) -> Arc<RenderFiles> {
        Arc::clone(&self.render_files_handle)
    }

    /// Bump the render-files generation counter and schedule a background
    /// render-bar refresh.
    pub(crate) fn bump_render_files_generation(&mut self) {
        self.render_files_generation.fetch_add(1, Ordering::AcqRel);
        self.request_render_bar_refresh();
    }

    fn current_bar_stamp(&self, files: &RenderFiles) -> BarStamp {
        BarStamp {
            revision: self.revision,
            mark_in: self.mark_in,
            mark_out: self.mark_out,
            generation: self.render_files_generation.load(Ordering::Acquire),
            env: files.environment().to_owned(),
            dir: files.dir().to_path_buf(),
        }
    }

    /// Enqueue a background render-bar recomputation when the project revision,
    /// work area, or render-file generation has moved.
    pub(crate) fn request_render_bar_refresh(&mut self) {
        let files = self.sync_render_files_handle();
        if self.render_bar_state.is_observed() {
            let stamp = self.current_bar_stamp(&files);
            self.render_bar_state
                .ensure_submitted(&self.preview_worker, &self.host, &files, stamp);
        }
        self.render_bar = self.render_bar_state.latest_nonblocking();
    }

    /// Pull the latest completed [`RenderSegment`] list from the background
    /// worker into [`Self::render_bar`]. Returns `true` when it changed.
    pub(crate) fn poll_render_bar(&mut self) -> bool {
        let files = self.sync_render_files_handle();
        let stamp = self.current_bar_stamp(&files);
        self.render_bar_state
            .ensure_submitted(&self.preview_worker, &self.host, &files, stamp);
        let latest = self.render_bar_state.latest_nonblocking();
        if !Arc::ptr_eq(&self.render_bar, &latest) && *self.render_bar != *latest {
            self.render_bar = latest;
            true
        } else {
            self.render_bar = latest;
            false
        }
    }

    /// Non-blocking read of the latest completed render-bar segments for the
    /// timeline ruler paint path. Never computes keys or stats files on the UI
    /// thread.
    pub fn render_bar(&self) -> Arc<Vec<RenderSegment>> {
        let files = self.render_files();
        let stamp = self.current_bar_stamp(&files);
        self.render_bar_state
            .ensure_submitted(&self.preview_worker, &self.host, &files, stamp);
        self.render_bar_state.latest_nonblocking()
    }

    /// Wait for the background worker to finish computing the render-bar
    /// segments for the current revision, work area, and render-file generation,
    /// update [`Self::render_bar`], and return the result.
    pub fn render_bar_segments(&mut self) -> Arc<Vec<RenderSegment>> {
        let files = self.sync_render_files_handle();
        let stamp = self.current_bar_stamp(&files);
        let seq =
            self.render_bar_state
                .ensure_submitted(&self.preview_worker, &self.host, &files, stamp);
        let completed = self.render_bar_state.wait_for(seq);
        self.render_bar = Arc::clone(&completed);
        completed
    }

    /// The work-area span `[mark_in, mark_out)` when both marks are set and
    /// `mark_out > mark_in`, otherwise the sequence span `[0, duration)` when
    /// `duration > 0`.
    pub fn work_area_or_sequence_range(&self) -> Option<(i64, i64)> {
        if let Some(range) = self.marked_range() {
            return Some(range);
        }
        let duration = self.store.sequence.duration_ticks;
        (duration > 0).then_some((0, duration))
    }

    /// Render every uncached frame in [`Self::work_area_or_sequence_range`] into
    /// the render-files cache, as a background job.
    pub fn render_in_to_out(&mut self) -> Result<u64, String> {
        let (in_tick, out_tick) = self
            .work_area_or_sequence_range()
            .ok_or_else(|| "Nothing to render. Your timeline is empty.".to_string())?;
        let start_frame = in_tick.max(0);
        let end_frame = out_tick.max(start_frame);
        if end_frame <= start_frame {
            return Err("That span covers no frames.".into());
        }
        self.render_spans(RenderScope::InToOut, vec![(start_frame, end_frame)])
    }

    /// The frames the selected clips cover, as sorted, disjoint, half-open
    /// `[start, end)` spans: overlapping and abutting clips join one span.
    pub fn selection_spans(&self) -> Vec<(i64, i64)> {
        let mut spans: Vec<(i64, i64)> = self
            .selection
            .iter()
            .filter_map(|clip| self.ui_clip_ref(*clip))
            .map(|clip| (clip.timeline_start_ticks.max(0), clip.timeline_end_ticks))
            .filter(|(start, end)| end > start)
            .collect();
        spans.sort_unstable();
        let mut merged: Vec<(i64, i64)> = Vec::with_capacity(spans.len());
        for (start, end) in spans {
            match merged.last_mut() {
                Some(last) if start <= last.1 => last.1 = last.1.max(end),
                _ => merged.push((start, end)),
            }
        }
        merged
    }

    /// Render every uncached frame the selected clips cover into the
    /// render-files cache, as a background job. Frames between two selected
    /// clips that neither covers are not rendered.
    pub fn render_selection(&mut self) -> Result<u64, String> {
        let spans = self.selection_spans();
        if spans.is_empty() {
            return Err("Select the clips to render.".into());
        }
        self.render_spans(RenderScope::Selection, spans)
    }

    /// Render the uncached frames of `spans`, sorted and disjoint, as one job.
    fn render_spans(&mut self, scope: RenderScope, spans: Vec<(i64, i64)>) -> Result<u64, String> {
        let project = self.host.snapshot();
        let rate = project.timeline().frame_rate;
        let total: u64 = spans.iter().map(|(start, end)| (end - start) as u64).sum();
        let files = self.sync_render_files_handle();
        let gpu = super::render_device::shared_device(&self.ctx);
        let mut job_stamp = self.current_bar_stamp(&files);
        let generation = Arc::clone(&self.render_files_generation);
        let bar_weak = Arc::downgrade(&self.render_bar_state.shared);
        let preview_worker = self.preview_worker.clone_wake();
        self.status_message = Some(format!("Rendering {total} frame(s) {}", scope.what()));
        let id = self.spawn_job(scope.job_label(), move |context| {
            let mut gpu = gpu;
            let mut renderer: Option<aria_render::Renderer> = None;
            let mut rendered = 0_u64;
            let mut skipped = 0_u64;
            let frames = spans.iter().flat_map(|&(start, end)| start..end);
            for (idx, f) in frames.enumerate() {
                if context.cancelled() || bar_weak.strong_count() == 0 {
                    return Err("cancelled".into());
                }
                let at = RationalTime::new(f, rate);
                let scene = aria_render::resolve(&project, at).map_err(|err| err.to_string())?;
                match files.key(&project, &scene) {
                    None => {
                        skipped += 1;
                    }
                    Some(key) if files.contains(&key) => {
                        skipped += 1;
                    }
                    Some(key) => {
                        if bar_weak.strong_count() == 0 {
                            return Err("cancelled".into());
                        }
                        let r = match &mut renderer {
                            Some(r) => r,
                            None => {
                                let mut r =
                                    super::render_device::renderer_on(gpu.take(), "the render")?;
                                r.set_apply_motion_blur(true);
                                r.set_render_files(None);
                                renderer.insert(r)
                            }
                        };
                        let image = r
                            .render_frame(&project, at)
                            .map_err(|err| err.to_string())?;
                        files.store(&key, &image).map_err(|err| err.to_string())?;
                        rendered += 1;
                    }
                }
                let done = (idx + 1) as u64;
                context.set_progress(
                    done as f32 / total as f32,
                    format!("frame {done}/{total} ({rendered} rendered, {skipped} cached)"),
                );
            }
            let next_gen = generation.fetch_add(1, Ordering::AcqRel) + 1;
            job_stamp.generation = next_gen;
            if let Some(bar_shared) = bar_weak.upgrade() {
                let segments = Arc::new(compute_render_segments(&project, &files));
                bar_shared.record_from_job(job_stamp, segments);
            }
            preview_worker.wake(());
            JobCompletion::new(format!(
                "{rendered} frame(s) rendered, {skipped} already cached"
            ))
            .with_output("rendered", rendered.to_string())
            .and_then(|c| c.with_output("cached", skipped.to_string()))
            .map_err(String::from)
        });
        Ok(id.raw())
    }

    /// Delete the cached render files for every frame of the open sequence, as
    /// a background job.
    pub fn delete_render_files(&mut self) -> Result<u64, String> {
        let project = self.host.snapshot();
        let rate = project.timeline().frame_rate;
        let end_frame = project
            .timeline()
            .duration()
            .value
            .max(self.store.sequence.duration_ticks)
            .max(0);
        if end_frame <= 0 {
            return Err("Nothing to delete. Your timeline is empty.".into());
        }
        let total = end_frame as u64;
        let files = self.sync_render_files_handle();
        let generation = Arc::clone(&self.render_files_generation);
        let mut job_stamp = self.current_bar_stamp(&files);
        let bar_weak = Arc::downgrade(&self.render_bar_state.shared);
        let preview_worker = self.preview_worker.clone_wake();
        self.preview_worker.clear();
        self.status_message = Some(format!("Deleting render files for {total} frame(s)"));
        let id = self.spawn_job("Deleting render files", move |context| {
            let mut removed = 0_u64;
            for (idx, f) in (0..end_frame).enumerate() {
                if context.cancelled() || bar_weak.strong_count() == 0 {
                    return Err("cancelled".into());
                }
                let at = RationalTime::new(f, rate);
                if let Ok(scene) = aria_render::resolve(&project, at)
                    && let Some(key) = files.key(&project, &scene)
                    && files.remove(&key).map_err(|err| err.to_string())?
                {
                    removed += 1;
                }
                let done = (idx + 1) as u64;
                context.set_progress(
                    done as f32 / total as f32,
                    format!("frame {done}/{total} ({removed} removed)"),
                );
            }
            let next_gen = generation.fetch_add(1, Ordering::AcqRel) + 1;
            job_stamp.generation = next_gen;
            if let Some(bar_shared) = bar_weak.upgrade() {
                let segments = Arc::new(compute_render_segments(&project, &files));
                bar_shared.record_from_job(job_stamp, segments);
            }
            preview_worker.wake(());
            JobCompletion::new(format!("Removed {removed} render file(s)"))
                .with_output("removed", removed.to_string())
                .map_err(String::from)
        });
        Ok(id.raw())
    }
}
#[cfg(test)]
#[path = "render_files_tests.rs"]
mod tests;
