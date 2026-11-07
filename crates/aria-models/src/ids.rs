//! Strongly-typed entity identifiers.
//!
//! Each ID is a distinct newtype around `u64`, so the type system prevents
//! passing (say) a [`ClipId`] where a [`TrackId`] is expected. IDs are cheap to
//! copy, hash, and compare. Use [`from_raw`](ProjectId::from_raw) for
//! deterministic IDs in tests, or `next()` for process-unique allocation.

use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

/// The largest id a file or a command may state.
///
/// Every id read through [`Deserialize`] advances its type's allocator past
/// itself, and [`next`](ClipId::next) counts up from there. An id near
/// `u64::MAX` would wrap the allocator to ids already in use, and a map insert
/// at a re-handed id replaces the entity that holds it. Capped at `2^62 - 1`,
/// the allocator has `3 * 2^62` ids left before it wraps. An id this process
/// allocates after reading one within that many of the ceiling lies above it,
/// so a project saved from a file stating such an id does not open again; no
/// Aria build allocates near the ceiling.
const MAX_ID: u64 = (1 << 62) - 1;

/// `value` as an id of the type `name`, or the error stating that it is above
/// [`MAX_ID`].
fn within_ceiling(name: &str, value: u64) -> Result<u64, String> {
    if value > MAX_ID {
        return Err(format!(
            "{name} {value} is above {MAX_ID}, the largest id a project holds"
        ));
    }
    Ok(value)
}

macro_rules! define_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
        #[serde(transparent)]
        pub struct $name(u64);

        impl $name {
            /// Construct from a raw value (useful for deterministic tests).
            pub const fn from_raw(value: u64) -> Self {
                Self(value)
            }

            /// The underlying numeric value.
            pub const fn raw(self) -> u64 {
                self.0
            }

            /// The process-unique allocator backing [`next`](Self::next).
            fn counter() -> &'static AtomicU64 {
                static COUNTER: AtomicU64 = AtomicU64::new(1);
                &COUNTER
            }

            /// Advance the allocator past `value`, so a subsequent
            /// [`next`](Self::next) never re-hands an id already in use.
            /// Loading a persisted project routes every id through here (see
            /// the [`Deserialize`] impl) so cross-session ids never collide.
            fn observe(value: u64) {
                Self::counter().fetch_max(value.saturating_add(1), Ordering::Relaxed);
            }

            /// Allocate the next process-unique ID for this type.
            pub fn next() -> Self {
                Self(Self::counter().fetch_add(1, Ordering::Relaxed))
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}#{}", stringify!($name), self.0)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                let value = within_ceiling(stringify!($name), u64::deserialize(deserializer)?)
                    .map_err(serde::de::Error::custom)?;
                // A loaded id is now live in this process: bump the allocator
                // so freshly created entities can't reuse (and silently
                // overwrite) it. Without this, per-process counters reset to 1
                // on relaunch and the first new clip collides with a persisted
                // one — the cross-session undo data-loss bug.
                Self::observe(value);
                Ok(Self(value))
            }
        }
    };
}

define_id!(
    /// Identifies a [`Project`](crate::Project).
    ProjectId
);
define_id!(
    /// Identifies a [`MediaSource`](crate::MediaSource) in the media pool.
    MediaId
);
define_id!(
    /// Identifies a [`Track`](crate::Track) within the timeline.
    TrackId
);
define_id!(
    /// Identifies a [`Clip`](crate::Clip) placed on a track.
    ClipId
);
define_id!(
    /// Identifies a link group: clips sharing a `LinkId` move/trim together
    /// (CapCut linkage — e.g. the video+audio pair from one media drop).
    LinkId
);
define_id!(
    /// Identifies a clip group (`Clip` > `Group`): clips sharing a `GroupId`
    /// select and move together while trims stay per-clip.
    GroupId
);
define_id!(
    /// Identifies a [`Marker`](crate::Marker) on the timeline ruler.
    MarkerId
);
define_id!(
    /// Identifies a [`Bin`](crate::Bin): a folder in the media pool.
    BinId
);
define_id!(
    /// Identifies a [`Template`](crate::Template) document.
    TemplateId
);
define_id!(
    /// Identifies a [`MulticamGroup`](crate::MulticamGroup).
    MulticamGroupId
);
define_id!(
    /// Identifies a [`Sequence`](crate::Sequence) in the secondary-sequence store.
    SequenceId
);

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    macro_rules! assert_from_raw_roundtrips {
        ($from_raw:path) => {
            assert_eq!($from_raw(0).raw(), 0);
            assert_eq!($from_raw(42).raw(), 42);
            assert_eq!($from_raw(u64::MAX).raw(), u64::MAX);
        };
    }

    // --- from_raw / raw ---------------------------------------------------

    #[test]
    fn project_id_from_raw_roundtrips() {
        assert_from_raw_roundtrips!(ProjectId::from_raw);
    }

    #[test]
    fn media_id_from_raw_roundtrips() {
        assert_from_raw_roundtrips!(MediaId::from_raw);
    }

    #[test]
    fn track_id_from_raw_roundtrips() {
        assert_from_raw_roundtrips!(TrackId::from_raw);
    }

    #[test]
    fn clip_id_from_raw_roundtrips() {
        assert_from_raw_roundtrips!(ClipId::from_raw);
    }

    #[test]
    fn template_id_from_raw_roundtrips() {
        assert_from_raw_roundtrips!(TemplateId::from_raw);
    }
    #[test]
    fn multicam_group_id_from_raw_roundtrips() {
        assert_from_raw_roundtrips!(MulticamGroupId::from_raw);
    }
    #[test]
    fn sequence_id_from_raw_roundtrips() {
        assert_from_raw_roundtrips!(SequenceId::from_raw);
    }

    // --- next() allocation ------------------------------------------------

    #[test]
    fn next_allocates_monotonically_within_a_type() {
        let a = ClipId::next();
        let b = ClipId::next();
        let c = ClipId::next();
        assert_ne!(a, b);
        assert_ne!(b, c);
        assert!(a.raw() < b.raw());
        assert!(b.raw() < c.raw());
    }

    #[test]
    fn each_id_type_has_its_own_counter() {
        // Counters are per-type statics — two different types can share the
        // same raw value without being comparable to each other.
        let project = ProjectId::next();
        let media = MediaId::next();
        let track = TrackId::next();
        let clip = ClipId::next();

        // All are freshly allocated; raw values are independent per type.
        assert!(project.raw() >= 1);
        assert!(media.raw() >= 1);
        assert!(track.raw() >= 1);
        assert!(clip.raw() >= 1);
    }

    #[test]
    fn from_raw_and_next_are_independent() {
        let fixed = TrackId::from_raw(999);
        let allocated = TrackId::next();
        assert_eq!(fixed.raw(), 999);
        // `next()` does not consult previously `from_raw` values.
        assert_ne!(allocated, fixed);
    }

    /// The class closed here is "a hardcoded raw id stands in for an absent one".
    /// Ids come from one process-global counter, so any small raw value is a
    /// value the allocator hands out sooner or later: a test asserting that
    /// `from_raw(77)` is unknown passes until a test binary allocates 77 ids,
    /// then fails with a real object in hand. An id that was allocated and
    /// never inserted is absent from every container at any counter value.
    #[test]
    fn a_small_raw_id_is_a_value_the_allocator_hands_out() {
        let allocated: Vec<TrackId> = (0..64).map(|_| TrackId::next()).collect();
        let highest = allocated.last().expect("64 ids").raw();
        // Some raw value at or below the counter names one of them, which is
        // why a raw constant cannot mean "missing".
        let collides = allocated.iter().any(|id| id.raw() <= highest);
        assert!(
            collides,
            "the allocator handed out no id at or below {highest}"
        );
        // A fresh id, never stored anywhere, is what "missing" has to mean.
        assert!(TrackId::next().raw() > highest);
    }

    #[test]
    fn deserialize_reseeds_the_allocator() {
        // Simulates loading a persisted id: a huge raw value (well above
        // anything `next()` reaches during the test run, so it's immune to
        // parallel allocation) must push the allocator past itself, so no
        // freshly created id can collide with (and overwrite) the loaded one.
        // This is the fix for the cross-session undo data-loss bug.
        let loaded: ClipId = serde_json::from_str("281474976710656").unwrap(); // 1 << 48
        assert_eq!(loaded.raw(), 1 << 48);
        assert!(
            ClipId::next().raw() > loaded.raw(),
            "next() must not re-hand a deserialized id"
        );
    }

    #[test]
    fn deserialize_never_lowers_the_allocator() {
        // A small loaded id must not drag the allocator backwards.
        let before = MediaId::next().raw();
        let _: MediaId = serde_json::from_str("1").unwrap();
        assert!(
            MediaId::next().raw() > before,
            "a low deserialized id leaves the allocator monotonic"
        );
    }

    /// `name`'s reader rejects every id above [`MAX_ID`] with a message stating
    /// the type, the value and the ceiling.
    fn rejects_above_the_ceiling<T: serde::de::DeserializeOwned + std::fmt::Debug>(name: &str) {
        for value in [MAX_ID + 1, i64::MAX as u64, u64::MAX] {
            let err = serde_json::from_str::<T>(&value.to_string())
                .expect_err(&format!("{name} {value} read as an id"));
            let expected =
                format!("{name} {value} is above {MAX_ID}, the largest id a project holds");
            assert!(
                err.to_string().starts_with(&expected),
                "{name} {value}: {err}"
            );
        }
    }

    /// WHY: every id a file or a command states advances its type's
    /// allocator past itself, so an id of `u64::MAX` wrapped the allocator,
    /// the next allocation re-handed that id, and creating a bin replaced the
    /// bin a loaded file held at it; undoing the creation deleted both. Every
    /// id type is read from a value above the ceiling, and the list is checked
    /// against the `define_id!` invocations in this file, so a new id type
    /// fails here until it is listed. Does not catch an id that reaches the
    /// model through `from_raw`, which does not advance the allocator.
    #[test]
    fn every_id_type_rejects_an_id_above_the_ceiling() {
        let checked = [
            (
                "ProjectId",
                rejects_above_the_ceiling::<ProjectId> as fn(&str),
            ),
            ("MediaId", rejects_above_the_ceiling::<MediaId>),
            ("TrackId", rejects_above_the_ceiling::<TrackId>),
            ("ClipId", rejects_above_the_ceiling::<ClipId>),
            ("LinkId", rejects_above_the_ceiling::<LinkId>),
            ("GroupId", rejects_above_the_ceiling::<GroupId>),
            ("MarkerId", rejects_above_the_ceiling::<MarkerId>),
            ("BinId", rejects_above_the_ceiling::<BinId>),
            ("TemplateId", rejects_above_the_ceiling::<TemplateId>),
            (
                "MulticamGroupId",
                rejects_above_the_ceiling::<MulticamGroupId>,
            ),
            ("SequenceId", rejects_above_the_ceiling::<SequenceId>),
        ];
        let defined = include_str!("ids.rs")
            .matches(concat!("define_", "id!(\n"))
            .count();
        assert_eq!(
            checked.len(),
            defined,
            "an id type is missing from this list"
        );
        for (name, check) in checked {
            check(name);
        }
    }

    /// The ceiling itself is an id and the next value is not. Checked on the
    /// bound rather than read: a read at the ceiling advances that type's
    /// allocator past it for the rest of the test process, and every id
    /// allocated after it would then be refused when a later test reopens
    /// what it saved.
    #[test]
    fn the_ceiling_is_the_largest_id_read() {
        assert_eq!(within_ceiling("BinId", MAX_ID), Ok(MAX_ID));
        assert_eq!(
            within_ceiling("BinId", MAX_ID + 1),
            Err(format!(
                "BinId {} is above {MAX_ID}, the largest id a project holds",
                MAX_ID + 1
            ))
        );
    }

    // --- Display ----------------------------------------------------------

    #[test]
    fn display_formats_type_name_and_raw_value() {
        assert_eq!(ProjectId::from_raw(1).to_string(), "ProjectId#1");
        assert_eq!(MediaId::from_raw(2).to_string(), "MediaId#2");
        assert_eq!(TrackId::from_raw(3).to_string(), "TrackId#3");
        assert_eq!(ClipId::from_raw(4).to_string(), "ClipId#4");
        assert_eq!(TemplateId::from_raw(5).to_string(), "TemplateId#5");
    }

    // --- Ord ----------------------------------------------------------------

    #[test]
    fn ordering_follows_raw_numeric_order() {
        let low = TrackId::from_raw(1);
        let mid = TrackId::from_raw(50);
        let high = TrackId::from_raw(100);
        assert!(low < mid);
        assert!(mid < high);
        assert!(high > low);
    }

    #[test]
    fn ids_sort_by_raw_value() {
        let mut ids = [
            ClipId::from_raw(30),
            ClipId::from_raw(10),
            ClipId::from_raw(20),
        ];
        ids.sort();
        assert_eq!(ids.map(|id| id.raw()), [10, 20, 30]);
    }

    // --- Hash -------------------------------------------------------------

    #[test]
    fn hash_is_stable_for_equal_ids() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        fn hash_of<T: Hash>(value: &T) -> u64 {
            let mut hasher = DefaultHasher::new();
            value.hash(&mut hasher);
            hasher.finish()
        }

        let a = MediaId::from_raw(42);
        let b = MediaId::from_raw(42);
        assert_eq!(hash_of(&a), hash_of(&b));
        assert_ne!(hash_of(&a), hash_of(&MediaId::from_raw(43)));
    }

    #[test]
    fn ids_work_as_hashset_keys() {
        let mut set = HashSet::new();
        let a = ProjectId::from_raw(1);
        let b = ProjectId::from_raw(2);
        assert!(set.insert(a));
        assert!(set.insert(b));
        assert!(!set.insert(a)); // duplicate
        assert_eq!(set.len(), 2);
        assert!(set.contains(&ProjectId::from_raw(1)));
    }
}
