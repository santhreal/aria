//! Built-in Whisper model catalog, download, and transactional installation.

mod catalog;
mod download;
// File is `fs.rs` per the split layout; the module is named `filesystem` so it
// never shadows `std::fs` in a module that imports both.
#[path = "fs.rs"]
mod filesystem;
mod manager;

#[cfg(test)]
mod tests;

pub use catalog::{
    DownloadError, ModelIntegrityError, ModelManagerError, ModelSpec, ModelStatus, WhisperModel,
};
pub use download::{DownloadReader, HttpDownloader, ModelDownloader};
pub use manager::ModelManager;
