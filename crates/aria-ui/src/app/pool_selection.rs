//! Which pool items are selected.
//!
//! # Why
//!
//! The pool held one selection: a single `Option<u64>`. Everything that acts on
//! several items at once had to pretend otherwise, and one surface shipped that
//! way: the sidebar's multicam button builds its group from
//! `selected_pool_media_ids`, which read the single field and answered with one
//! angle. A group with one angle is not a multicam group, and the command's reach
//! row claimed "a multi-item selection" that nothing could make.
//!
//! The set lives here rather than in the panel, so the order is the product's
//! and not the list's: a group's angles are the order the operator picked them
//! in, which a sorted set would decide for them.
//!
//! What a click means is not here. That is
//! [`pool_pick`](crate::surfaces::pool_pick), which drives the three writes
//! below; this owns the set and the anchor and nothing about the pointer.

/// The selected pool items, in the order they were picked.
#[derive(Debug, Default)]
pub struct PoolSelection {
    /// Every selected item. A `Vec` rather than a set: the order is the angle
    /// order of any group built from it, and it is short by construction, so a
    /// scan over it costs less than the hashing would.
    picked: Vec<u64>,
    /// The item a span extends from: the last one picked outright.
    anchor: Option<u64>,
}

impl PoolSelection {
    /// The item a single-item action works on, which is the last one picked.
    ///
    /// Everything that acted on `bin_selection` reads this: the properties
    /// table, the source monitor, a reload, a reveal, a ripple insert. One of
    /// several selected items still has to be the one a single-item row means,
    /// and it is the one the operator touched last.
    pub fn focus(&self) -> Option<u64> {
        self.anchor.or_else(|| self.picked.last().copied())
    }

    /// Every selected item, in pick order.
    pub fn ids(&self) -> &[u64] {
        &self.picked
    }

    pub fn contains(&self, id: u64) -> bool {
        self.picked.contains(&id)
    }

    pub fn is_empty(&self) -> bool {
        self.picked.is_empty()
    }

    pub fn len(&self) -> usize {
        self.picked.len()
    }

    /// Drop the whole selection. Opening another folder does this: a selection
    /// the operator cannot see is one they cannot correct.
    pub fn clear(&mut self) {
        self.picked.clear();
        self.anchor = None;
    }

    /// Select one item and nothing else.
    pub fn replace(&mut self, id: u64) {
        self.picked.clear();
        self.picked.push(id);
        self.anchor = Some(id);
    }

    /// Add one item, or drop it if it was already in.
    pub fn toggle(&mut self, id: u64) {
        if let Some(at) = self.picked.iter().position(|item| *item == id) {
            self.picked.remove(at);
            if self.anchor == Some(id) {
                // The anchor a span extends from has to be a selected item, so
                // it falls back to whatever is still picked.
                self.anchor = self.picked.last().copied();
            }
            return;
        }
        self.picked.push(id);
        self.anchor = Some(id);
    }

    /// Select the run between the anchor and `id`, inclusive.
    ///
    /// The anchor stays where it was, so a second span from the same anchor
    /// re-picks the run rather than walking away from it. Without an anchor, or
    /// with one the list is no longer showing, the click is a plain one.
    pub fn span(&mut self, id: u64, visible: &[u64]) {
        let at = |wanted: u64| visible.iter().position(|item| *item == wanted);
        let (Some(anchor), Some(from), Some(to)) = (self.anchor, self.anchor.and_then(at), at(id))
        else {
            self.replace(id);
            return;
        };
        let run = if from <= to { from..=to } else { to..=from };
        self.picked = visible[run].to_vec();
        self.anchor = Some(anchor);
    }
}
