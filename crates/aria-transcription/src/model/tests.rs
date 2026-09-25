//! Tests for this module.
//!
//! The cases live in sibling files, one per area, declared at the bottom. This
//! file holds only the shared fixtures; a child sees them through `use super::*`.

use std::io::Cursor;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Barrier, mpsc};
use std::thread;
use std::time::Instant;

use tempfile::TempDir;

use super::*;

const HELLO_SHA256: &str = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
const TINY_SPEC: ModelSpec = ModelSpec::new(
    "test.tiny",
    "tiny-model.bin",
    "https://example.invalid/tiny-model.bin",
    &[],
    5,
    HELLO_SHA256,
    true,
    "five test bytes",
);

struct CountingDownloader {
    bytes: &'static [u8],
    calls: AtomicUsize,
}

impl CountingDownloader {
    fn new(bytes: &'static [u8]) -> Self {
        Self {
            bytes,
            calls: AtomicUsize::new(0),
        }
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl ModelDownloader for CountingDownloader {
    fn download(&self, _spec: &ModelSpec) -> Result<DownloadReader, DownloadError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(Cursor::new(self.bytes)))
    }
}

struct InterruptedReader {
    emitted: bool,
}

impl Read for InterruptedReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.emitted {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionReset,
                "injected interruption",
            ));
        }
        self.emitted = true;
        buffer[..2].copy_from_slice(b"he");
        Ok(2)
    }
}

struct CancellingReader {
    read_count: usize,
    cancelled: Arc<AtomicBool>,
}

impl Read for CancellingReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let bytes = if self.read_count == 0 {
            &b"he"[..]
        } else {
            self.cancelled.store(true, Ordering::SeqCst);
            &b"ll"[..]
        };
        self.read_count += 1;
        buffer[..bytes.len()].copy_from_slice(bytes);
        Ok(bytes.len())
    }
}

struct CompletionTrackingReader {
    cursor: Cursor<&'static [u8]>,
    read_calls: Arc<AtomicUsize>,
    saw_eof: Arc<AtomicBool>,
}

impl Read for CompletionTrackingReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.read_calls.fetch_add(1, Ordering::SeqCst);
        let count = self.cursor.read(buffer)?;
        if count == 0 {
            self.saw_eof.store(true, Ordering::SeqCst);
        }
        Ok(count)
    }
}

struct CancelAtEofReader {
    cursor: Cursor<&'static [u8]>,
    cancelled: Arc<AtomicBool>,
}

impl Read for CancelAtEofReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let count = self.cursor.read(buffer)?;
        if count == 0 {
            self.cancelled.store(true, Ordering::SeqCst);
        }
        Ok(count)
    }
}

fn assert_directory_empty(path: &Path) {
    assert_eq!(fs::read_dir(path).expect("read model directory").count(), 0);
}

fn assert_no_temporary_files(path: &Path) {
    for entry in fs::read_dir(path).expect("read model directory") {
        let entry = entry.expect("directory entry");
        assert!(
            !entry.file_name().to_string_lossy().ends_with(".tmp"),
            "temporary model file leaked: {}",
            entry.path().display()
        );
    }
}

#[path = "tests_begin_commit.rs"]
mod begin_commit;

#[path = "tests_cancellation_after.rs"]
mod cancellation_after;

#[path = "tests_rejects_symlink.rs"]
mod rejects_symlink;
