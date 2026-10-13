//! Session-scoped editing runtime: project state plus inverse undo/redo.
//!
//! Preview (`get_frame`) and file export delegate to the GPU `aria-render`
//! pipeline and the native `aria-encoder`; this type owns the session
//! [`Renderer`] and the undo [`History`].

use std::{borrow::Cow, path::PathBuf, sync::Arc};

use aria_commands::{Command, EditCommand, ProjectCommand};
use aria_models::{
    AnimationRef, AnimationSlot, ClipId, ClipTransform, ColorAdjustments, CropRect, Filter,
    Generator, LayerStyles, LookAnimations, MediaId, Project, Rational, RationalTime, SequenceId,
    TrackKind,
};
use aria_render::{
    ExportSettings, FrameSink, Renderer, ResolveOverrides, RgbaImage, Scene, SeekPolicy,
    export_to_file_observed, resolve_with,
};

use crate::action::{ApplyContext, ApplyOutcome, EditAction, History, HistoryStep, dispatch};
use crate::error::EngineError;

mod source_frame;

/// Session configuration for [`Engine`].
#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// Maximum inverse actions retained on the undo stack.
    pub undo_limit: usize,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self { undo_limit: 100 }
    }
}

/// One row of the history surface: what the step is called, and whether it is
/// currently applied to the project.
///
/// The label borrows the history step. Static command labels and grouped
/// dynamic labels therefore require no copy when the history is inspected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryEntry<'a> {
    pub label: Cow<'a, str>,
    pub applied: bool,
}

/// The animation slots one clip renders with while a preview override is set.
///
/// A free function over two fields rather than a method, so the caller can hold this
/// alongside the mutable borrow of the renderer. `None` for an override naming a clip
/// that no longer exists: a stale preview shows the project, not a panic.
fn animation_override_view<'a>(
    project: &'a Project,
    over: &'a Option<(ClipId, AnimationSlot, Option<AnimationRef>)>,
) -> Option<(ClipId, LookAnimations<'a>)> {
    let (id, slot, animation) = over.as_ref()?;
    let clip = project.timeline().clip(*id)?;
    Some((
        *id,
        LookAnimations::of(clip).with_slot(*slot, animation.as_ref()),
    ))
}

/// The live preview overrides one session can hold: state a frame renders with
/// that no project revision describes, and that no edit, history entry, or export
/// ever sees.
///
/// One struct rather than six fields on [`Engine`], so a render entry point takes
/// every override at once ([`view`](LivePreviews::view)) and a new kind of preview
/// cannot reach three of the four render paths.
#[derive(Default)]
struct LivePreviews {
    /// Live transform for one clip: a canvas gesture previews a move/scale/rotate
    /// before it is committed. Release commits a real `SetClipTransform`.
    transform: Option<(ClipId, ClipTransform)>,
    /// Live generator content for one clip (inspector slider preview).
    generator: Option<(ClipId, Generator)>,
    /// Live filter/adjustment look for one clip (inspector slider preview).
    look: Option<(ClipId, Option<Filter>, ColorAdjustments)>,
    /// Live layer styles for one clip (inspector slider preview).
    styles: Option<(ClipId, LayerStyles)>,
    /// Live animation slots for one clip: a catalog hover previews a preset before
    /// any edit exists, so moving off the entry restores the clip with nothing on
    /// the undo stack.
    animation: Option<(ClipId, AnimationSlot, Option<AnimationRef>)>,
    /// Live crop window and mirror flags for one clip. An on-canvas crop drag shows
    /// the uncropped frame while the handles move, so the operator cuts into the
    /// picture they can see instead of one the crop already hid.
    crop: Option<(ClipId, CropRect, bool, bool)>,
}

impl LivePreviews {
    /// What the resolver should apply. Takes `project` because the animation
    /// override is stored as a slot and read against the clip it names.
    fn view<'a>(&'a self, project: &'a Project) -> ResolveOverrides<'a> {
        ResolveOverrides {
            transform: self.transform,
            generator: self.generator.as_ref().map(|(id, g)| (*id, g)),
            look: self
                .look
                .as_ref()
                .map(|(id, filter, adjust)| (*id, filter.as_ref(), adjust)),
            styles: self.styles.as_ref().map(|(id, s)| (*id, s)),
            animation: animation_override_view(project, &self.animation),
            crop: self.crop,
        }
    }

    /// Whether any override is set: a frame rendered now is session-only.
    fn any(&self) -> bool {
        self.transform.is_some()
            || self.generator.is_some()
            || self.look.is_some()
            || self.styles.is_some()
            || self.animation.is_some()
            || self.crop.is_some()
    }
}

/// Aria editing engine: project state, inverse undo/redo, and a session
/// renderer for preview and export.
pub struct Engine {
    /// Shared with every outstanding [`snapshot`](Self::snapshot); a mutation
    /// copies the project only while one is held.
    project: Arc<Project>,
    config: EngineConfig,
    history: History,
    project_path: Option<PathBuf>,
    renderer: Renderer,
    /// Session revision: bumped on every successful project mutation (edits,
    /// open/load, undo, redo). Never serialized.
    revision: u64,
    /// Revision last written to (or read from) disk; with `revision` this is
    /// the dirty flag (see [`is_dirty`](Self::is_dirty)).
    saved_revision: u64,
    /// Whether the session was saved-clean when the open group began. A
    /// rolled-back group restores the project it started from, so a session
    /// that was clean before it is clean after: without this, a rehearsal or an
    /// aborted turn would leave the operator with unsaved changes that nothing
    /// made.
    group_started_clean: bool,
    /// Proxy substitutions applied so far: bumped whenever a media item starts
    /// or stops decoding through a proxy. Renderer state, so it must not move
    /// `revision` (see [`proxy_revision`](Self::proxy_revision)).
    proxy_revision: u64,
    /// Live preview overrides: session-only state that no project revision
    /// describes (see [`LivePreviews`]).
    previews: LivePreviews,
}

impl Engine {
    /// Build an engine with a fresh, empty project and a headless renderer.
    pub fn new(config: EngineConfig) -> Result<Self, EngineError> {
        Self::with_project(config, Project::new("untitled", Rational::FPS_24))
    }

    /// Build an engine around an existing project, on a renderer of its own.
    pub fn with_project(config: EngineConfig, project: Project) -> Result<Self, EngineError> {
        Ok(Self::with_renderer(
            config,
            project,
            Renderer::new_headless()?,
        ))
    }

    /// Build an engine around an existing project and an existing renderer.
    ///
    /// For a host that already has a GPU device: `Renderer::with_gpu` shares it
    /// rather than bringing up a second one. Nothing else differs — the engine
    /// owns the renderer either way, which is why it is passed by value and not
    /// behind a lock.
    pub fn with_renderer(config: EngineConfig, project: Project, renderer: Renderer) -> Self {
        let history = History::new(config.undo_limit);
        Self {
            project: Arc::new(project),
            config,
            history,
            project_path: None,
            renderer,
            revision: 0,
            saved_revision: 0,
            group_started_clean: true,
            proxy_revision: 0,
            previews: LivePreviews::default(),
        }
    }

    pub fn config(&self) -> &EngineConfig {
        &self.config
    }

    /// Read-only view of the session project. Timeline and media mutations must
    /// go through [`apply`](Self::apply) so undo/redo stays consistent.
    pub fn project(&self) -> &Project {
        &self.project
    }

    /// The project as it is now, as an immutable value that later edits do not
    /// change. Costs a reference count; the next edit copies the project once if
    /// the snapshot is still held.
    pub fn snapshot(&self) -> Arc<Project> {
        Arc::clone(&self.project)
    }

    /// Whether a command group is open: applied commands collect into one
    /// history entry until it is committed or rolled back.
    pub fn in_group(&self) -> bool {
        self.history.in_group()
    }

    /// How many steps the open group holds, for
    /// [`rollback_group_to`](Self::rollback_group_to). Zero with no group open.
    pub fn group_len(&self) -> usize {
        self.history.group_len()
    }

    /// The currently active sequence for editing (`None` ⇔ main root timeline).
    pub fn active_sequence(&self) -> Option<SequenceId> {
        self.project.active_sequence()
    }

    /// Open a secondary sequence for editing (`None` ⇔ main root timeline).
    pub fn open_sequence(
        &mut self,
        sequence: Option<SequenceId>,
    ) -> Result<ApplyOutcome, EngineError> {
        self.apply(Command::Edit(EditCommand::OpenSequence { sequence }))
    }

    /// Path last written with `Save` or read with `Open`/`Load`.
    pub fn project_path(&self) -> Option<&PathBuf> {
        self.project_path.as_ref()
    }

    /// Set the project's per-project AI agent rules (`ProjectMetadata`).
    /// Metadata, not timeline state: it bypasses the command layer and is
    /// not undoable by design (like `RelinkMedia`), but it does dirty the
    /// session so the rules save with the project.
    pub fn set_agent_rules(&mut self, rules: String) {
        if self.project.metadata().agent_rules == rules {
            return;
        }
        Arc::make_mut(&mut self.project).metadata_mut().agent_rules = rules;
        self.revision += 1;
    }

    /// Monotonic session revision, bumped by every successful mutation.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// True when the session has mutations not yet saved. Conservative: an undo
    /// back to saved content still reads dirty (revisions only grow).
    pub fn is_dirty(&self) -> bool {
        self.revision != self.saved_revision
    }

    /// Declare the session unsaved without editing it.
    ///
    /// A recovered autosave is content the file on disk does not hold, so the
    /// session has to read dirty even though this process made no edit. Bumps
    /// the revision rather than clearing the baseline, so the projected flag and
    /// the render caches agree with every other mutation.
    pub fn mark_unsaved(&mut self) {
        self.revision += 1;
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    /// Every step in this session's history, oldest first, each with whether it
    /// is currently applied. The applied ones are exactly what undo would walk
    /// back through; the rest are what redo would walk forward through.
    ///
    /// Labels borrow their history steps; callers that retain the list after
    /// releasing the engine must project them into their own storage.
    pub fn history_entries(&self) -> Vec<HistoryEntry<'_>> {
        let undo = self.history.undo_labels();
        let redo = self.history.redo_labels();
        let mut out = Vec::with_capacity(undo.len() + redo.len());
        out.extend(undo.map(|label| HistoryEntry {
            label: Cow::Borrowed(label),
            applied: true,
        }));
        out.extend(redo.map(|label| HistoryEntry {
            label: Cow::Borrowed(label),
            applied: false,
        }));
        out
    }

    /// How many steps are currently applied: the index into
    /// [`history_entries`](Self::history_entries) one past the current point.
    pub fn history_depth(&self) -> usize {
        self.history.undo_labels().len()
    }

    /// Undo or redo until exactly `depth` steps are applied, and report whether
    /// that point was reached.
    ///
    /// This is the history panel's one operation: clicking a row asks for the
    /// state after that step, which is a depth, not a direction. A failed
    /// inverse clears history (see [`undo`](Self::undo)), so the walk stops
    /// rather than looping on a step that will not move.
    pub fn history_jump(&mut self, depth: usize) -> bool {
        while self.history_depth() > depth {
            if !self.undo() {
                return false;
            }
        }
        while self.history_depth() < depth {
            if !self.redo() {
                return false;
            }
        }
        true
    }

    /// Group every command applied until [`commit_group`](Self::commit_group)
    /// into one history entry, so a multi-command gesture reverts with one undo.
    pub fn begin_group(&mut self) {
        self.group_started_clean = !self.is_dirty();
        self.history.begin_group();
    }

    /// Close the open group and record it as one undo entry (no-op if empty).
    pub fn commit_group(&mut self) {
        self.history.commit_group();
    }

    /// Close the open group and record it as one undo entry named `label`.
    pub fn commit_group_as(&mut self, label: impl Into<Cow<'static, str>>) {
        self.history.commit_group_as(label.into());
    }

    /// Abort the open group: revert its commands in reverse, restoring the
    /// pre-group state. History is left untouched, and so is the dirty flag: a
    /// group that changed nothing in the end cannot be what makes a saved
    /// session unsaved.
    pub fn rollback_group(&mut self) {
        let inverses = self.history.take_group();
        if inverses.is_empty() {
            return;
        }
        // The rollback mutates the project again after the group's commands
        // already bumped `revision`: bump once more so revision-keyed
        // observers (the preview frame cache) can never confuse mid-group
        // frames with the restored state.
        self.revision += 1;
        for inverse in inverses.into_iter().rev() {
            if self.run_action(inverse).is_err() {
                tracing::error!("history group rollback failed; state may be partial");
                return;
            }
        }
        if self.group_started_clean {
            self.saved_revision = self.revision;
        }
    }

    /// Revert the open group's steps after the first `mark`, newest first,
    /// and keep the group open with the first `mark` steps in it.
    ///
    /// A savepoint: a transaction that fails inside a larger group undoes its
    /// own commands without discarding the ones before it. `mark` comes from
    /// [`group_len`](Self::group_len) taken when the inner work began.
    pub fn rollback_group_to(&mut self, mark: usize) {
        let inverses = self.history.take_group_from(mark);
        if inverses.is_empty() {
            return;
        }
        self.revision += 1;
        for inverse in inverses.into_iter().rev() {
            if self.run_action(inverse).is_err() {
                tracing::error!("history savepoint rollback failed; state may be partial");
                return;
            }
        }
    }

    /// Replace the session with a fresh, unsaved project (File → New).
    ///
    /// The project starts with the persistent main video track (CapCut's
    /// magnetic lane): it is the only lane that exists without clips, and
    /// the timeline UI renders it even when empty.
    pub fn new_session(&mut self) {
        let mut project = Project::new("untitled", Rational::FPS_24);
        project.add_track(TrackKind::Video, "Main");
        self.reset_project(project);
    }

    /// Replace the session with `project` (e.g. the AI-agent sandbox replaying
    /// a validated plan). Clears history and the project path; rebaselines clean.
    pub fn reset_project(&mut self, project: Project) {
        self.project = Arc::new(project);
        self.history.clear();
        self.project_path = None;
        self.previews = LivePreviews::default();
        // Media ids are project-local: the same id can name a different
        // file in the incoming project, so all id-keyed render state is
        // stale (open decoders and still bitmaps, not only proxies).
        self.renderer.reset_media_sources();
        self.revision += 1;
        self.saved_revision = self.revision;
    }

    /// Apply a wire command. On success, pushes the inverse onto the undo stack.
    pub fn apply(&mut self, command: Command) -> Result<ApplyOutcome, EngineError> {
        if let Command::Project(ProjectCommand::Export { path, format }) = &command {
            // What was asked for, resolved against this machine's encoder before
            // anything is created: a codec or bitrate it cannot honour is a
            // refusal, not a file that quietly differs from the request.
            let settings = ExportSettings::resolved(&self.project, *format)?;
            // Export must render full quality from the originals; this
            // renderer doubles as the preview's, so suspend its proxy
            // substitutions for the pass.
            self.renderer.set_use_proxies(false);
            let result = export_to_file_observed(
                &mut self.renderer,
                &self.project,
                path,
                settings,
                &mut |_, _| true,
            );
            self.renderer.set_use_proxies(true);
            return Ok(ApplyOutcome::Exported { frames: result? });
        }

        // The command is consumed by dispatch, so its name is taken first.
        let label = command.label();
        let mut ctx = ApplyContext {
            project: Arc::make_mut(&mut self.project),
            project_path: &mut self.project_path,
            history: &mut self.history,
        };
        let (outcome, inverse) = dispatch(command, &mut ctx)?;
        match outcome {
            ApplyOutcome::Opened | ApplyOutcome::Loaded => {
                // Every session shows the magnetic main lane; files saved
                // without any video track (audio-only cuts, old versions)
                // gain an empty one on entry. Pre-history, not undoable.
                if self.project.timeline().main_track().is_none() {
                    Arc::make_mut(&mut self.project).add_track(TrackKind::Video, "Main");
                }
                // Same media-id hazard as `reset_project`: the incoming
                // file's ids owe nothing to the outgoing registry.
                self.renderer.reset_media_sources();
                // The session now mirrors the file it came from: rebaseline as
                // clean (revision still bumps so observers see a change).
                self.revision += 1;
                self.saved_revision = self.revision;
            }
            ApplyOutcome::Saved => self.saved_revision = self.revision,
            // The media now names a different file: every id-keyed cache
            // (open decoder, still bitmap, proxy) belongs to the old path.
            ApplyOutcome::Relinked { media } => {
                self.renderer.invalidate_media_source(media);
                self.revision += 1;
            }
            ApplyOutcome::Edited(_)
            | ApplyOutcome::RemovedMedia { .. }
            | ApplyOutcome::LabeledMedia { .. }
            | ApplyOutcome::Imported { .. } => self.revision += 1,
            // Unlike Open/Load, a filled template exists nowhere on disk as a
            // project file: bump without rebaselining so the session reads
            // dirty until first saved.
            ApplyOutcome::AppliedTemplate => {
                self.renderer.reset_media_sources();
                self.revision += 1;
            }
            // Writing a `.ariat` (like exporting an MP4) doesn't touch the
            // session project.
            ApplyOutcome::Exported { .. } | ApplyOutcome::SavedTemplate => {}
        }
        if let Some(inverse) = inverse {
            self.history.record_do(label, inverse);
        }
        Ok(outcome)
    }

    /// Set (or clear with `None`) the live gesture transform for one clip.
    ///
    /// While set, preview frames render this transform in place of the clip's
    /// committed one; the project, history, and export are untouched. Release
    /// commits a real `SetClipTransform` and clears the override.
    pub fn set_transform_override(&mut self, value: Option<(ClipId, ClipTransform)>) {
        self.previews.transform = value;
    }

    /// Set (or clear with `None`) the live generator content for one clip —
    /// the inspector-slider analogue of
    /// [`set_transform_override`](Self::set_transform_override).
    pub fn set_generator_override(&mut self, value: Option<(ClipId, Generator)>) {
        self.previews.generator = value;
    }

    /// Set (or clear with `None`) the live look for one clip — the
    /// color-grading analogue of
    /// [`set_transform_override`](Self::set_transform_override).
    pub fn set_look_override(&mut self, value: Option<(ClipId, Option<Filter>, ColorAdjustments)>) {
        self.previews.look = value;
    }

    /// Set (or clear with `None`) the live layer styles for one clip — the
    /// shadow/glow/outline/background analogue of
    /// [`set_transform_override`](Self::set_transform_override).
    pub fn set_styles_override(&mut self, value: Option<(ClipId, LayerStyles)>) {
        self.previews.styles = value;
    }

    /// Set (or clear with `None`) the live crop window and mirror flags for one clip —
    /// the on-canvas crop-drag analogue of
    /// [`set_transform_override`](Self::set_transform_override).
    pub fn set_crop_override(&mut self, value: Option<(ClipId, CropRect, bool, bool)>) {
        self.previews.crop = value;
    }

    /// Set (or clear with `None`) the previewed animation slot for one clip — the
    /// catalog-hover analogue of [`set_transform_override`](Self::set_transform_override).
    ///
    /// The slot is carried rather than the whole trio because the model owns the rule
    /// that a combo replaces both sides; see [`LookAnimations::with_slot`].
    pub fn set_animation_override(
        &mut self,
        value: Option<(ClipId, AnimationSlot, Option<AnimationRef>)>,
    ) {
        self.previews.animation = value;
    }

    /// True while a live preview override is set: frames rendered now show
    /// session-only state that no project revision describes.
    pub fn has_live_overrides(&self) -> bool {
        self.previews.any()
    }

    /// The scene a preview frame would draw at `at`: the project as the
    /// resolver sees it, live overrides included.
    ///
    /// Anything that has to agree with the picture on screen resolves through
    /// this rather than through the committed project, because during a gesture
    /// the two disagree by exactly the gesture: a hit test against the committed
    /// scene picks the layer where the picture used to be.
    pub fn preview_scene(&self, at: RationalTime) -> Result<Scene, aria_models::ModelError> {
        resolve_with(&self.project, at, self.previews.view(&self.project))
    }

    /// Stage timings of the most recent successful preview/export render.
    pub fn last_frame_stats(&self) -> aria_render::FrameStats {
        self.renderer.last_frame_stats()
    }

    /// Decode `media` from `path` (a preview proxy) instead of the pool file.
    pub fn set_media_proxy(&mut self, media: MediaId, path: PathBuf) {
        self.renderer.set_proxy(media, path);
        self.proxy_revision += 1;
    }

    /// Remove `media`'s proxy substitution, returning decode to the pool file.
    pub fn clear_media_proxy(&mut self, media: MediaId) {
        self.renderer.clear_proxy(media);
        self.proxy_revision += 1;
    }

    /// The proxy path registered for `media`, if any.
    pub fn media_proxy(&self, media: MediaId) -> Option<&std::path::Path> {
        self.renderer.proxy_for(media)
    }

    /// Monotonic counter over proxy substitutions.
    ///
    /// Which media decode through a proxy is renderer state, so it changes
    /// without touching the project and cannot move
    /// [`revision`](Self::revision) — that is the dirty flag, and substituting
    /// a proxy does not make a session unsaved. A reader that caches anything
    /// derived from proxy state watches this instead.
    pub fn proxy_revision(&self) -> u64 {
        self.proxy_revision
    }

    /// Tight size (canvas px, at transform scale 1.0) of the content
    /// `generator` draws on the current canvas — what a preview selection box
    /// should hug, since text/path rasters are mostly transparent padding.
    /// Animated params sample at clip-local `tick`. `None` for generators the
    /// compositor doesn't draw. Served from the renderer's raster caches
    /// (`&mut self`: a miss rasterizes once, warming preview too).
    pub fn generator_content_size(
        &mut self,
        generator: &Generator,
        tick: i64,
    ) -> Option<(u32, u32)> {
        let (width, height) = aria_render::canvas_size(&self.project);
        self.renderer
            .generator_content_size(generator, width, height, tick)
    }

    /// Let the next frame this engine renders be abandoned when `cancel` fires.
    ///
    /// The token is spent by that frame (see
    /// [`aria_render::Renderer::cancel_next_frame`]), so it must be set under
    /// the same lock the frame call takes — which is what a scrub does: one
    /// lock, set the token, ask for the frame. A cancelled frame returns
    /// [`RenderError::Cancelled`](aria_render::RenderError::Cancelled) and
    /// releases the lock where the check caught it, instead of paying for the
    /// layers the operator has already scrubbed past.
    pub fn cancel_next_frame(&mut self, cancel: aria_core::Cancel) {
        self.renderer.cancel_next_frame(cancel);
    }

    /// Composite enabled visual layers at `time` into an RGBA preview frame.
    pub fn get_frame(&mut self, time: RationalTime) -> Result<RgbaImage, EngineError> {
        let overrides = self.previews.view(&self.project);
        Ok(self
            .renderer
            .render_frame_with(&self.project, time, overrides)?)
    }

    /// [`get_frame`](Self::get_frame) scaled to fit within
    /// `max_width`×`max_height` (aspect preserved, never upscaled) — what an
    /// interactive preview should request so scrubbing pays for view-sized
    /// pixels instead of the full canvas.
    pub fn get_frame_fit(
        &mut self,
        time: RationalTime,
        max_width: u32,
        max_height: u32,
    ) -> Result<RgbaImage, EngineError> {
        let overrides = self.previews.view(&self.project);
        Ok(self.renderer.render_frame_fit_with(
            &self.project,
            time,
            max_width,
            max_height,
            overrides,
        )?)
    }

    /// Partitioned gesture frames for zero-drift preview transform drags.
    /// See [`aria_render::Renderer::render_gesture_frames`].
    pub fn get_gesture_frames(
        &mut self,
        time: RationalTime,
        clip_id: ClipId,
        max_width: u32,
        max_height: u32,
    ) -> Result<Option<aria_render::GestureFrames>, EngineError> {
        Ok(self.renderer.render_gesture_frames(
            &self.project,
            time,
            clip_id,
            max_width,
            max_height,
        )?)
    }

    /// [`get_frame`](Self::get_frame) writing composited rows directly into `sink`.
    pub fn get_frame_into(
        &mut self,
        time: RationalTime,
        policy: SeekPolicy,
        sink: &mut dyn FrameSink,
    ) -> Result<(), EngineError> {
        let overrides = self.previews.view(&self.project);
        Ok(self
            .renderer
            .render_frame_into_with(&self.project, time, overrides, policy, sink)?)
    }

    /// [`get_frame_fit`](Self::get_frame_fit) writing composited rows directly into `sink`.
    pub fn get_frame_fit_into(
        &mut self,
        time: RationalTime,
        max_width: u32,
        max_height: u32,
        policy: SeekPolicy,
        sink: &mut dyn FrameSink,
    ) -> Result<(), EngineError> {
        let overrides = self.previews.view(&self.project);
        Ok(self.renderer.render_frame_fit_into_with(
            &self.project,
            time,
            max_width,
            max_height,
            overrides,
            policy,
            sink,
        )?)
    }

    pub fn undo(&mut self) -> bool {
        debug_assert!(
            !self.history.in_group(),
            "undo inside an open history group"
        );
        let Some(step) = self.history.pop_undo() else {
            return false;
        };
        let label = step.label;
        match self.run_action(step.action) {
            Ok(inverse) => {
                self.history.push_redo(HistoryStep {
                    label,
                    action: inverse,
                });
                self.revision += 1;
                true
            }
            // Inverses are meant to be infallible once recorded. A failure
            // here means the project no longer matches what history expects,
            // so the remaining stacks can't be trusted either — clearing both
            // is safer than silently dropping one entry and marching on (which
            // is what once turned an id collision into permanent data loss).
            Err(e) => {
                tracing::error!("undo failed ({e}); clearing history to avoid corruption");
                self.history.clear();
                self.revision += 1;
                false
            }
        }
    }

    pub fn redo(&mut self) -> bool {
        debug_assert!(
            !self.history.in_group(),
            "redo inside an open history group"
        );
        let Some(step) = self.history.pop_redo() else {
            return false;
        };
        let label = step.label;
        match self.run_action(step.action) {
            Ok(inverse) => {
                self.history.push_undo(HistoryStep {
                    label,
                    action: inverse,
                });
                self.revision += 1;
                true
            }
            // See `undo`: a failed redo means history and project have
            // diverged, so drop both stacks rather than corrupt further.
            Err(e) => {
                tracing::error!("redo failed ({e}); clearing history to avoid corruption");
                self.history.clear();
                self.revision += 1;
                false
            }
        }
    }

    fn run_action(
        &mut self,
        action: Box<dyn EditAction>,
    ) -> Result<Box<dyn EditAction>, EngineError> {
        let mut ctx = ApplyContext {
            project: Arc::make_mut(&mut self.project),
            project_path: &mut self.project_path,
            history: &mut self.history,
        };
        action.apply(&mut ctx)
    }
}

// The mobile FFI hands a session across threads (calls serialized by the shell,
// e.g. a Swift actor whose executor hops threads), which is only sound if the
// engine is `Send`. Assert it at compile time so a non-Send component (a decoder
// handle, a GPU resource) can never sneak in silently.
const _: () = {
    const fn assert_send<T: Send>() {}
    assert_send::<Engine>()
};

#[cfg(test)]
mod tests {
    use super::*;

    /// An inverse that always fails, simulating a corrupted/mismatched history
    /// entry.
    struct FailAction;

    impl EditAction for FailAction {
        fn apply(
            self: Box<Self>,
            _ctx: &mut ApplyContext<'_>,
        ) -> Result<Box<dyn EditAction>, EngineError> {
            Err(aria_models::ModelError::InvalidRange.into())
        }
    }

    struct NoopAction;

    impl EditAction for NoopAction {
        fn apply(
            self: Box<Self>,
            _ctx: &mut ApplyContext<'_>,
        ) -> Result<Box<dyn EditAction>, EngineError> {
            Ok(Box::new(NoopAction))
        }
    }

    fn step(action: Box<dyn EditAction>) -> HistoryStep {
        HistoryStep {
            label: std::borrow::Cow::Borrowed("Test step"),
            action,
        }
    }

    #[test]
    fn failed_undo_clears_both_stacks() {
        let mut engine = Engine::new(EngineConfig::default()).expect("engine");
        // A redo entry that WOULD apply, plus a failing undo entry on top.
        engine.history.push_redo(step(Box::new(NoopAction)));
        engine.history.push_undo(step(Box::new(FailAction)));
        assert!(engine.can_undo() && engine.can_redo());

        assert!(!engine.undo(), "a failing inverse reports no step");
        assert!(
            !engine.can_undo() && !engine.can_redo(),
            "a diverged history is dropped wholesale rather than trusted"
        );
    }

    #[test]
    fn failed_redo_clears_both_stacks() {
        let mut engine = Engine::new(EngineConfig::default()).expect("engine");
        engine.history.push_undo(step(Box::new(NoopAction)));
        engine.history.push_redo(step(Box::new(FailAction)));
        assert!(engine.can_undo() && engine.can_redo());

        assert!(!engine.redo());
        assert!(!engine.can_undo() && !engine.can_redo());
    }

    #[test]
    fn history_entries_borrow_static_and_grouped_labels() {
        let mut engine = Engine::new(EngineConfig::default()).expect("engine");
        engine.history.push_undo(step(Box::new(NoopAction)));
        engine.history.begin_group();
        engine
            .history
            .record_do("Grouped step", Box::new(NoopAction));
        engine
            .history
            .record_do("Second step", Box::new(NoopAction));
        engine.history.commit_group();

        let stored: Vec<(*const u8, usize)> = engine
            .history
            .undo_labels()
            .map(|label| (label.as_ptr(), label.len()))
            .collect();
        let entries = engine.history_entries();
        assert_eq!(entries.len(), stored.len());
        for (entry, (ptr, len)) in entries.iter().zip(stored) {
            assert!(
                matches!(entry.label, Cow::Borrowed(_)),
                "history inspection allocated an owned label"
            );
            assert_eq!((entry.label.as_ptr(), entry.label.len()), (ptr, len));
        }
    }

    #[test]
    fn grouped_rollback_bumps_revision_once_for_every_group_size() {
        for action_count in [1, 2, 8] {
            let mut engine = Engine::new(EngineConfig::default()).expect("engine");
            engine.begin_group();
            for _ in 0..action_count {
                engine.history.record_do("Test step", Box::new(NoopAction));
            }
            let before = engine.revision();

            engine.rollback_group();

            assert_eq!(
                engine.revision(),
                before + 1,
                "{action_count} reverted actions must publish one restored state"
            );
        }
    }

    #[test]
    fn a_jump_that_cannot_reach_its_depth_reports_failure() {
        let mut engine = Engine::new(EngineConfig::default()).expect("engine");
        engine.history.push_undo(step(Box::new(NoopAction)));
        // Nothing to redo, so a depth past the end is unreachable and the walk
        // stops instead of spinning.
        assert!(!engine.history_jump(4));
        assert_eq!(engine.history_depth(), 1);
        assert!(engine.history_jump(0), "walking back is reachable");
        assert_eq!(engine.history_depth(), 0);
        assert_eq!(
            engine
                .history_entries()
                .into_iter()
                .map(|entry| (entry.label, entry.applied))
                .collect::<Vec<_>>(),
            vec![(Cow::Borrowed("Test step"), false)],
            "an undone step stays in the list, marked unapplied"
        );
    }
}
