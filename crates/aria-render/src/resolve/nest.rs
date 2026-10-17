//! A nested sequence clip: the sequence's own frame, resolved apart from the
//! timeline it is placed on and placed there as one picture.
//!
//! The sequence resolves on its own canvas, every visible lane of it, with its
//! own transitions, mattes, references and drivers, over a transparent
//! background, and that canvas fits inside one the size of the outer canvas.
//! The clip then places the sequence's frame as it places media: fitted into
//! the canvas, with its transform, crop, look, mask, chroma key, styles and
//! blend mode.

use aria_models::{ModelError, SequenceId};

use super::*;

/// The sequences a resolve is inside, innermost first: the timeline being
/// resolved, then each timeline that shows it.
#[derive(Debug, Clone, Copy)]
pub(super) struct Nesting<'a> {
    /// The sequence being resolved; `None` for the project's root timeline.
    sequence: Option<SequenceId>,
    outer: Option<&'a Nesting<'a>>,
}

impl<'a> Nesting<'a> {
    /// The open timeline, inside nothing.
    pub(super) fn root(project: &Project) -> Self {
        Self {
            sequence: project.active_sequence(),
            outer: None,
        }
    }

    /// Whether `sequence` is being resolved here or by a timeline showing
    /// this one.
    fn contains(&self, sequence: SequenceId) -> bool {
        self.sequence == Some(sequence) || self.outer.is_some_and(|outer| outer.contains(sequence))
    }

    /// `sequence` resolved inside this one.
    fn inside(&'a self, sequence: SequenceId) -> Self {
        Self {
            sequence: Some(sequence),
            outer: Some(self),
        }
    }
}

/// A nested sequence's frame, ready to place: its scene, where its own frame
/// sits on that scene's canvas, and that frame's size in canvas pixels.
pub(super) struct NestFrame {
    pub(super) scene: Scene,
    /// `[x0, y0, x1, y1]` as fractions of the scene's canvas.
    pub(super) frame: [f32; 4],
    pub(super) size: [f32; 2],
}

/// The frame of `sequence` at `source_time`, its own clock, for a clip placed
/// on a `cw`×`ch` canvas.
///
/// The sequence resolves on its own canvas, which then fits inside a canvas
/// the size of the outer one, centered, so a 4K sequence placed in an HD
/// timeline draws at HD and a canvas pass inside it centers on its frame.
/// `None` when the sequence is missing or shows nothing at that time. A
/// sequence that shows itself, directly or through another, is
/// [`ModelError::SequenceCycle`]: the model refuses to place one, and this
/// bounds a project edited outside it.
pub(super) fn resolve_nest(
    project: &Project,
    sequence: SequenceId,
    source_time: RationalTime,
    [cw, ch]: [f32; 2],
    overrides: ResolveOverrides<'_>,
    nesting: &Nesting<'_>,
) -> Result<Option<NestFrame>, ModelError> {
    if nesting.contains(sequence) {
        return Err(ModelError::SequenceCycle(sequence));
    }
    let Some(inner) = project.sequence(sequence) else {
        return Ok(None);
    };
    let timeline = &inner.timeline;
    let (iw, ih) = timeline_canvas(project, timeline);
    let mut scene = resolve_timeline(
        project,
        timeline,
        source_time,
        [iw, ih],
        [0, 0, 0, 0],
        overrides,
        &nesting.inside(sequence),
    )?;
    if scene.layers.is_empty() {
        return Ok(None);
    }
    let (width, height) = (cw.round().max(1.0) as u32, ch.round().max(1.0) as u32);
    scene.fit_into(width, height);
    let fit = fit_scale(iw as f32, ih as f32, width as f32, height as f32);
    let size = [iw as f32 * fit, ih as f32 * fit];
    let x0 = (1.0 - size[0] / width as f32) * 0.5;
    let y0 = (1.0 - size[1] / height as f32) * 0.5;
    Ok(Some(NestFrame {
        scene,
        frame: [x0, y0, 1.0 - x0, 1.0 - y0],
        size,
    }))
}

/// A crop `uv` over a nest's frame, as UV over the nest scene's canvas.
pub(super) fn uv_in_frame([u0, v0, u1, v1]: [f32; 4], [x0, y0, x1, y1]: [f32; 4]) -> [f32; 4] {
    let (w, h) = (x1 - x0, y1 - y0);
    [x0 + u0 * w, y0 + v0 * h, x0 + u1 * w, y0 + v1 * h]
}
