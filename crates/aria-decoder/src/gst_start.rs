//! Opening a decode pipeline at a start position.
//!
//! A pipeline prerolls on the first frame it decodes, so a decoder opened and
//! then seeked decodes frames at stream start only for the seek to flush them.
//! A video decoder takes frames in before its first output (the B-frame
//! reorder delay) and decodes on into the sink's queue after the preroll, so
//! the seek's flush also waits for a decode in flight. On `hevc-2160p60` on an
//! i7-11700K the preroll decoded three 4K frames in 42 ms, and the seek's
//! flush waited 16 ms for a fourth.
//!
//! [`StartHold`] blocks the input of each video decoder the pipeline plugs. A
//! buffer reaching one shows the stream is demuxed and the decoder has its
//! caps. [`seek_held`] then sends the start seek upstream from that decoder's
//! sink pad to the demuxer, whose flush drops the blocked buffer, and releases
//! every block: the first frame decoded is at the start's sync sample. A start
//! at or past the stream's duration, a pipeline that prerolls with no decoder
//! held, and a seek the demuxer rejects each release the hold unseeked, and the
//! pipeline prerolls at stream start.

use parking_lot::{Condvar, Mutex};
use std::sync::Arc;
use std::time::{Duration, Instant};

use gstreamer as gst;
use gstreamer::prelude::*;

use crate::gst_key_snap::KeySnap;

/// How often a wait for a held decoder checks whether the pipeline finished
/// its state change without one.
const POLL: Duration = Duration::from_millis(5);

#[derive(Default)]
struct Held {
    /// Each held decoder sink pad and its blocking probe.
    blocks: Vec<(gst::Pad, gst::PadProbeId)>,
    /// The first held sink pad a buffer reached.
    reached: Option<gst::Pad>,
    /// Set by [`StartHold::release`]: a decoder plugged afterwards is not held.
    released: bool,
}

/// The input of each video decoder in one pipeline, blocked until the start
/// seek is sent.
#[derive(Default)]
pub(crate) struct StartHold {
    held: Mutex<Held>,
    reached: Condvar,
}

impl StartHold {
    /// Block the sink pad of each video decoder `bin` plugs.
    pub(crate) fn install(bin: &gst::Bin) -> Arc<Self> {
        let hold = Arc::new(Self::default());
        let weak = Arc::downgrade(&hold);
        bin.connect_deep_element_added(move |_, _, element| {
            let Some(hold) = weak.upgrade() else {
                return;
            };
            if !element
                .factory()
                .is_some_and(|factory| crate::gst::is_video_decoder(&factory))
            {
                return;
            }
            if let Some(pad) = element.static_pad("sink") {
                hold.block(pad);
            }
        });
        hold
    }

    fn block(self: &Arc<Self>, pad: gst::Pad) {
        if self.held.lock().released {
            return;
        }
        let hold = Arc::downgrade(self);
        let Some(id) = pad.add_probe(
            gst::PadProbeType::BLOCK | gst::PadProbeType::BUFFER,
            move |pad, _| match hold.upgrade() {
                Some(hold) => hold.reach(pad),
                // No hold is left to release this probe.
                None => gst::PadProbeReturn::Remove,
            },
        ) else {
            return;
        };
        let mut held = self.held.lock();
        if held.released {
            drop(held);
            pad.remove_probe(id);
            return;
        }
        held.blocks.push((pad, id));
    }

    /// A buffer reached held `pad`: keep it blocked until the hold is
    /// released.
    fn reach(&self, pad: &gst::Pad) -> gst::PadProbeReturn {
        let mut held = self.held.lock();
        if held.released {
            // `release` removes this probe once it takes the lock back.
            return gst::PadProbeReturn::Pass;
        }
        if held.reached.is_none() {
            held.reached = Some(pad.clone());
            self.reached.notify_all();
        }
        gst::PadProbeReturn::Ok
    }

    /// Wait until a buffer reaches a held decoder, and return its sink pad.
    /// `None` once `done` holds or `within` passes first.
    fn wait_reached(&self, within: Duration, done: impl Fn() -> bool) -> Option<gst::Pad> {
        let deadline = Instant::now() + within;
        loop {
            {
                let mut held = self.held.lock();
                if let Some(pad) = &held.reached {
                    return Some(pad.clone());
                }
                self.reached.wait_for(&mut held, POLL);
                if let Some(pad) = &held.reached {
                    return Some(pad.clone());
                }
            }
            if done() || Instant::now() >= deadline {
                return None;
            }
        }
    }

    /// Unblock every held decoder. A decoder plugged afterwards is not held.
    pub(crate) fn release(&self) {
        let blocks = {
            let mut held = self.held.lock();
            held.released = true;
            std::mem::take(&mut held.blocks)
        };
        for (pad, id) in blocks {
            pad.remove_probe(id);
        }
    }
}

/// Once a buffer reaches a decoder `hold` blocks, seek the pipeline to the
/// sync sample at or before `start` through `snap`, then release the hold.
/// Returns the seek point the preroll lands at: `start` after the seek, zero
/// when the hold was released unseeked because `done` held or `within` passed
/// before a buffer reached a held decoder, `start` is not inside the stream,
/// or the demuxer rejected the seek.
pub(crate) fn seek_held(
    hold: &StartHold,
    snap: &KeySnap,
    start: gst::ClockTime,
    within: Duration,
    done: impl Fn() -> bool,
) -> gst::ClockTime {
    let at = hold
        .wait_reached(within, done)
        .filter(|pad| {
            pad.peer_query_duration::<gst::ClockTime>()
                .is_some_and(|duration| start < duration)
        })
        .map_or(gst::ClockTime::ZERO, |pad| {
            snap.arm(start);
            if pad.push_event(crate::gst::key_unit_seek(start)) {
                start
            } else {
                gst::ClockTime::ZERO
            }
        });
    hold.release();
    at
}

#[cfg(test)]
#[path = "gst_start_tests.rs"]
mod tests;
