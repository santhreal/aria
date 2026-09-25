use crate::error::ModelError;
use crate::ids::SequenceId;
use crate::sequence::Sequence;
use crate::time::Rational;

use super::Project;

impl Project {
    /// Retrieve a secondary sequence by [`SequenceId`].
    pub fn sequence(&self, id: SequenceId) -> Option<&Sequence> {
        self.sequences.get(&id)
    }

    /// Retrieve a secondary sequence mutably by [`SequenceId`].
    pub fn sequence_mut(&mut self, id: SequenceId) -> Option<&mut Sequence> {
        self.invalidate_largest_used_video();
        self.sequences.get_mut(&id)
    }

    /// Iterate over all secondary sequences, in id order.
    ///
    /// Ordered because every caller lists them. The store is a hash map, so
    /// `values()` hands out a different order per process, and a panel drawn
    /// from that listed the same two nests in a different order each run. The
    /// sort is over as many ids as the project has nests.
    pub fn sequences(&self) -> impl Iterator<Item = &Sequence> {
        let mut out: Vec<&Sequence> = self.sequences.values().collect();
        out.sort_unstable_by_key(|sequence| sequence.id);
        out.into_iter()
    }

    /// Number of secondary sequences in the project.
    pub fn sequence_count(&self) -> usize {
        self.sequences.len()
    }

    /// Create an empty sequence running at `frame_rate` and add it to the project.
    pub fn create_sequence(&mut self, name: impl Into<String>, frame_rate: Rational) -> SequenceId {
        let sequence = Sequence::new(name, frame_rate);
        let id = sequence.id;
        self.sequences.insert(id, sequence);
        id
    }

    /// Register a [`Sequence`] in the project's sequence store.
    pub fn add_sequence(&mut self, sequence: Sequence) -> SequenceId {
        let id = sequence.id;
        self.sequences.insert(id, sequence);
        id
    }

    /// Remove a secondary sequence from the store. If active, resets active sequence to root.
    pub fn remove_sequence(&mut self, id: SequenceId) -> Option<Sequence> {
        self.invalidate_largest_used_video();
        if self.active_sequence == Some(id) {
            self.active_sequence = None;
        }
        self.sequences.remove(&id)
    }

    /// The currently active sequence for editing (`None` ⇔ main root timeline).
    pub fn active_sequence(&self) -> Option<SequenceId> {
        self.active_sequence
    }

    /// Set the currently active sequence for editing (`None` ⇔ main root timeline).
    pub fn set_active_sequence(&mut self, id: Option<SequenceId>) -> Result<(), ModelError> {
        if let Some(seq_id) = id {
            if !self.sequences.contains_key(&seq_id) {
                return Err(ModelError::UnknownSequence(seq_id));
            }
        }
        self.invalidate_largest_used_video();
        self.active_sequence = id;
        Ok(())
    }

    /// Check whether placing `nested_seq` into `target_seq` would form a cycle.
    ///
    /// A cycle occurs if `nested_seq == target_seq`, or if `nested_seq` transitively contains
    /// `target_seq` in any of its clips.
    pub fn would_create_sequence_cycle(
        &self,
        target_seq: Option<SequenceId>,
        nested_seq: SequenceId,
    ) -> bool {
        let Some(target) = target_seq else {
            return false;
        };
        if target == nested_seq {
            return true;
        }
        let mut visited = rustc_hash::FxHashSet::default();
        let mut stack = vec![nested_seq];
        while let Some(current) = stack.pop() {
            if !visited.insert(current) {
                continue;
            }
            if current == target {
                return true;
            }
            if let Some(seq) = self.sequence(current) {
                for clip in seq
                    .timeline
                    .tracks_ordered()
                    .flat_map(crate::track::Track::clips)
                {
                    if let Some(child_id) = clip.sequence() {
                        if child_id == target {
                            return true;
                        }
                        stack.push(child_id);
                    }
                }
            }
        }
        false
    }
}
