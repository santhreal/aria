//! Catalog entries added while the process runs.
//!
//! The shipped effect and transition catalogs are read once from data files. A
//! plugin adds an effect, a transition or an audio effect here, and the live
//! catalogs ([`crate::effect_catalog`], [`crate::transition_catalog`],
//! [`crate::audio_effect_catalog`]) list the shipped entries first, then the
//! registered ones in registration order.
//!
//! Entries are shared, not borrowed: a lookup returns an `Arc`, so a caller that
//! keeps a spec keeps it alive after the entry is removed, and nothing outlives
//! its owner.
//!
//! Dropping the [`Registered`] guard removes the entry. A project keeps its
//! instances of a removed entry; they resolve to no spec, which is also what an
//! effect from a plugin that is not installed resolves to.

use std::sync::{Arc, LazyLock, PoisonError, RwLock};

use crate::audio_effects::{AudioEffectSpec, check_audio_effect};
use crate::effects::{EffectSpec, check_effect_params};
use crate::transition::TransitionSpec;

/// A catalog at one revision: shipped entries, then registered ones.
#[derive(Debug)]
pub struct Catalog<T> {
    entries: Arc<[Arc<T>]>,
    revision: u64,
}

impl<T> Clone for Catalog<T> {
    fn clone(&self) -> Self {
        Self {
            entries: Arc::clone(&self.entries),
            revision: self.revision,
        }
    }
}

impl<T> Catalog<T> {
    /// Entries in catalog order.
    pub fn iter(&self) -> std::slice::Iter<'_, Arc<T>> {
        self.entries.iter()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The entry at `index`, or `None`.
    pub fn get(&self, index: usize) -> Option<&Arc<T>> {
        self.entries.get(index)
    }

    pub fn first(&self) -> Option<&Arc<T>> {
        self.entries.first()
    }

    /// Bumped by every registration and removal. Two snapshots of one catalog
    /// with equal revisions list the same entries.
    pub fn revision(&self) -> u64 {
        self.revision
    }
}

impl<'a, T> IntoIterator for &'a Catalog<T> {
    type Item = &'a Arc<T>;
    type IntoIter = std::slice::Iter<'a, Arc<T>>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<T> IntoIterator for Catalog<T> {
    type Item = Arc<T>;
    type IntoIter = CatalogIter<T>;

    fn into_iter(self) -> CatalogIter<T> {
        CatalogIter {
            entries: self.entries,
            next: 0,
        }
    }
}

/// An owning iterator over one catalog snapshot.
pub struct CatalogIter<T> {
    entries: Arc<[Arc<T>]>,
    next: usize,
}

impl<T> Iterator for CatalogIter<T> {
    type Item = Arc<T>;

    fn next(&mut self) -> Option<Arc<T>> {
        let entry = Arc::clone(self.entries.get(self.next)?);
        self.next += 1;
        Some(entry)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let left = self.entries.len() - self.next;
        (left, Some(left))
    }
}

impl<T> ExactSizeIterator for CatalogIter<T> {}

/// What a registrable entry has to answer.
trait Entry: Send + Sync + 'static {
    fn id(&self) -> &str;
    fn label(&self) -> &str;
    /// Rules beyond a present id and label.
    fn check(&self) -> Result<(), String>;
}

impl Entry for EffectSpec {
    fn id(&self) -> &str {
        &self.id
    }

    fn label(&self) -> &str {
        &self.label
    }

    fn check(&self) -> Result<(), String> {
        check_effect_params(self)
    }
}

impl Entry for TransitionSpec {
    fn id(&self) -> &str {
        &self.id
    }

    fn label(&self) -> &str {
        &self.label
    }

    fn check(&self) -> Result<(), String> {
        Ok(())
    }
}

impl Entry for AudioEffectSpec {
    fn id(&self) -> &str {
        &self.id
    }

    fn label(&self) -> &str {
        &self.label
    }

    fn check(&self) -> Result<(), String> {
        check_audio_effect(self)
    }
}

struct Live<T: 'static> {
    shipped: &'static [Arc<T>],
    registered: Vec<(u64, Arc<T>)>,
    snapshot: Arc<[Arc<T>]>,
    revision: u64,
    next_token: u64,
}

impl<T: Entry> Live<T> {
    fn new(shipped: &'static [Arc<T>]) -> Self {
        Self {
            shipped,
            registered: Vec::new(),
            snapshot: shipped.iter().cloned().collect(),
            revision: 0,
            next_token: 0,
        }
    }

    fn catalog(&self) -> Catalog<T> {
        Catalog {
            entries: Arc::clone(&self.snapshot),
            revision: self.revision,
        }
    }

    fn find(&self, id: &str) -> Option<Arc<T>> {
        self.snapshot.iter().find(|entry| entry.id() == id).cloned()
    }

    fn register(&mut self, entry: T) -> Result<(u64, Arc<T>), String> {
        let id = entry.id();
        if id.is_empty() {
            return Err("a registered entry needs an id".into());
        }
        if !id.contains('.') {
            return Err(format!(
                "{id}: a registered id needs a namespace, such as `example.{id}`, so it \
                 cannot collide with a shipped id"
            ));
        }
        if entry.label().is_empty() {
            return Err(format!("{id} has no label"));
        }
        entry.check()?;
        if self.snapshot.iter().any(|held| held.id() == id) {
            return Err(format!("{id} is already in the catalog"));
        }
        let entry = Arc::new(entry);
        let token = self.next_token;
        self.next_token += 1;
        self.registered.push((token, Arc::clone(&entry)));
        self.rebuild();
        Ok((token, entry))
    }

    fn remove(&mut self, token: u64) {
        if let Some(index) = self.registered.iter().position(|(held, _)| *held == token) {
            self.registered.remove(index);
            self.rebuild();
        }
    }

    fn rebuild(&mut self) {
        self.snapshot = self
            .shipped
            .iter()
            .chain(self.registered.iter().map(|(_, entry)| entry))
            .cloned()
            .collect();
        self.revision += 1;
    }
}

static EFFECTS: LazyLock<RwLock<Live<EffectSpec>>> =
    LazyLock::new(|| RwLock::new(Live::new(crate::effects::shipped_effect_catalog())));

static TRANSITIONS: LazyLock<RwLock<Live<TransitionSpec>>> =
    LazyLock::new(|| RwLock::new(Live::new(crate::transition::shipped_transition_catalog())));

static AUDIO_EFFECTS: LazyLock<RwLock<Live<AudioEffectSpec>>> = LazyLock::new(|| {
    RwLock::new(Live::new(
        crate::audio_effects::shipped_audio_effect_catalog(),
    ))
});

// A writer holds the lock only to push, remove, or rebuild a snapshot, none of
// which leaves the table half-written, so a poisoned lock still holds a whole
// table.

fn read<T: Entry, R>(live: &RwLock<Live<T>>, f: impl FnOnce(&Live<T>) -> R) -> R {
    f(&live.read().unwrap_or_else(PoisonError::into_inner))
}

fn write<T: Entry, R>(live: &RwLock<Live<T>>, f: impl FnOnce(&mut Live<T>) -> R) -> R {
    f(&mut live.write().unwrap_or_else(PoisonError::into_inner))
}

pub(crate) fn effect_catalog() -> Catalog<EffectSpec> {
    read(&EFFECTS, Live::catalog)
}

pub(crate) fn effect_spec(id: &str) -> Option<Arc<EffectSpec>> {
    read(&EFFECTS, |live| live.find(id))
}

pub(crate) fn transition_catalog() -> Catalog<TransitionSpec> {
    read(&TRANSITIONS, Live::catalog)
}

pub(crate) fn transition_spec(id: &str) -> Option<Arc<TransitionSpec>> {
    read(&TRANSITIONS, |live| live.find(id))
}

pub(crate) fn audio_effect_catalog() -> Catalog<AudioEffectSpec> {
    read(&AUDIO_EFFECTS, Live::catalog)
}

pub(crate) fn audio_effect_spec(id: &str) -> Option<Arc<AudioEffectSpec>> {
    read(&AUDIO_EFFECTS, |live| live.find(id))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Which {
    Effect,
    Transition,
    AudioEffect,
}

/// A registered catalog entry. Dropping it removes the entry from its catalog.
#[derive(Debug)]
#[must_use = "dropping the registration removes the entry"]
pub struct Registered {
    which: Which,
    token: u64,
}

impl Drop for Registered {
    fn drop(&mut self) {
        match self.which {
            Which::Effect => write(&EFFECTS, |live| live.remove(self.token)),
            Which::Transition => write(&TRANSITIONS, |live| live.remove(self.token)),
            Which::AudioEffect => write(&AUDIO_EFFECTS, |live| live.remove(self.token)),
        }
    }
}

/// Add `spec` to the effect catalog until the returned guard drops, and return
/// the entry as the catalog holds it.
///
/// Refused when the id has no `.` namespace, is already in the catalog, or the
/// spec breaks a rule the shipped catalog follows: an empty label, an unnamed
/// or repeated parameter, an inverted range.
pub fn register_effect(spec: EffectSpec) -> Result<(Registered, Arc<EffectSpec>), String> {
    let (token, entry) = write(&EFFECTS, |live| live.register(spec))?;
    Ok((
        Registered {
            which: Which::Effect,
            token,
        },
        entry,
    ))
}

/// Add `spec` to the transition catalog until the returned guard drops. Refused
/// by the same id and label rules as [`register_effect`].
pub fn register_transition(
    spec: TransitionSpec,
) -> Result<(Registered, Arc<TransitionSpec>), String> {
    let (token, entry) = write(&TRANSITIONS, |live| live.register(spec))?;
    Ok((
        Registered {
            which: Which::Transition,
            token,
        },
        entry,
    ))
}

/// Add `spec` to the audio effect catalog until the returned guard drops.
/// Refused by the same id, label and parameter rules as [`register_effect`],
/// and when a parameter is not scalar, a default lies outside its range, or
/// the latency exceeds [`crate::MAX_AUDIO_LATENCY`].
pub fn register_audio_effect(
    spec: AudioEffectSpec,
) -> Result<(Registered, Arc<AudioEffectSpec>), String> {
    let (token, entry) = write(&AUDIO_EFFECTS, |live| live.register(spec))?;
    Ok((
        Registered {
            which: Which::AudioEffect,
            token,
        },
        entry,
    ))
}
