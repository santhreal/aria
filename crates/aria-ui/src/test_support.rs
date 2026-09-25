//! What the crate's tests share: the walker the censuses read the sources
//! with, and the census modules themselves.
//!
//! Each census reads the crate, or the workspace, off disk and asserts something
//! about its shape that no single unit test can see: one owner for temp paths,
//! one module bringing up the GPU, one profile root, one owner for threads, one
//! direction for the dependency between the UI and the plugin boundary. Each one
//! is derived from the sources at run time rather than from a list of known call
//! sites, so a new file is covered on the run that adds it, and each one goes red
//! when a declared owner stops being one, so an allowlist cannot rot into an
//! exemption for a file that does nothing.
//!
//! Scratch directories are not here. `crate::scratch_pool` owns the pool, the
//! lease, and every rule about a pool with no free slot left.

use std::path::{Path, PathBuf};

/// Every `.rs` file under `dir`, recursively.
///
/// The censuses below read the sources rather than a list of call sites, and so
/// does the pool's own rule about spawned threads, so the walker is shared.
pub(crate) fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

#[cfg(test)]
mod error_writes;

#[cfg(test)]
mod temp_ownership;

#[cfg(test)]
mod gpu;

#[cfg(test)]
mod profile_root;

#[cfg(test)]
mod temp_growth;

#[cfg(test)]
mod processes;

#[cfg(test)]
mod threads;

#[cfg(test)]
mod dependency_direction;

#[cfg(test)]
mod shell_draw;

#[cfg(test)]
mod deferred_docs;
