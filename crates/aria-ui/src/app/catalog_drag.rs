//! Landing a dragged sidebar catalog entry.
//!
//! Every apply-to-a-clip setter in this crate addresses the selection, because that
//! is what the inspector edits. A drop names its own target, so the drop selects
//! that clip first and then reuses the same setter: one owner per operation, and
//! the selection visibly follows the drop instead of a preset landing on a clip
//! that is no longer selected.

use aria_engine::EngineError;
use aria_models::{ClipId, Generator, MaskKind, TrackId};

use super::AppState;
use crate::types::{CatalogDrag, CatalogDragKind};

impl AppState {
    /// Apply a dragged catalog entry to `clip`, released at tick `at` when the
    /// drop has a position.
    ///
    /// Inert for a payload that belongs on empty lane space, and inert for an id
    /// this build does not know, so a stale drag cannot half-apply. A
    /// transition lands on the cut [`Self::transition_junction`] picks and
    /// selects the clip keyed on it, the clip the inspector draws that cut for.
    pub fn drop_catalog_on_clip(
        &mut self,
        drag: &CatalogDrag,
        clip: ClipId,
        at: Option<i64>,
    ) -> Result<(), EngineError> {
        if !drag.targets_a_clip() {
            return Ok(());
        }
        // A clip can be deleted while a drag is in flight. Selecting a clip that no
        // longer exists would leave the selection pointing at a ghost and turn the
        // apply into an `UnknownClip` error no input caused, so the drop
        // changes nothing instead.
        if self.engine.lock().project().clip(clip).is_none() {
            return Ok(());
        }
        self.select_clip(clip);
        match drag.kind {
            CatalogDragKind::Animation => self.set_selected_animation(&drag.id),
            CatalogDragKind::Mask => match MaskKind::from_id(&drag.id) {
                Some(kind) => self.add_selected_mask(kind),
                None => Ok(()),
            },
            CatalogDragKind::Transition => {
                let junction = self.transition_junction(clip, at);
                self.select_clip(junction);
                self.set_clip_transition(junction, &drag.id)
            }
            CatalogDragKind::Effect => self.add_effect_to_selected(drag.id.as_str()),
            CatalogDragKind::AudioEffect => self.add_audio_effect_to_selection(&drag.id),
            CatalogDragKind::Preset => self.apply_preset(&drag.id),
            CatalogDragKind::Sticker | CatalogDragKind::Generator | CatalogDragKind::ClipType => {
                Ok(())
            }
        }
    }

    /// Land a dragged catalog entry released over `track` at `at`.
    ///
    /// A payload that targets a clip applies to the clip on `track` at `at`,
    /// as [`Self::drop_catalog_on_clip`] does, and is inert over empty lane
    /// space: a preset with nothing under it has nothing to change, and
    /// placing a bare clip to hold it would be inventing an edit nothing
    /// requested. A payload that places a clip of its own places it at `at`.
    pub fn drop_catalog_on_track(
        &mut self,
        drag: &CatalogDrag,
        track: Option<TrackId>,
        at: i64,
    ) -> Result<(), EngineError> {
        match drag.kind {
            CatalogDragKind::Sticker => {
                self.drop_generated_on_timeline(Generator::sticker(drag.id.as_str()), track, at)
            }
            CatalogDragKind::Generator => {
                self.drop_effect_generator_on_timeline(&drag.id, track, at)
            }
            CatalogDragKind::ClipType => self.drop_clip_type_on_timeline(&drag.id, track, at),
            CatalogDragKind::Animation
            | CatalogDragKind::Mask
            | CatalogDragKind::Transition
            | CatalogDragKind::Effect
            | CatalogDragKind::AudioEffect
            | CatalogDragKind::Preset => match track.and_then(|track| self.clip_on(track, at)) {
                Some(clip) => self.drop_catalog_on_clip(drag, clip, Some(at)),
                None => Ok(()),
            },
        }
    }

    /// The tick on `track` the lane line marks while `drag` is held at `at`, or
    /// `None` when `drag` has no landing there.
    ///
    /// A transition lands on the cut nearer `at` within the clip under the
    /// pointer (`timeline_end_ticks` of [`Self::transition_junction`]) when that
    /// clip has an abutting cut, and nowhere over empty lane space or an
    /// isolated clip. Every other catalog payload marks `at`.
    pub fn catalog_drop_tick(&self, drag: &CatalogDrag, track: TrackId, at: i64) -> Option<i64> {
        match drag.kind {
            CatalogDragKind::Transition => {
                let clip = self
                    .store
                    .sequence
                    .tracks
                    .iter()
                    .find(|lane| lane.id == track.raw())?
                    .clips
                    .iter()
                    .find(|clip| clip.timeline_start_ticks <= at && at < clip.timeline_end_ticks)
                    .map(|clip| ClipId::from_raw(clip.id))?;
                let junction = self.transition_junction(clip, Some(at));
                self.clip_transition_media(junction)?;
                self.ui_clip_ref(junction).map(|ui| ui.timeline_end_ticks)
            }
            CatalogDragKind::Sticker
            | CatalogDragKind::Generator
            | CatalogDragKind::ClipType
            | CatalogDragKind::Animation
            | CatalogDragKind::Mask
            | CatalogDragKind::Effect
            | CatalogDragKind::AudioEffect
            | CatalogDragKind::Preset => Some(at),
        }
    }

    /// The clip on `track` at tick `at`. The lane is the single definition of
    /// which clip is at a position, as for `select_clip_under_playhead`.
    fn clip_on(&self, track: TrackId, at: i64) -> Option<ClipId> {
        let at = aria_models::RationalTime::new(at, self.playhead.rate);
        let engine = self.engine.lock();
        let track = engine.project().timeline().track(track)?;
        track.clip_at(at).ok().flatten().map(|clip| clip.id)
    }

    /// Apply a dragged catalog entry dropped on the inspector's Effects tab to
    /// the selected clip.
    ///
    /// An effect lands before the chain's effect at `at`, or at the end when
    /// `at` is `None` or past the end, on every selected clip on the same kind
    /// of lane as the first; a shorter chain takes it at its end. Every other
    /// payload that targets a clip applies as a drop on the selected clip does.
    /// A payload that places a clip of its own is refused with where it goes.
    pub fn drop_catalog_on_effect_controls(
        &mut self,
        drag: &CatalogDrag,
        at: Option<usize>,
    ) -> Result<(), EngineError> {
        match drag.kind {
            CatalogDragKind::Effect => {
                let peers = self.peers_sharing_effect(None);
                self.edit_each(peers, |state| state.insert_effect_in_selected(&drag.id, at))
            }
            CatalogDragKind::Animation
            | CatalogDragKind::Mask
            | CatalogDragKind::Transition
            | CatalogDragKind::AudioEffect
            | CatalogDragKind::Preset => match self.selected_clip_or_decline() {
                Some(clip) => self.drop_catalog_on_clip(drag, clip, None),
                None => Ok(()),
            },
            CatalogDragKind::Sticker | CatalogDragKind::Generator | CatalogDragKind::ClipType => {
                Err(EngineError::Refused(format!(
                    "{} places a clip of its own: drop it on a track.",
                    drag.label
                )))
            }
        }
    }
}
