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

/// SHA-256 of a file as lowercase hex — catalog downloads verify against
/// `CatalogEntry::checksum_sha256` before install. Pure-Rust, no new deps:
/// small files (bundles, LUTs, presets), cold path.
pub fn sha256_hex(path: &Path) -> Result<String, CloudError> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256Hasher::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex(&hasher.finalize()))
}

#[derive(Clone)]
pub(crate) struct Sha256Hasher {
    h: [u32; 8],
    buffer: [u8; 64],
    buffer_len: usize,
    total_len: u64,
}

impl Sha256Hasher {
    pub(crate) fn new() -> Self {
        Self {
            h: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
                0x5be0cd19,
            ],
            buffer: [0u8; 64],
            buffer_len: 0,
            total_len: 0,
        }
    }

    pub(crate) fn update(&mut self, mut data: &[u8]) {
        self.total_len += data.len() as u64;
        if self.buffer_len > 0 {
            let to_fill = 64 - self.buffer_len;
            if data.len() < to_fill {
                self.buffer[self.buffer_len..self.buffer_len + data.len()].copy_from_slice(data);
                self.buffer_len += data.len();
                return;
            }
            self.buffer[self.buffer_len..64].copy_from_slice(&data[..to_fill]);
            Self::process_block(&mut self.h, &self.buffer);
            self.buffer_len = 0;
            data = &data[to_fill..];
        }

        while data.len() >= 64 {
            let block: &[u8; 64] = data[..64].try_into().unwrap();
            Self::process_block(&mut self.h, block);
            data = &data[64..];
        }

        if !data.is_empty() {
            self.buffer[..data.len()].copy_from_slice(data);
            self.buffer_len = data.len();
        }
    }

    pub(crate) fn finalize(mut self) -> [u8; 32] {
        let total_bits = self.total_len.wrapping_mul(8);
        self.buffer[self.buffer_len] = 0x80;
        self.buffer_len += 1;
        if self.buffer_len > 56 {
            for b in &mut self.buffer[self.buffer_len..64] {
                *b = 0;
            }
            Self::process_block(&mut self.h, &self.buffer);
            self.buffer_len = 0;
        }
        for b in &mut self.buffer[self.buffer_len..56] {
            *b = 0;
        }
        self.buffer[56..64].copy_from_slice(&total_bits.to_be_bytes());
        Self::process_block(&mut self.h, &self.buffer);

        let mut out = [0u8; 32];
        for (i, word) in self.h.iter().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
        }
        out
    }

    fn process_block(h: &mut [u32; 8], chunk: &[u8; 64]) {
        const K: [u32; 64] = [
            0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
            0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
            0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
            0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
            0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
            0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
            0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
            0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
            0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
            0xc67178f2,
        ];
        let mut w = [0u32; 64];
        for (i, word) in chunk.as_chunks::<4>().0.iter().enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = *h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }
}

#[cfg(test)]
fn sha256(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256Hasher::new();
    hasher.update(bytes);
    hasher.finalize()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_matches_known_vectors() {
        // NIST test vectors.
        assert_eq!(
            hex(&sha256(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hex(&sha256(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        // Multi-block (>64 bytes) input.
        assert_eq!(
            hex(&sha256(
                b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
            )),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    #[test]
    fn sha256_hex_of_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f.bin");
        std::fs::write(&path, b"abc").unwrap();
        assert_eq!(
            sha256_hex(&path).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
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
        let in_mem_hex = hex(&sha256(&data));
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
