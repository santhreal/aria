//! Lock-free runtime cost ledger for mounted plugins.
//!
//! Each mounted plugin has one [`PluginCostSlot`] holding atomic accumulators
//! for per-frame render work and per-block audio processor work. Registration
//! captures a [`Weak`](std::sync::Weak) pointer to the plugin's slot inside the
//! compositor and audio cost sinks, so the render and audio threads record
//! timings with atomic operations alone and never contend on a lock or allocate.
//! Unmounting or failing a plugin drops its [`Arc`] from the ledger, which
//! invalidates every outstanding [`Weak`] handle and starts a remounted plugin
//! with zeroed counters.

use std::borrow::Cow;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;

use parking_lot::RwLock;
use rustc_hash::FxHashMap;

const UNSET_NS: u64 = u64::MAX;

/// Measured runtime cost for one category of plugin work (per-frame video
/// render or per-block audio processing).
///
/// For a [`CpuEffect`](aria_compositor::CpuEffect), frame cost measures the CPU
/// callback together with its `u8 <-> f16` staging conversion and texture
/// upload. For a WGSL effect or transition, frame cost measures the CPU-side
/// uniform preparation, bind-group lookup, and render-pass command encoding
/// across the effect's passes (the compositor device does not enable GPU
/// timestamp queries). For an audio processor, block cost measures each
/// [`AudioProcessor::process`](aria_render::AudioProcessor::process) call on
/// the mixer thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CostMetric {
    /// Total recorded invocations since the plugin mounted.
    pub calls: u64,
    /// Duration of the most recent invocation.
    pub last: Duration,
    /// Exponentially smoothed mean duration (weight `1/8` per new sample,
    /// seeded by the first sample).
    pub mean: Duration,
    /// Maximum duration observed across all invocations since mount.
    pub max: Duration,
}

impl CostMetric {
    /// Whether no invocations have been recorded yet.
    pub const fn is_empty(&self) -> bool {
        self.calls == 0
    }

    /// Most recent invocation duration in milliseconds, or `None` when
    /// [`calls`](Self::calls) is zero.
    pub fn last_ms(&self) -> Option<f32> {
        (self.calls > 0).then_some(self.last.as_secs_f32() * 1_000.0)
    }

    /// Smoothed mean invocation duration in milliseconds, or `None` when
    /// [`calls`](Self::calls) is zero.
    pub fn mean_ms(&self) -> Option<f32> {
        (self.calls > 0).then_some(self.mean.as_secs_f32() * 1_000.0)
    }

    /// Maximum invocation duration in milliseconds, or `None` when
    /// [`calls`](Self::calls) is zero.
    pub fn max_ms(&self) -> Option<f32> {
        (self.calls > 0).then_some(self.max.as_secs_f32() * 1_000.0)
    }
}

/// Measured per-frame and per-audio-block runtime costs for one mounted plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginCost {
    /// Manifest id of the plugin.
    pub plugin: Cow<'static, str>,
    /// Per-frame video effect, transition, and generator cost.
    pub frame: CostMetric,
    /// Per-block audio processor cost.
    pub block: CostMetric,
}

#[derive(Debug)]
pub(crate) struct AtomicMetric {
    calls: AtomicU64,
    last_ns: AtomicU64,
    mean_ns: AtomicU64,
    max_ns: AtomicU64,
}

impl AtomicMetric {
    pub(crate) const fn new() -> Self {
        Self {
            calls: AtomicU64::new(0),
            last_ns: AtomicU64::new(0),
            mean_ns: AtomicU64::new(UNSET_NS),
            max_ns: AtomicU64::new(0),
        }
    }

    pub(crate) fn record(&self, duration: Duration) {
        let sample_ns = u64::try_from(duration.as_nanos()).unwrap_or(UNSET_NS - 1);
        self.last_ns.store(sample_ns, Ordering::Relaxed);
        let _ = self
            .mean_ns
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |prev| {
                let next = if prev == UNSET_NS {
                    sample_ns
                } else {
                    ((u128::from(prev) * 7 + u128::from(sample_ns) + 4) / 8) as u64
                };
                Some(next)
            });
        self.max_ns.fetch_max(sample_ns, Ordering::Relaxed);
        self.calls.fetch_add(1, Ordering::Release);
    }

    pub(crate) fn snapshot(&self) -> CostMetric {
        let calls = self.calls.load(Ordering::Acquire);
        if calls == 0 {
            return CostMetric::default();
        }
        let last_ns = self.last_ns.load(Ordering::Relaxed);
        let raw_mean = self.mean_ns.load(Ordering::Relaxed);
        let mean_ns = if raw_mean == UNSET_NS {
            last_ns
        } else {
            raw_mean
        };
        let max_ns = self.max_ns.load(Ordering::Relaxed);
        CostMetric {
            calls,
            last: Duration::from_nanos(last_ns),
            mean: Duration::from_nanos(mean_ns),
            max: Duration::from_nanos(max_ns),
        }
    }
}

#[derive(Debug)]
pub(crate) struct PluginCostSlot {
    pub(crate) frame: AtomicMetric,
    pub(crate) block: AtomicMetric,
}

impl PluginCostSlot {
    pub(crate) const fn new() -> Self {
        Self {
            frame: AtomicMetric::new(),
            block: AtomicMetric::new(),
        }
    }

    pub(crate) fn record_frame(&self, duration: Duration) {
        self.frame.record(duration);
    }

    pub(crate) fn record_block(&self, duration: Duration) {
        self.block.record(duration);
    }
}

#[derive(Debug, Default)]
pub(crate) struct CostLedger {
    slots: RwLock<FxHashMap<Cow<'static, str>, Arc<PluginCostSlot>>>,
}

impl CostLedger {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Ensure a fresh zeroed slot for `plugin` when mounting begins.
    pub(crate) fn reset(&self, plugin: Cow<'static, str>) -> Arc<PluginCostSlot> {
        let slot = Arc::new(PluginCostSlot::new());
        self.slots.write().insert(plugin, Arc::clone(&slot));
        slot
    }

    /// Return a weak handle to `plugin`'s cost slot, creating the slot if
    /// needed (e.g. when a scoped view registers an effect outside `mount`).
    pub(crate) fn weak_slot(&self, plugin: Cow<'static, str>) -> Weak<PluginCostSlot> {
        if let Some(slot) = self.slots.read().get(plugin.as_ref()) {
            return Arc::downgrade(slot);
        }
        let mut slots = self.slots.write();
        let slot = slots
            .entry(plugin)
            .or_insert_with(|| Arc::new(PluginCostSlot::new()));
        Arc::downgrade(slot)
    }

    /// Record a frame duration directly by plugin id.
    pub(crate) fn record_frame(&self, plugin: &str, duration: Duration) {
        if let Some(slot) = self.slots.read().get(plugin) {
            slot.record_frame(duration);
        }
    }

    /// Record an audio block duration directly by plugin id.
    pub(crate) fn record_block(&self, plugin: &str, duration: Duration) {
        if let Some(slot) = self.slots.read().get(plugin) {
            slot.record_block(duration);
        }
    }

    /// Drop `plugin`'s slot on unmount or failure so outstanding weak sinks
    /// stop recording and a later mount starts fresh.
    pub(crate) fn remove(&self, plugin: &str) {
        self.slots.write().remove(plugin);
    }

    /// Current `(frame, block)` metrics for `plugin`, or empty metrics when no
    /// slot exists.
    pub(crate) fn snapshot_for(&self, plugin: &str) -> (CostMetric, CostMetric) {
        self.slots
            .read()
            .get(plugin)
            .map(|slot| (slot.frame.snapshot(), slot.block.snapshot()))
            .unwrap_or_default()
    }

    /// Current [`PluginCost`] for `plugin`, if mounted/tracked.
    pub(crate) fn cost_of(&self, plugin: &str) -> Option<PluginCost> {
        let slots = self.slots.read();
        let (key, slot) = slots.get_key_value(plugin)?;
        Some(PluginCost {
            plugin: key.clone(),
            frame: slot.frame.snapshot(),
            block: slot.block.snapshot(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WHY: The Plugins panel reads `CostMetric` to show per-frame and
    /// per-block runtime cost. An uncalled plugin must report `None` (`calls == 0`)
    /// rather than `0.0 ms`, while recorded samples must track `last`, smoothed
    /// `mean`, `max`, and `calls` per plugin id without cross-talk, and clearing
    /// a plugin on unmount must invalidate its old sink and start fresh.
    #[test]
    fn ledger_records_per_plugin_metrics_and_clears_on_remove() {
        let ledger = CostLedger::new();
        let glow_weak = ledger.weak_slot(Cow::Borrowed("com.example.glow"));
        let eq_weak = ledger.weak_slot(Cow::Borrowed("com.example.eq"));

        let (glow_frame, glow_block) = ledger.snapshot_for("com.example.glow");
        assert!(glow_frame.is_empty());
        assert!(glow_block.is_empty());
        assert_eq!(glow_frame.mean_ms(), None);
        assert_eq!(glow_block.mean_ms(), None);

        let glow = glow_weak.upgrade().expect("glow slot alive");
        glow.record_frame(Duration::from_micros(2_000));
        glow.record_frame(Duration::from_micros(10_000));

        let eq = eq_weak.upgrade().expect("eq slot alive");
        eq.record_block(Duration::from_micros(40));

        let glow_cost = ledger.cost_of("com.example.glow").expect("glow cost");
        assert_eq!(glow_cost.frame.calls, 2);
        assert_eq!(glow_cost.frame.last, Duration::from_micros(10_000));
        assert_eq!(glow_cost.frame.mean, Duration::from_micros(3_000));
        assert_eq!(glow_cost.frame.max, Duration::from_micros(10_000));
        assert!(glow_cost.block.is_empty());

        let eq_cost = ledger.cost_of("com.example.eq").expect("eq cost");
        assert!(eq_cost.frame.is_empty());
        assert_eq!(eq_cost.block.calls, 1);
        assert_eq!(eq_cost.block.last, Duration::from_micros(40));
        assert_eq!(eq_cost.block.mean, Duration::from_micros(40));
        assert_eq!(eq_cost.block.max, Duration::from_micros(40));

        // Removing a plugin drops its strong Arc so lingering weak handles no
        // longer record and a remount starts with zeroed counters.
        drop(glow);
        ledger.remove("com.example.glow");
        assert!(glow_weak.upgrade().is_none());
        assert!(ledger.cost_of("com.example.glow").is_none());

        let _fresh = ledger.reset(Cow::Borrowed("com.example.glow"));
        let (fresh_frame, fresh_block) = ledger.snapshot_for("com.example.glow");
        assert!(fresh_frame.is_empty());
        assert!(fresh_block.is_empty());
    }
}
