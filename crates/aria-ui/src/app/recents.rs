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
//! `AppState::refresh_recents` is the only loader. Every other verb does its
//! work through this module's functions and then calls it, so the held list and
//! the file can never disagree.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use gpui::SharedString;
use serde::{Deserialize, Serialize};

use super::AppState;
use crate::app::picker::{Ask, FileSpec};

const RECENTS_FILE: &str = "recents.json";
const MAX_RECENTS: usize = 12;
const RECENTS_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
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
        self.recents = visible_recents(&self.data_dir).into();
    }

    /// Put a project at the top of the recents list.
    pub(crate) fn remember_recent(&mut self, path: &Path) {
        remember(&self.data_dir, path);
        self.refresh_recents();
    }

    /// Take a project off the recents list, leaving the file alone.
    pub(crate) fn forget_recent(&mut self, path: &Path) {
        forget(&self.data_dir, path);
        self.refresh_recents();
    }

    /// Delete a project's file and take it off the list.
    pub(crate) fn delete_recent(&mut self, path: &Path) -> Result<(), String> {
        let result = delete_recent_file(&self.data_dir, path);
        self.refresh_recents();
        result
    }
}

/// Relink a recent entry whose file has moved: ask for the project, forget the
/// old path, and open the one that was chosen.
pub(crate) fn relink_recent(state: &mut AppState, old: &Path) {
    let old = old.to_path_buf();
    state.request_pick(
        Ask::OpenFile(FileSpec::new("Open project").filter("Aria project", &["aria"])),
        move |state, answer| {
            let Some(path) = answer.one() else {
                return;
            };
            state.forget_recent(&old);
            if let Err(err) = state.open_project(path) {
                state.report_error(&err);
            }
        },
    );
}

pub fn remember(data_dir: &Path, path: &Path) {
    let mut recents = load_entries(data_dir);
    recents.retain(|entry| entry.path != path);
    recents.insert(
        0,
        RecentEntry {
            path: path.to_path_buf(),
            opened_at: now_secs(),
        },
    );
    recents.truncate(MAX_RECENTS);
    save_entries(data_dir, &recents);
}

pub fn forget(data_dir: &Path, path: &Path) {
    let mut recents = load_entries(data_dir);
    recents.retain(|entry| entry.path != path);
    save_entries(data_dir, &recents);
}

pub fn delete_recent_file(data_dir: &Path, path: &Path) -> Result<(), String> {
    if path.exists() {
        fs::remove_file(path)
            .map_err(|err| format!("could not delete {}: {err}", path.display()))?;
    }
    forget(data_dir, path);
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

pub fn sample_still(data_dir: &Path) -> PathBuf {
    let dest = data_dir.join("samples").join("stills").join("texture.png");
    if !dest.exists() {
        if let Some(parent) = dest.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Err(err) = fs::write(
            &dest,
            include_bytes!("../../../../tests/fixtures/texture.png"),
        ) {
            tracing::warn!(error = %err, "could not write sample still");
        }
    }
    dest
}

pub fn visible_recents(data_dir: &Path) -> Vec<RecentView> {
    let home = dirs::home_dir();
    load_entries(data_dir)
        .into_iter()
        .map(|entry| {
            let missing = !entry.path.exists();
            let label = entry
                .path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("untitled")
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

fn load_entries(data_dir: &Path) -> Vec<RecentEntry> {
    let path = data_dir.join(RECENTS_FILE);
    let Ok(text) = fs::read_to_string(&path) else {
        return Vec::new();
    };
    if let Ok(file) = serde_json::from_str::<RecentsFile>(&text) {
        return file.entries;
    }
    serde_json::from_str::<Vec<PathBuf>>(&text)
        .unwrap_or_default()
        .into_iter()
        .map(|path| RecentEntry { path, opened_at: 0 })
        .collect()
}

fn save_entries(data_dir: &Path, entries: &[RecentEntry]) {
    if let Err(err) = fs::create_dir_all(data_dir) {
        tracing::warn!(error = %err, "could not create recents dir");
        return;
    }
    let file = RecentsFile {
        version: RECENTS_VERSION,
        entries: entries.to_vec(),
    };
    match serde_json::to_string_pretty(&file) {
        Ok(text) => {
            if let Err(err) = fs::write(data_dir.join(RECENTS_FILE), text) {
                tracing::warn!(error = %err, "could not write recents");
            }
        }
        Err(err) => tracing::warn!(error = %err, "could not serialize recents"),
    }
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
        let recents = visible_recents(&dir);
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
        let recents = visible_recents(&dir);
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
        let recents = visible_recents(&dir);
        assert_eq!(recents[0].path.as_ref(), &path);
        remember(&dir, &path);
        let text = fs::read_to_string(dir.join(RECENTS_FILE)).unwrap();
        let file: RecentsFile = serde_json::from_str(&text).unwrap();
        assert_eq!(file.version, RECENTS_VERSION);
    }

    #[test]
    fn forget_drops_entry() {
        let dir = scratch();
        let path = dir.join("drop.aria");
        fs::write(&path, "{}").unwrap();
        remember(&dir, &path);
        forget(&dir, &path);
        assert!(visible_recents(&dir).is_empty());
    }
}
