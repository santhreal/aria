//! Following a mask across frames instead of keyframing it by hand.
//!
//! A mask is drawn and dragged on the preview, and its centre keyframes through
//! the property catalog, so animating one over a moving subject was possible and
//! meant placing a keyframe on every frame. This walks the range once, correlates
//! the patch under the mask from each frame to the next
//! ([`aria_compositor::track_patch`]), and writes the centre keyframes the
//! operator would have placed.
//!
//! Three rules the pass exists to keep:
//!
//! - **It stops when it is lost.** Below the confidence floor the pass ends and
//!   reports the tick, because keyframes written past a lost feature look
//!   deliberate and are wrong.
//! - **It is one gesture.** Every keyframe lands inside one history group, so a
//!   track that went wrong is one Ctrl+Z, not one per frame.
//! - **It never holds the window.** One frame per tick is a decode and a
//!   composite per tick, which is minutes over a long shot, so the shipped
//!   surface runs the walk as a job. [`walk_mask`] is a free function that takes
//!   pictures, a cancel question and a progress sink and returns keyframes: it
//!   touches no session, so the job thread can run it and the window can write
//!   what it returns.
//!
//! Frames come through [`MaskFrames`] rather than straight off the engine: the
//! pass is then assertible without a GPU, the job reads them from a renderer of
//! its own ([`RenderedFrames`]), and the synchronous entry point still reads them
//! from the same preview path the canvas draws ([`EngineFrames`]).

use std::sync::atomic::{AtomicBool, Ordering};

use aria_commands::{Command, EditCommand};
use aria_core::RgbaImage;
use aria_core::count::counted;
use aria_engine::EngineError;
use aria_jobs::JobCompletion;
use aria_models::{ClipId, ClipParam, Easing, Mask, MaskParam, ParamValue, Project, RationalTime};
use gpui::Context;

use super::AppState;

/// The confidence below which a track has lost its feature.
///
/// Normalized cross-correlation of the winning position: a clean lock on real
/// footage sits well above this, a featureless or occluded patch below it. Held
/// here as one number because it is the pass's honesty threshold, not a taste
/// knob.
pub(crate) const CONFIDENCE_FLOOR: f32 = 0.5;

/// How far, in pixels, a feature may move between two frames and still be found.
///
/// Bounds the search: the cost is `(2r + 1)^2` correlations per frame, and a
/// window wide enough for any motion is also wide enough to lock onto the wrong
/// thing.
pub(crate) const SEARCH_RADIUS: i32 = 24;

/// What a tracking pass did.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MaskTrackReport {
    /// Centre keyframes written, including the one at the starting tick.
    pub written: usize,
    /// The tick where confidence fell below [`CONFIDENCE_FLOOR`], if it did.
    /// Keyframes before it stand; nothing was written at or after it.
    pub lost_at: Option<i64>,
    /// Whether the caller cancelled the pass before it reached the end.
    pub cancelled: bool,
    /// The weakest score any accepted frame scored: how close the pass came to
    /// giving up, which is the signal to re-draw the mask.
    pub weakest: f32,
}

impl MaskTrackReport {
    /// The report of a pass that had nothing to track.
    ///
    /// `weakest` is 1.0 because it is a minimum over accepted frames, and a pass
    /// that accepted none never came close to giving up.
    pub const fn nothing() -> Self {
        Self {
            written: 0,
            lost_at: None,
            cancelled: false,
            weakest: 1.0,
        }
    }
}

/// Where the pass reads pictures.
///
/// One frame at a time, at the canvas size the compositor draws, so the patch
/// arithmetic below is in the same pixels as the mask geometry it writes.
pub trait MaskFrames {
    /// The composited frame at `at`, or `None` when there is none to read.
    fn frame(&mut self, at: RationalTime) -> Option<RgbaImage>;
}

/// The synchronous source: the session's own preview renderer.
pub(crate) struct EngineFrames<'a> {
    engine: &'a std::sync::Arc<parking_lot::Mutex<aria_engine::Engine>>,
}

impl MaskFrames for EngineFrames<'_> {
    fn frame(&mut self, at: RationalTime) -> Option<RgbaImage> {
        // One lock per frame, not one per pass: a tracking pass is not a frame
        // path, and holding the session lock across a whole range would park the
        // window for the length of the track.
        self.engine.lock().get_frame(at).ok()
    }
}

/// The job's source: a renderer of its own over a copy of the project.
///
/// The preview's renderer lives on the UI thread behind the session lock, so a
/// whole-shot walk through it would park the window even sliced. This is the
/// device every background render already shares
/// ([`super::render_device::renderer_on`]) with decoders and proxy state of its
/// own, which is what an export, a proxy build and a PNG sequence each use.
struct RenderedFrames {
    renderer: aria_render::Renderer,
    project: std::sync::Arc<Project>,
}

impl MaskFrames for RenderedFrames {
    fn frame(&mut self, at: RationalTime) -> Option<RgbaImage> {
        // The full canvas, like the preview: the quad the walk correlates in is
        // in canvas pixels, so a fitted frame would measure the wrong scale.
        self.renderer.render_frame(&self.project, at).ok()
    }
}

/// The geometry and range of one pass: what the walk needs, and nothing that
/// names a session.
///
/// Deliberately holds no clip id and no engine handle. The walk runs on a job
/// thread and the keyframes are written by the window, and a walk that could
/// name the engine would be a walk somebody could run on the wrong thread.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct MaskWalk {
    /// The tick the mask sits at now, and the first keyframe written.
    pub start: RationalTime,
    /// Ticks forward from `start`, at the timeline rate.
    pub span: i64,
    /// The layer quad's centre in canvas pixels: what frames are measured in.
    pub quad_center: [f32; 2],
    /// The layer quad's size in canvas pixels.
    pub quad_size: [f32; 2],
    /// The patch to correlate, in canvas pixels.
    pub patch: [f32; 2],
    /// Where the mask sits at `start`, in fractions of the layer.
    pub center: [f32; 2],
    /// The score a frame must reach to be accepted.
    pub floor: f32,
}

/// The mask a track writes to: a clip, and the mask as it stood when the track
/// started.
///
/// The stack can change while a job runs: removing or moving a mask above the
/// tracked one shifts its index. [`MaskTarget::locate`] finds the mask again when
/// the keyframes come back.
#[derive(Debug, Clone, PartialEq)]
struct MaskTarget {
    clip: ClipId,
    /// The mask's index in the stack when the track started.
    index: u32,
    /// The mask when the track started.
    start: Mask,
}

impl MaskTarget {
    /// The index of the tracked mask in `masks`, or `None` when no mask, or
    /// more than one, matches the mask the track started from.
    ///
    /// A mask matches when its kind, path, and centre are unchanged. Those place
    /// the mask; an edit to its feather, mode, or opacity while the track runs
    /// leaves them alone. The start index is tried first, so an unchanged stack
    /// resolves to it even when another mask in the stack is identical.
    fn locate(&self, masks: &[Mask]) -> Option<u32> {
        let same = |mask: &Mask| {
            mask.kind == self.start.kind
                && mask.path == self.start.path
                && mask.path_closed == self.start.path_closed
                && mask.center == self.start.center
        };
        if masks.get(self.index as usize).is_some_and(same) {
            return Some(self.index);
        }
        let mut found = masks
            .iter()
            .enumerate()
            .filter(|&(_, mask)| same(mask))
            .map(|(index, _)| index);
        match (found.next(), found.next()) {
            (Some(index), None) => u32::try_from(index).ok(),
            _ => None,
        }
    }
}

/// Walk `frames` forward and return the centre keyframes the track earned.
///
/// Writes nothing. Keyframes come back as a list so the caller decides where
/// they land, which is what lets the shipped pass run off the UI thread and the
/// window apply the result as one history entry.
///
/// `cancel` is asked once per frame and `progress` is told `(done, total)` per
/// accepted frame, so a job draws a determinate bar and stops mid-pass without
/// this function knowing what a job is.
pub(crate) fn walk_mask(
    frames: &mut dyn MaskFrames,
    walk: &MaskWalk,
    cancel: &dyn Fn() -> bool,
    progress: &mut dyn FnMut(i64, i64),
) -> (Vec<(RationalTime, [f32; 2])>, MaskTrackReport) {
    let mut report = MaskTrackReport::nothing();
    let mut keys = Vec::new();
    // No first frame is no track: reporting nothing written is honest, and an
    // operator sees the mask unchanged.
    let Some(mut previous) = frames.frame(walk.start) else {
        return (keys, report);
    };
    let mut center = walk.center;
    keys.push((walk.start, center));
    report.written = 1;
    progress(0, walk.span);

    for step in 1..=walk.span {
        if cancel() {
            report.cancelled = true;
            break;
        }
        let at = RationalTime::new(walk.start.value + step, walk.start.rate);
        let Some(next) = frames.frame(at) else {
            break;
        };
        // The patch sits where the mask sits now, which moves as the track
        // advances: correlating against the original position would lose the
        // feature the moment it left the first window.
        let patch = aria_compositor::TrackPatch::centered(
            walk.quad_center[0] + center[0] * walk.quad_size[0],
            walk.quad_center[1] + center[1] * walk.quad_size[1],
            walk.patch[0],
            walk.patch[1],
        );
        let Some(found) = aria_compositor::track_patch(&previous, &next, patch, SEARCH_RADIUS)
        else {
            break;
        };
        if found.score < walk.floor {
            report.lost_at = Some(at.value);
            break;
        }
        report.weakest = report.weakest.min(found.score);
        center[0] += found.dx as f32 / walk.quad_size[0];
        center[1] += found.dy as f32 / walk.quad_size[1];
        keys.push((at, center));
        report.written += 1;
        progress(step, walk.span);
        previous = next;
    }
    (keys, report)
}

impl AppState {
    /// Track the selected clip's mask forward `span` ticks from the playhead,
    /// here and now, on this thread.
    ///
    /// Reads frames from the session's renderer, which needs a GPU. The shipped
    /// control is [`track_selected_mask_job`](Self::track_selected_mask_job);
    /// this is the synchronous form, for a caller that already owns the wait.
    pub fn track_selected_mask(
        &mut self,
        span: i64,
        cancel: &AtomicBool,
    ) -> Result<MaskTrackReport, EngineError> {
        let engine = std::sync::Arc::clone(self.engine());
        let mut frames = EngineFrames { engine: &engine };
        self.track_selected_mask_with(&mut frames, span, cancel, CONFIDENCE_FLOOR)
    }

    /// The synchronous pass, reading from `frames`.
    ///
    /// `span` is ticks forward from the playhead at the timeline rate. Reports an
    /// empty pass when there is nothing to track: no selection, no mask, no drawn
    /// quad, or a span of zero is a no-op, not an error.
    pub fn track_selected_mask_with(
        &mut self,
        frames: &mut dyn MaskFrames,
        span: i64,
        cancel: &AtomicBool,
        floor: f32,
    ) -> Result<MaskTrackReport, EngineError> {
        let Ok((target, walk)) = self.mask_walk(span, floor) else {
            return Ok(MaskTrackReport::nothing());
        };
        let (keys, report) = walk_mask(
            frames,
            &walk,
            &|| cancel.load(Ordering::Relaxed),
            &mut |_, _| {},
        );
        self.write_track(target.clip, target.index, &keys)?;
        Ok(report)
    }

    /// Track the selected mask to the end of its clip, as a job.
    ///
    /// The span is the rest of the clip: a track runs until the feature is lost,
    /// Stop is pressed, or the shot ends, and asking for a length would mean
    /// guessing how long the shot holds.
    ///
    /// The walk happens on the job's thread over a copy of the project, so the
    /// window keeps drawing and stays scrubbable while it runs. The keyframes
    /// come back to the window, which writes them as one history entry — the
    /// only thread that edits the session is still the one that draws it.
    pub fn track_selected_mask_job(&mut self, cx: &mut Context<Self>) -> Result<u64, String> {
        let (target, walk) = self
            .mask_walk(self.mask_track_span(), CONFIDENCE_FLOOR)
            .map_err(str::to_string)?;
        let name = self
            .selected_ui_clip()
            .map(|ui| ui.name)
            .unwrap_or_else(|| "the clip".into());
        let project = self.host().snapshot();
        let gpu = super::render_device::shared_device(&self.ctx);
        let (wake, wakes) = crate::handoff::wakes::<
            Result<(Vec<(RationalTime, [f32; 2])>, MaskTrackReport), String>,
        >();
        let id = self.spawn_job(format!("Tracking the mask on {name}"), move |context| {
            let renderer = match super::render_device::renderer_on(gpu, "the mask track") {
                Ok(renderer) => renderer,
                // Both surfaces get it: the job row shows the refusal on the
                // board, and the window shows it beside the pressed control.
                Err(err) => {
                    wake.wake(Err(err.clone()));
                    return Err(err);
                }
            };
            let mut frames = RenderedFrames { renderer, project };
            let (keys, report) = walk_mask(
                &mut frames,
                &walk,
                &|| context.cancelled(),
                &mut |done, total| {
                    let fraction = if total == 0 {
                        1.0
                    } else {
                        done as f32 / total as f32
                    };
                    context.set_progress(fraction, format!("frame {done}/{total}"));
                },
            );
            // Published even when it was stopped: a cancelled track keeps the
            // frames it already followed, which is what stopping it half way
            // requests. No commit guard for that reason
            // — the publication is a history entry, one keystroke from gone,
            // not a file on disk.
            let written = report.written;
            wake.wake(Ok((keys, report)));
            let keyframes = counted(written, "keyframe", "keyframes");
            let detail = match (report.lost_at, report.cancelled) {
                (Some(tick), _) => format!("{keyframes}, lost the feature at {tick}"),
                (None, true) => format!("{keyframes} before it was stopped"),
                (None, false) => keyframes.to_string(),
            };
            JobCompletion::new(detail)
                .with_output("keyframes", written.to_string())
                .map_err(String::from)
        });
        cx.spawn(async move |this, cx| {
            let outcome = wakes.next_from_a_thread(cx.background_executor()).await;
            let _ = this.update(cx, |state, cx| {
                match outcome {
                    Some(Ok((keys, report))) => state.adopt_track(&target, &keys, report),
                    Some(Err(reason)) => state.report_error(reason),
                    None => state.report_error("The mask track ended without reporting"),
                }
                cx.notify();
            });
        })
        .detach();
        self.status_message = Some(format!("Tracking the mask on {name}…"));
        Ok(id.raw())
    }

    /// Start a track, and show its refusal in the status line.
    ///
    /// The wrapper every surface spends: a palette row and a panel button both
    /// want "start it, and if it will not start, say so", and two copies of that
    /// are two chances for one of them to swallow the reason.
    pub(crate) fn run_mask_track(&mut self, cx: &mut Context<Self>) {
        if let Err(err) = self.track_selected_mask_job(cx) {
            self.report_error(err);
        }
    }

    /// Ticks from the playhead to the last frame of the selected clip.
    ///
    /// Zero when the selection ends at the playhead, which the refusal below
    /// turns into words. The clip's end tick is exclusive, so the last frame a
    /// track can read is one before it.
    fn mask_track_span(&self) -> i64 {
        self.selected_ui_clip()
            .map(|ui| ui.timeline_end_ticks - 1 - self.playhead.value)
            .unwrap_or(0)
            .max(0)
    }

    /// What a pass would track, or the reason there is nothing to.
    ///
    /// One owner for the questions a track has to have answered, because the
    /// synchronous pass reports "nothing to track" as an empty report and the job
    /// reports it in words. Two copies of the questions are two
    /// chances for the two surfaces to disagree about what a trackable mask is.
    fn mask_walk(&self, span: i64, floor: f32) -> Result<(MaskTarget, MaskWalk), &'static str> {
        let clip = self
            .selected_clip()
            .ok_or("Select a clip with a mask to track")?;
        let (index, mask) = self
            .active_mask()
            .zip(self.active_ui_mask())
            .ok_or("Draw a mask on the clip before tracking it")?;
        let start = self
            .active_model_mask()
            .cloned()
            .ok_or("Draw a mask on the clip before tracking it")?;
        let (quad_center, quad_size) = self
            .selected_layer_quad()
            .ok_or("The selected clip shows no picture at the playhead to track from")?;
        if quad_size[0] <= 0.0 || quad_size[1] <= 0.0 {
            return Err("The selected clip has no size at the playhead to track across");
        }
        if span <= 0 {
            return Err(
                "The playhead is at the end of the clip, so there is nothing ahead to track",
            );
        }
        Ok((
            MaskTarget { clip, index, start },
            MaskWalk {
                start: self.playhead,
                span,
                quad_center,
                quad_size,
                // The mask's centre is an offset from the layer centre in
                // fractions of layer size, so the patch it names is that offset
                // scaled back into canvas pixels — the units the frames are in.
                patch: [
                    (mask.size[0] * quad_size[0]).clamp(8.0, quad_size[0]),
                    (mask.size[1] * quad_size[1]).clamp(8.0, quad_size[1]),
                ],
                center: mask.center,
                floor,
            },
        ))
    }

    /// Adopt a finished track: write what it followed onto the mask it
    /// followed, and say what it did.
    ///
    /// The mask is found again by [`MaskTarget::locate`], so a mask removed or
    /// moved above it while the job ran does not send the keyframes to its
    /// neighbour. A tracked mask that is gone or reshaped gets nothing, and the
    /// window reports why.
    fn adopt_track(
        &mut self,
        target: &MaskTarget,
        keys: &[(RationalTime, [f32; 2])],
        report: MaskTrackReport,
    ) {
        let found = self
            .engine()
            .lock()
            .project()
            .clip(target.clip)
            .and_then(|clip| target.locate(&clip.masks));
        let Some(index) = found else {
            self.report_error(
                "The tracked mask was removed or reshaped while the track ran, \
                 so no keyframes were written. Track it again.",
            );
            return;
        };
        if let Err(err) = self.write_track(target.clip, index, keys) {
            self.report_error(&err);
            return;
        }
        let written = report.written;
        self.status_message = Some(match (report.lost_at, report.cancelled) {
            (Some(_), _) => {
                format!(
                    "Tracked {} and stopped where the feature was lost",
                    counted(written, "frame", "frames")
                )
            }
            (None, true) => format!(
                "Stopped the track after {}",
                counted(written, "frame", "frames")
            ),
            (None, false) => format!("Tracked {}", counted(written, "frame", "frames")),
        });
    }

    /// Write `keys` as the centre of mask `mask` of `clip`, as one history
    /// entry.
    ///
    /// One gesture, one undo entry: a track that went wrong reverts in a
    /// keystroke instead of one per frame. A pass that followed nothing opens no
    /// group, so an empty track leaves the history alone.
    fn write_track(
        &mut self,
        clip: ClipId,
        mask: u32,
        keys: &[(RationalTime, [f32; 2])],
    ) -> Result<(), EngineError> {
        if keys.is_empty() {
            return Ok(());
        }
        let param = ClipParam::Mask {
            mask,
            param: MaskParam::Center,
        };
        let group = self.host().group(&aria_plugin::Origin::Editor)?;
        let outcome = keys
            .iter()
            .try_for_each(|(at, center)| self.write_center(clip, param, *at, *center));
        // The keys written before a failure are a usable track up to that frame,
        // so they are kept as the one entry.
        group.commit();
        // The commit changes the History list and not the project.
        self.sync_from_engine();
        outcome
    }

    fn write_center(
        &mut self,
        clip: ClipId,
        param: ClipParam,
        at: RationalTime,
        center: [f32; 2],
    ) -> Result<(), EngineError> {
        self.apply_command(Command::Edit(EditCommand::SetParamKeyframe {
            clip,
            param,
            at,
            value: ParamValue::Vec2(center),
            easing: Easing::Linear,
            tangents: None,
        }))?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "mask_track_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "mask_track_job_tests.rs"]
mod job;
