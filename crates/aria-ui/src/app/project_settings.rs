//! Project-level settings and pool maintenance: the canvas, and removing a pool
//! item.
//!
//! `SetCanvas` and `RemoveMedia` had no operator path, so a project's shape was
//! fixed at creation and the pool only ever grew. Both read back through the
//! engine rather than a cached copy, so the dialog cannot drift from the project.

use aria_commands::{Command, EditCommand, ProjectCommand};
use aria_engine::EngineError;
use aria_models::{AudioLayout, CanvasAspect, CanvasSettings, MediaId, Rational, TimeDisplay};

/// The delivery sizes the Canvas section offers, as the shorter edge in pixels.
/// A baseline is what "1080p" names in both framings, so one table serves a wide
/// and a vertical project without a second list per aspect.
pub(crate) const CANVAS_BASELINES: [u32; 5] = [720, 1080, 1440, 2160, 4320];

use super::AppState;

impl AppState {
    /// Set the canvas aspect, keeping the background. `SetCanvas` carries every
    /// field, so the untouched ones are resent from the project rather than
    /// defaulted — otherwise picking an aspect would reset the background.
    ///
    /// An explicit size is reshaped, not dropped: the delivery baseline the
    /// operator chose survives a change of shape. Picking `Auto` clears it,
    /// because following the footage is exactly what `Auto` means, and so does a
    /// reshape that would leave the canvas unencodable (a very wide preset at a
    /// very tall baseline) — losing the size beats refusing the aspect.
    pub fn set_canvas_aspect(&mut self, aspect: CanvasAspect) -> Result<(), EngineError> {
        let current = self.canvas();
        if current.aspect == aspect {
            return Ok(());
        }
        let resolution = current
            .resolution
            .and_then(|[w, h]| aspect.pixels_for_baseline(w.min(h)))
            .filter(|size| CanvasSettings::resolution_is_valid(*size));
        self.apply_command(Command::Edit(EditCommand::SetCanvas {
            aspect,
            background: current.background,
            resolution,
        }))?;
        Ok(())
    }

    /// Set the canvas background, keeping the aspect.
    pub fn set_canvas_background(&mut self, background: [u8; 3]) -> Result<(), EngineError> {
        let current = self.canvas();
        if current.background == background {
            return Ok(());
        }
        self.apply_command(Command::Edit(EditCommand::SetCanvas {
            aspect: current.aspect,
            background,
            resolution: current.resolution,
        }))?;
        Ok(())
    }

    /// Set an explicit canvas size, or `None` to derive it again from the aspect
    /// or the footage. The engine refuses a size it cannot encode, and the
    /// refusal is surfaced rather than clamped here: a delivery size is either
    /// the intended one or a mistake to report.
    pub fn set_canvas_resolution(
        &mut self,
        resolution: Option<[u32; 2]>,
    ) -> Result<(), EngineError> {
        let current = self.canvas();
        if current.resolution == resolution {
            return Ok(());
        }
        self.apply_command(Command::Edit(EditCommand::SetCanvas {
            aspect: current.aspect,
            background: current.background,
            resolution,
        }))?;
        Ok(())
    }

    /// Set the sequence frame rate. The model resamples every clip's timeline
    /// placement, fade envelopes, transitions, marker positions, and keyframes.
    pub fn set_sequence_rate(&mut self, frame_rate: Rational) -> Result<(), EngineError> {
        if aria_core::rate_eq(self.store.sequence.frame_rate, frame_rate) {
            return Ok(());
        }
        self.apply_command(Command::Edit(EditCommand::SetSequenceRate { frame_rate }))?;
        Ok(())
    }

    /// Set the sequence audio channel layout (mono, stereo, 5.1).
    pub fn set_audio_layout(&mut self, layout: AudioLayout) -> Result<(), EngineError> {
        if self.audio_layout() == layout {
            return Ok(());
        }
        self.apply_command(Command::Edit(EditCommand::SetAudioLayout { layout }))?;
        Ok(())
    }

    /// Set how the ruler, the timeline timecode and the Viewer's clock count
    /// time. Picking the display already in use records no undo step.
    pub fn set_time_display(&mut self, display: TimeDisplay) -> Result<(), EngineError> {
        if self.store.sequence.time_display == display {
            return Ok(());
        }
        self.apply_command(Command::Edit(EditCommand::SetTimeDisplay { display }))?;
        Ok(())
    }

    /// The offered delivery sizes as `(baseline, [w, h])`, in the current canvas
    /// shape. A fixed aspect scales its own ratio; `Auto` scales the shape the
    /// footage already produced, so choosing a delivery size never silently
    /// reshapes the frame. A baseline whose long edge would exceed what the
    /// encoder takes is not offered, because an offered choice that is refused on
    /// click is worse than an absent one.
    pub fn canvas_resolution_choices(&self) -> Vec<(u32, [u32; 2])> {
        let aspect = self.canvas().aspect;
        // The shape to scale: with an explicit size already set, `canvas_size`
        // would report that size, so the *derived* shape is asked for directly.
        let shape = aspect
            .pixels_for_baseline(1080)
            .unwrap_or_else(|| self.derived_canvas_shape());
        CANVAS_BASELINES
            .into_iter()
            .map(|baseline| (baseline, scale_to_baseline(shape, baseline)))
            .filter(|(_, size)| CanvasSettings::resolution_is_valid(*size))
            .collect()
    }

    /// Drop a pool item. The model refuses while any clip references it; the
    /// refusal is surfaced rather than swallowed, because "Remove did nothing" is
    /// worse than an explained failure.
    pub fn remove_media(&mut self, media: MediaId) -> Result<(), EngineError> {
        self.apply_command(Command::Project(ProjectCommand::RemoveMedia { media }))?;
        Ok(())
    }

    /// Select every clip that references `media`, and say how many there were.
    ///
    /// The other half of a refused removal: the model will not drop a pool item
    /// the timeline still holds, and being told so is only useful if the clips
    /// holding it can be reached. Reads the projection rather than the engine —
    /// the selection is projected ids, and the lock is not worth taking twice.
    pub fn select_media_clips(&mut self, media: MediaId) -> usize {
        let raw = media.raw();
        self.selection = self
            .store
            .sequence
            .tracks
            .iter()
            .flat_map(|track| track.clips.iter())
            .filter(|clip| clip.media_id == Some(raw))
            .map(|clip| aria_models::ClipId::from_raw(clip.id))
            .collect();
        let found = self.selection.len();
        self.status_message = Some(match found {
            0 => "No clip uses that item".into(),
            1 => "Selected the clip using that item".into(),
            n => format!("Selected the {n} clips using that item"),
        });
        self.rebuild_store();
        found
    }
}

/// `shape` rescaled so its shorter edge is `baseline`, keeping the ratio and
/// both edges even. The result can exceed the encodable bounds for an extreme
/// ratio; callers filter with [`CanvasSettings::resolution_is_valid`] rather than
/// clamping, because a clamped edge would quietly change the shape. Kept out of
/// `AppState` so the arithmetic is testable without a project.
pub(crate) fn scale_to_baseline([w, h]: [u32; 2], baseline: u32) -> [u32; 2] {
    let short = w.min(h).max(1);
    let factor = baseline as f64 / short as f64;
    let scale = |v: u32| ((v as f64 * factor).round() as u32) & !1;
    if w <= h {
        [baseline, scale(h)]
    } else {
        [scale(w), baseline]
    }
}
