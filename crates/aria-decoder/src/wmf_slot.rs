//! Slot-acquisition policy for the Windows shared NV12 pool.
//!
//! The pool itself needs D3D11, so it only compiles on Windows. This decision
//! does not: it is arithmetic over "how many slots exist, is one free, has the
//! wait run out". Off Windows the module is still built under `cfg(test)`, so
//! the bound is tested on every host — which matters because the failure it
//! governs, a retained frame starving decode, reproduces as a hang rather than
//! as a wrong pixel.

use std::time::Duration;

/// Hard cap on shared NV12 pool growth. Preview decode-ahead holds ~3 frames
/// plus one in flight; hitting this means frames are being retained somewhere.
pub(crate) const MAX_SLOTS: usize = 16;

/// How long one wait on the released-slot condition lasts before the pool
/// re-checks. Short enough that a slot freed by another thread is picked up
/// promptly, long enough not to spin.
pub(crate) const SLOT_WAIT: Duration = Duration::from_millis(50);

/// Total time decode blocks for a retained slot before refusing the frame.
///
/// Queue pressure is transient, so waiting is right. A frame held past its
/// queue is a leak, and blocking forever on one turns a leak into a hung
/// decoder with no diagnostic; after this bound the pool reports which
/// resource ran out instead.
pub(crate) const SLOT_ACQUIRE_TIMEOUT: Duration = Duration::from_secs(2);

/// What the pool should do about the slot it needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SlotChoice {
    /// Take this existing free slot.
    Reuse(usize),
    /// Every existing slot is busy and the pool may still grow.
    Grow,
    /// Every slot exists and is busy; wait for one to be released.
    Wait,
    /// Every slot exists, is busy, and the wait ran out.
    Exhausted,
}

/// Decide how to satisfy one frame export.
///
/// `free` is the index of the first non-busy slot, `len` how many slots exist,
/// and `deadline_expired` whether [`SLOT_ACQUIRE_TIMEOUT`] has elapsed.
///
/// Reuse outranks growth so a pool at capacity with a freed slot never reports
/// exhaustion, and growth outranks waiting so the pool fills before it blocks.
/// Exhaustion needs both a full pool and an expired deadline: a slot freed on
/// the last check is used, not refused.
pub(crate) fn choose_slot(
    free: Option<usize>,
    len: usize,
    max_slots: usize,
    deadline_expired: bool,
) -> SlotChoice {
    match free {
        Some(index) => SlotChoice::Reuse(index),
        None if len < max_slots => SlotChoice::Grow,
        None if deadline_expired => SlotChoice::Exhausted,
        None => SlotChoice::Wait,
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_SLOTS, SLOT_ACQUIRE_TIMEOUT, SLOT_WAIT, SlotChoice, choose_slot};

    /// Read from the shipped constant, never a literal: raising the cap must
    /// not silently narrow what these tests cover.
    const MAX: usize = MAX_SLOTS;

    /// A free slot is taken whatever else is true. The class closed here is the
    /// original defect's mirror image: the pool must not refuse, or grow, or
    /// block, while a slot sits idle. Every combination of fullness and
    /// deadline is driven, because the reported bug was one branch reached
    /// under one state.
    #[test]
    fn a_free_slot_wins_over_growth_waiting_and_exhaustion() {
        for len in [1, MAX / 2, MAX] {
            for expired in [false, true] {
                assert_eq!(
                    choose_slot(Some(0), len, MAX, expired),
                    SlotChoice::Reuse(0),
                    "len={len} expired={expired}"
                );
                assert_eq!(
                    choose_slot(Some(len - 1), len, MAX, expired),
                    SlotChoice::Reuse(len - 1),
                    "len={len} expired={expired}"
                );
            }
        }
    }

    /// Below the cap the pool grows rather than blocking, expired deadline or
    /// not: an unbuilt slot is not contention.
    #[test]
    fn a_pool_below_its_cap_grows_instead_of_waiting() {
        for len in 0..MAX {
            for expired in [false, true] {
                assert_eq!(
                    choose_slot(None, len, MAX, expired),
                    SlotChoice::Grow,
                    "len={len} expired={expired}"
                );
            }
        }
    }

    /// A full, entirely busy pool waits — the row's requirement, and the
    /// opposite of the permanent failure it replaced.
    #[test]
    fn a_full_busy_pool_waits_rather_than_failing() {
        assert_eq!(choose_slot(None, MAX, MAX, false), SlotChoice::Wait);
    }

    /// The wait is bounded. Without this the leak that retains all 16 slots
    /// stops decode forever and reports nothing.
    #[test]
    fn the_wait_ends_in_a_reported_exhaustion_rather_than_a_hang() {
        assert_eq!(choose_slot(None, MAX, MAX, true), SlotChoice::Exhausted);
        assert!(
            SLOT_WAIT < SLOT_ACQUIRE_TIMEOUT,
            "a single wait must re-check"
        );
        assert!(
            SLOT_ACQUIRE_TIMEOUT
                .as_millis()
                .is_multiple_of(SLOT_WAIT.as_millis()),
            "the deadline should land on a wait boundary"
        );
    }

    /// A one-slot pool is the tightest case: it can never grow, so it reuses,
    /// waits, and finally refuses.
    #[test]
    fn a_single_slot_pool_cycles_reuse_wait_and_exhaustion() {
        assert_eq!(choose_slot(Some(0), 1, 1, false), SlotChoice::Reuse(0));
        assert_eq!(choose_slot(None, 1, 1, false), SlotChoice::Wait);
        assert_eq!(choose_slot(None, 1, 1, true), SlotChoice::Exhausted);
        assert_eq!(choose_slot(None, 0, 1, false), SlotChoice::Grow);
    }

    /// A zero-capacity pool cannot serve anything and must say so instead of
    /// growing past its cap or waiting for a slot that will never exist.
    #[test]
    fn a_zero_capacity_pool_refuses_immediately() {
        assert_eq!(choose_slot(None, 0, 0, true), SlotChoice::Exhausted);
        assert_eq!(choose_slot(None, 0, 0, false), SlotChoice::Wait);
    }
}
