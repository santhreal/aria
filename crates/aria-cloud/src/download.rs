//! Direct-CDN file downloads (stock media, template bundles, packs).
//!
//! The `proxy.rs` worker pattern: blocking on a worker thread, progress
//! callbacks, a shared cancel flag, and atomic tmp-then-rename so a crash
//! or cancel can never leave a truncated file a later session would trust.
//! Media bytes never transit the backend — every URL here points at a
//! provider CDN or our asset CDN.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(test)]
use std::time::Duration;

use sha2::{Digest as _, Sha256};

use crate::error::CloudError;

/// Download progress, reported after each chunk.
#[derive(Debug, Clone, Copy)]
pub struct Progress {
    pub bytes_downloaded: u64,
    /// From `Content-Length` when the CDN sends one; 0 = unknown.
    pub total_bytes: u64,
}

/// Download `url` to `dest`, calling `on_progress` as bytes arrive and
/// aborting promptly when `cancel` flips. On success the file is complete
/// at `dest`; on any failure `dest` is untouched (the partial write lives
/// and dies as a `.part` sibling).
pub fn download_to(
    url: &str,
    dest: &Path,
    cancel: &Arc<AtomicBool>,
    mut on_progress: impl FnMut(Progress),
) -> Result<(), CloudError> {
    let agent = crate::http::HttpAgentBuilder::for_caller(crate::http::HttpCaller::CloudDownload)
        .for_url(url)
        .build();
    let response = agent
        .get(url)
        .call()
        .map_err(|e| CloudError::from_ureq(url, e))?;
    let total_bytes: u64 = response
        .header("Content-Length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = part_path(dest);
    let result = stream_to_file(
        response.into_reader(),
        &tmp,
        total_bytes,
        cancel,
        &mut on_progress,
    );
    match result {
        Ok(()) => {
            std::fs::rename(&tmp, dest)?;
            Ok(())
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

fn stream_to_file(
    mut reader: impl Read,
    tmp: &Path,
    total_bytes: u64,
    cancel: &Arc<AtomicBool>,
    on_progress: &mut impl FnMut(Progress),
) -> Result<(), CloudError> {
    let mut file = std::fs::File::create(tmp)?;
    let mut buf = [0u8; 64 * 1024];
    let mut downloaded: u64 = 0;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(CloudError::Cancelled);
        }
        let n = reader
            .read(&mut buf)
            .map_err(|e| CloudError::Network(format!("download stream: {e}")))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        downloaded += n as u64;
        on_progress(Progress {
            bytes_downloaded: downloaded,
            total_bytes,
        });
    }
    file.flush()?;
    Ok(())
}

/// The in-flight sibling of `dest` (`clip.mp4` → `clip.mp4.part`).
fn part_path(dest: &Path) -> PathBuf {
    let mut name = dest.file_name().unwrap_or_default().to_os_string();
    name.push(".part");
    dest.with_file_name(name)
}

/// SHA-256 of a file as lowercase hex. Catalog downloads verify against
/// `CatalogEntry::checksum_sha256` before install.
pub fn sha256_hex(path: &Path) -> Result<String, CloudError> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Catalog checksums compare as lowercase hex, so the file digest is
    /// checked against published SHA-256 vectors, one of them longer than a
    /// 64-byte block.
    #[test]
    fn sha256_hex_of_a_file_matches_known_vectors() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f.bin");
        for (input, digest) in [
            (
                &b""[..],
                "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            ),
            (
                b"abc",
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            ),
            (
                b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq",
                "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
            ),
        ] {
            std::fs::write(&path, input).unwrap();
            assert_eq!(sha256_hex(&path).unwrap(), digest, "{input:?}");
        }
    }

    #[test]
    fn cancelled_download_leaves_no_dest_file() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("media/clip.mp4");
        let cancel = Arc::new(AtomicBool::new(true));
        // Pre-cancelled: the stream loop aborts on entry even though the
        // connection itself fails first here (offline host) — either way,
        // dest must not exist.
        let result = download_to("https://example.invalid/x.mp4", &dest, &cancel, |_| {});
        assert!(result.is_err());
        assert!(!dest.exists());
    }

    #[test]
    fn part_path_appends_suffix() {
        assert_eq!(
            part_path(Path::new("/a/b/clip.mp4")),
            PathBuf::from("/a/b/clip.mp4.part")
        );
    }

    #[test]
    fn sha256_streaming_equivalence() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("large.bin");
        // 200 KiB to cross multiple 64 KiB chunks
        let data: Vec<u8> = (0..200_000).map(|i| (i % 251) as u8).collect();
        std::fs::write(&path, &data).unwrap();
        let stream_hex = sha256_hex(&path).unwrap();
        let in_mem_hex = format!("{:x}", Sha256::digest(&data));
        assert_eq!(stream_hex, in_mem_hex);
    }

    /// A response that stops mid-body must fail on the read timeout rather
    /// than blocking the caller forever.
    ///
    /// The class closed here is a stall with no bound. The assertions are
    /// deliberately unconditional: an earlier version wrapped them in
    /// `if let Ok(resp)`, and because the agent enforces HTTPS-only by default
    /// the plaintext request was rejected before it ever reached the listener,
    /// so the body never ran, nothing was asserted, and the server thread
    /// blocked in `accept` for the life of the process. A vacuous pass that
    /// hangs the suite is worse than a failure.
    ///
    /// Not covered: the connect timeout, and what a proxy or a TLS handshake
    /// does to either bound.
    #[test]
    fn a_stream_that_stops_mid_body_fails_on_the_read_timeout() {
        use std::io::Write as _;
        use std::net::TcpListener;
        use std::time::Instant;

        const READ_TIMEOUT: Duration = Duration::from_millis(100);
        /// Long enough that the read timeout fires first, so the failure is a
        /// timeout and not an end-of-stream, and short enough that the test
        /// costs about a second.
        const HOLD: Duration = Duration::from_millis(1000);
        /// Generous next to a 100 ms read timeout, and still far below any
        /// plausible harness timeout, so a regression to an unbounded wait
        /// fails here instead of hanging the run.
        const BOUND: Duration = Duration::from_secs(5);
        const BODY_BYTES: u64 = 1000;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("the client connects");
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1000\r\n\r\npartial");
            let _ = stream.flush();
            // Hold the connection open without sending the remaining bytes.
            std::thread::sleep(HOLD);
        });

        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("stalled.bin");
        let cancel = Arc::new(AtomicBool::new(false));
        let agent =
            crate::http::HttpAgentBuilder::for_caller(crate::http::HttpCaller::CloudDownload)
                .timeout_connect(Duration::from_millis(100))
                .timeout_read(READ_TIMEOUT)
                // Loopback plaintext: the default agent is HTTPS-only, which
                // would reject the request before it reached the listener.
                .https_only(false)
                .build();
        let response = agent
            .get(&format!("http://127.0.0.1:{port}"))
            .call()
            .expect("the loopback server answers with headers");

        let tmp = part_path(&dest);
        let started = Instant::now();
        let result = stream_to_file(
            response.into_reader(),
            &tmp,
            BODY_BYTES,
            &cancel,
            &mut |_| {},
        );
        let elapsed = started.elapsed();

        match result {
            Err(CloudError::Network(message)) => assert!(
                message.contains("download stream"),
                "the failure should name the stalled read: {message}"
            ),
            other => panic!("expected a network failure on the stalled read, got {other:?}"),
        }
        assert!(
            elapsed < BOUND,
            "the read gave up after {elapsed:?}, past the {BOUND:?} bound"
        );
        // The bytes that did arrive were written, which is what distinguishes a
        // mid-body stall from a request that never connected at all.
        let partial = std::fs::metadata(&tmp).expect("the partial file was created");
        assert!(
            partial.len() > 0 && partial.len() < BODY_BYTES,
            "expected a short partial body, got {} of {BODY_BYTES} bytes",
            partial.len()
        );
        server.join().unwrap();
    }
}
