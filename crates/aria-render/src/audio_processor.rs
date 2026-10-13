//! Audio processors: the code behind an audio effect.
//!
//! An [`AudioEffectSpec`](aria_models::AudioEffectSpec) in the model catalog
//! states what an effect is: its parameters, its latency, whether it reads a
//! sidechain key. The processor for a shipped effect is built in; a plugin
//! registers a processor factory here under its effect's id. The mixer creates
//! one processor per insert when it opens over a project and drives it one
//! block at a time; an insert whose spec or processor is missing passes its
//! input through.
//!
//! A processor runs on the mixer's thread, inside `catch_unwind`. A panic
//! bypasses that insert for the rest of the mix and is reported to the fault
//! sink registered with the factory. During playback a block has a deadline,
//! the real time it covers; three misses in a row bypass the insert the same
//! way. A bypassed insert keeps delaying its input by its declared latency, so
//! the rest of the mix stays aligned.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, PoisonError, RwLock};
use std::time::Duration;

/// One instance of an audio effect, processing a stream in blocks.
pub trait AudioProcessor: Send {
    /// Process one block in place.
    fn process(&mut self, block: &mut AudioBlock<'_>);

    /// Drop every value carried over from earlier blocks: delay lines,
    /// envelopes, filter state. Called before the mixer starts a stretch that
    /// does not continue the last block, such as after a seek.
    fn reset(&mut self);
}

/// Makes processors for one registered audio effect.
pub trait AudioProcessorFactory: Send + Sync {
    /// A new processor at `sample_rate` frames per second, with fresh state.
    fn create(&self, sample_rate: u32) -> Box<dyn AudioProcessor>;
}

impl<F> AudioProcessorFactory for F
where
    F: Fn(u32) -> Box<dyn AudioProcessor> + Send + Sync,
{
    fn create(&self, sample_rate: u32) -> Box<dyn AudioProcessor> {
        self(sample_rate)
    }
}

/// One parameter's value across a block.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ParamSignal<'a> {
    /// The same value for every frame.
    Constant(f32),
    /// One value per frame, for an automated parameter.
    PerFrame(&'a [f32]),
}

impl ParamSignal<'_> {
    /// The value at `frame`. A frame past a per-frame signal reads its last
    /// value.
    pub fn at(&self, frame: usize) -> f32 {
        match self {
            Self::Constant(value) => *value,
            Self::PerFrame(values) => values
                .get(frame)
                .or(values.last())
                .copied()
                .unwrap_or_default(),
        }
    }

    /// The value, when it holds for the whole block.
    pub fn constant(&self) -> Option<f32> {
        match self {
            Self::Constant(value) => Some(*value),
            Self::PerFrame(_) => None,
        }
    }
}

/// A block's parameter values: signals built by a caller, or the mixer's
/// table, which costs no allocation per block.
#[derive(Debug, Clone, Copy)]
enum Params<'a> {
    Signals(&'a [ParamSignal<'a>]),
    /// A constant per slot, or `None` with that slot's values in `automated`
    /// at `slot * frames`.
    Table {
        constants: &'a [Option<f32>],
        automated: &'a [f32],
    },
}

/// One block of stereo audio handed to a processor.
///
/// Channels are planar and equally long. Parameter values are in the ranges
/// the effect's spec declares, one signal per parameter slot in spec order.
#[derive(Debug)]
pub struct AudioBlock<'a> {
    left: &'a mut [f32],
    right: &'a mut [f32],
    position: i64,
    sample_rate: u32,
    params: Params<'a>,
    sidechain: Option<(&'a [f32], &'a [f32])>,
}

impl<'a> AudioBlock<'a> {
    /// A block over `left` and `right`. Panics when the channels differ in
    /// length, or a per-frame parameter or a sidechain channel is not as long
    /// as them.
    pub fn new(
        left: &'a mut [f32],
        right: &'a mut [f32],
        position: i64,
        sample_rate: u32,
        params: &'a [ParamSignal<'a>],
        sidechain: Option<(&'a [f32], &'a [f32])>,
    ) -> Self {
        let frames = left.len();
        assert_eq!(frames, right.len(), "planar channels differ in length");
        for param in params {
            if let ParamSignal::PerFrame(values) = param {
                assert_eq!(
                    values.len(),
                    frames,
                    "a per-frame parameter covers the block"
                );
            }
        }
        if let Some((key_left, key_right)) = sidechain {
            assert!(
                key_left.len() == frames && key_right.len() == frames,
                "a sidechain key is as long as the block"
            );
        }
        Self {
            left,
            right,
            position,
            sample_rate,
            params: Params::Signals(params),
            sidechain,
        }
    }

    /// The mixer's block. Every length is the mixer's to keep consistent.
    pub(crate) fn from_table(
        left: &'a mut [f32],
        right: &'a mut [f32],
        position: i64,
        sample_rate: u32,
        constants: &'a [Option<f32>],
        automated: &'a [f32],
        sidechain: Option<(&'a [f32], &'a [f32])>,
    ) -> Self {
        Self {
            left,
            right,
            position,
            sample_rate,
            params: Params::Table {
                constants,
                automated,
            },
            sidechain,
        }
    }

    /// Frames in the block.
    pub fn frames(&self) -> usize {
        self.left.len()
    }

    /// Both channels, for processing in place.
    pub fn channels(&mut self) -> (&mut [f32], &mut [f32]) {
        (&mut *self.left, &mut *self.right)
    }

    /// The timeline position of the block's first frame, in frames at
    /// [`sample_rate`](Self::sample_rate): the time the signal in the block
    /// was recorded at. Consecutive blocks continue each other until the next
    /// [`AudioProcessor::reset`].
    pub fn position(&self) -> i64 {
        self.position
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Parameter slot `slot`, or `None` past the spec's parameters.
    pub fn param(&self, slot: usize) -> Option<ParamSignal<'a>> {
        match self.params {
            Params::Signals(signals) => signals.get(slot).copied(),
            Params::Table {
                constants,
                automated,
            } => match constants.get(slot)? {
                Some(value) => Some(ParamSignal::Constant(*value)),
                None => {
                    let frames = self.left.len();
                    automated
                        .get(slot * frames..(slot + 1) * frames)
                        .map(ParamSignal::PerFrame)
                }
            },
        }
    }

    /// The key signal, planar, for an effect that reads one and an insert
    /// with a key lane set. A key lane that is silent or muted reads as
    /// zeros. The key outlives the borrow, so it can be read while the
    /// channels are written.
    pub fn sidechain(&self) -> Option<(&'a [f32], &'a [f32])> {
        self.sidechain
    }
}

/// Why an insert stopped processing.
#[derive(Debug, Clone, PartialEq)]
pub enum FaultKind {
    /// The processor, or its factory, panicked with this message.
    Panicked(String),
    /// Three blocks in a row took longer than the real time they cover.
    MissedDeadlines { budget: Duration, took: Duration },
}

/// An insert the mixer bypassed because its processor failed.
#[derive(Debug, Clone, PartialEq)]
pub struct ProcessorFault {
    pub effect_id: String,
    pub kind: FaultKind,
}

impl std::fmt::Display for ProcessorFault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.kind {
            FaultKind::Panicked(message) => write!(
                f,
                "audio effect {} panicked and was bypassed: {message}",
                self.effect_id
            ),
            FaultKind::MissedDeadlines { budget, took } => write!(
                f,
                "audio effect {} missed three playback deadlines in a row (took {took:?} for a \
                 {budget:?} block) and was bypassed",
                self.effect_id
            ),
        }
    }
}

/// Receives the faults of one registered processor.
pub type FaultSink = Arc<dyn Fn(&ProcessorFault) + Send + Sync>;

struct Entry {
    id: String,
    token: u64,
    factory: Arc<dyn AudioProcessorFactory>,
    on_fault: Option<FaultSink>,
}

static PROCESSORS: RwLock<Vec<Entry>> = RwLock::new(Vec::new());
static NEXT_TOKEN: AtomicU64 = AtomicU64::new(1);

/// Keeps a processor registered; dropping it removes the factory. A mixer
/// already open keeps the processors it created.
#[derive(Debug)]
#[must_use = "dropping the registration removes the processor"]
pub struct ProcessorRegistration {
    token: u64,
}

impl Drop for ProcessorRegistration {
    fn drop(&mut self) {
        PROCESSORS
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|entry| entry.token != self.token);
    }
}

/// Register the factory for audio effect `id`. Faults of its processors go to
/// `on_fault`. Refused for an empty id, a shipped id, and an id already
/// registered.
pub fn register_audio_processor(
    id: impl Into<String>,
    factory: Arc<dyn AudioProcessorFactory>,
    on_fault: Option<FaultSink>,
) -> Result<ProcessorRegistration, String> {
    let id = id.into();
    if id.is_empty() {
        return Err("an audio processor needs an id".into());
    }
    if crate::audio_fx::shipped(&id).is_some() {
        return Err(format!("{id} is a shipped audio effect"));
    }
    let mut processors = PROCESSORS.write().unwrap_or_else(PoisonError::into_inner);
    if processors.iter().any(|entry| entry.id == id) {
        return Err(format!("an audio processor is already registered as {id}"));
    }
    let token = NEXT_TOKEN.fetch_add(1, Ordering::Relaxed);
    processors.push(Entry {
        id,
        token,
        factory,
        on_fault,
    });
    Ok(ProcessorRegistration { token })
}

/// Whether a plugin registered a processor as `id`. A shipped processor is
/// not registered.
pub fn audio_processor_registered(id: &str) -> bool {
    PROCESSORS
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .any(|entry| entry.id == id)
}

/// A created processor, or the fault its factory raised, with its fault sink.
type Created = (
    Result<Box<dyn AudioProcessor>, ProcessorFault>,
    Option<FaultSink>,
);

/// A new processor for `id` at `sample_rate`, with the sink its faults go to:
/// the registered one, else the shipped one, which has no sink.
///
/// `None` when there is neither. A factory that panics returns `Err` with the
/// fault, for the caller to report.
pub(crate) fn create_processor(id: &str, sample_rate: u32) -> Option<Created> {
    let registered = {
        let processors = PROCESSORS.read().unwrap_or_else(PoisonError::into_inner);
        processors
            .iter()
            .find(|entry| entry.id == id)
            .map(|entry| (Arc::clone(&entry.factory), entry.on_fault.clone()))
    };
    let (factory, on_fault) = match registered {
        Some(found) => found,
        None => {
            let shipped: Arc<dyn AudioProcessorFactory> =
                Arc::new(crate::audio_fx::shipped(id)?.make);
            (shipped, None)
        }
    };
    let created =
        catch_unwind(AssertUnwindSafe(|| factory.create(sample_rate))).map_err(|payload| {
            ProcessorFault {
                effect_id: id.to_owned(),
                kind: FaultKind::Panicked(panic_message(payload.as_ref())),
            }
        });
    Some((created, on_fault))
}

/// The text of a panic payload, or a placeholder for a payload that is not
/// text.
pub(crate) fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|text| (*text).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "a panic with no message".to_owned())
}

#[cfg(test)]
#[path = "audio_processor_tests.rs"]
mod tests;
