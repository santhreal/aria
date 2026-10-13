//! Transitions (v1 roadmap M4): a timed blend at the junction between two
//! abutting clips on a track.
//!
//! A [`Transition`] is **data**, like effects: the model stores
//! `{left, right, transition_id, duration}` and the compositor owns the WGSL
//! that blends the two frames by progress. The [`transition_catalog`] is the
//! validation + UI source of truth (ids, display names); it is drift-checked
//! against the compositor's renderable set from `aria-engine`.
//!
//! A transition lives only while its pair of clips still abuts (left's end
//! tick equals right's start tick). Structural edits that break the abutment
//! prune the dead junction; undo restores it.

use std::sync::{Arc, LazyLock};

use serde::{Deserialize, Serialize};

use crate::ids::ClipId;

/// A renderable transition: its stable id and a human label.
///
/// Owned strings, not `&'static str`: the catalog is a file now, so the text is
/// read at startup rather than compiled in.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct TransitionSpec {
    pub id: String,
    pub label: String,
}

/// The shipped transitions, read once. Shipped set at `data/transitions.toml`,
/// replaced by a file of that name under the host's catalog root.
static SHIPPED: LazyLock<Vec<Arc<TransitionSpec>>> = LazyLock::new(|| {
    crate::catalog::load(
        "transitions",
        include_str!("../data/transitions.toml"),
        |entries: &[TransitionSpec]| {
            crate::catalog::unique_ids(entries, |s| &s.id, |s| &s.label)
        },
    )
    .into_iter()
    .map(Arc::new)
    .collect()
});

/// Default transition window length, in timeline ticks (centered on the cut).
pub const DEFAULT_TRANSITION_TICKS: i64 = 24;

/// The transitions that ship with the app, without registered ones.
pub fn shipped_transition_catalog() -> &'static [Arc<TransitionSpec>] {
    &SHIPPED
}

/// Every transition the model knows about now (validation + UI browsing): the
/// shipped set, then transitions registered by plugins.
pub fn transition_catalog() -> crate::Catalog<TransitionSpec> {
    crate::registered::transition_catalog()
}

/// The catalog entry for `id`, shipped or registered, or `None`.
pub fn transition_spec(id: &str) -> Option<Arc<TransitionSpec>> {
    crate::registered::transition_spec(id)
}

/// A transition at the junction between `left` and the clip immediately to its
/// right on the same track. Centered on the cut; `duration` is the total
/// window length in timeline ticks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Transition {
    /// The clip on the outgoing (left) side of the cut.
    pub left: ClipId,
    /// The clip on the incoming (right) side of the cut.
    pub right: ClipId,
    /// Catalog id of the blend to render.
    pub transition_id: String,
    /// Total window length in timeline ticks, centered on the junction.
    pub duration: i64,
}

impl Transition {
    pub fn new(
        left: ClipId,
        right: ClipId,
        transition_id: impl Into<String>,
        duration: i64,
    ) -> Self {
        Self {
            left,
            right,
            transition_id: transition_id.into(),
            duration: duration.max(1),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shipped file is what loads when nothing overrides it, and its ids are
    /// unique — a duplicate would shadow a transition the compositor can render.
    #[test]
    fn catalog_ids_are_unique() {
        let mut ids: Vec<&str> = SHIPPED.iter().map(|s| s.id.as_str()).collect();
        let count = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), count, "transition ids are unique");
        assert_eq!(count, 8, "the M4 starter set is eight transitions");
        assert!(SHIPPED.iter().all(|spec| !spec.label.is_empty()));
    }

    #[test]
    fn spec_lookup_resolves_known_and_rejects_unknown() {
        assert_eq!(transition_spec("crossfade").unwrap().label, "Cross Fade");
        assert!(transition_spec("nope").is_none());
    }

    #[test]
    fn new_clamps_duration_to_at_least_one() {
        let t = Transition::new(ClipId::from_raw(1), ClipId::from_raw(2), "crossfade", 0);
        assert_eq!(t.duration, 1);
    }
}
