//! WHY: the GStreamer and AVFoundation audio readers each kept a copy of the
//! loop that hands decoded blocks out in whole frames. These tests hold the
//! single [`read_frames`] loop to that contract: frames cross block
//! boundaries in order, a short `out` leaves the rest queued, a trailing
//! partial frame is dropped instead of spinning, a source that ends gives a
//! short read and then zero, and every read terminates within a fixed number
//! of queue checks.
//!
//! Not caught: how each backend decodes bytes into the queue (endianness,
//! empty samples, position anchoring). Those stay in the backends and are
//! exercised by the readers' own fixture tests.

use std::collections::VecDeque;

use aria_core::DecodeError;

use super::{PendingSamples, PendingSource, read_frames};

/// Queue checks one read may make before the test calls it a hang.
const CHECK_BOUND: usize = 1_000;

/// A source that serves fixed blocks, then reports end of stream.
#[derive(Default)]
struct Blocks {
    pending: PendingSamples,
    blocks: VecDeque<Vec<f32>>,
    fails: bool,
    fills: usize,
    checks: usize,
}

impl Blocks {
    fn new(blocks: &[&[f32]]) -> Self {
        Self {
            blocks: blocks.iter().map(|block| block.to_vec()).collect(),
            ..Self::default()
        }
    }

    fn read(&mut self, frames: usize, channels: usize) -> (usize, Vec<f32>) {
        self.checks = 0;
        let mut out = vec![f32::NAN; frames * channels];
        let produced = read_frames(self, &mut out, channels).expect("blocks never fail");
        (produced, out)
    }
}

impl PendingSource for Blocks {
    fn pending(&mut self) -> &mut PendingSamples {
        self.checks += 1;
        assert!(
            self.checks <= CHECK_BOUND,
            "read_frames made {CHECK_BOUND} queue checks without finishing"
        );
        &mut self.pending
    }

    fn fill_pending(&mut self) -> Result<bool, DecodeError> {
        self.fills += 1;
        if self.fails {
            return Err(DecodeError::Decode("fixture decode failure".into()));
        }
        Ok(match self.blocks.pop_front() {
            Some(block) => {
                self.pending.refill().extend(block);
                true
            }
            None => false,
        })
    }
}

#[test]
fn frames_cross_block_boundaries_in_order() {
    let mut source = Blocks::new(&[&[1.0, 2.0, 3.0, 4.0], &[5.0, 6.0], &[7.0, 8.0]]);
    let (produced, out) = source.read(3, 2);
    assert_eq!(produced, 3);
    assert_eq!(out, [1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
    assert_eq!(source.fills, 2, "the third block is not needed yet");
}

#[test]
fn short_out_leaves_the_rest_queued() {
    let mut source = Blocks::new(&[&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]]);
    let (produced, out) = source.read(1, 2);
    assert_eq!((produced, out), (1, vec![1.0, 2.0]));
    let (produced, out) = source.read(2, 2);
    assert_eq!((produced, out), (2, vec![3.0, 4.0, 5.0, 6.0]));
    assert_eq!(source.fills, 1, "queued samples are served before a refill");
}

#[test]
fn trailing_partial_frame_is_dropped() {
    let mut source = Blocks::new(&[&[1.0, 2.0, 3.0], &[7.0, 8.0]]);
    let (produced, out) = source.read(2, 2);
    assert_eq!(produced, 2);
    assert_eq!(out, [1.0, 2.0, 7.0, 8.0]);
}

#[test]
fn block_shorter_than_a_frame_is_dropped() {
    let mut source = Blocks::new(&[&[9.0], &[1.0, 2.0, 3.0]]);
    let (produced, out) = source.read(1, 3);
    assert_eq!((produced, out), (1, vec![1.0, 2.0, 3.0]));
}

#[test]
fn end_of_stream_gives_a_short_read_then_zero() {
    let mut source = Blocks::new(&[&[1.0, 2.0]]);
    let (produced, out) = source.read(2, 2);
    assert_eq!(produced, 1);
    assert_eq!(out[..2], [1.0, 2.0]);
    assert!(
        out[2..].iter().all(|sample| sample.is_nan()),
        "unwritten tail"
    );
    assert_eq!(source.read(2, 2).0, 0);
    assert_eq!(
        source.fills, 3,
        "two fills for the first read, one for the second"
    );
}

#[test]
fn empty_out_decodes_nothing() {
    let mut source = Blocks::new(&[&[1.0, 2.0]]);
    assert_eq!(source.read(0, 2).0, 0);
    assert_eq!(source.fills, 0);
}

#[test]
fn clear_discards_queued_samples() {
    let mut source = Blocks::new(&[&[1.0, 2.0, 3.0, 4.0], &[9.0, 10.0]]);
    assert_eq!(source.read(1, 2).0, 1);
    source.pending.clear();
    let (produced, out) = source.read(1, 2);
    assert_eq!((produced, out), (1, vec![9.0, 10.0]));
    assert_eq!(source.fills, 2);
}

#[test]
fn fill_error_propagates() {
    let mut source = Blocks::new(&[&[1.0, 2.0]]);
    source.fails = true;
    let mut out = [0.0; 4];
    assert!(matches!(
        read_frames(&mut source, &mut out, 2),
        Err(DecodeError::Decode(_))
    ));
}

#[test]
fn refill_reuses_the_allocation() {
    let mut pending = PendingSamples::default();
    pending.refill().extend((0..64).map(|i| i as f32));
    let first = pending.samples.as_ptr();
    let samples = pending.refill();
    assert!(samples.is_empty(), "refill empties the queue");
    samples.extend((0..64).map(|i| -(i as f32)));
    assert_eq!(pending.samples.as_ptr(), first);
    assert_eq!(pending.cursor, 0);
}
