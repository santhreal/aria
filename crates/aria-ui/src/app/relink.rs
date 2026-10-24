//! Relink All: deciding which file on disk is which missing pool entry.
//!
//! A basename is not an identity. Two cameras write `C0001.MP4`, an export and
//! its proxy share a name, and a folder chosen in a hurry holds both. Binding
//! the first file whose name matched put the wrong footage in the timeline and
//! said "Relinked" about it, which is worse than staying missing: the entry
//! stops asking to be repaired.
//!
//! So the match is name **and** facts — frame size, duration, audio presence —
//! read from the candidate with [`aria_engine::import_media`], the same call
//! the relink itself uses to adopt the file. Anything the facts do not settle
//! is refused with a reason, never guessed: [`Plan::refused`] carries one
//! [`Refusal`] per entry that stayed missing, and the pool row shows it.
//!
//! [`plan`] is pure — it reads facts through a closure — so every refusal is a
//! test without a filesystem.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::types::UiMediaItem;

/// How deep under the chosen folder a candidate may sit.
///
/// Card offloads nest (`DCIM/100MEDIA/…`), so a flat listing finds nothing;
/// an unbounded walk from a home directory reads the whole disk. Four levels
/// covers the offload shapes and stops.
pub(crate) const RELINK_MAX_DEPTH: usize = 4;

/// How many files the walk will look at before it stops descending.
///
/// A folder that large was not chosen to relink six clips; refusing beats
/// freezing the window while a network share is enumerated.
pub(crate) const RELINK_MAX_FILES: usize = 20_000;

/// What one missing entry remembers about its file. The match may use these
/// and nothing else — there is no stored byte size or checksum to appeal to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Facts {
    pub width: u32,
    pub height: u32,
    /// Source length in the entry's own units (frames for footage, still
    /// ticks for images) — whatever `import_media` would record.
    pub duration_ticks: i64,
    pub has_audio: bool,
}

impl Facts {
    /// What the project remembers about a pool entry.
    pub(crate) fn of(item: &UiMediaItem) -> Self {
        Self {
            width: item.width,
            height: item.height,
            duration_ticks: item.duration_ticks,
            has_audio: item.has_audio,
        }
    }

    /// The first fact this candidate states differently, if any.
    ///
    /// Duration is compared exactly: both sides come from the same probe of
    /// the same container, so a difference is a different file, not rounding.
    fn differs_from(&self, wanted: &Facts) -> Option<Fact> {
        if self.width != wanted.width || self.height != wanted.height {
            return Some(Fact::Size);
        }
        if self.duration_ticks != wanted.duration_ticks {
            return Some(Fact::Duration);
        }
        if self.has_audio != wanted.has_audio {
            return Some(Fact::Audio);
        }
        None
    }
}

/// A fact a candidate disagreed about. Named rather than a message, so the
/// reason is one value the UI phrases and the tests assert on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Fact {
    Size,
    Duration,
    Audio,
}

impl Fact {
    fn label(self) -> &'static str {
        match self {
            Fact::Size => "frame size",
            Fact::Duration => "duration",
            Fact::Audio => "audio",
        }
    }
}

/// Why one entry was left missing. Every entry that is not bound gets one:
/// the plan has no silent skips.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) enum Refusal {
    /// No file under the folder carries that name.
    NoCandidate,
    /// More than one file carries that name and matches every fact, so which
    /// one the entry means is not knowable from here.
    Ambiguous(usize),
    /// The name matched but the file is a different source. Carries the count
    /// of same-named files inspected, so "there were three, none of them it"
    /// reads differently from "the one there is not it".
    Differs { fact: Fact, seen: usize },
    /// The name matched but the file could not be read.
    Unreadable(String),
}

impl Refusal {
    /// One line for the pool row, phrased for display.
    pub(crate) fn describe(&self) -> String {
        match self {
            Refusal::NoCandidate => "not in that folder".into(),
            Refusal::Ambiguous(count) => {
                format!("{count} files there match; relink this one by hand")
            }
            Refusal::Differs { fact, seen: 1 } => {
                format!("the file there has a different {}", fact.label())
            }
            Refusal::Differs { fact, seen } => {
                format!(
                    "{seen} files there share the name, none matches the {}",
                    fact.label()
                )
            }
            Refusal::Unreadable(why) => format!("could not read the file there: {why}"),
        }
    }
}

/// What Relink All will do, decided before anything is applied.
#[derive(Default, Debug)]
pub(crate) struct Plan {
    /// `(media id, file to bind)` for entries the facts settled.
    pub binds: Vec<(u64, PathBuf)>,
    /// `(media id, entry name, why)` for entries that stay missing.
    pub refused: Vec<(u64, String, Refusal)>,
}

/// Decide each missing entry against the files found under the folder.
///
/// `facts` reads a candidate — in the app it is a probe, in tests a table.
/// It is called at most once per distinct candidate path, because a probe
/// opens the file.
pub(crate) fn plan<F>(missing: &[UiMediaItem], found: &[PathBuf], mut facts: F) -> Plan
where
    F: FnMut(&Path) -> Result<Facts, String>,
{
    let mut by_name: HashMap<String, Vec<&PathBuf>> = HashMap::new();
    for path in found {
        let Some(name) = file_key(path) else {
            continue;
        };
        by_name.entry(name).or_default().push(path);
    }
    let mut read: HashMap<&Path, Result<Facts, String>> = HashMap::new();
    let mut plan = Plan::default();
    for item in missing {
        let name = Path::new(&*item.path)
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| item.name.to_string());
        let candidates = file_key(Path::new(&*item.path))
            .and_then(|key| by_name.get(&key))
            .map(Vec::as_slice)
            .unwrap_or_default();
        if candidates.is_empty() {
            plan.refused.push((item.id, name, Refusal::NoCandidate));
            continue;
        }
        let wanted = Facts::of(item);
        let mut matched: Vec<&PathBuf> = Vec::new();
        let mut differed: Option<Fact> = None;
        let mut unreadable: Option<String> = None;
        for path in candidates {
            let read = read
                .entry(path.as_path())
                .or_insert_with(|| facts(path.as_path()));
            match read {
                Ok(candidate) => match candidate.differs_from(&wanted) {
                    None => matched.push(path),
                    Some(fact) => differed = differed.or(Some(fact)),
                },
                Err(why) => unreadable = unreadable.or_else(|| Some(why.clone())),
            }
        }
        match matched.as_slice() {
            [one] => plan.binds.push((item.id, (*one).clone())),
            [] => {
                let refusal = match differed {
                    Some(fact) => Refusal::Differs {
                        fact,
                        seen: candidates.len(),
                    },
                    // Nothing disagreed and nothing matched, so every candidate
                    // failed to open: the message is the first failure's.
                    None => Refusal::Unreadable(
                        unreadable.unwrap_or_else(|| "the file could not be probed".into()),
                    ),
                };
                plan.refused.push((item.id, name, refusal));
            }
            many => plan
                .refused
                .push((item.id, name, Refusal::Ambiguous(many.len()))),
        }
    }
    plan
}

/// The name two files are compared by: case-folded, because the same card
/// reads back `C0001.MP4` on one platform and `c0001.mp4` on another.
fn file_key(path: &Path) -> Option<String> {
    Some(path.file_name()?.to_string_lossy().to_lowercase())
}

/// Every file under `folder`, breadth-first, bounded by [`RELINK_MAX_DEPTH`]
/// and [`RELINK_MAX_FILES`].
///
/// Unreadable directories are skipped rather than failing the walk: one
/// permission-denied subfolder must not cost the whole relink.
pub(crate) fn walk(folder: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut level = vec![folder.to_path_buf()];
    for _ in 0..RELINK_MAX_DEPTH {
        let mut next = Vec::new();
        for dir in level {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                match entry.file_type() {
                    Ok(kind) if kind.is_dir() => next.push(path),
                    Ok(_) => files.push(path),
                    Err(_) => {}
                }
                if files.len() >= RELINK_MAX_FILES {
                    return files;
                }
            }
        }
        if next.is_empty() {
            break;
        }
        level = next;
    }
    files
}

/// Read a candidate the way the relink would adopt it.
pub(crate) fn probe_facts(path: &Path) -> Result<Facts, String> {
    match aria_engine::import_media(path) {
        Ok(source) => Ok(Facts {
            width: source.width,
            height: source.height,
            duration_ticks: source.duration.value,
            has_audio: source.has_audio,
        }),
        Err(err) => Err(super::describe_engine_error(&err)),
    }
}

#[cfg(test)]
#[path = "relink_tests.rs"]
mod tests;
