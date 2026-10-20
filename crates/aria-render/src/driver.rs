//! Property drivers: code that computes a property's value every frame.
//!
//! A clip stores driver code per property ([`aria_models::PropertyDriver`]).
//! A driver language, registered here by a plugin, compiles the code into a
//! [`DriverProgram`]. The program declares the properties it reads, on its
//! own clip or on clips named in the sequence, and evaluates against a
//! [`DriverScope`] that answers for time, the property's own keyframed
//! value, and the declared inputs after their own drivers.
//!
//! [`resolve`](crate::resolve) evaluates the drivers of every clip it places,
//! and the audio mixer evaluates volume and pan drivers once per frame of the
//! clip when it opens. A driver that fails, panics, or returns a value of the
//! wrong kind leaves its property at the keyframed value and records a fault,
//! read back with [`driver_fault`]. A driver whose evaluation takes longer
//! than [`driver_budget`] is stopped: it is not run again until its code
//! changes.
//!
//! [`check_driver`] proves that attaching code keeps the input graph acyclic.
//! Evaluation also refuses a cycle it walks into, which a clip rename can make
//! after the check.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, PoisonError, RwLock};
use std::time::Duration;

use aria_models::{ClipId, ClipParam, DriverCode, ParamValue};

mod frame;

pub use frame::{DriverFrame, check_driver, driven_envelope, driver_input, driver_target};

/// A clip a driver reads from.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum InputClip {
    /// The clip the driver is attached to.
    This,
    /// The clip with this name in the sequence: its own name, else its
    /// source's ([`aria_models::Project::clip_title`]). Among several, the one
    /// under the time being evaluated on the topmost lane, else the first.
    Named(String),
}

/// A property a driver reads.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum InputProperty {
    /// A property by its catalog id: `position`, `opacity`, `text-size`,
    /// `effect-0-radius`.
    Id(String),
    /// An effect parameter: the first effect on the clip whose label or id is
    /// `effect`, and its parameter whose label or name is `param`. Both match
    /// without regard to ASCII case.
    Effect { effect: String, param: String },
}

/// One property a driver program reads.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DriverInput {
    pub clip: InputClip,
    pub property: InputProperty,
}

/// Where a clip sits, as a driver reads it.
#[derive(Debug, Clone, PartialEq)]
pub struct DriverClipInfo {
    pub name: String,
    /// Sequence seconds of the clip's first frame.
    pub in_point: f64,
    /// Sequence seconds just past the clip's last frame.
    pub out_point: f64,
}

/// What a driver program reads while it evaluates.
///
/// Times are sequence seconds. Values are in the model's units: a position
/// is the anchor's offset from the canvas centre as a fraction of the canvas,
/// a scale and an opacity are fractions, a rotation is in degrees, a colour
/// is four bytes.
pub trait DriverScope {
    /// The time being evaluated.
    fn time(&self) -> f64;
    /// The property the driver replaces.
    fn property(&self) -> ClipParam;
    /// Seconds since the first frame of the driver's clip.
    fn clip_time(&self) -> f64;
    /// Frames per second of the sequence.
    fn frame_rate(&self) -> f64;
    /// Length of the sequence in seconds.
    fn duration(&self) -> f64;
    /// Canvas width and height in pixels.
    fn canvas(&self) -> [u32; 2];
    /// The property's own keyframed value at [`time`](Self::time).
    fn value(&self) -> ParamValue;
    /// The property's own keyframed value at `seconds`.
    fn value_at(&self, seconds: f64) -> ParamValue;
    /// How many keyframes the property itself has.
    fn key_count(&self) -> usize;
    /// The time of the property's keyframe at `index`, counting from 0.
    fn key_time(&self, index: usize) -> Option<f64>;
    /// Declared input `index` at [`time`](Self::time), after its own driver.
    fn input(&mut self, index: usize) -> Result<ParamValue, String>;
    /// Declared input `index` at `seconds`, after its own driver.
    fn input_at(&mut self, index: usize, seconds: f64) -> Result<ParamValue, String>;
    /// Name and extent of a clip.
    fn clip_info(&mut self, clip: &InputClip) -> Result<DriverClipInfo, String>;
    /// A number fixed for this clip and property, for seeded noise.
    fn seed(&self) -> u64;
    /// Whether this evaluation is past its time budget. A long-running
    /// program checks this and returns an error when it is.
    fn expired(&self) -> bool;
}

/// Compiled driver code.
pub trait DriverProgram: Send + Sync {
    /// Every property the program reads, in the order
    /// [`DriverScope::input`] addresses them.
    fn inputs(&self) -> &[DriverInput];

    /// The property's value at the scope's time. A value of another kind than
    /// [`DriverScope::value`] is a failure.
    fn evaluate(&self, scope: &mut dyn DriverScope) -> Result<ParamValue, String>;
}

/// A language that compiles driver code.
pub trait DriverLanguage: Send + Sync {
    /// Compile `source`. The editor shows the error text as is.
    fn compile(&self, source: &str) -> Result<Arc<dyn DriverProgram>, String>;

    /// Source that reads `input`, which the inspector's pick whip inserts.
    /// `None` when the language has no syntax for it.
    fn reference(&self, input: &DriverInput) -> Option<String> {
        let _ = input;
        None
    }

    /// Source a new driver starts with. Code that returns the property's own
    /// value leaves the property unchanged until the code is edited.
    fn template(&self) -> String {
        String::new()
    }
}

struct Language {
    id: String,
    token: u64,
    language: Arc<dyn DriverLanguage>,
}

static LANGUAGES: RwLock<Vec<Language>> = RwLock::new(Vec::new());
static NEXT_TOKEN: AtomicU64 = AtomicU64::new(1);
/// Bumped by every registration and removal, so compiled programs from a
/// removed language are not served.
static LANGUAGES_REVISION: AtomicU64 = AtomicU64::new(0);

/// Keeps a driver language registered; dropping it removes the language.
#[derive(Debug)]
#[must_use = "dropping the registration removes the driver language"]
pub struct DriverLanguageRegistration {
    token: u64,
}

impl Drop for DriverLanguageRegistration {
    fn drop(&mut self) {
        LANGUAGES
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|entry| entry.token != self.token);
        LANGUAGES_REVISION.fetch_add(1, Ordering::AcqRel);
    }
}

/// Register `language` under `id`, the id driver code names in
/// [`DriverCode::language`]. Refused for an id that is not a valid language
/// id.
///
/// The list is process-wide and each plugin host registers its own
/// languages, so two hosts in one process can register the same id. The
/// latest live registration of an id compiles its code; dropping it restores
/// the one before.
pub fn register_driver_language(
    id: impl Into<String>,
    language: Arc<dyn DriverLanguage>,
) -> Result<DriverLanguageRegistration, String> {
    let id = id.into();
    DriverCode::new(id.clone(), "")
        .validate()
        .map_err(|err| err.to_string())?;
    let mut languages = LANGUAGES.write().unwrap_or_else(PoisonError::into_inner);
    let token = NEXT_TOKEN.fetch_add(1, Ordering::Relaxed);
    languages.push(Language {
        id,
        token,
        language,
    });
    LANGUAGES_REVISION.fetch_add(1, Ordering::AcqRel);
    Ok(DriverLanguageRegistration { token })
}

/// The ids of every registered driver language, once each, in the order they
/// were first registered.
pub fn driver_languages() -> Vec<String> {
    let languages = LANGUAGES.read().unwrap_or_else(PoisonError::into_inner);
    let mut ids: Vec<String> = Vec::with_capacity(languages.len());
    for entry in languages.iter() {
        if !ids.contains(&entry.id) {
            ids.push(entry.id.clone());
        }
    }
    ids
}

fn language(id: &str) -> Option<Arc<dyn DriverLanguage>> {
    LANGUAGES
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .rev()
        .find(|entry| entry.id == id)
        .map(|entry| Arc::clone(&entry.language))
}

/// Calls into a language with a panic caught, as `None`.
fn ask<T>(id: &str, call: impl FnOnce(&dyn DriverLanguage) -> Option<T>) -> Option<T> {
    let language = language(id)?;
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| call(language.as_ref())))
        .ok()
        .flatten()
}

/// Source in `language` that reads `input`, for the pick whip. `None` when
/// the language is not installed or has no syntax for the input.
pub fn driver_reference(language: &str, input: &DriverInput) -> Option<String> {
    ask(language, |language| language.reference(input))
}

/// Source a new driver in `language` starts with, `None` when the language
/// is not installed.
pub fn driver_template(language: &str) -> Option<String> {
    ask(language, |language| Some(language.template()))
}

type Compiled = Result<Arc<dyn DriverProgram>, String>;

/// Compiled programs by code, tagged with the language revision they were
/// compiled under.
static PROGRAMS: LazyLock<Mutex<HashMap<DriverCode, (u64, Compiled)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// How many compiled programs are kept before the cache starts over.
const PROGRAM_CACHE: usize = 512;

/// `code` compiled by its language, from the cache when it was compiled under
/// the current set of languages.
pub fn compile_driver(code: &DriverCode) -> Result<Arc<dyn DriverProgram>, String> {
    let revision = LANGUAGES_REVISION.load(Ordering::Acquire);
    if let Some((at, compiled)) = PROGRAMS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(code)
        && *at == revision
    {
        return compiled.clone();
    }
    let language = language(&code.language);
    let compiled = match language {
        None => Err(format!(
            "no driver language {} is installed; install the plugin that provides it",
            code.language
        )),
        Some(language) => std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            language.compile(&code.source)
        }))
        .unwrap_or_else(|payload| {
            Err(format!(
                "driver language {} panicked while compiling: {}",
                code.language,
                crate::audio_processor::panic_message(payload.as_ref())
            ))
        }),
    };
    let mut programs = PROGRAMS.lock().unwrap_or_else(PoisonError::into_inner);
    if programs.len() >= PROGRAM_CACHE {
        programs.clear();
    }
    programs.insert(code.clone(), (revision, compiled.clone()));
    compiled
}

/// A driver's last failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriverFault {
    pub clip: ClipId,
    pub param: ClipParam,
    pub message: String,
    /// The driver ran over its budget and is not run again until its code
    /// changes.
    pub stopped: bool,
}

struct Fault {
    message: String,
    /// Hash of the code that failed, so a stop ends when the code changes.
    code: u64,
    stopped: bool,
}

static FAULTS: LazyLock<RwLock<HashMap<(ClipId, ClipParam), Fault>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));
static FAULTS_REVISION: AtomicU64 = AtomicU64::new(0);

fn code_hash(code: &DriverCode) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    code.hash(&mut hasher);
    hasher.finish()
}

/// The last failure of the driver on `param` of `clip`, if its last
/// evaluation failed.
pub fn driver_fault(clip: ClipId, param: ClipParam) -> Option<DriverFault> {
    FAULTS
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&(clip, param))
        .map(|fault| DriverFault {
            clip,
            param,
            message: fault.message.clone(),
            stopped: fault.stopped,
        })
}

/// Every driver whose last evaluation failed.
pub fn driver_faults() -> Vec<DriverFault> {
    FAULTS
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .map(|(&(clip, param), fault)| DriverFault {
            clip,
            param,
            message: fault.message.clone(),
            stopped: fault.stopped,
        })
        .collect()
}

/// Bumped whenever a driver fault is recorded or cleared.
pub fn driver_faults_revision() -> u64 {
    FAULTS_REVISION.load(Ordering::Acquire)
}

fn record_fault(clip: ClipId, param: ClipParam, code: &DriverCode, message: String, stopped: bool) {
    let hash = code_hash(code);
    let key = (clip, param);
    if FAULTS
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&key)
        .is_some_and(|held| held.message == message && held.code == hash && held.stopped == stopped)
    {
        return;
    }
    if stopped {
        tracing::warn!(%message, "driver stopped");
    }
    FAULTS
        .write()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(
            key,
            Fault {
                message,
                code: hash,
                stopped,
            },
        );
    FAULTS_REVISION.fetch_add(1, Ordering::AcqRel);
}

fn clear_fault(clip: ClipId, param: ClipParam) {
    let key = (clip, param);
    if !FAULTS
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .contains_key(&key)
    {
        return;
    }
    if FAULTS
        .write()
        .unwrap_or_else(PoisonError::into_inner)
        .remove(&key)
        .is_some()
    {
        FAULTS_REVISION.fetch_add(1, Ordering::AcqRel);
    }
}

/// Whether the driver on `param` of `clip` was stopped while running `code`.
fn is_stopped(clip: ClipId, param: ClipParam, code: &DriverCode) -> bool {
    FAULTS
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&(clip, param))
        .is_some_and(|fault| fault.stopped && fault.code == code_hash(code))
}

/// Default time one driver evaluation may take.
pub const DEFAULT_DRIVER_BUDGET: Duration = Duration::from_millis(20);

static BUDGET_MICROS: AtomicU64 = AtomicU64::new(DEFAULT_DRIVER_BUDGET.as_micros() as u64);

/// The time one driver evaluation may take before the driver is stopped.
pub fn driver_budget() -> Duration {
    Duration::from_micros(BUDGET_MICROS.load(Ordering::Relaxed))
}

/// Set the time one driver evaluation may take, at least one millisecond.
pub fn set_driver_budget(budget: Duration) {
    let micros = budget.max(Duration::from_millis(1)).as_micros();
    BUDGET_MICROS.store(u64::try_from(micros).unwrap_or(u64::MAX), Ordering::Relaxed);
}

#[cfg(test)]
#[path = "driver/tests.rs"]
mod tests;
