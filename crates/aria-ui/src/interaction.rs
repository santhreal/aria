//! Cross-thread "the preview is being scrubbed or played" flag.
//!
//! Background decode should defer during a scrub or playback.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

const SCRUB_HOLDOFF: Duration = Duration::from_millis(500);

/// Shared interaction state. Clone the [`Arc`] into every worker that
/// should yield to the preview.
#[derive(Debug)]
pub struct InteractionGate {
    last_touch_ms: AtomicU64,
    playing: AtomicBool,
    epoch: Instant,
}

impl InteractionGate {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            last_touch_ms: AtomicU64::new(u64::MAX),
            playing: AtomicBool::new(false),
            epoch: Instant::now(),
        })
    }

    pub fn touch(&self) {
        let ms = self.epoch.elapsed().as_millis() as u64;
        self.last_touch_ms.store(ms, Ordering::Relaxed);
    }

    pub fn set_playing(&self, playing: bool) {
        self.playing.store(playing, Ordering::Relaxed);
    }

    pub fn busy(&self) -> bool {
        if self.playing.load(Ordering::Relaxed) {
            return true;
        }
        let last = self.last_touch_ms.load(Ordering::Relaxed);
        if last == u64::MAX {
            return false;
        }
        let now = self.epoch.elapsed().as_millis() as u64;
        now.saturating_sub(last) < SCRUB_HOLDOFF.as_millis() as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_opens_when_idle_and_closes_on_interaction() {
        let gate = InteractionGate::new();
        assert!(!gate.busy(), "fresh gate is open");

        gate.touch();
        assert!(gate.busy(), "a scrub tick closes the gate");

        gate.set_playing(true);
        assert!(gate.busy(), "playback holds the gate closed");
        gate.set_playing(false);
        assert!(gate.busy());
    }
}
