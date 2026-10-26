//! Session facts a frame reads that the store does not carry.
//!
//! The canvas settings, the size the project renders at, the shape its footage
//! implies, which pool items the timeline still holds, and the undo list are all
//! answers about the session rather than about the sequence, so the projection
//! never carried them and every reader asked the engine instead — the settings
//! dialog three times per frame, the export sheet with a raw lock in its
//! drawing code, the history panel twice, and the pool's context menu once per
//! item. A preview composite holds that same lock for its whole decode, so each
//! of those was a frame that could park behind a decode.
//!
//! They are part of the projection now, built from the same engine read and
//! keyed by the same `ProjectionKey`, so
//! a value here is exactly as fresh as the store beside it. History labels are
//! copied only when this change-keyed projection is rebuilt, never when the
//! panel polls it.

use aria_models::{AudioLayout, CanvasSettings, Project, Rational};

use crate::app::AppState;

/// One retained history row for UI frames that outlive the engine read lock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectHistoryEntry {
    pub label: String,
    pub applied: bool,
}

/// What a frame asks the session, answered once per change.
#[derive(Debug, PartialEq)]
pub struct ProjectFacts {
    /// The project's canvas settings: aspect, background, explicit size.
    pub canvas: CanvasSettings,
    /// The sequence's audio channel layout.
    pub audio_layout: AudioLayout,
    /// The frame rate of the largest video source in the pool (by area).
    pub largest_source_rate: Option<Rational>,
    /// The size the project renders at right now, explicit or derived.
    pub canvas_size: [u32; 2],
    /// The shape `Auto` would produce, ignoring any explicit size. The delivery
    /// chooser scales this, so picking a size never reshapes the frame.
    pub derived_shape: [u32; 2],
    /// Pool items with at least one clip on the timeline, by raw id. The pool
    /// menu explains a refusal before it happens instead of offering a dead row.
    pub media_in_use: Vec<u64>,
    /// The undo list, newest step last, with whether each is applied. A handle,
    /// because the dialog that draws it cannot hold a borrow of the state across
    /// the frame and would otherwise copy every step.
    pub history: std::sync::Arc<[ProjectHistoryEntry]>,
    /// How many steps are applied. Row `depth - 1` is the current point.
    pub history_depth: usize,
    /// Ruler markers the sequence holds.
    pub marker_count: usize,
    /// The mix: every strip in fold order with its destination lists, or the
    /// refusal that stops the routing being read.
    pub mixer: crate::app::mixer::MixerFacts,
}

impl ProjectFacts {
    pub fn of(engine: &aria_engine::Engine) -> Self {
        Self::of_with_basis(engine, None)
    }

    pub(crate) fn of_with_basis(
        engine: &aria_engine::Engine,
        basis: Option<&crate::projection::LaneBasis>,
    ) -> Self {
        let project: &Project = engine.project();
        let canvas = project.timeline().canvas();
        // An `Auto` canvas follows the largest video placed. The basis holds the
        // sources every lane placed when it was built, so reading it costs no
        // walk over every clip of every lane, which an edit to one clip would
        // otherwise pay.
        let largest = || match basis {
            Some(basis) => basis.largest_video(project),
            None => project.largest_used_video_dimensions(),
        };
        let (w, h) = aria_render::canvas_size_of(canvas, largest);
        let (dw, dh) = aria_render::derived_canvas_size_of(canvas, largest);
        let media_in_use = match basis {
            Some(basis) => basis.media_in_use(),
            None => {
                let mut used: Vec<u64> = project
                    .timeline()
                    .tracks_ordered()
                    .flat_map(aria_models::Track::clips)
                    .filter_map(|clip| clip.media().map(aria_models::MediaId::raw))
                    .collect();
                used.sort_unstable();
                used.dedup();
                used
            }
        };
        let marker_count = match basis {
            Some(basis) => project.timeline().markers().len() + basis.clip_marker_count(),
            None => project.timeline().marker_count(),
        };
        Self {
            canvas,
            audio_layout: project.timeline().audio_layout(),
            largest_source_rate: project.largest_source_frame_rate(),
            canvas_size: [w, h],
            derived_shape: [dw, dh],
            media_in_use,
            history: engine
                .history_entries()
                .into_iter()
                .map(|entry| ProjectHistoryEntry {
                    label: entry.label.into_owned(),
                    applied: entry.applied,
                })
                .collect::<Vec<_>>()
                .into(),
            history_depth: engine.history_depth(),
            marker_count,
            mixer: match basis {
                Some(basis) => basis.mixer(),
                None => crate::app::mixer::MixerFacts::of(engine),
            },
        }
    }
}

impl AppState {
    /// The project's canvas settings.
    pub fn canvas(&self) -> CanvasSettings {
        self.facts.canvas
    }

    /// The sequence's audio channel layout.
    pub fn audio_layout(&self) -> AudioLayout {
        self.facts.audio_layout
    }

    /// The frame rate of the largest video source in the pool (by area), or `None`.
    pub fn largest_source_frame_rate(&self) -> Option<Rational> {
        self.facts.largest_source_rate
    }

    /// Frame rate mismatch between the largest pool source and the sequence, or `None`.
    pub fn frame_rate_mismatch(&self) -> Option<Rational> {
        let source_rate = self.facts.largest_source_rate?;
        let seq_rate = self.store.sequence.frame_rate;
        if !aria_core::rate_eq(seq_rate, source_rate) {
            Some(source_rate)
        } else {
            None
        }
    }

    /// The size the project renders at right now, whether explicit or derived.
    /// The Canvas section shows it so "derived" is a readable value rather than
    /// a blank, and export never disagrees with what the dialog said.
    pub fn canvas_size(&self) -> [u32; 2] {
        self.facts.canvas_size
    }

    /// The same size in the shape [`aria_render`] speaks: export settings and
    /// the renderer take a tuple.
    pub(crate) fn canvas_size_tuple(&self) -> (u32, u32) {
        let [w, h] = self.facts.canvas_size;
        (w, h)
    }

    /// The size `Auto` would produce for this project, ignoring any explicit
    /// size: the largest video in the pool, or the 1920×1080 fallback.
    pub(crate) fn derived_canvas_shape(&self) -> [u32; 2] {
        self.facts.derived_shape
    }

    /// Whether a pool item still has a clip on the timeline. The pool menu reads
    /// this to explain a refusal before it happens instead of showing a dead item.
    pub fn media_is_in_use(&self, media: aria_models::MediaId) -> bool {
        self.facts.media_in_use.binary_search(&media.raw()).is_ok()
    }

    /// The undo list as the History panel draws it.
    ///
    /// A handle: the dialog owns the list for the frame without copying it.
    pub fn history_entries(&self) -> std::sync::Arc<[ProjectHistoryEntry]> {
        std::sync::Arc::clone(&self.facts.history)
    }

    /// How many steps are applied. Row `depth - 1` is the current point; `0`
    /// means the project is at its opening state.
    pub fn history_depth(&self) -> usize {
        self.facts.history_depth
    }

    /// Ruler markers on the sequence, as the session holds them.
    pub fn marker_count(&self) -> usize {
        self.facts.marker_count
    }
}
