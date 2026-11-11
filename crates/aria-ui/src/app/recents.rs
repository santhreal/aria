//! The recents list: the file behind it, the state that holds it, and the verbs
//! that change it.
//!
//! It lived under `panels::home`, which pointed the dependency the wrong way:
//! the list outlives Home, is read before any panel is drawn (`AppState::new`
//! seeds it), is written by opening a project from the menu bar or the palette,
//! and is revealed from the Help menu. Home is one reader of it. So the format,
//! the migration, and the four verbs live here, and the panel draws what it is
//! handed.
//!
//! The list is held rather than read per draw because Home is animated: a read
//! of `recents.json` plus a stat of every path in it, once a frame, answers a
//! question that only changes when a project is remembered, forgotten, deleted,
//! or when Home is entered and the files could have moved underneath.
//!
//! The verbs change the held list and send it to the file through
//! [`RecentsWriter`], off the UI thread. The file is read again only when no
//! write of it is in flight: until the last write lands, the held list is the
//! newer copy.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use gpui::SharedString;
use serde::{Deserialize, Serialize};

use super::AppState;
use crate::app::picker::{Ask, FileSpec};

pub(crate) const RECENTS_FILE: &str = "recents.json";
pub(crate) const MAX_RECENTS: usize = 12;
const RECENTS_VERSION: u32 = 1;

#[derive(Debug, Clone, Deserialize)]
struct RecentsFile {
    #[serde(default)]
    version: u32,
    entries: Vec<RecentEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentEntry {
    pub path: PathBuf,
    #[serde(default)]
    pub opened_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentView {
    pub path: Arc<PathBuf>,
    pub opened_at: u64,
    pub missing: bool,
    pub label: SharedString,
    pub detail: SharedString,
    pub opened: SharedString,
}

impl AppState {
    /// Recent projects, as Home draws them.
    pub(crate) fn recents(&self) -> &Arc<[RecentView]> {
        &self.recents
    }

    /// Re-read the recents file and re-check which of its projects are still
    /// there. Home draws what this leaves behind, so a frame costs no file read
    /// and no stat.
    pub(crate) fn refresh_recents(&mut self) {
        let (entries, set_aside) = self.recent_entries();
        self.recents = views(entries).into();
        self.report_set_aside(set_aside);
    }

    /// Put a project at the top of the recents list.
    pub(crate) fn remember_recent(&mut self, path: &Path) {
        let (mut entries, set_aside) = self.recent_entries();
        put_first(&mut entries, path);
        self.hold_recents(entries);
        self.report_set_aside(set_aside);
    }

    /// Take a project off the recents list, leaving the file alone.
    pub(crate) fn forget_recent(&mut self, path: &Path) {
        let (mut entries, set_aside) = self.recent_entries();
        entries.retain(|entry| entry.path != path);
        self.hold_recents(entries);
        self.report_set_aside(set_aside);
    }

    /// Wait for every recents write sent so far, so the list a session ends
    /// with is the one the next launch reads.
    pub(crate) fn flush_recents(&mut self) {
        self.recents_writer.flush();
    }

    /// The list a verb changes: the file's, or the held one while a write of it
    /// is still on its way to the file.
    fn recent_entries(&self) -> (Vec<RecentEntry>, Option<String>) {
        if !self.recents_writer.settled() {
            let held = self.recents.iter().map(|view| RecentEntry {
                path: PathBuf::clone(&view.path),
                opened_at: view.opened_at,
            });
            return (held.collect(), None);
        }
        load_entries(&self.data_dir)
    }

    /// Hold `entries` as the list Home draws, and send them to the file.
    fn hold_recents(&mut self, entries: Vec<RecentEntry>) {
        self.recents_writer.send(&self.data_dir, &entries);
        self.recents = views(entries).into();
    }

    /// Delete a project's file and take it off the list.
    pub(crate) fn delete_recent(&mut self, path: &Path) -> Result<(), String> {
        delete_recent_file(path)?;
        self.forget_recent(path);
        Ok(())
    }

    /// Post the line for a recents file that did not parse and was set aside.
    fn report_set_aside(&mut self, set_aside: Option<String>) {
        if let Some(line) = set_aside {
            self.report_error(line);
        }
    }

    /// Open a recent project, or prompt to relink it when its file has moved.
    pub(crate) fn open_or_relink_recent(&mut self, path: &Path) {
        if !path.exists() {
            relink_recent(self, path);
            return;
        }
        self.request_open_project(path.to_path_buf());
    }
}

/// Relink a recent entry whose file has moved: ask for the project, forget the
/// old path, and open the one that was chosen.
///
/// Home's relink and the recent row's Open both come here. The open goes
/// through the unsaved-changes prompt like every other way of replacing the
/// session.
pub(crate) fn relink_recent(state: &mut AppState, old: &Path) {
    let old = old.to_path_buf();
    state.request_pick(
        Ask::OpenFile(FileSpec::new("Relink Project").filter("Aria project", &["aria"])),
        move |state, answer| {
            let Some(path) = answer.one() else {
                return;
            };
            state.forget_recent(&old);
            state.request_open_project(path);
        },
    );
}

/// Put `path` at the top of the recents list in `data_dir`. Returns the status
/// line when the file there did not parse and was set aside.
pub fn remember(data_dir: &Path, path: &Path) -> Option<String> {
    let (mut recents, set_aside) = load_entries(data_dir);
    put_first(&mut recents, path);
    save_entries(data_dir, &recents);
    set_aside
}

/// Move `path` to the top of `entries`, stamped now, keeping at most
/// [`MAX_RECENTS`].
fn put_first(entries: &mut Vec<RecentEntry>, path: &Path) {
    entries.retain(|entry| entry.path != path);
    entries.insert(
        0,
        RecentEntry {
            path: path.to_path_buf(),
            opened_at: now_secs(),
        },
    );
    entries.truncate(MAX_RECENTS);
}

/// Take `path` off the recents list in `data_dir`. Returns the status line
/// when the file there did not parse and was set aside.
pub fn forget(data_dir: &Path, path: &Path) -> Option<String> {
    let (mut recents, set_aside) = load_entries(data_dir);
    recents.retain(|entry| entry.path != path);
    save_entries(data_dir, &recents);
    set_aside
}

/// Delete the project file at `path`, if it is there.
pub fn delete_recent_file(path: &Path) -> Result<(), String> {
    if path.exists() {
        fs::remove_file(path).map_err(|err| {
            format!(
                "could not delete {}: {err}",
                path.display(),
                err = aria_models::io_reason(&err)
            )
        })?;
    }
    Ok(())
}

/// Show the folder holding `path` in the platform file manager.
///
/// The launch itself belongs to [`crate::app::launch`]: one owner decides
/// whether this process may start another application, and it is off unless the
/// shipped editor turned it on.
pub fn reveal_in_folder(path: &Path) {
    let folder = if path.is_dir() {
        path
    } else {
        path.parent().unwrap_or(path)
    };
    if let Err(err) = crate::app::launch::open_folder(folder) {
        tracing::warn!(error = %err, "could not reveal folder");
    }
}

const SAMPLE_TONE_RATE: u32 = 48_000;
const SAMPLE_TONE_CHANNELS: u16 = 2;
const SAMPLE_TONE_SECONDS: u32 = 6;

pub fn sample_tone(data_dir: &Path) -> PathBuf {
    let dest = data_dir.join("samples").join("audio").join("tone.wav");
    if !dest.exists()
        && let Err(err) = write_sample_tone(&dest)
    {
        tracing::warn!(error = %err, "could not write sample tone");
    }
    dest
}

fn write_sample_tone(dest: &Path) -> std::io::Result<()> {
    let frames = (SAMPLE_TONE_RATE * SAMPLE_TONE_SECONDS) as usize;
    let half = frames / 2;
    let channels = usize::from(SAMPLE_TONE_CHANNELS);
    let data_bytes = (frames * channels * 2) as u32;
    let byte_rate = SAMPLE_TONE_RATE * u32::from(SAMPLE_TONE_CHANNELS) * 2;
    let block_align = SAMPLE_TONE_CHANNELS * 2;
    let mut wav = Vec::with_capacity(44 + data_bytes as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&SAMPLE_TONE_CHANNELS.to_le_bytes());
    wav.extend_from_slice(&SAMPLE_TONE_RATE.to_le_bytes());
    wav.extend_from_slice(&byte_rate.to_le_bytes());
    wav.extend_from_slice(&block_align.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_bytes.to_le_bytes());
    for frame in 0..frames {
        let hz = if frame < half { 440.0_f32 } else { 554.37_f32 };
        let t = frame as f32 / SAMPLE_TONE_RATE as f32;
        let sample = ((std::f32::consts::TAU * hz * t).sin() * 12_000.0) as i16;
        let bytes = sample.to_le_bytes();
        for _ in 0..channels {
            wav.extend_from_slice(&bytes);
        }
    }
    aria_settings::replace_file(dest, &wav).map_err(std::io::Error::other)
}

/// The recents list in `data_dir` as Home draws it, and the status line when
/// the file did not parse and was set aside.
pub fn visible_recents(data_dir: &Path) -> (Vec<RecentView>, Option<String>) {
    let (entries, set_aside) = load_entries(data_dir);
    (views(entries), set_aside)
}

/// `entries` as Home draws them, each checked for its project file.
fn views(entries: Vec<RecentEntry>) -> Vec<RecentView> {
    let home = dirs::home_dir();
    entries
        .into_iter()
        .map(|entry| {
            let missing = !entry.path.exists();
            let label = super::session::project_name_of(Some(&entry.path), "untitled")
                .to_owned()
                .into();
            let detail = folder_label(&entry.path, home.as_deref()).into();
            let opened = format_opened(entry.opened_at).into();
            RecentView {
                path: Arc::new(entry.path),
                opened_at: entry.opened_at,
                missing,
                label,
                detail,
                opened,
            }
        })
        .collect()
}

/// Where a recent project is, as its card prints it: the folder that holds
/// it, with `home` written `~`. The card's title is the file's stem, so the
/// folder is what tells two projects of one name apart.
pub(crate) fn folder_label(path: &Path, home: Option<&Path>) -> String {
    let folder = path
        .parent()
        .filter(|folder| !folder.as_os_str().is_empty())
        .unwrap_or(path);
    match home.and_then(|home| folder.strip_prefix(home).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
        Some(rest) => format!("~{}{}", std::path::MAIN_SEPARATOR, rest.display()),
        None => folder.display().to_string(),
    }
}

/// The entries in `data_dir`'s recents file, and the status line when the
/// file did not parse in either shape and was set aside. A missing or
/// unreadable file is an empty list.
fn load_entries(data_dir: &Path) -> (Vec<RecentEntry>, Option<String>) {
    let path = data_dir.join(RECENTS_FILE);
    let Ok(text) = fs::read_to_string(&path) else {
        return (Vec::new(), None);
    };
    let err = match serde_json::from_str::<RecentsFile>(&text) {
        Ok(file) => return (file.entries, None),
        Err(err) => err,
    };
    if let Ok(paths) = serde_json::from_str::<Vec<PathBuf>>(&text) {
        let entries = paths
            .into_iter()
            .map(|path| RecentEntry { path, opened_at: 0 })
            .collect();
        return (entries, None);
    }
    let line = crate::settings::set_aside_unparsable(&path, &err, "the recents list starts empty");
    (Vec::new(), Some(line))
}

/// `entries` as the recents file holds them.
fn encode(entries: &[RecentEntry]) -> Option<Vec<u8>> {
    #[derive(Serialize)]
    struct Written<'a> {
        version: u32,
        entries: &'a [RecentEntry],
    }
    let file = Written {
        version: RECENTS_VERSION,
        entries,
    };
    serde_json::to_vec_pretty(&file)
        .inspect_err(|err| tracing::warn!(error = %err, "could not serialize recents"))
        .ok()
}

fn save_entries(data_dir: &Path, entries: &[RecentEntry]) {
    if let Some(bytes) = encode(entries)
        && let Err(err) = aria_settings::replace_file(&data_dir.join(RECENTS_FILE), &bytes)
    {
        tracing::warn!(error = %err, "could not write recents");
    }
}

/// Writes the recents file off the UI thread. The write syncs the file and
/// its folder, which held a save or an open for seconds on a loaded disk.
///
/// Each write holds the whole list and runs on a thread of its own; one older
/// than a write already on disk is dropped, so the file ends as the newest
/// list sent, whatever order the threads run in.
pub(crate) struct RecentsWriter {
    sent: u64,
    written: Arc<parking_lot::Mutex<u64>>,
    pending: Vec<std::thread::JoinHandle<()>>,
    /// Replaces the file at a path with bytes: [`aria_settings::replace_file`].
    pub(super) write: fn(&Path, &[u8]) -> Result<(), String>,
}

impl Default for RecentsWriter {
    fn default() -> Self {
        Self {
            sent: 0,
            written: Arc::default(),
            pending: Vec::new(),
            write: aria_settings::replace_file,
        }
    }
}

impl std::fmt::Debug for RecentsWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecentsWriter")
            .field("sent", &self.sent)
            .field("pending", &self.pending.len())
            .finish_non_exhaustive()
    }
}

impl RecentsWriter {
    /// Whether every write sent so far has landed or been dropped.
    fn settled(&self) -> bool {
        self.pending
            .iter()
            .all(std::thread::JoinHandle::is_finished)
    }

    /// Send `entries` to the recents file in `data_dir`.
    fn send(&mut self, data_dir: &Path, entries: &[RecentEntry]) {
        let Some(bytes) = encode(entries) else {
            return;
        };
        self.pending.retain(|handle| !handle.is_finished());
        self.sent += 1;
        let generation = self.sent;
        let written = Arc::clone(&self.written);
        let path = data_dir.join(RECENTS_FILE);
        let write = self.write;
        let spawned = std::thread::Builder::new()
            .name("recents-write".into())
            .spawn(move || write_generation(&written, generation, write, &path, &bytes));
        match spawned {
            Ok(handle) => self.pending.push(handle),
            Err(err) => {
                tracing::warn!(error = %err, "no thread for the recents write; writing it here");
                let bytes = encode(entries).unwrap_or_default();
                write_generation(
                    &self.written,
                    generation,
                    self.write,
                    &data_dir.join(RECENTS_FILE),
                    &bytes,
                );
            }
        }
    }

    /// Wait for every write sent so far.
    fn flush(&mut self) {
        for handle in self.pending.drain(..) {
            if handle.join().is_err() {
                tracing::warn!("a recents write panicked");
            }
        }
    }
}

/// A session dropped mid-write waits for its writes: the last project it
/// saved is on the list the next session reads.
impl Drop for RecentsWriter {
    fn drop(&mut self) {
        self.flush();
    }
}

/// Write `bytes` to `path` as generation `generation`, unless a newer
/// generation is on disk already.
fn write_generation(
    written: &parking_lot::Mutex<u64>,
    generation: u64,
    write: fn(&Path, &[u8]) -> Result<(), String>,
    path: &Path,
    bytes: &[u8],
) {
    let mut last = written.lock();
    if *last > generation {
        return;
    }
    if let Err(err) = write(path, bytes) {
        tracing::warn!(error = %err, "could not write recents");
    }
    *last = generation;
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// How long ago a project was opened, in the words a card shows.
pub(crate) fn format_opened(opened_at: u64) -> String {
    if opened_at == 0 {
        return "Opened".into();
    }
    let now = now_secs();
    if now < opened_at {
        return "Opened".into();
    }
    let delta = now - opened_at;
    if delta < 60 {
        "Just now".into()
    } else if delta < 3600 {
        format!("{} min ago", delta / 60)
    } else if delta < 86_400 {
        format!("{} hr ago", delta / 3600)
    } else {
        format!("{} d ago", delta / 86_400)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch() -> PathBuf {
        crate::scratch_pool::scratch("recents")
    }

    #[test]
    fn remember_writes_mtime_and_promotes() {
        let dir = scratch();
        let a = dir.join("a.aria");
        let b = dir.join("b.aria");
        fs::write(&a, "{}").unwrap();
        fs::write(&b, "{}").unwrap();
        remember(&dir, &a);
        remember(&dir, &b);
        let (recents, _) = visible_recents(&dir);
        assert_eq!(recents[0].path.as_ref(), &b);
        assert_eq!(recents[1].path.as_ref(), &a);
        assert!(recents[0].opened_at > 0);
        assert_eq!(recents[0].label.as_ref(), "b");
        assert_eq!(
            recents[0].detail.as_ref(),
            folder_label(&b, dirs::home_dir().as_deref())
        );
        assert!(!recents[0].missing);
    }

    /// A card prints the folder its project is in, with the home folder as
    /// `~`. Closes the class "a card that prints the whole path": a deep path
    /// wrapped the card over several lines and repeated the title's file name.
    /// The home prefix matches whole components, so `/home/u` is not a prefix
    /// of `/home/user2`.
    #[test]
    #[cfg(unix)]
    fn a_card_prints_its_projects_folder_with_home_as_a_tilde() {
        let cases: [(&str, Option<&str>, &str); 6] = [
            ("/home/u/.aria/untitled.aria", Some("/home/u"), "~/.aria"),
            ("/home/u/films/cut/a.aria", Some("/home/u"), "~/films/cut"),
            ("/home/u/a.aria", Some("/home/u"), "~"),
            ("/home/user2/a.aria", Some("/home/u"), "/home/user2"),
            ("/srv/films/a.aria", None, "/srv/films"),
            ("a.aria", Some("/home/u"), "a.aria"),
        ];
        for (path, home, printed) in cases {
            assert_eq!(
                folder_label(Path::new(path), home.map(Path::new)),
                printed,
                "{path} under home {home:?}"
            );
        }
    }

    #[test]
    fn missing_recent_is_marked() {
        let dir = scratch();
        let gone = dir.join("gone.aria");
        remember(&dir, &gone);
        let (recents, _) = visible_recents(&dir);
        assert!(recents[0].missing);
    }

    #[test]
    fn legacy_path_list_migrates() {
        let dir = scratch();
        let path = dir.join("old.aria");
        fs::write(&path, "{}").unwrap();
        fs::write(
            dir.join(RECENTS_FILE),
            serde_json::to_string(&vec![&path]).unwrap(),
        )
        .unwrap();
        let (recents, _) = visible_recents(&dir);
        assert_eq!(recents[0].path.as_ref(), &path);
        remember(&dir, &path);
        let text = fs::read_to_string(dir.join(RECENTS_FILE)).unwrap();
        let file: RecentsFile = serde_json::from_str(&text).unwrap();
        assert_eq!(file.version, RECENTS_VERSION);
    }

    /// A write older than the list on disk is dropped, in whatever order the
    /// writer threads reach the file.
    #[test]
    fn an_older_write_never_replaces_a_newer_one() {
        let file = scratch().join(RECENTS_FILE);
        let written = parking_lot::Mutex::new(0);
        let write = aria_settings::replace_file;
        write_generation(&written, 2, write, &file, b"two");
        write_generation(&written, 1, write, &file, b"one");
        assert_eq!(fs::read(&file).unwrap(), b"two");
        write_generation(&written, 3, write, &file, b"three");
        assert_eq!(fs::read(&file).unwrap(), b"three");
    }

    #[test]
    fn forget_drops_entry() {
        let dir = scratch();
        let path = dir.join("drop.aria");
        fs::write(&path, "{}").unwrap();
        remember(&dir, &path);
        forget(&dir, &path);
        assert!(visible_recents(&dir).0.is_empty());
    }

    /// An unparsable `recents.json` is set aside and reported, never replaced.
    ///
    /// The class closed here: a recents file in neither shape read as an
    /// empty list without a line, and the next project opened replaced it, so
    /// every remembered project was gone. Driven through `AppState::new`, the
    /// first read of a session, and through `remember_recent`, a read in the
    /// middle of one: each sets the file aside under its own name and posts
    /// the name. Not covered: an unreadable file, which reads as empty and
    /// leaves the next save to fail on the same permission.
    #[test]
    fn an_unparsable_recents_file_is_set_aside_and_reported() {
        const CUT: &str = r#"{"version":1,"entries":[{"path":"/films/a.aria","#;
        let dir = crate::scratch_pool::scratch("recents-cut");
        let file = dir.join(RECENTS_FILE);
        let set_aside = || -> Vec<PathBuf> {
            let mut found: Vec<PathBuf> = fs::read_dir(&dir)
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .filter(|path| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.starts_with("recents.json.unreadable-"))
                })
                .collect();
            found.sort();
            found
        };

        fs::write(&file, CUT).unwrap();
        let mut state =
            AppState::new(aria_plugin::PluginContext::new(), dir.clone()).expect("state");
        assert!(state.recents().is_empty());
        let first = set_aside();
        assert_eq!(first.len(), 1, "the session did not set the file aside");
        assert_eq!(fs::read_to_string(&first[0]).unwrap(), CUT);
        let line = state
            .last_error
            .clone()
            .expect("the session opened with no line");
        assert!(
            line.starts_with("Couldn't parse ") && line.contains(&first[0].display().to_string()),
            "{line}"
        );

        fs::write(&file, CUT).unwrap();
        let project = dir.join("b.aria");
        fs::write(&project, "{}").unwrap();
        state.last_error = None;
        state.remember_recent(&project);
        let both = set_aside();
        assert_eq!(
            both.len(),
            2,
            "the second set-aside replaced the first: {both:?}"
        );
        let second = both.iter().find(|path| **path != first[0]).unwrap();
        assert_eq!(fs::read_to_string(second).unwrap(), CUT);
        assert_eq!(fs::read_to_string(&first[0]).unwrap(), CUT);
        let line = state
            .last_error
            .clone()
            .expect("remembering posted no line");
        assert!(line.contains(&second.display().to_string()), "{line}");
        assert_eq!(state.recents().len(), 1);
        assert_eq!(state.recents()[0].path.as_ref(), &project);
    }
}
