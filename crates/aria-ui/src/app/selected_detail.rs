//! The selected clip as the model holds it, projected once per change.
//!
//! The inspector draws values the projection does not carry: a parameter's
//! sampled value at the playhead, its keyframes and their easings, the ramp
//! curve, a title's generator, and the quad the compositor puts the selected
//! layer on. Every one of those used to answer by taking the session lock while
//! elements were being built — `param_rows` once per inspector group, and
//! `keyframes_of`, `selected_curve`, `selected_text_generator` and
//! `selected_layer_quad` again each — so a frame took a dozen locks and one of
//! them resolved the whole scene. A preview composite holds the same lock for
//! its whole decode, so any of those reads could park the window behind it.
//!
//! So they are part of the projection instead. This is built beside the store
//! from the same engine read, keyed by the same `ProjectionKey`, and is
//! therefore exactly as fresh as the rows drawn next to it: a value here can
//! never disagree with the clip the timeline is drawing, which is a stronger
//! guarantee than the per-element locks gave.
//!
//! One clip is cloned per re-projection. The projection already walks every
//! track, clip and keyframe, so the copy is small beside the work that produced
//! it, and it is paid once per change rather than once per element per frame.
//!
//! The clip's animatable properties are here for the same reason and are worse
//! without it: enumerating them clones the whole property catalog, and the
//! inspector did that once for the row list, again per row for the keys, and
//! again per sample while drawing a curve. They are one list now, built here.

use aria_models::{Clip, Project, RationalTime};

use crate::app::clip_keys::{KeyParam, key_params_of};

/// Everything the inspector and the canvas overlay read about the selected clip.
///
/// Comparable so a test can hold what a scrub left and check it against a full
/// re-projection field for field: a value sampled at the playhead that the scrub
/// path forgot to re-take shows up as a difference rather than as a stale
/// number nobody can see.
#[derive(PartialEq)]
pub struct SelectedDetail {
    /// The clip as the model holds it: params, keyframes, content, extent.
    pub model: Clip,
    /// Every property the clip can animate, with its keys, in catalog order.
    /// A surface reads this rather than enumerating the catalog itself.
    pub params: std::sync::Arc<[KeyParam]>,
    /// The selected layer's quad at the playhead in canvas pixels, `(center,
    /// size)`, or `None` when the clip places no picture at this instant or its
    /// extent is only known after rasterizing (a text or shape generator, whose
    /// size the resolver leaves relative).
    pub quad: Option<([f32; 2], [f32; 2])>,
}

impl SelectedDetail {
    /// Project the detail for `clip` at `at`.
    ///
    /// `None` when the clip is not in the project — a selection can name a clip
    /// an undo has taken away, and a stale selection draws nothing rather than
    /// panicking.
    pub fn of(project: &Project, clip: aria_models::ClipId, at: RationalTime) -> Option<Self> {
        let model = project.timeline().clip(clip)?.clone();
        // The lane, not the clip, decides whether the visual properties are
        // offered, and `TrackKind::is_audio` is the same answer the store's clip
        // kind is drawn from.
        let audio_lane = project
            .timeline()
            .track_of(clip)
            .and_then(|track| project.timeline().track(track))
            .is_some_and(|track| track.kind.is_audio());
        let params = key_params_of(&model, audio_lane);
        Some(Self {
            params,
            quad: Self::quad_of(project, clip, at),
            model,
        })
    }

    /// The same clip at another instant.
    ///
    /// A poll where only the playhead moved is every frame of playback and
    /// every frame of a scrub. Nothing else the projection reads has changed, so
    /// the clip and its property rows are the values already held — the rows are
    /// the clip's own properties and their keys, which no instant can move — and
    /// only what is genuinely sampled at the playhead is taken again.
    /// `scrub_stale_tests` holds this against a full re-projection field for
    /// field, so a field that does move with the playhead cannot be kept here by
    /// mistake.
    pub fn resampled(
        &self,
        project: &Project,
        clip: aria_models::ClipId,
        at: RationalTime,
    ) -> Self {
        Self {
            model: self.model.clone(),
            params: std::sync::Arc::clone(&self.params),
            quad: Self::quad_of(project, clip, at),
        }
    }

    /// The selected layer's quad at `at`, from the resolver.
    ///
    /// The resolver, not a recomputation, so the handles sit on the same quad
    /// the compositor draws: the aspect fit, the per-axis scale and the
    /// position offset are all already in it.
    fn quad_of(
        project: &Project,
        clip: aria_models::ClipId,
        at: RationalTime,
    ) -> Option<([f32; 2], [f32; 2])> {
        let scene = aria_render::resolve(project, at).ok()?;
        let layer = scene.layers.iter().find(|layer| layer.clip == Some(clip))?;
        match layer.size {
            aria_render::SizeSpec::Fixed(size) => Some((layer.center, size)),
            _ => None,
        }
    }
}

impl crate::app::AppState {
    /// The selected clip as the model holds it, from the projection.
    ///
    /// `None` when nothing is selected and when the selection names a clip the
    /// project no longer has — an undo can take one away — so a stale selection
    /// draws nothing rather than panicking, which is what asking the session
    /// for it did.
    pub(crate) fn selected_model(&self) -> Option<&Clip> {
        self.selected.as_ref().map(|detail| &detail.model)
    }

    /// The selected layer's quad at the playhead in canvas pixels, `(center,
    /// size)`.
    ///
    /// Read from the resolver rather than recomputed, so the handles sit on the
    /// same quad the compositor draws — including the aspect fit, the per-axis
    /// scale, and the position offset. `None` when nothing is selected, the clip
    /// places no picture at this instant, or its size is only known after
    /// rasterizing.
    pub fn selected_layer_quad(&self) -> Option<([f32; 2], [f32; 2])> {
        self.selected.as_ref()?.quad
    }
}
