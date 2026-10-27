//! Render In to Out cache wiring.
//!
//! [`RenderFiles`] is owned by `aria-render` and shared between the session's
//! `aria_engine::Engine` and background render jobs. The ruler's render bar
//! is computed in `super::render_bar`.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use aria_core::RationalTime;
use aria_core::count::counted;
use aria_jobs::JobCompletion;
use aria_plugin::PluginContext;
use aria_render::RenderFiles;
use aria_storage::CacheId;

use super::AppState;
use super::render_bar::{BarStamp, compute_render_segments};

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

impl AppState {
    /// Resolved disk directory for [`CacheId::RenderFiles`].
    pub(crate) fn render_files_cache_dir(&self) -> PathBuf {
        self.storage_layout()
            .resolve(CacheId::RenderFiles)
            .expect("RenderFiles is a disk cache")
    }

    /// Build the [`RenderFiles`] handle from the current storage layout and
    /// mounted plugin list, installing it on the engine and waking the render
    /// bar worker. Every change to the storage settings made in the app calls
    /// this where it is made.
    pub(crate) fn refresh_render_files(&mut self) {
        self.render_files_mounts = self.ctx.mounted_revision();
        let env = render_files_environment(&self.ctx);
        let dir = self.render_files_cache_dir();
        self.install_render_files(dir, env);
        self.request_render_bar_refresh();
    }

    /// The active [`RenderFiles`] handle, rebuilt first when a plugin has
    /// joined or left the mounted set since it was built. Its environment
    /// names every mounted plugin, so a frame rendered under another set is
    /// never served. A plugin leaves the set off the UI thread when a fault in
    /// its code fails it, so the set is compared here, by revision, rather
    /// than at each mount.
    pub(crate) fn sync_render_files_handle(&mut self) -> Arc<RenderFiles> {
        let mounts = self.ctx.mounted_revision();
        if mounts != self.render_files_mounts {
            self.render_files_mounts = mounts;
            let env = render_files_environment(&self.ctx);
            if self.render_files_handle.environment() != env {
                let dir = self.render_files_handle.dir().to_path_buf();
                self.install_render_files(dir, env);
            }
        }
        Arc::clone(&self.render_files_handle)
    }

    /// Point the session and the engine at a new [`RenderFiles`] handle and
    /// drop the frames looked up through the old one.
    fn install_render_files(&mut self, dir: PathBuf, env: String) {
        let rf = Arc::new(RenderFiles::new(dir, env));
        self.engine.lock().set_render_files(Some(Arc::clone(&rf)));
        self.render_files_handle = rf;
        self.preview_worker.clear();
        self.render_files_generation.fetch_add(1, Ordering::AcqRel);
    }

    /// The session's [`RenderFiles`] cache handle.
    pub fn render_files(&self) -> Arc<RenderFiles> {
        Arc::clone(&self.render_files_handle)
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
            self.render_bar_state.ensure_submitted(
                &self.preview_worker,
                self.host.engine(),
                &files,
                stamp,
            );
        }
        self.render_bar = self.render_bar_state.latest_nonblocking();
    }

    /// Pull the latest completed [`RenderSegment`] list from the background
    /// worker into [`Self::render_bar`]. Returns `true` when it changed.
    pub(crate) fn poll_render_bar(&mut self) -> bool {
        let files = self.sync_render_files_handle();
        let stamp = self.current_bar_stamp(&files);
        self.render_bar_state.ensure_submitted(
            &self.preview_worker,
            self.host.engine(),
            &files,
            stamp,
        );
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
        self.render_bar_state.ensure_submitted(
            &self.preview_worker,
            self.host.engine(),
            &files,
            stamp,
        );
        self.render_bar_state.latest_nonblocking()
    }

    /// Wait for the background worker to finish computing the render-bar
    /// segments for the current revision, work area, and render-file generation,
    /// update [`Self::render_bar`], and return the result.
    ///
    /// # Panics
    ///
    /// Panics when the pass ends without completing the request: its thread
    /// panicked or could not be started.
    #[cfg(test)]
    pub(crate) fn render_bar_segments(&mut self) -> Arc<Vec<RenderSegment>> {
        let files = self.sync_render_files_handle();
        let stamp = self.current_bar_stamp(&files);
        let seq = self.render_bar_state.ensure_submitted(
            &self.preview_worker,
            self.host.engine(),
            &files,
            stamp,
        );
        let completed = self
            .render_bar_state
            .shared
            .wait_for(seq)
            .unwrap_or_else(|| {
                panic!(
                    "the render-bar pass ended without completing request {seq}: its thread \
                 panicked or could not be started"
                )
            });
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
        self.status_message = Some(format!(
            "Rendering {} {}",
            counted(total, "frame", "frames"),
            scope.what()
        ));
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
                "{} rendered, {skipped} already cached",
                counted(rendered, "frame", "frames")
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
        self.status_message = Some(format!(
            "Deleting render files for {}",
            counted(total, "frame", "frames")
        ));
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
            JobCompletion::new(format!(
                "Removed {}",
                counted(removed, "render file", "render files")
            ))
            .with_output("removed", removed.to_string())
            .map_err(String::from)
        });
        Ok(id.raw())
    }
}
#[cfg(test)]
#[path = "render_files_tests.rs"]
mod tests;
