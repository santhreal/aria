//! Landing a dragged sidebar catalog entry.
//!
//! Every apply-to-a-clip setter in this crate addresses the selection, because that
//! is what the inspector edits. A drop names its own target, so the drop selects
//! that clip first and then reuses the same setter: one owner per operation, and
//! the selection visibly follows the drop instead of the operator applying a preset
//! to a clip they can no longer see.

use aria_engine::EngineError;
use aria_models::{ClipId, Generator, MaskKind, TrackId};

use super::AppState;
use crate::types::{CatalogDrag, CatalogDragKind};

impl AppState {
    /// Apply a dragged catalog entry to `clip`.
    ///
    /// Inert for a payload that belongs on empty lane space, and inert for an id
    /// this build does not know, so a stale drag cannot half-apply.
    pub fn drop_catalog_on_clip(
        &mut self,
        drag: CatalogDrag,
        clip: ClipId,
    ) -> Result<(), EngineError> {
        if !drag.targets_a_clip() {
            return Ok(());
        }
        // A clip can be deleted while a drag is in flight. Selecting a clip that no
        // longer exists would leave the selection pointing at a ghost and turn the
        // apply into an `UnknownClip` error the operator did not cause, so the drop
        // changes nothing instead.
        if self.engine.lock().project().clip(clip).is_none() {
            return Ok(());
        }
        self.select_clip(clip);
        match drag.kind {
            CatalogDragKind::Animation => self.set_selected_animation(drag.id),
            CatalogDragKind::Mask => match MaskKind::from_id(drag.id) {
                Some(kind) => self.set_selected_mask_kind(kind),
                None => Ok(()),
            },
            CatalogDragKind::Transition => self.set_selected_transition(drag.id),
            CatalogDragKind::Sticker => Ok(()),
        }
    }

    /// Place a dragged catalog entry on empty lane space at `at`.
    ///
    /// Inert for a payload that needs a clip: a preset with nothing under it has
    /// nothing to change, and placing a bare clip to hold it would be inventing an
    /// edit the operator did not ask for.
    pub fn drop_catalog_on_track(
        &mut self,
        drag: CatalogDrag,
        track: Option<TrackId>,
        at: i64,
    ) -> Result<(), EngineError> {
        if drag.targets_a_clip() {
            return Ok(());
        }
        self.drop_generated_on_timeline(Generator::sticker(drag.id), track, at)
    }
}
