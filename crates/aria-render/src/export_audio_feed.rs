//! The export's audio, mixed ahead of the frame loop on a thread of its own.
//!
//! Mixing a block can block on a decoder: the first block of an export opens
//! every audible clip's decoder (184 ms for one AAC track on the Desktop
//! machine), and a later block can wait on a demuxer. On the frame loop's
//! thread that time adds to every frame's; on a thread of its own it overlaps
//! with rendering and encoding, and the loop only waits when the mix falls
//! more than [`AHEAD`] frames behind.

use std::sync::mpsc::{Receiver, Sender, SyncSender, channel, sync_channel};
use std::thread::Scope;

use aria_core::{DecodeError, EncodeError, Rational};

use crate::error::RenderError;
use crate::export_audio::{EXPORT_AUDIO_CHANNELS, ExportAudioMixer, sample_boundary};

/// Frames the mix may run ahead of the frame loop.
const AHEAD: usize = 16;

/// The mixed block of each exported frame, in frame order.
pub(crate) struct AudioFeed {
    blocks: Receiver<Result<Vec<f32>, DecodeError>>,
    /// Blocks the encoder is done with, for the mix to write the next ones
    /// into.
    spent: Sender<Vec<f32>>,
}

impl AudioFeed {
    /// Starts mixing `frames` frames of `mixer` from timeline frame `start` at
    /// `rate`, on a thread of `scope`.
    ///
    /// The thread ends after the last frame, after a block fails to mix (that
    /// block's error is the last thing sent), or when the feed is dropped.
    pub(crate) fn start<'scope>(
        scope: &'scope Scope<'scope, '_>,
        mut mixer: ExportAudioMixer,
        start: i64,
        frames: u64,
        rate: Rational,
    ) -> Result<Self, RenderError> {
        let (send, blocks) = sync_channel(AHEAD);
        let (spent, reuse) = channel::<Vec<f32>>();
        std::thread::Builder::new()
            .name("aria-export-audio".into())
            .spawn_scoped(scope, move || {
                mix_ahead(&mut mixer, start, frames, rate, &send, &reuse)
            })?;
        Ok(Self { blocks, spent })
    }

    /// The mixed block of the next frame.
    pub(crate) fn next(&self) -> Result<Vec<f32>, RenderError> {
        match self.blocks.recv() {
            Ok(block) => Ok(block?),
            // The thread sends a block for every frame unless it panicked,
            // and the scope re-raises that panic when it joins it.
            Err(_) => Err(RenderError::Encode(EncodeError::Encode(
                "the export audio mix stopped before the last frame".into(),
            ))),
        }
    }

    /// Hands `block` back to be mixed into again.
    pub(crate) fn recycle(&self, block: Vec<f32>) {
        // The thread is gone once the last block is mixed; the block is then
        // dropped here.
        let _ = self.spent.send(block);
    }
}

fn mix_ahead(
    mixer: &mut ExportAudioMixer,
    start: i64,
    frames: u64,
    rate: Rational,
    send: &SyncSender<Result<Vec<f32>, DecodeError>>,
    reuse: &Receiver<Vec<f32>>,
) {
    for index in 0..frames as i64 {
        let from = sample_boundary(start + index, rate.num, rate.den);
        let to = sample_boundary(start + index + 1, rate.num, rate.den);
        let mut block = reuse.try_recv().unwrap_or_default();
        block.clear();
        block.resize(
            (to - from).max(0) as usize * EXPORT_AUDIO_CHANNELS as usize,
            0.0,
        );
        let mixed = mixer.mix_into(from, &mut block).map(|()| block);
        let failed = mixed.is_err();
        if send.send(mixed).is_err() || failed {
            return;
        }
    }
}

#[cfg(test)]
#[path = "export_audio_feed_tests.rs"]
mod tests;
