//! Interleaved samples an audio reader decoded ahead of
//! [`AudioReader::read`](aria_core::AudioReader::read), and the read loop the
//! GStreamer and AVFoundation readers share.

use aria_core::DecodeError;

/// Decoded interleaved `f32` samples not yet handed out, and the read offset
/// into them.
#[derive(Debug, Default)]
pub(crate) struct PendingSamples {
    samples: Vec<f32>,
    cursor: usize,
}

impl PendingSamples {
    /// Empties the queue and returns its buffer for the next decoded block.
    /// The buffer keeps its allocation.
    pub(crate) fn refill(&mut self) -> &mut Vec<f32> {
        self.clear();
        &mut self.samples
    }

    /// Drops every pending sample, as after a seek.
    pub(crate) fn clear(&mut self) {
        self.samples.clear();
        self.cursor = 0;
    }

    fn is_drained(&self) -> bool {
        self.cursor >= self.samples.len()
    }

    /// Moves whole frames of `channels` samples into the front of `out` and
    /// returns how many. A trailing partial frame is discarded.
    fn drain_into(&mut self, out: &mut [f32], channels: usize) -> usize {
        let available = (self.samples.len() - self.cursor) / channels;
        let take = available.min(out.len() / channels);
        if available == 0 {
            self.cursor = self.samples.len();
            return 0;
        }
        let len = take * channels;
        out[..len].copy_from_slice(&self.samples[self.cursor..self.cursor + len]);
        self.cursor += len;
        take
    }
}

/// An audio reader that decodes interleaved blocks into a [`PendingSamples`].
pub(crate) trait PendingSource {
    /// The queue [`fill_pending`](Self::fill_pending) decodes into.
    fn pending(&mut self) -> &mut PendingSamples;

    /// Decodes the next block into [`pending`](Self::pending). Returns `false`
    /// when it decoded no samples, which ends the current read short; at end
    /// of stream it returns `false` on every call without touching the decoder.
    fn fill_pending(&mut self) -> Result<bool, DecodeError>;
}

/// Fills `out` with whole frames of `channels` interleaved samples, decoding
/// blocks as the queue empties, and returns the frames written. Fewer frames
/// than `out` holds means [`PendingSource::fill_pending`] returned `false`.
pub(crate) fn read_frames(
    source: &mut impl PendingSource,
    out: &mut [f32],
    channels: usize,
) -> Result<usize, DecodeError> {
    debug_assert_eq!(out.len() % channels, 0, "buffer not a frame multiple");
    let want = out.len() / channels;
    let mut produced = 0;
    while produced < want {
        let pending = source.pending();
        if pending.is_drained() {
            if !source.fill_pending()? {
                break;
            }
            continue;
        }
        produced += pending.drain_into(&mut out[produced * channels..], channels);
    }
    Ok(produced)
}

#[cfg(test)]
#[path = "pending_tests.rs"]
mod tests;
