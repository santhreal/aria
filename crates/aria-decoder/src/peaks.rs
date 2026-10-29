//! Waveform peak extraction for library tiles and timeline strips.
//!
//! Decodes a source's whole audio stream through the platform reader
//! ([`crate::open_audio_reader`]) and folds it into peak amplitudes — the
//! ported contract of main's FFmpeg-side helpers, re-based on this branch's
//! native decode path. Mono at a fixed analysis rate is plenty for pixels:
//! a waveform never needs channel separation or codec-native rates.

use std::path::Path;

use aria_core::DecodeError;

/// Analysis sample rate. Peaks are per *time*, so any fixed rate works; 48 kHz
/// matches the mix/export rate and every backend resamples to it natively.
const PEAK_RATE: u32 = 48_000;

/// Samples folded into one coarse peak while streaming (final buckets are
/// reduced from these). ~21ms at 48kHz: fine enough for any waveform image.
const COARSE_CHUNK: usize = 1024;

/// Read granularity while streaming the source (mono sample frames).
const READ_FRAMES: usize = 32_768;

/// Peak amplitudes at a fixed time resolution — the "peak file" for a media
/// source, computed once and re-rendered per zoom by the strip worker.
#[derive(Debug, Clone)]
pub struct AudioPeaks {
    /// Actual peaks per second (the requested rate adjusted to a whole number
    /// of samples per peak, and lowered if `max_peaks` capped the output).
    pub per_second: f64,
    /// Peak amplitudes in `0.0..=1.0`, one per `1 / per_second` of audio.
    pub peaks: Vec<f32>,
}

fn cancellation_requested(cancelled: &(impl Fn() -> bool + ?Sized)) -> bool {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(cancelled)).unwrap_or(true)
}

/// Decode the whole audio stream of `path` and return `buckets` peak
/// amplitudes in `0.0..=1.0`, evenly spread across the stream's duration,
/// with cancellation support.
pub(crate) fn audio_peaks_with_cancel(
    path: &Path,
    buckets: usize,
    cancelled: impl Fn() -> bool,
) -> Result<Vec<f32>, DecodeError> {
    if buckets == 0 {
        return Err(DecodeError::unsupported("zero waveform buckets"));
    }
    let coarse = decode_coarse_peaks_with_cancel(path, COARSE_CHUNK, &cancelled)?;
    if coarse.is_empty() {
        return Err(DecodeError::unsupported("audio stream has no samples"));
    }
    Ok(reduce_to_buckets(&coarse, buckets))
}

/// Decode the whole audio stream of `path` and return `buckets` peak
/// amplitudes in `0.0..=1.0`, evenly spread across the stream's duration.
pub fn audio_peaks(path: &Path, buckets: usize) -> Result<Vec<f32>, DecodeError> {
    audio_peaks_with_cancel(path, buckets, || false)
}

/// Decode the whole audio stream of `path` into peaks at (approximately)
/// `per_second` peaks per second of audio, capped at `max_peaks` entries,
/// with cancellation support.
pub(crate) fn audio_peaks_per_second_with_cancel(
    path: &Path,
    per_second: f64,
    max_peaks: usize,
    cancelled: impl Fn() -> bool,
) -> Result<AudioPeaks, DecodeError> {
    if per_second <= 0.0 || per_second.is_nan() || max_peaks == 0 {
        return Err(DecodeError::unsupported("invalid waveform resolution"));
    }

    let chunk = ((f64::from(PEAK_RATE) / per_second).round() as usize).max(1);
    let coarse = decode_coarse_peaks_with_cancel(path, chunk, &cancelled)?;
    if coarse.is_empty() {
        return Err(DecodeError::unsupported("audio stream has no samples"));
    }

    // The chunk is a whole number of samples, so the realized rate differs
    // slightly from the request; report what the data actually is.
    let realized = f64::from(PEAK_RATE) / chunk as f64;
    if coarse.len() <= max_peaks {
        return Ok(AudioPeaks {
            per_second: realized,
            peaks: coarse,
        });
    }
    let reduced = reduce_to_buckets(&coarse, max_peaks);
    Ok(AudioPeaks {
        per_second: realized * max_peaks as f64 / coarse.len() as f64,
        peaks: reduced,
    })
}

/// Decode the whole audio stream of `path` into peaks at (approximately)
/// `per_second` peaks per second of audio, capped at `max_peaks` entries.
pub fn audio_peaks_per_second(
    path: &Path,
    per_second: f64,
    max_peaks: usize,
) -> Result<AudioPeaks, DecodeError> {
    audio_peaks_per_second_with_cancel(path, per_second, max_peaks, || false)
}

/// Stream the source as mono f32 at [`PEAK_RATE`] and fold `|sample|` maxima
/// per `chunk` samples, so memory stays O(duration / chunk).
pub(crate) fn decode_coarse_peaks_with_cancel<C>(
    path: &Path,
    chunk: usize,
    cancelled: &C,
) -> Result<Vec<f32>, DecodeError>
where
    C: Fn() -> bool + ?Sized,
{
    if cancellation_requested(cancelled) {
        return Err(DecodeError::Decode("audio peak decode cancelled".into()));
    }
    let mut reader = crate::open_audio_reader(path, PEAK_RATE, 1)?;
    decode_coarse_peaks_from_reader(reader.as_mut(), chunk, cancelled)
}

pub(crate) fn decode_coarse_peaks_from_reader<C>(
    reader: &mut dyn aria_core::AudioReader,
    chunk: usize,
    cancelled: &C,
) -> Result<Vec<f32>, DecodeError>
where
    C: Fn() -> bool + ?Sized,
{
    let chunk = chunk.max(1);
    let mut coarse = Vec::new();
    let mut current = 0f32;
    let mut filled = 0usize;
    let mut buf = vec![0f32; READ_FRAMES];

    loop {
        if cancellation_requested(cancelled) {
            return Err(DecodeError::Decode("audio peak decode cancelled".into()));
        }
        let got = reader.read(&mut buf)?;
        if got == 0 {
            break;
        }
        for &s in &buf[..got] {
            current = current.max(s.abs().min(1.0));
            filled += 1;
            if filled == chunk {
                coarse.push(current);
                current = 0.0;
                filled = 0;
            }
        }
        if cancellation_requested(cancelled) {
            return Err(DecodeError::Decode("audio peak decode cancelled".into()));
        }
    }
    if filled > 0 {
        coarse.push(current);
    }
    Ok(coarse)
}

/// Max-reduce `coarse` peaks into exactly `buckets` values.
fn reduce_to_buckets(coarse: &[f32], buckets: usize) -> Vec<f32> {
    let n = coarse.len();
    (0..buckets)
        .map(|b| {
            let start = b * n / buckets;
            let end = (((b + 1) * n).div_ceil(buckets)).clamp(start + 1, n);
            coarse[start..end].iter().copied().fold(0.0, f32::max)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn any_audio_asset() -> Option<PathBuf> {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        // The checked-in iOS fixture tone guarantees real decode coverage;
        // local-assets lets developers point at richer media. Require the
        // reader to actually open it, not just that the file exists: platforms
        // with no native audio backend (e.g. Linux CI) can't decode it, and
        // these tests skip rather than assert on an Unsupported error.
        let fixture = root.join("../../apps/aria-ios-macos/aria-ios-macos/Fixtures/tone.m4a");
        if fixture.exists() && crate::open_audio_reader(&fixture, PEAK_RATE, 1).is_ok() {
            return Some(fixture);
        }
        std::fs::read_dir(root.join("../../local-assets/assets"))
            .ok()?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .find(|p| {
                p.extension().is_some_and(|e| e == "mp3" || e == "m4a")
                    || (p.extension().is_some_and(|e| e == "mp4")
                        && crate::open_audio_reader(p, PEAK_RATE, 1).is_ok())
            })
    }

    #[test]
    fn peaks_have_requested_bucket_count_and_range() {
        let Some(path) = any_audio_asset() else {
            return;
        };
        let peaks = audio_peaks(&path, 48).expect("peaks");
        assert_eq!(peaks.len(), 48);
        assert!(peaks.iter().all(|p| (0.0..=1.0).contains(p)));
        // Real audio is not pure silence.
        assert!(peaks.iter().any(|&p| p > 0.0));
    }

    #[test]
    fn reduce_to_buckets_takes_max_per_range() {
        let coarse = vec![0.1, 0.9, 0.2, 0.3, 0.8, 0.1];
        assert_eq!(reduce_to_buckets(&coarse, 3), vec![0.9, 0.3, 0.8]);
    }

    #[test]
    fn reduce_to_buckets_handles_fewer_coarse_than_buckets() {
        let coarse = vec![0.5, 1.0];
        let out = reduce_to_buckets(&coarse, 4);
        assert_eq!(out.len(), 4);
        assert!(out.iter().all(|p| *p == 0.5 || *p == 1.0));
    }

    #[test]
    fn zero_buckets_is_rejected() {
        let err = audio_peaks(Path::new("/nonexistent.mp3"), 0);
        assert!(matches!(err, Err(DecodeError::Unsupported(_))));
    }

    #[test]
    fn per_second_peaks_match_duration() {
        let Some(path) = any_audio_asset() else {
            return;
        };
        let peaks = audio_peaks_per_second(&path, 100.0, 1_000_000).expect("peaks");
        assert!(
            (peaks.per_second - 100.0).abs() < 1.0,
            "realized rate ≈ requested"
        );
        assert!(peaks.peaks.iter().all(|p| (0.0..=1.0).contains(p)));
        assert!(peaks.peaks.iter().any(|&p| p > 0.0));
        // Sanity: count implies a plausible duration (between 1s and 1h).
        let duration_s = peaks.peaks.len() as f64 / peaks.per_second;
        assert!(
            duration_s > 1.0 && duration_s < 3600.0,
            "duration {duration_s}"
        );
    }

    struct MockAudioReader {
        reads_left: usize,
        position: i64,
    }

    impl aria_core::AudioReader for MockAudioReader {
        fn read(&mut self, buf: &mut [f32]) -> Result<usize, DecodeError> {
            if self.reads_left == 0 {
                return Ok(0);
            }
            self.reads_left -= 1;
            for s in buf.iter_mut() {
                *s = 0.5;
            }
            self.position += buf.len() as i64;
            Ok(buf.len())
        }

        fn seek_to_frame(&mut self, frame: i64) -> Result<(), DecodeError> {
            self.position = frame;
            Ok(())
        }

        fn position(&self) -> Option<i64> {
            Some(self.position)
        }
    }

    #[test]
    fn mid_decode_cancel_returns_immediately() {
        let mut reader = MockAudioReader {
            reads_left: 100,
            position: 0,
        };
        let read_count = std::sync::atomic::AtomicUsize::new(0);
        let cancel_predicate = || {
            let count = read_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            count >= 2
        };
        let res = decode_coarse_peaks_from_reader(&mut reader, 1024, &cancel_predicate);
        assert!(matches!(res, Err(DecodeError::Decode(msg)) if msg.contains("cancelled")));
        // Reader was stopped early
        assert!(reader.reads_left > 90);
    }
}
