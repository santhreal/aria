//! What a drag is: which edge is moving, where it started, and what letting go
//! of it is worth.
//!
//! Split out of `shell.rs` as a pure move, so the chrome file holds the window
//! and this one holds the gesture.

use gpui::motion::fling;

use super::*;

#[derive(Clone, Copy)]
pub(super) enum DragKind {
    Sidebar,
    Inspector,
    Timeline,
    Pan,
    /// Resizing one timeline lane by its lower border. The height is previewed
    /// on the state and committed as a single edit on mouse-up, so a drag over
    /// forty pixels is one undo entry rather than forty.
    Lane(u64),
}
/// Copy, so a handler can sample the live drag and then read a snapshot of it
/// without holding a borrow of the shell across the state it has to update.
#[derive(Clone, Copy)]
pub(super) struct Drag {
    pub(super) kind: DragKind,
    pub(super) origin: f32,
    pub(super) start: f32,
    /// The last pointer position this drag was sampled at, and when. Only a pan
    /// spends them: a release needs the rate the hand was moving at, and GPUI
    /// hands out positions rather than velocities.
    sample: f32,
    sampled_at: Instant,
    /// Pixels per millisecond, smoothed across samples by
    /// [`fling::sample_velocity`], so one jittery frame cannot decide a throw.
    velocity: f32,
}

impl Drag {
    /// A drag that has been sampled once, at the point it started from.
    ///
    /// Every kind is built here so a drag that does not care about velocity
    /// cannot leave the fields uninitialised for the one that does.
    pub(super) fn new(kind: DragKind, origin: f32, start: f32) -> Self {
        Self {
            kind,
            origin,
            start,
            sample: origin,
            sampled_at: Instant::now(),
            velocity: 0.0,
        }
    }

    /// Fold one pointer position into the running velocity.
    pub(super) fn sample(&mut self, at: f32) {
        let now = Instant::now();
        let dt = now.saturating_duration_since(self.sampled_at).as_secs_f32() * 1000.0;
        self.velocity = fling::sample_velocity(self.velocity, at - self.sample, dt);
        self.sample = at;
        self.sampled_at = now;
    }

    /// How far the release of this drag keeps travelling, in pixels. Zero for
    /// every kind but a pan: a splitter that was let go is where it was let go.
    pub(super) fn throw(&self, reduced: bool) -> f32 {
        if !matches!(self.kind, DragKind::Pan) {
            return 0.0;
        }
        let age = Instant::now()
            .saturating_duration_since(self.sampled_at)
            .as_secs_f32()
            * 1000.0;
        fling::fling_distance(self.velocity, age, reduced)
    }
}
