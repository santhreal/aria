//! File > Import Recent: the files imported last, newest first.
//!
//! The list is `<data dir>/import-recents.json`, a JSON array of paths at most
//! [`LIMIT`] long. `AppState::import_recents` holds it and every change
//! writes it through, so the menu reads memory and the list survives a restart.
//! A file that no longer exists stays listed and its row is disabled.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::AppState;

/// The most files the list holds.
pub const LIMIT: usize = 10;

/// The list's file name in the data directory.
const FILE: &str = "import-recents.json";

/// The file the list is saved in.
pub fn file(data_dir: &Path) -> PathBuf {
    data_dir.join(FILE)
}

/// The saved list, or an empty one when there is none or it does not parse.
pub fn load(data_dir: &Path) -> Vec<PathBuf> {
    let Ok(bytes) = std::fs::read(file(data_dir)) else {
        return Vec::new();
    };
    match serde_json::from_slice::<Vec<PathBuf>>(&bytes) {
        Ok(mut list) => {
            list.truncate(LIMIT);
            list
        }
        Err(err) => {
            tracing::warn!(error = %err, "import-recents.json does not parse; the list starts empty");
            Vec::new()
        }
    }
}

/// `list` with `added` in front in the order given, each path once, cut to
/// [`LIMIT`].
pub fn promoted(list: &[PathBuf], added: &[PathBuf]) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::with_capacity(LIMIT);
    for path in added.iter().chain(list) {
        if out.len() == LIMIT {
            break;
        }
        if !out.contains(path) {
            out.push(path.clone());
        }
    }
    out
}

/// Write `list` to the data directory: to a sibling file first, renamed over
/// the old one, so a crash mid-write leaves the previous list.
fn save(data_dir: &Path, list: &[PathBuf]) -> std::io::Result<()> {
    std::fs::create_dir_all(data_dir)?;
    let target = file(data_dir);
    let partial = target.with_extension("json.partial");
    let bytes = serde_json::to_vec_pretty(list).map_err(std::io::Error::other)?;
    std::fs::write(&partial, bytes)?;
    std::fs::rename(&partial, &target)
}

impl AppState {
    /// The Import Recent list, newest first.
    pub(crate) fn import_recents(&self) -> &[PathBuf] {
        &self.import_recents
    }

    /// Put `paths` at the head of the Import Recent list and save it.
    pub(crate) fn remember_imports(&mut self, paths: &[PathBuf]) {
        if paths.is_empty() {
            return;
        }
        let list = promoted(&self.import_recents, paths);
        if let Err(err) = save(&self.data_dir, &list) {
            self.status_message = Some(format!(
                "The Import Recent list could not be saved to {}: {err}",
                file(&self.data_dir).display()
            ));
        }
        self.import_recents = Arc::from(list);
    }

    /// File > Import Recent > Clear Recent Imports: empty the list and delete
    /// its file.
    pub fn clear_import_recents(&mut self) {
        let target = file(&self.data_dir);
        match std::fs::remove_file(&target) {
            Ok(()) => self.status_message = Some("Recent imports cleared".into()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                self.status_message = Some("Recent imports cleared".into());
            }
            Err(err) => {
                self.status_message =
                    Some(format!("{} could not be deleted: {err}", target.display()));
                return;
            }
        }
        self.import_recents = Arc::from(Vec::new());
    }

    /// File > Import Recent > one file: import it again as File > Import does.
    pub fn import_recent(&mut self, path: &Path) {
        if !path.exists() {
            self.status_message = Some(format!("{} is missing", path.display()));
            return;
        }
        self.import_paths(vec![path.to_path_buf()]);
    }
}

#[cfg(test)]
#[path = "import_recents_tests.rs"]
mod tests;
