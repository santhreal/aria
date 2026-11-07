//! The catalog entries a project holds by id that no catalog lists.

use std::collections::BTreeMap;

use super::Project;
use crate::registered::CatalogKind;
use crate::timeline::Timeline;

/// An id a project holds that no catalog lists, the kind of entry it is held
/// as, and the number of instances: one per clip effect, lane insert, cut, or
/// clip type that holds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MissingPlugin<'p> {
    /// The id no catalog lists.
    pub id: &'p str,
    /// The kind of entry the project holds it as.
    pub kind: CatalogKind,
    /// The number of instances.
    pub uses: usize,
}

/// The instance count of each `(id, kind)`.
type Uses<'p> = BTreeMap<(&'p str, CatalogKind), usize>;

impl Project {
    /// Every id this project holds that no catalog lists, in the root sequence
    /// and every other one, ordered by id and then by kind. A plugin that is
    /// missing, failed, or disabled drops its registrations, and each entry
    /// renders as [`CatalogKind::stand_in`] states.
    pub fn missing_plugins(&self) -> Vec<MissingPlugin<'_>> {
        let mut uses = Uses::new();
        let others = self.sequences.values().map(|sequence| &sequence.timeline);
        for timeline in std::iter::once(&self.timeline).chain(others) {
            count_missing(timeline, &mut uses);
        }
        uses.into_iter()
            .map(|((id, kind), uses)| MissingPlugin { id, kind, uses })
            .collect()
    }
}

/// Add each unlisted id `timeline` holds to `uses`: in each lane, its clips'
/// parts ([`crate::Clip::missing_plugin_parts`]), its insert chain, and its
/// transitions.
fn count_missing<'p>(timeline: &'p Timeline, uses: &mut Uses<'p>) {
    for track in timeline.tracks_ordered() {
        let parts = track.clips().flat_map(|clip| clip.missing_plugin_parts());
        let inserts = (track.audio_effects.iter())
            .filter(|fx| crate::audio_effect_spec(&fx.effect_id).is_none())
            .map(|fx| (CatalogKind::AudioEffect, fx.effect_id.as_str()));
        let cuts = (track.transitions().iter())
            .filter(|transition| transition.is_unlisted())
            .map(|transition| (CatalogKind::Transition, transition.transition_id.as_str()));
        for (kind, id) in parts.chain(inserts).chain(cuts) {
            *uses.entry((id, kind)).or_default() += 1;
        }
    }
}
