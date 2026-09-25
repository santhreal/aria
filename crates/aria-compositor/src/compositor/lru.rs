use std::borrow::Borrow;
use std::hash::Hash;

use rustc_hash::FxHashMap;

struct Entry<V> {
    value: V,
    last_used: u64,
    frame: u64,
}

/// Bounded least-recently-used storage that never evicts an entry used by the
/// frame currently being encoded.
pub(super) struct FrameLru<K, V> {
    entries: FxHashMap<K, Entry<V>>,
    clock: u64,
    frame: u64,
}

impl<K, V> Default for FrameLru<K, V> {
    fn default() -> Self {
        Self {
            entries: FxHashMap::default(),
            clock: 0,
            frame: 0,
        }
    }
}

impl<K, V> FrameLru<K, V>
where
    K: Eq + Hash + Clone,
{
    pub(super) fn begin_frame(&mut self) {
        self.frame = self.frame.saturating_add(1);
    }

    pub(super) fn get<Q>(&mut self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: Eq + Hash + ?Sized,
    {
        let entry = self.entries.get_mut(key)?;
        self.clock = self.clock.saturating_add(1);
        entry.last_used = self.clock;
        entry.frame = self.frame;
        Some(&entry.value)
    }

    pub(super) fn peek<Q>(&self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: Eq + Hash + ?Sized,
    {
        self.entries.get(key).map(|entry| &entry.value)
    }

    pub(super) fn remove<Q>(&mut self, key: &Q) -> Option<V>
    where
        K: Borrow<Q>,
        Q: Eq + Hash + ?Sized,
    {
        self.entries.remove(key).map(|entry| entry.value)
    }

    /// Insert one entry, evicting at most one inactive least-recently-used row.
    /// If every retained row is needed by this frame, the working set may exceed
    /// `max` until a later miss can retire an inactive row safely.
    pub(super) fn insert(&mut self, key: K, value: V, max: usize) -> Option<K> {
        let victim = (self.entries.len() >= max).then(|| {
            self.entries
                .iter()
                .filter(|(_, entry)| entry.frame != self.frame)
                .min_by_key(|(_, entry)| entry.last_used)
                .map(|(key, _)| key.clone())
        });
        let victim = victim.flatten();
        if let Some(key) = victim.as_ref() {
            self.entries.remove(key);
        }

        self.clock = self.clock.saturating_add(1);
        self.entries.insert(
            key,
            Entry {
                value,
                last_used: self.clock,
                frame: self.frame,
            },
        );
        victim
    }

    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.entries.len()
    }

    #[cfg(test)]
    pub(super) fn contains_key<Q>(&self, key: &Q) -> bool
    where
        K: Borrow<Q>,
        Q: Eq + Hash + ?Sized,
    {
        self.entries.contains_key(key)
    }
}

#[cfg(test)]
mod tests {
    use super::FrameLru;

    #[test]
    fn twenty_entries_evict_one_lru_row_per_miss() {
        const MAX: usize = 16;
        let mut cache = FrameLru::default();
        for key in 0..MAX as u32 {
            cache.begin_frame();
            assert_eq!(cache.insert(key, key, MAX), None);
        }

        for key in MAX as u32..20 {
            cache.begin_frame();
            assert_eq!(cache.get(&0), Some(&0));
            let expected = key - MAX as u32 + 1;
            assert_eq!(cache.insert(key, key, MAX), Some(expected));
            assert_eq!(cache.len(), MAX);
            assert!(cache.contains_key(&0));
            assert!(!cache.contains_key(&expected));
        }
    }

    #[test]
    fn subtitle_atlas_stress_drops_only_the_oldest_entry() {
        const MAX: usize = 32;
        let mut cache = FrameLru::default();
        for key in 0..MAX as u64 {
            cache.begin_frame();
            cache.insert(key, key, MAX);
        }

        cache.begin_frame();
        assert_eq!(cache.get(&0), Some(&0));
        assert_eq!(cache.insert(MAX as u64, MAX as u64, MAX), Some(1));
        assert_eq!(cache.len(), MAX);
        assert!(cache.contains_key(&0));
        assert!(!cache.contains_key(&1));
        for retained in 2..=MAX as u64 {
            assert!(cache.contains_key(&retained));
        }
    }

    #[test]
    fn current_frame_working_set_is_not_evicted() {
        const MAX: usize = 2;
        let mut cache = FrameLru::default();
        cache.begin_frame();
        cache.insert(1, 1, MAX);
        cache.insert(2, 2, MAX);
        assert_eq!(cache.get(&1), Some(&1));
        assert_eq!(cache.get(&2), Some(&2));

        assert_eq!(cache.insert(3, 3, MAX), None);
        assert_eq!(cache.len(), 3);
    }
}
