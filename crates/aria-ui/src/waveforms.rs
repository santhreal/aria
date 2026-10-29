//! Audio peaks for the timeline.
//!
//! A waveform is a picture of the source, not of the clip: peaks are decoded
//! once per media entry, time-anchored at a fixed rate, and every clip on that
//! media slices the same array by its own source window. Trimming, moving, or
//! splitting a clip therefore costs no decode.
//!
//! Decoding is off the UI thread and requested at most once per media: the
//! entry is marked pending before the thread starts, and a source with no
//! usable audio is remembered as refused so a failing file is not retried every
//! frame.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use gpui::{App, Bounds, Entity, IntoElement, Pixels, Styled, canvas, fill, point, px, size};

use crate::app::AppState;

/// Peaks decoded per second of source audio. 40 is ~1 peak per 25ms: at the
/// default zoom that is finer than a pixel, and at a deep zoom the bars widen
/// rather than lie about detail that was never decoded.
const PEAKS_PER_SECOND: f64 = 40.0;

/// Ceiling on peaks kept for one source, so a feature-length file cannot pin an
/// unbounded array. At `PEAKS_PER_SECOND` this covers about 2.7 hours; beyond
/// it the peaks coarsen instead of growing.
const MAX_PEAKS: usize = 400_000;

/// One decoded source's peaks, and the rate they were decoded at.
#[derive(Debug, Clone)]
pub struct Waveform {
    /// Peak amplitude in `0.0..=1.0`, one per `1 / per_second` of source audio.
    pub peaks: Arc<Vec<f32>>,
    pub per_second: f64,
}

/// Every source's peaks, plus what is in flight and what refused to decode.
#[derive(Debug, Default)]
pub struct WaveformStore {
    ready: HashMap<u64, Waveform>,
    pending: HashSet<u64>,
    refused: HashSet<u64>,
}

impl WaveformStore {
    pub fn get(&self, media: u64) -> Option<&Waveform> {
        self.ready.get(&media)
    }

    /// Whether a decode should be started for `media`: nothing is known about
    /// it, nothing is in flight, and it has not already refused.
    pub fn wants(&self, media: u64) -> bool {
        !self.ready.contains_key(&media)
            && !self.pending.contains(&media)
            && !self.refused.contains(&media)
    }

    /// Whether `media` has already answered that it has no waveform: no audio
    /// stream, an unreadable file, or a decode that panicked.
    ///
    /// Read where a refusal has to be told apart from a decode still running:
    /// the timeline draws nothing either way, but a command that measures sound
    /// declines for a different reason in each case.
    pub fn refused(&self, media: u64) -> bool {
        self.refused.contains(&media)
    }

    fn mark_pending(&mut self, media: u64) {
        self.pending.insert(media);
    }

    /// Hold a decoded source's peaks.
    ///
    /// `pub(crate)` rather than private to this module because a test dresses a
    /// state with sound: a command that measures sound has nothing to measure
    /// otherwise, and a census that could only ever see its refusal would prove
    /// the refusal and nothing else.
    pub(crate) fn insert(&mut self, media: u64, waveform: Waveform) {
        self.pending.remove(&media);
        self.ready.insert(media, waveform);
    }

    fn refuse(&mut self, media: u64) {
        self.pending.remove(&media);
        self.refused.insert(media);
    }

    /// Forget everything. Media ids are re-allocated per session, so a new or
    /// reopened project must not read another project's peaks.
    pub fn clear(&mut self) {
        self.ready.clear();
        self.pending.clear();
        self.refused.clear();
    }
}

/// The bar heights to draw for one clip: `bars` values in `0.0..=1.0` covering
/// `[from_secs, to_secs)` of the source.
///
/// Each bar is the loudest peak in its own slice, so a bar never hides a
/// transient the way an average would, and a range past the decoded end reads
/// as silence rather than repeating the last peak.
pub fn bars(waveform: &Waveform, from_secs: f64, to_secs: f64, bars: usize) -> Vec<f32> {
    if bars == 0 || waveform.peaks.is_empty() || waveform.per_second <= 0.0 {
        return Vec::new();
    }
    let span = (to_secs - from_secs).max(0.0);
    if span == 0.0 {
        return vec![0.0; bars];
    }
    let mut out = Vec::with_capacity(bars);
    for bar in 0..bars {
        let a = from_secs + span * bar as f64 / bars as f64;
        let b = from_secs + span * (bar + 1) as f64 / bars as f64;
        let first = (a * waveform.per_second).floor().max(0.0) as usize;
        let last = (b * waveform.per_second).ceil().max(0.0) as usize;
        let slice = waveform
            .peaks
            .get(first..last.min(waveform.peaks.len()))
            .unwrap_or(&[]);
        let peak = slice.iter().copied().fold(0.0f32, f32::max);
        out.push(peak.clamp(0.0, 1.0));
    }
    out
}

/// Paint one bar per entry, mirrored around the strip's centre line.
///
/// Here rather than in the panel that first needed it, because the peaks and
/// the pixels are one concern: a second painter would be free to disagree about
/// what a bar means. The timeline draws a clip's slice of a source and the pool
/// sidebar draws a camera's place on a group's clock, and both are this.
///
/// A canvas rather than one element per bar: a wide clip has hundreds of bars,
/// and each would otherwise cost a laid-out element every frame. Anything
/// shorter than a pixel is still drawn as one pixel, so quiet audio reads as a
/// thin line instead of disappearing.
pub fn wave(bars: Vec<f32>, theme: &crate::theme::Theme) -> impl IntoElement {
    let peak = theme.waveform_peak;
    let rms = theme.waveform_rms;
    canvas(
        |_, _, _| (),
        move |bounds: Bounds<Pixels>, _, window, _| {
            let count = bars.len().max(1);
            let step = f32::from(bounds.size.width) / count as f32;
            let half = f32::from(bounds.size.height) / 2.0;
            let mid = f32::from(bounds.origin.y) + half;
            let bar_w = (step - 1.0).max(1.0);
            for (index, amplitude) in bars.iter().enumerate() {
                let h = (amplitude.clamp(0.0, 1.0) * half).max(0.5);
                let x = f32::from(bounds.origin.x) + index as f32 * step;
                window.paint_quad(fill(
                    Bounds {
                        origin: point(px(x), px(mid - h)),
                        size: size(px(bar_w), px(h * 2.0)),
                    },
                    peak,
                ));
                // A second, shorter bar in the quieter colour reads as the body
                // of the wave, so a transient stands out from sustained level.
                let body = h * 0.45;
                window.paint_quad(fill(
                    Bounds {
                        origin: point(px(x), px(mid - body)),
                        size: size(px(bar_w), px(body * 2.0)),
                    },
                    rms,
                ));
            }
        },
    )
    .size_full()
}

/// Scale bar heights by what the mix will actually do to them: the clip's own
/// volume, then its fade ramps.
///
/// A wave drawn at source level after a volume change is a lie about what will
/// be heard, and a fade that is invisible on the card is the most common
/// mixing mistake this can prevent. The ramps are linear in amplitude, which is
/// what the renderer applies.
pub fn apply_gain(
    bars: &mut [f32],
    volume: f32,
    fade_in_ticks: i64,
    fade_out_ticks: i64,
    clip_ticks: i64,
) {
    let volume = volume.clamp(0.0, 4.0);
    let count = bars.len();
    if count == 0 {
        return;
    }
    let span = clip_ticks.max(1) as f64;
    for (index, bar) in bars.iter_mut().enumerate() {
        // The middle of the bar's own slice, so a ramp is sampled where the bar
        // is drawn rather than at its leading edge.
        let at = span * (index as f64 + 0.5) / count as f64;
        let mut gain = volume;
        if fade_in_ticks > 0 {
            gain *= (at / fade_in_ticks as f64).min(1.0) as f32;
        }
        if fade_out_ticks > 0 {
            let from_end = span - at;
            gain *= (from_end / fade_out_ticks as f64).clamp(0.0, 1.0) as f32;
        }
        *bar = (*bar * gain).clamp(0.0, 1.0);
    }
}

/// Start decoding `media`'s peaks if nothing is known about it yet.
///
/// Marking pending before the thread starts is what makes this safe to call
/// from a render: a second call in the same frame does nothing.
pub fn request(state: &Entity<AppState>, cx: &mut App, media: u64, path: PathBuf) {
    if !state.read(cx).waveforms.wants(media) {
        return;
    }
    state.update(cx, |state, _| state.waveforms.mark_pending(media));
    let state = state.clone();
    cx.spawn(async move |cx| {
        // Its own thread, so a file that panics the decoder cannot take the
        // window with it, and the wait is as long as the decode: waiting on the
        // foreground executor would hold the window still for a whole audio
        // decode, which for a long take is seconds rather than milliseconds.
        let decoded =
            crate::handoff::on_a_thread(cx.background_executor(), "aria-waveform", move || {
                aria_decoder::audio_peaks_per_second(&path, PEAKS_PER_SECOND, MAX_PEAKS)
            })
            .await;
        state.update(cx, |state, cx| {
            match decoded {
                Some(Ok(peaks)) if !peaks.peaks.is_empty() => state.waveforms.insert(
                    media,
                    Waveform {
                        peaks: Arc::new(peaks.peaks),
                        per_second: peaks.per_second,
                    },
                ),
                // A still image, a video with no audio stream, an unreadable
                // file, or a panicked decode all mean the same thing to the
                // timeline: this source has no waveform to draw.
                _ => state.waveforms.refuse(media),
            }
            cx.notify();
        });
    })
    .detach();
}

#[cfg(test)]
#[path = "waveform_tests.rs"]
mod waveform_tests;
