//! A scalar that travels to its target instead of jumping to it.
//!
//! Two things in the inspector move between values and cannot remember where
//! they were: the underline under the active tab, and the fill of every
//! property slider. GPUI builds a fresh element tree every frame, so "where it
//! was" has to live outside the tree. This is that store, and it is the only
//! one — the underline and the sliders travel by the same rule, so they cannot
//! drift into two different feels.
//!
//! Keys are registry ids: a tab id from [`super::tab_host`], a row id from the
//! property catalog. The map is therefore bounded by those registries and does
//! not grow with the project, the session, or the number of clips, which is
//! why nothing here retires entries.

use std::cell::RefCell;
use std::collections::HashMap;
use std::time::Instant;

use crate::motion::{self, MotionSpec};

/// One value in flight.
#[derive(Clone, Copy)]
struct Travel {
    from: f32,
    to: f32,
    started: Instant,
}

impl Travel {
    /// Where the value is now. Past the end it is simply the target, so a
    /// settled control costs a comparison rather than a curve evaluation.
    fn at(&self, spec: &MotionSpec, now: Instant) -> f32 {
        let total = spec.total().mul_f32(motion::speed_scale());
        if total.is_zero() {
            return self.to;
        }
        let raw = now.saturating_duration_since(self.started).as_secs_f32() / total.as_secs_f32();
        if raw >= 1.0 {
            return self.to;
        }
        motion::lerp(self.from, self.to, spec.progress(raw))
    }
}

thread_local! {
    static TRAVELS: RefCell<HashMap<crate::motion::MotionKey, Travel>> = RefCell::new(HashMap::new());
}

/// Where `key` is this frame, having been asked to be at `target`.
///
/// Retargets from wherever it had reached, so a slider nudged twice in quick
/// succession keeps moving rather than restarting from the first nudge's
/// origin. Reduced motion pins the value to its target: a travel nobody wants
/// to watch should not cost frames to draw.
pub(in crate::panels::inspector) fn toward(
    key: crate::motion::MotionKey,
    target: f32,
    spec: &MotionSpec,
    reduced: bool,
) -> f32 {
    let now = Instant::now();
    TRAVELS.with(|cell| {
        let mut travels = cell.borrow_mut();
        let next = if reduced {
            Travel {
                from: target,
                to: target,
                started: now,
            }
        } else {
            match travels.get(&key) {
                Some(travel) if travel.to == target => *travel,
                Some(travel) => Travel {
                    from: travel.at(spec, now),
                    to: target,
                    started: now,
                },
                None => Travel {
                    from: target,
                    to: target,
                    started: now,
                },
            }
        };
        match travels.get_mut(&key) {
            Some(slot) => *slot = next,
            None => {
                travels.insert(key, next);
            }
        }
        next.at(spec, now)
    })
}
