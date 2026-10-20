//! Decoders registered at run time. A registered decoder claims files by
//! extension; [`crate::probe`], [`crate::open_video_decoder`], and
//! [`crate::open_audio_reader`] try every claimant of a path, highest priority
//! first and then in registration order, before the platform decoder.
//!
//! The list is process-wide, as the platform codecs are: every renderer,
//! importer, and analysis job opens media through the same functions.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, PoisonError, RwLock};

use aria_core::{AudioReader, DecodeError, VideoDecoder};

use crate::probe::MediaProbe;

/// A decoder contributed at run time.
///
/// An error from any call passes the path to the next claimant and then to the
/// platform decoder. [`DecodeError::Unsupported`] states that this decoder
/// does not read the file; any other error is reported if no later decoder
/// opens it.
pub trait ExternalDecoder: Send + Sync {
    /// Read `path`'s metadata without decoding frames.
    fn probe(&self, path: &Path) -> Result<MediaProbe, DecodeError>;

    /// Open `path`'s video track. Frames are CPU planes in a format the
    /// compositor uploads: [`aria_core::PixelFormat::Nv12`] or
    /// [`aria_core::PixelFormat::Yuv420p`].
    fn open_video(&self, path: &Path) -> Result<Box<dyn VideoDecoder>, DecodeError>;

    /// Open `path`'s audio track as interleaved `f32` at `out_rate` Hz and
    /// `channels` channels.
    fn open_audio(
        &self,
        path: &Path,
        out_rate: u32,
        channels: u16,
    ) -> Result<Box<dyn AudioReader>, DecodeError> {
        let _ = (path, out_rate, channels);
        Err(DecodeError::unsupported("this decoder reads no audio"))
    }
}

struct Claimant {
    serial: u64,
    id: Arc<str>,
    /// Lowercase, without the dot.
    extensions: Box<[String]>,
    priority: i32,
    decoder: Arc<dyn ExternalDecoder>,
}

static CLAIMANTS: RwLock<Vec<Arc<Claimant>>> = RwLock::new(Vec::new());
static SERIAL: AtomicU64 = AtomicU64::new(0);

/// Keeps one decoder registered; dropping it removes the decoder.
#[derive(Debug)]
pub struct ExternalDecoderHandle {
    serial: u64,
}

impl Drop for ExternalDecoderHandle {
    fn drop(&mut self) {
        CLAIMANTS
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|claimant| claimant.serial != self.serial);
    }
}

/// Register `decoder` under `id` for files whose lowercase extension is in
/// `extensions`. Among claimants of one path, a higher `priority` is tried
/// first; equal priorities are tried in registration order.
pub fn register_external_decoder(
    id: &str,
    extensions: &[String],
    priority: i32,
    decoder: Arc<dyn ExternalDecoder>,
) -> ExternalDecoderHandle {
    let serial = SERIAL.fetch_add(1, Ordering::Relaxed);
    let claimant = Arc::new(Claimant {
        serial,
        id: Arc::from(id),
        extensions: extensions
            .iter()
            .map(|ext| ext.to_ascii_lowercase())
            .collect(),
        priority,
        decoder,
    });
    let mut claimants = CLAIMANTS.write().unwrap_or_else(PoisonError::into_inner);
    // Sorted by descending priority, then serial: a new entry goes after every
    // entry of its priority.
    let at = claimants.partition_point(|other| other.priority >= priority);
    claimants.insert(at, claimant);
    ExternalDecoderHandle { serial }
}

/// The lowercase extensions every registered decoder claims, sorted and
/// without duplicates.
pub fn external_decoder_extensions() -> Vec<String> {
    let claimants = CLAIMANTS.read().unwrap_or_else(PoisonError::into_inner);
    let mut out: Vec<String> = claimants
        .iter()
        .flat_map(|claimant| claimant.extensions.iter().cloned())
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// The claimants of `path`, in the order they are tried.
fn claimants(path: &Path) -> Vec<Arc<Claimant>> {
    let Some(ext) = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase)
    else {
        return Vec::new();
    };
    CLAIMANTS
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .filter(|claimant| claimant.extensions.contains(&ext))
        .cloned()
        .collect()
}

/// Run `open` on each claimant of `path` in order and return the first
/// success, or `Ok(Err(failures))` with each claimant's error that was not
/// [`DecodeError::Unsupported`], prefixed with its id.
fn first<T>(
    path: &Path,
    open: impl Fn(&dyn ExternalDecoder) -> Result<T, DecodeError>,
) -> Result<T, Vec<String>> {
    let mut failures = Vec::new();
    for claimant in claimants(path) {
        match open(claimant.decoder.as_ref()) {
            Ok(opened) => return Ok(opened),
            Err(DecodeError::Unsupported(_)) => {}
            Err(err) => failures.push(format!("decoder {}: {err}", claimant.id)),
        }
    }
    Err(failures)
}

/// Try the claimants, then `native`. When every one fails, the native error
/// is returned with the claimants' errors appended.
pub(crate) fn dispatch<T>(
    path: &Path,
    external: impl Fn(&dyn ExternalDecoder) -> Result<T, DecodeError>,
    native: impl FnOnce() -> Result<T, DecodeError>,
) -> Result<T, DecodeError> {
    let failures = match first(path, external) {
        Ok(opened) => return Ok(opened),
        Err(failures) => failures,
    };
    native().map_err(|err| {
        if failures.is_empty() {
            return err;
        }
        let joined = failures.join("; ");
        match err {
            DecodeError::Open(msg) => DecodeError::Open(format!("{msg}; {joined}")),
            DecodeError::Io(msg) => DecodeError::Io(format!("{msg}; {joined}")),
            DecodeError::Decode(msg) => DecodeError::Decode(format!("{msg}; {joined}")),
            DecodeError::Seek(msg) => DecodeError::Seek(format!("{msg}; {joined}")),
            DecodeError::Unsupported(msg) => DecodeError::Open(format!("{msg}; {joined}")),
        }
    })
}

#[cfg(test)]
#[path = "external_tests.rs"]
mod tests;
