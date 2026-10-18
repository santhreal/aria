//! A key-unit seek that starts the video decoder at the video keyframe.
//!
//! `qtdemux` answers a `KEY_UNIT | SNAP_BEFORE` seek with the earliest
//! keyframe at or before the target across every track, and every AAC packet
//! is a keyframe. When the target is on a video keyframe, or less than one
//! audio packet after one, the segment starts at the audio packet before it,
//! and the video track starts at the keyframe before that: the decoder
//! decodes a whole GOP only to have the frames clipped. On `h264-1080p60`
//! (1 s GOP) a seek to a keyframe took 92 to 147 ms on an i7-11700K and 26 to
//! 29 ms on a Ryzen 9 9950X, against 12 to 24 ms and 1.3 ms for a target
//! between keyframes.
//!
//! [`KeySnap`] watches the video decoder's sink pad. After a seek it reads
//! the new segment start: when the first buffer is a keyframe before that
//! start and the start is before the target, the seek has this shape, and
//! `qtdemux` guarantees a video keyframe in `(start, target]`. Every buffer
//! before that keyframe is consumed, so the decoder starts there. A
//! fragmented MP4 is not adjusted by `qtdemux` (its segment starts at the
//! target), so it is left alone.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use gstreamer as gst;
use gstreamer::prelude::*;

/// Slack for comparing buffer and segment times, which `qtdemux` derives
/// from one sample table but rounds separately.
const TOLERANCE: gst::ClockTime = gst::ClockTime::from_mseconds(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// No seek in flight: every buffer passes.
    Idle,
    /// A seek was issued; its flush has not reached the decoder, so buffers
    /// and segments are still the old position's.
    AwaitFlush,
    /// The seek's flush arrived; its segment has not.
    AwaitSegment,
    /// The seek's segment arrived; its first buffer decides.
    Fresh { start: gst::ClockTime },
    /// Consuming buffers until the video keyframe after the segment start.
    Skipping,
}

#[derive(Debug)]
struct State {
    target: Option<gst::ClockTime>,
    phase: Phase,
    /// The first buffer the decoder received after the last seek's segment.
    #[cfg(test)]
    first_fed: Option<gst::ClockTime>,
}

/// Per-pipeline state of the keyframe snap.
#[derive(Debug)]
pub(crate) struct KeySnap {
    /// Whether the demuxer is `qtdemux`, whose seek this corrects.
    qtdemux: AtomicBool,
    state: Mutex<State>,
}

impl Default for KeySnap {
    fn default() -> Self {
        Self {
            qtdemux: AtomicBool::new(false),
            state: Mutex::new(State {
                target: None,
                phase: Phase::Idle,
                #[cfg(test)]
                first_fed: None,
            }),
        }
    }
}

impl KeySnap {
    /// Record that a key-unit seek to `target` is about to be sent.
    pub(crate) fn arm(&self, target: gst::ClockTime) {
        let mut state = self.lock();
        state.target = Some(target);
        state.phase = Phase::AwaitFlush;
        #[cfg(test)]
        {
            state.first_fed = None;
        }
    }

    #[cfg(test)]
    pub(crate) fn first_fed(&self) -> Option<gst::ClockTime> {
        self.lock().first_fed
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Whether `buffer` reaches the decoder.
    fn admit(&self, buffer: &gst::BufferRef) -> bool {
        let mut state = self.lock();
        let key = !buffer.flags().contains(gst::BufferFlags::DELTA_UNIT);
        let pts = buffer.pts();
        #[cfg(test)]
        let deciding = matches!(state.phase, Phase::Fresh { .. } | Phase::Skipping);
        let admit = match state.phase {
            Phase::Idle | Phase::AwaitFlush | Phase::AwaitSegment => true,
            Phase::Fresh { start } => {
                let early = key
                    && self.qtdemux.load(Ordering::Acquire)
                    && pts.is_some_and(|pts| pts + TOLERANCE < start)
                    && state
                        .target
                        .is_some_and(|target| start + TOLERANCE < target);
                state.phase = if early { Phase::Skipping } else { Phase::Idle };
                !early
            }
            Phase::Skipping if key => {
                state.phase = Phase::Idle;
                true
            }
            Phase::Skipping => {
                let past = matches!(
                    (pts, state.target),
                    (Some(pts), Some(target)) if pts > target + TOLERANCE
                );
                if past {
                    tracing::warn!(
                        ?pts,
                        target = ?state.target,
                        "no video keyframe between the seek's segment start and its target; \
                         the decoder resumes on a delta frame and skips the frames it cannot decode"
                    );
                    state.phase = Phase::Idle;
                }
                past
            }
        };
        if admit && state.phase == Phase::Idle {
            state.target = None;
        }
        #[cfg(test)]
        if admit && deciding && state.first_fed.is_none() {
            state.first_fed = pts;
        }
        admit
    }

    fn event(&self, event: &gst::EventRef) {
        let mut state = self.lock();
        match event.view() {
            gst::EventView::FlushStop(_) if state.target.is_some() => {
                state.phase = Phase::AwaitSegment;
            }
            gst::EventView::Segment(segment) if state.phase == Phase::AwaitSegment => {
                if let Some(start) = segment
                    .segment()
                    .downcast_ref::<gst::ClockTime>()
                    .and_then(|segment| segment.start())
                {
                    state.phase = Phase::Fresh { start };
                }
            }
            _ => {}
        }
    }
}

/// Watch `bin` for its demuxer and its video decoders, and correct each
/// decoder's start after a key-unit seek through `snap`.
pub(crate) fn snap_video_to_keyframes(bin: &gst::Bin, snap: &Arc<KeySnap>) {
    let snap = Arc::clone(snap);
    bin.connect_deep_element_added(move |_, _, element| {
        let Some(factory) = element.factory() else {
            return;
        };
        if factory.name() == "qtdemux" {
            snap.qtdemux.store(true, Ordering::Release);
            return;
        }
        let klass = factory
            .metadata(gst::ELEMENT_METADATA_KLASS)
            .unwrap_or_default();
        if !(klass.contains("Decoder") && klass.contains("Video")) {
            return;
        }
        let Some(pad) = element.static_pad("sink") else {
            return;
        };
        let snap = Arc::clone(&snap);
        pad.add_probe(
            gst::PadProbeType::BUFFER
                | gst::PadProbeType::EVENT_DOWNSTREAM
                | gst::PadProbeType::EVENT_FLUSH,
            move |_, info| match &info.data {
                Some(gst::PadProbeData::Buffer(buffer)) => {
                    if snap.admit(buffer) {
                        gst::PadProbeReturn::Ok
                    } else {
                        // `Handled`, not `Drop`: gstreamer-rs has taken the
                        // buffer out of the probe info and frees it here; on
                        // `Drop` GStreamer would unref the emptied slot.
                        gst::PadProbeReturn::Handled
                    }
                }
                Some(gst::PadProbeData::Event(event)) => {
                    snap.event(event);
                    gst::PadProbeReturn::Ok
                }
                _ => gst::PadProbeReturn::Ok,
            },
        );
    });
}

#[cfg(test)]
#[path = "gst_key_snap_tests.rs"]
mod tests;
