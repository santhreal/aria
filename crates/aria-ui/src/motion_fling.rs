//! What a released gesture is worth: the velocity a drag was carrying, and the
//! distance that velocity is allowed to throw.
//!
//! A drag under the hand is the hand's: every frame draws where the pointer put
//! it. The frame the pointer leaves is the only one that needs physics, and it
//! needs one number — how far the thing should keep going. That number is a
//! distance, so the travel to it is an ordinary transition on the one catalog
//! spec that overshoots ([`super::FLING`]) rather than a second animation
//! system integrating velocity per frame.

/// Fastest rate a single sample is believed at, in pixels per millisecond.
///
/// 6 px/ms is 6000 px/s: faster than a hand crosses a 4K screen in a second.
/// One sample above it is a pointer that jumped — a window drag, a warp, a
/// tablet arriving somewhere else — and without a ceiling that one sample would
/// throw the view across the sequence.
const MAX_RATE: f32 = 6.0;

/// Weight of the newest sample in the running velocity.
///
/// Low enough that a single jittery frame cannot decide the throw, high enough
/// that the end of the gesture is what the throw is made of: after three
/// samples at a new rate, 78% of the old rate is gone.
const BLEND: f32 = 0.4;

/// A gap this long, in milliseconds, means the samples before it are not part of
/// this movement. Both ends use it: a gap mid-drag restarts the running
/// velocity, and a gap before the release means the hand had already stopped, so
/// the release throws nothing however fast the drag once was.
const STALE_MS: f32 = 90.0;

/// Below this rate the gesture placed the view rather than threw it, and a
/// placement that drifts on is a placement the operator has to correct.
const MIN_RATE: f32 = 0.05;

/// How long the momentum is worth, in milliseconds of travel at the release
/// rate. Shorter than the transition that carries it: the spring spends its
/// length decelerating, so this is the distance, not the duration.
const COAST_MS: f32 = 220.0;

/// The running velocity after one pointer sample, in pixels per millisecond.
///
/// `dt_ms` is the time since the previous sample and `delta_px` the distance
/// since it. A sample with no time behind it has no rate and leaves the
/// velocity alone; one arriving after a gap replaces the velocity instead of
/// blending into it, because the movement it belongs to is a new one.
pub(crate) fn sample_velocity(previous: f32, delta_px: f32, dt_ms: f32) -> f32 {
    if !(dt_ms > 0.0) {
        return previous;
    }
    let raw = (delta_px / dt_ms).clamp(-MAX_RATE, MAX_RATE);
    if dt_ms >= STALE_MS {
        return raw;
    }
    previous * (1.0 - BLEND) + raw * BLEND
}

/// How far a release at `velocity` keeps travelling, in pixels, signed the way
/// the movement was.
///
/// `age_ms` is how old the last sample is at the release. Zero means the
/// gesture is not a throw: it was too slow, it had already stopped, or the
/// operator turned motion off — and a zero is worth returning rather than
/// hiding, because the caller's other branch is "leave the view where the hand
/// left it", which is exactly right for all three.
pub(crate) fn fling_distance(velocity: f32, age_ms: f32, reduced: bool) -> f32 {
    if reduced || age_ms >= STALE_MS || velocity.abs() < MIN_RATE || !velocity.is_finite() {
        return 0.0;
    }
    velocity * COAST_MS
}

#[cfg(test)]
#[path = "motion_fling_tests.rs"]
mod tests;
