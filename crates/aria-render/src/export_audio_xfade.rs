//! Audio crossfades at cuts.
//!
//! A [`Transition`](aria_models::Transition) on an audio lane stores an
//! [`AudioTransition`] id. Its window is `duration` ticks centred on the cut.
//! Across that window the outgoing span plays past the cut into its source
//! tail and ramps out, and the incoming span starts early from its source head
//! and ramps in, both on the transition's curve. The ramp replaces the clip's
//! own fade on that side.
//!
//! Only a linear span (unity speed, no speed curve) plays into its handle: a
//! retimed span has no one-to-one source position past its edge. A span with
//! less handle than the window asks for ramps over the part of the window it
//! covers, so it reaches silence where its media ends instead of stopping at a
//! partial level.

use aria_models::{AudioTransition, Rational, Track};

use super::{Span, ticks_to_samples};
use crate::audio_dsp::SpanWarp;

/// A gain ramp over part of a span, from a crossfade at one of its cuts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Crossfade {
    /// First sample of the ramp, relative to the span start. Negative when the
    /// window opens before the span does.
    pub start: i64,
    /// Ramp length in samples; always positive.
    pub len: i64,
    /// The curve the ramp follows.
    pub curve: AudioTransition,
}

impl Crossfade {
    fn progress(self, pos: i64) -> f32 {
        ((pos - self.start) as f64 / self.len as f64) as f32
    }
}

impl Span {
    /// The product of this span's crossfade ramps at span-relative `pos`: 1
    /// outside every ramp.
    pub(crate) fn crossfade_gain(&self, pos: i64) -> f32 {
        let rising = self
            .crossfade_in
            .map_or(1.0, |ramp| ramp.curve.incoming(ramp.progress(pos)));
        let falling = self
            .crossfade_out
            .map_or(1.0, |ramp| ramp.curve.outgoing(ramp.progress(pos)));
        rising * falling
    }
}

/// One audible clip of a lane, as the mixer resolved it.
pub(crate) struct LaneSpan {
    pub clip: aria_models::ClipId,
    /// Index into the mixer's spans.
    pub span: usize,
    /// The clip's media length in output sample frames.
    pub media_len: i64,
}

/// Apply every audio transition on `track` to the spans its clips resolved to.
/// A transition whose cut no longer abuts, or whose id is a picture blend, is
/// skipped. A side with no span (a silent clip, or one without audio) leaves
/// the other side ramping against silence.
pub(crate) fn crossfade_lane(track: &Track, lane: &[LaneSpan], spans: &mut [Span], fps: Rational) {
    let span_of = |clip| lane.iter().find(|entry| entry.clip == clip);
    for transition in track.transitions() {
        let Some(curve) = AudioTransition::from_id(&transition.transition_id) else {
            continue;
        };
        let (Some(left), Some(right)) = (track.clip(transition.left), track.clip(transition.right))
        else {
            continue;
        };
        let cut = left.timeline.end_tick();
        if cut != right.timeline.start.value {
            continue;
        }
        let (window_start, window_end) = transition.window(cut);
        let window = (
            ticks_to_samples(window_start, fps.num, fps.den),
            ticks_to_samples(window_end, fps.num, fps.den),
        );
        if let Some(entry) = span_of(transition.left) {
            fade_outgoing(&mut spans[entry.span], entry.media_len, window, curve);
        }
        if let Some(entry) = span_of(transition.right) {
            fade_incoming(&mut spans[entry.span], entry.media_len, window, curve);
        }
    }
}

/// The outgoing side: play on past the cut into the source tail, up to the
/// window end, and ramp out from the window start to the span's new end.
pub(crate) fn fade_outgoing(
    span: &mut Span,
    media_len: i64,
    (window_start, window_end): (i64, i64),
    curve: AudioTransition,
) {
    if matches!(span.warp, SpanWarp::Linear) {
        let want = (window_end - span.end).max(0);
        // A reversed span walks its source backward, so its tail is the
        // material before the source window.
        let handle = if span.reversed {
            span.source_start
        } else {
            media_len - (span.source_start + span.source_len)
        };
        let grow = want.min(handle.max(0));
        if span.reversed {
            span.source_start -= grow;
        }
        span.source_len += grow;
        span.end += grow;
    }
    let len = span.end - window_start;
    if len <= 0 {
        return;
    }
    span.crossfade_out = Some(Crossfade {
        start: window_start - span.start,
        len,
        curve,
    });
    span.fade_out = 0;
}

/// The incoming side: start early from the source head, back to the window
/// start, and ramp in from the span's new start to the window end.
pub(crate) fn fade_incoming(
    span: &mut Span,
    media_len: i64,
    (window_start, window_end): (i64, i64),
    curve: AudioTransition,
) {
    if matches!(span.warp, SpanWarp::Linear) {
        let want = (span.start - window_start).max(0);
        let handle = if span.reversed {
            media_len - (span.source_start + span.source_len)
        } else {
            span.source_start
        };
        let grow = want.min(handle.max(0));
        if !span.reversed {
            span.source_start -= grow;
        }
        span.source_len += grow;
        span.start -= grow;
        // The envelopes are span-relative: what sat at the old start now sits
        // `grow` samples in.
        span.volume = span.volume.map_ticks(|tick| tick + grow);
        span.pan = span.pan.map_ticks(|tick| tick + grow);
    }
    let len = window_end - span.start;
    if len <= 0 {
        return;
    }
    span.crossfade_in = Some(Crossfade {
        start: 0,
        len,
        curve,
    });
    span.fade_in = 0;
}
